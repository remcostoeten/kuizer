use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::AppError;
use crate::cli::{ScanArgs, SortKey};
use crate::config::Config;
use crate::detect::Safety;
use crate::output;
use crate::platform;
use crate::scan::{self, Cancel, Candidate, Issue, Summary};
use crate::tui;
use crate::units::Idle;

pub fn sort_candidates(candidates: &mut [Candidate], key: SortKey) {
    match key {
        SortKey::Size => candidates.sort_by(|a, b| {
            b.usage
                .allocated
                .cmp(&a.usage.allocated)
                .then_with(|| a.path.cmp(&b.path))
        }),
        SortKey::Path => candidates.sort_by(|a, b| a.path.cmp(&b.path)),
        SortKey::Category => candidates.sort_by(|a, b| {
            a.rule
                .category
                .cmp(&b.rule.category)
                .then_with(|| b.usage.allocated.cmp(&a.usage.allocated))
        }),
        SortKey::Idle => candidates.sort_by(|a, b| {
            a.project_last_active
                .is_none()
                .cmp(&b.project_last_active.is_none())
                .then_with(|| a.project_last_active.cmp(&b.project_last_active))
                .then_with(|| b.usage.allocated.cmp(&a.usage.allocated))
        }),
    }
}

/// @name keep_idle
/// @description Measures project activity when `idle` is set or `annotate` asks for it, then
/// keeps only candidates whose project has been inactive for at least `idle`. A candidate whose
/// activity is unknown never matches `idle`.
///
/// @example
/// commands::scan::keep_idle(&config, &mut candidates, args.idle, false)?;
pub fn keep_idle(
    config: &Config,
    candidates: &mut Vec<Candidate>,
    idle: Option<Idle>,
    annotate: bool,
) -> Result<(), AppError> {
    if idle.is_none() && !annotate {
        return Ok(());
    }
    let cutoff = idle.and_then(|Idle(age)| SystemTime::now().checked_sub(age));
    scan::annotate(
        candidates,
        &config.detector()?,
        &config.scan.skip_dirs,
        cutoff,
        scan::size_threads(config),
    );
    if let Some(cutoff) = cutoff {
        candidates.retain(|candidate| {
            candidate
                .project_last_active
                .is_some_and(|active| active <= cutoff)
        });
    }
    Ok(())
}

pub fn resolve_roots(config: &Config, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, AppError> {
    if !paths.is_empty() {
        return Ok(paths);
    }
    if !config.scan.roots.is_empty() {
        return Ok(config.scan.roots.clone());
    }
    Ok(vec![std::env::current_dir()?])
}

pub fn run(config: &Config, args: ScanArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    let mut config = config.clone();
    if !args.categories.is_empty() {
        config.detect.categories = args.categories;
    }
    if let Some(min_size) = args.min_size {
        config.scan.min_size = min_size;
    }
    if let Some(Some(target)) = &args.report {
        refuse_overwrite(target, args.force)?;
    }
    let roots = scan::normalize_roots(&resolve_roots(&config, args.paths)?);
    let handle = scan::start(&config, &roots, Cancel::new())?;
    let terminal = std::io::stdout().is_terminal()
        && std::io::stderr().is_terminal()
        && std::io::stdin().is_terminal();
    let annotate = args.sort == SortKey::Idle;
    let live = !json && !args.no_live && terminal && args.idle.is_none() && !annotate;
    let show_progress = !json && !quiet && std::io::stderr().is_terminal();
    let safe_only = args.safe_only;
    let report = args.report;
    let force = args.force;
    let open = !args.no_open && std::io::stderr().is_terminal();
    let collected = if live {
        tui::watch(&handle, safe_only)?
    } else {
        let collected = scan::collect(&handle, |progress| {
            if show_progress {
                output::print_progress(progress);
            }
        });
        if show_progress {
            output::clear_progress();
        }
        collected
    };

    let mut candidates = collected.candidates;
    if safe_only {
        candidates.retain(|candidate| candidate.rule.safety == Safety::Safe);
    }
    keep_idle(&config, &mut candidates, args.idle, annotate)?;

    sort_candidates(&mut candidates, args.sort);
    let written = report
        .map(|target| {
            write_report(
                target,
                force,
                &roots,
                &candidates,
                &collected.issues,
                &collected.summary,
            )
        })
        .transpose()?;

    if live {
        output::print_overview(
            &candidates,
            &collected.issues,
            &collected.summary,
            output::largest_count(args.limit),
        );
        announce_report(written.as_deref(), open);
        return Ok(());
    }

    if let Some(limit) = args.limit {
        candidates.truncate(limit);
    }
    if json {
        println!(
            "{}",
            output::scan_report(&roots, &candidates, &collected.issues, &collected.summary)?
        );
    } else {
        output::print_scan(&candidates, &collected.issues, &collected.summary, quiet);
    }
    announce_report(written.as_deref(), open);
    Ok(())
}

pub(super) fn announce_report(path: Option<&Path>, open: bool) {
    let Some(path) = path else {
        return;
    };
    eprintln!("Report written to {}", output::display_path(path));
    if open && let Err(error) = platform::open_in_browser(path) {
        eprintln!("Could not open a browser: {error}");
    }
}

fn default_page_path(prefix: &str) -> Result<PathBuf, AppError> {
    let dir = dirs::cache_dir().ok_or(AppError::NoHome)?.join("kuizer");
    let stamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S");
    Ok(dir.join(format!("{prefix}-{stamp}.html")))
}

pub(super) fn generated_at() -> String {
    jiff::Zoned::now().strftime("%Y-%m-%d %H:%M").to_string()
}

pub(super) fn refuse_overwrite(target: &Path, force: bool) -> Result<(), AppError> {
    if force || std::fs::symlink_metadata(target).is_err() {
        return Ok(());
    }
    Err(AppError::PageExists(target.to_path_buf()))
}

pub(super) fn write_page(
    target: Option<PathBuf>,
    force: bool,
    prefix: &str,
    page: &str,
) -> Result<PathBuf, AppError> {
    let path = match target {
        Some(path) => path,
        None => default_page_path(prefix)?,
    };
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    if force {
        options.write(true).create(true).truncate(true);
    } else {
        options.write(true).create_new(true);
    }
    let mut file = options.open(&path).map_err(|error| match error.kind() {
        std::io::ErrorKind::AlreadyExists => AppError::PageExists(path.clone()),
        _ => AppError::Io(error),
    })?;
    std::io::Write::write_all(&mut file, page.as_bytes())?;
    Ok(std::path::absolute(&path).unwrap_or(path))
}

fn write_report(
    target: Option<PathBuf>,
    force: bool,
    roots: &[PathBuf],
    candidates: &[Candidate],
    issues: &[Issue],
    summary: &Summary,
) -> Result<PathBuf, AppError> {
    let page = output::scan_html(roots, candidates, issues, summary, &generated_at())?;
    write_page(target, force, "report", &page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_page_keeps_an_existing_file_unless_forced() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("report.html");
        std::fs::write(&path, "mine").unwrap();

        assert!(matches!(
            refuse_overwrite(&path, false),
            Err(AppError::PageExists(_))
        ));
        assert!(matches!(
            write_page(Some(path.clone()), false, "report", "new"),
            Err(AppError::PageExists(_))
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine");

        refuse_overwrite(&path, true).unwrap();
        write_page(Some(path.clone()), true, "report", "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn write_page_creates_a_new_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("nested/report.html");
        refuse_overwrite(&path, false).unwrap();
        write_page(Some(path.clone()), false, "report", "page").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "page");
    }
}
