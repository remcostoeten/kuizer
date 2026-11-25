use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::AppError;
use super::filter::{self, Filters};
use crate::cli::{Format, ScanArgs};
use crate::config::Config;
use crate::detect::{Category, Safety};
use crate::output;
use crate::platform;
use crate::scan::{self, Cancel, Candidate, Issue, SortKey, Summary};
use crate::tui;
use crate::units::{ByteSize, Idle};

/// @name keep_idle
/// @description Measures project activity when `idle` is set or `annotate` asks for it, then
/// keeps only candidates whose project has been inactive for at least `idle`. Activity is measured
/// within the same scope as the scan under `config`, so excluded folders and other filesystems
/// never count. A candidate whose activity is unknown never matches `idle`.
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
    let now = SystemTime::now();
    let cutoff = idle.and_then(|Idle(age)| now.checked_sub(age));
    if idle.is_some() && cutoff.is_none() {
        candidates.clear();
        return Ok(());
    }
    scan::annotate(candidates, config, cutoff)?;
    if let Some(global_idle) = idle {
        let mut overrides: std::collections::HashMap<PathBuf, Option<Idle>> =
            std::collections::HashMap::new();
        candidates.retain(|candidate| {
            let project_idle = candidate.project.as_ref().and_then(|project| {
                *overrides
                    .entry(project.clone())
                    .or_insert_with(|| crate::config::project_idle_override(project))
            });
            let effective = crate::config::effective_idle(global_idle, project_idle);
            let Some(effective_cutoff) = now.checked_sub(effective.0) else {
                return false;
            };
            candidate
                .project_last_active
                .is_some_and(|active| active <= effective_cutoff)
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

/// @name narrow
/// @description Applies the `--category` and `--min-size` flags on top of the config. Flags can
/// only narrow it: categories intersect and the larger minimum size wins. An empty intersection
/// leaves no category, so nothing is reported.
///
/// @example
/// commands::scan::narrow(&mut config, &args.categories, args.min_size);
pub fn narrow(config: &mut Config, categories: &[Category], min_size: Option<ByteSize>) {
    if !categories.is_empty() {
        config
            .detect
            .categories
            .retain(|category| categories.contains(category));
    }
    if let Some(min_size) = min_size {
        config.scan.min_size = config.scan.min_size.max(min_size);
    }
}

pub fn run(config: &Config, args: ScanArgs, format: Format, quiet: bool) -> Result<(), AppError> {
    let rules = filter::project_rules(config);
    let names: Vec<&str> = rules.iter().map(String::as_str).collect();
    let filters = Filters::new(config, &args.categories, args.min_size, &args.scope, &names)?;
    let config = filters.config();
    let json = format != Format::Text;
    if let Some(Some(target)) = &args.report {
        refuse_overwrite(target, args.force)?;
    }
    let roots = scan::normalize_roots(&resolve_roots(config, args.paths)?);
    let handle = scan::start(config, &roots, Cancel::new())?;
    let terminal = std::io::stdout().is_terminal()
        && std::io::stderr().is_terminal()
        && std::io::stdin().is_terminal();
    let annotate = args.sort == SortKey::Idle;
    let unfiltered = args.scope.rules.is_empty() && args.scope.projects.is_empty();
    let live = !json && !args.no_live && terminal && args.idle.is_none() && !annotate && unfiltered;
    let stream =
        format == Format::JsonLines && args.idle.is_none() && !annotate && args.limit.is_none();
    let show_progress = !json && !quiet && std::io::stderr().is_terminal();
    let safe_only = args.safe_only;
    let keep = |candidate: &Candidate| {
        (!safe_only || candidate.rule.safety == Safety::Safe) && filters.matches(candidate)
    };
    let report = args.report;
    let force = args.force;
    let open = !args.no_open && std::io::stderr().is_terminal();
    let collected = if live {
        tui::watch(&handle, safe_only)?
    } else {
        if show_progress {
            let mounts = scan::mount_spaces(&roots, &[]);
            if !mounts.is_empty() {
                let summary = mounts
                    .iter()
                    .map(|m| {
                        format!(
                            "{}: {} free of {}",
                            output::display_path(&m.mount),
                            ByteSize(m.available),
                            ByteSize(m.total)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("  |  ");
                eprintln!("{summary}");
            }
        }
        let mut failed = None;
        let collected = scan::collect_with(
            &handle,
            |progress| {
                if show_progress {
                    output::print_progress(progress);
                }
            },
            |candidate| {
                if stream && failed.is_none() && keep(candidate) {
                    match output::scan_line(candidate) {
                        Ok(line) => println!("{line}"),
                        Err(error) => failed = Some(error),
                    }
                }
            },
        );
        if show_progress {
            output::clear_progress();
        }
        if let Some(error) = failed {
            return Err(error.into());
        }
        collected
    };

    let mut candidates = collected.candidates;
    candidates.retain(keep);
    keep_idle(filters.evidence(), &mut candidates, args.idle, annotate)?;

    scan::sort_candidates(&mut candidates, args.sort);
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
            &roots,
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
    match format {
        Format::Json => println!(
            "{}",
            output::scan_report(&roots, &candidates, &collected.issues, &collected.summary)?
        ),
        Format::JsonLines => {
            if !stream {
                for candidate in &candidates {
                    println!("{}", output::scan_line(candidate)?);
                }
            }
            println!(
                "{}",
                output::scan_summary_line(&roots, &collected.issues, &collected.summary)?
            );
        }
        Format::Text => output::print_scan(
            &roots,
            &candidates,
            &collected.issues,
            &collected.summary,
            quiet,
        ),
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
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    Ok(dir.join(format!("{prefix}-{stamp}.html")))
}

pub(super) fn generated_at() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
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
