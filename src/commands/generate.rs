use std::io::Write;

use clap::CommandFactory;

use super::AppError;
use crate::cli::{Cli, GenerateTarget};

pub fn run(target: &GenerateTarget) -> Result<(), AppError> {
    let mut command = Cli::command();
    match target {
        GenerateTarget::Completions { shell } => {
            let mut script = Vec::new();
            clap_complete::generate(*shell, &mut command, "kuizer", &mut script);
            let mut out = std::io::stdout().lock();
            out.write_all(&script)?;
            out.flush()?;
        }
        GenerateTarget::Man { out: Some(dir) } => {
            std::fs::create_dir_all(dir)?;
            clap_mangen::generate_to(command, dir)?;
        }
        GenerateTarget::Man { out: None } => {
            let mut out = std::io::stdout().lock();
            clap_mangen::Man::new(command).render(&mut out)?;
            out.flush()?;
        }
    }
    Ok(())
}
