use std::io::{self, BufRead, IsTerminal, Write};

use super::AppError;
use super::interrupt::Interrupt;
use super::scan::resolve_roots;
use crate::cli::{DuplicatesAction, DuplicatesArgs, DuplicatesLinkArgs};
use crate::config::Config;
use crate::dupes::{self, LinkMode, Options, Stage};
use crate::output;
use crate::scan;
use crate::units::ByteSize;

const TEXT_GROUPS: usize = 25;

pub fn run(config: &Config, args: DuplicatesArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    if let Some(DuplicatesAction::Link(link_args)) = args.command {
        let mut paths = args.paths;
        paths.extend(link_args.paths.iter().cloned());
        return run_link(
            config,
            paths,
            link_args.min_size.or(args.min_size),
            link_args.limit.or(args.limit),
            link_args,
            json,
            quiet,
        );
    }

    let roots = scan::normalize_roots(&resolve_roots(config, args.paths)?);
    let mut options = Options::from_config(config);
    if let Some(min_size) = args.min_size {
        options.min_size = min_size.0;
    }
    options.verify |= args.verify;
    let show_progress = !json && !quiet && io::stderr().is_terminal();
    let found = dupes::find(config, &roots, options, |stage| {
        if !show_progress {
            return;
        }
        output::print_status(&match stage {
            Stage::Walking { files } => format!("walking  {files} files"),
            Stage::Hashing { files } => format!("hashing  {files} files"),
            Stage::Verifying { groups } => format!("comparing bytes in {groups} groups"),
        });
    })?;
    if show_progress {
        output::clear_progress();
    }
    let limit = match (args.limit, json) {
        (Some(limit), _) => limit,
        (None, true) => usize::MAX,
        (None, false) => TEXT_GROUPS,
    };
    let shown = &found.groups[..found.groups.len().min(limit)];
    if json {
        println!("{}", output::duplicates_report(&roots, &found, shown)?);
    } else {
        output::print_duplicates(&found, shown, quiet);
    }
    Ok(())
}

fn run_link(
    config: &Config,
    paths: Vec<std::path::PathBuf>,
    min_size: Option<ByteSize>,
    limit: Option<usize>,
    args: DuplicatesLinkArgs,
    json: bool,
    quiet: bool,
) -> Result<(), AppError> {
    let roots = scan::normalize_roots(&resolve_roots(config, paths)?);
    let mut options = Options::from_config(config);
    if let Some(min_size) = min_size {
        options.min_size = min_size.0;
    }
    // Always verify duplicate contents byte by byte before replacing any file with a link.
    options.verify = true;

    let mode = if args.hardlink {
        LinkMode::Hardlink
    } else {
        LinkMode::Reflink
    };

    let interrupt = Interrupt::catch();
    let cancel = scan::Cancel::new();
    let show_progress = !json && !quiet && io::stderr().is_terminal();
    let found = dupes::find_cancellable(config, &roots, options, &cancel, |stage| {
        if interrupt.requested() {
            cancel.cancel();
            return;
        }
        if !show_progress {
            return;
        }
        output::print_status(&match stage {
            Stage::Walking { files } => format!("walking  {files} files"),
            Stage::Hashing { files } => format!("hashing  {files} files"),
            Stage::Verifying { groups } => format!("comparing bytes in {groups} groups"),
        });
    })?;
    if show_progress {
        output::clear_progress();
    }
    if interrupt.requested() || cancel.is_cancelled() {
        return Err(AppError::Interrupted(0));
    }

    let groups = match limit {
        Some(n) => &found.groups[..found.groups.len().min(n)],
        None => &found.groups[..],
    };

    let copies: usize = groups
        .iter()
        .map(|g| g.members.len().saturating_sub(1))
        .sum();
    let reclaimable: u64 = groups.iter().map(dupes::Group::reclaimable).sum();

    if copies == 0 {
        let report = dupes::LinkReport::default();
        if json {
            println!(
                "{}",
                output::duplicates_link_report(&roots, mode, args.dry_run, &report, &found.issues)?
            );
        } else if !quiet {
            println!("No duplicate files to link among {} files", found.files);
            output::print_issues(&found.issues);
        }
        return Ok(());
    }

    if mode == LinkMode::Hardlink && !json && (!quiet || !args.yes) {
        eprintln!(
            "Warning: hard-linked copies share one inode — editing any copy changes all of them."
        );
    }

    if !args.dry_run && !args.yes && !confirm_link(mode, copies, reclaimable)? {
        if !json && !quiet {
            eprintln!("Cancelled.");
        }
        return Ok(());
    }

    let report = dupes::link_groups(groups, mode, args.dry_run, &cancel);
    if json {
        println!(
            "{}",
            output::duplicates_link_report(&roots, mode, args.dry_run, &report, &found.issues)?
        );
    } else {
        output::print_duplicates_link(mode, args.dry_run, &report, quiet);
        output::print_issues(&found.issues);
    }

    if report.failed > 0 {
        return Err(AppError::LinkFailed(report.failed));
    }
    Ok(())
}

fn confirm_link(mode: LinkMode, copies: usize, reclaimable: u64) -> Result<bool, AppError> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(AppError::NeedsConfirmation);
    }
    let verb = match mode {
        LinkMode::Reflink => "Reflink",
        LinkMode::Hardlink => "Hard-link",
    };
    let mut err = io::stderr().lock();
    let _ = write!(
        err,
        "{verb} {copies} duplicate copies (up to {} saved)? [y/N] ",
        ByteSize(reclaimable)
    );
    let _ = err.flush();
    drop(err);
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}
