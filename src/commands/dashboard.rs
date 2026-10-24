use std::io::IsTerminal;

use super::AppError;
use super::cache;
use super::scan::{
    announce_report, generated_at, refuse_overwrite, resolve_roots, sort_candidates, write_page,
};
use crate::caches::{self, Locations};
use crate::cli::{DashboardArgs, SortKey};
use crate::config::Config;
use crate::dupes::{self, Files, Options, Stage};
use crate::history;
use crate::output::{self, Dashboard};
use crate::protect::Protected;
use crate::scan::{self, Cancel};

pub fn run(config: &Config, args: DashboardArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    if let Some(target) = &args.out {
        refuse_overwrite(target, args.force)?;
    }
    let roots = scan::normalize_roots(&resolve_roots(config, args.paths)?);
    let show_progress = !json && !quiet && std::io::stderr().is_terminal();
    let open = !args.no_open && std::io::stderr().is_terminal();

    let options = (!args.no_duplicates).then(|| {
        let mut options = Options::from_config(config);
        if let Some(min_size) = args.min_size {
            options.min_size = min_size.0;
        }
        options
    });
    let (handle, files) = scan::start_with(
        config,
        &roots,
        Cancel::new(),
        options.map(|options| Files::for_roots(options, &roots)),
    )?;
    let collected = scan::collect(&handle, |progress| {
        if show_progress {
            output::print_progress(progress);
        }
    });
    let files = files.recv().ok().flatten();
    let mut candidates = collected.candidates;
    sort_candidates(&mut candidates, SortKey::Size);

    let duplicates = options.zip(files).map(|(options, files)| {
        dupes::group(files, options, |stage| {
            if !show_progress {
                return;
            }
            output::print_status(&match stage {
                Stage::Walking { files } => format!("duplicates: walking  {files} files"),
                Stage::Hashing { files } => format!("duplicates: hashing  {files} files"),
                Stage::Verifying { groups } => format!("duplicates: comparing {groups} groups"),
            });
        })
    });

    if show_progress {
        output::print_status("measuring global caches");
    }
    let locations = Locations::for_user().ok_or(AppError::NoHome)?;
    let cache_entries = cache::measure_threaded(
        caches::find(&locations, &Protected::for_user()),
        &Cancel::new(),
        scan::size_threads(config),
        |_| {},
    );
    if show_progress {
        output::clear_progress();
    }

    let history = history::load(&history::default_path()?)?;
    let trashed = history::trashed_paths();
    let kept = trashed
        .as_deref()
        .map(|paths| history::still_in_trash(&history, paths))
        .unwrap_or_default();

    let generated = generated_at();
    let dashboard = Dashboard {
        roots: &roots,
        candidates: &candidates,
        issues: &collected.issues,
        summary: &collected.summary,
        caches: &cache_entries,
        duplicates: duplicates.as_ref(),
        history: &history,
        kept: &kept,
        trash_known: trashed.is_some(),
        generated: &generated,
    };
    if json {
        println!("{}", output::dashboard_data(&dashboard)?);
        return Ok(());
    }
    let path = write_page(
        args.out,
        args.force,
        "dashboard",
        &output::dashboard_html(&dashboard)?,
    )?;
    announce_report(Some(&path), open);
    Ok(())
}
