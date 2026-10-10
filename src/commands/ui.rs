use std::path::PathBuf;

use super::AppError;
use super::scan::resolve_roots;
use crate::config::Config;
use crate::scan;
use crate::tui;

pub fn run(config: &Config, paths: Vec<PathBuf>) -> Result<(), AppError> {
    let roots = scan::normalize_roots(&resolve_roots(config, paths)?);
    Ok(tui::run(config, &roots)?)
}
