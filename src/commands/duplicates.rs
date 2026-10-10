use std::io::IsTerminal;

use super::AppError;
use super::scan::resolve_roots;
use crate::cli::DuplicatesArgs;
use crate::config::Config;
use crate::dupes::{self, Options, Stage};
use crate::output;
use crate::scan;

const TEXT_GROUPS: usize = 25;

pub fn run(config: &Config, args: DuplicatesArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    let roots = scan::normalize_roots(&resolve_roots(config, args.paths)?);
    let mut options = Options::from_config(config);
    if let Some(min_size) = args.min_size {
        options.min_size = min_size.0;
    }
    options.verify |= args.verify;
    let show_progress = !json && !quiet && std::io::stderr().is_terminal();
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
        println!("{}", output::duplicates_report(&roots, &found, shown));
    } else {
        output::print_duplicates(&found, shown, quiet);
    }
    Ok(())
}
