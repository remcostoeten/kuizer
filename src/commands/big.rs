use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::SystemTime;

use super::AppError;
use crate::big;
use crate::cli::BigArgs;
use crate::config::{Config, expand_home};
use crate::output;
use crate::protect::Protected;
use crate::scan;

pub fn run(config: &Config, args: BigArgs, json: bool, quiet: bool) -> Result<(), AppError> {
    let raw_roots: Vec<PathBuf> = if !args.paths.is_empty() {
        args.paths
    } else if !config.big.roots.is_empty() {
        config
            .big
            .roots
            .iter()
            .map(|path| expand_home(path))
            .collect()
    } else {
        vec![dirs::home_dir().ok_or(AppError::NoHome)?]
    };
    let roots = scan::normalize_roots(&raw_roots);
    let min_size = args.min_size.unwrap_or(config.big.min_size).0;
    let limit = args.limit.unwrap_or(config.big.limit);

    let show_progress = !json && !quiet && std::io::stderr().is_terminal();
    let found = big::find(config, &roots, min_size, |files| {
        if show_progress {
            output::print_status(&format!("scanning  {files} files"));
        }
    })?;
    if show_progress {
        output::clear_progress();
    }

    let old = if config.big.downloads {
        big::downloads_dir()
            .filter(|dir| dir.is_dir())
            .map(|dir| {
                big::old_downloads(
                    &dir,
                    config.big.downloads_age.0,
                    config.big.downloads_min_size.0,
                    SystemTime::now(),
                    &Protected::for_user(),
                )
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let candidates = big::candidates(found.files, old);
    if json {
        println!(
            "{}",
            output::big::big_report(&roots, &candidates, &found.issues)?
        );
    } else {
        let shown = &candidates[..candidates.len().min(limit)];
        output::big::print_big(shown, &found.issues, quiet);
    }
    Ok(())
}
