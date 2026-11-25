use std::fs;

use serde::Serialize;

use super::AppError;
use crate::cli::ConfigAction;
use crate::config::{Config, ConfigChange, Loaded, default_path, fix_file};

#[derive(Serialize)]
struct FixOutput<'a> {
    path: Option<String>,
    changes: &'a [ConfigChange],
}

pub fn run(loaded: &Loaded, action: Option<ConfigAction>, json: bool) -> Result<(), AppError> {
    match action.unwrap_or(ConfigAction::Show) {
        ConfigAction::Show if json => {
            println!("{}", serde_json::to_string_pretty(&loaded.config)?);
        }
        ConfigAction::Show => {
            match &loaded.source {
                Some(path) => println!("# loaded from {}", path.display()),
                None => println!("# no config file found, showing defaults"),
            }
            print!("{}", loaded.config.to_toml());
        }
        ConfigAction::Path => {
            let path = loaded.source.clone().or_else(default_path);
            println!("{}", path.ok_or(AppError::NoConfigDir)?.display());
        }
        ConfigAction::Init => {
            let path = default_path().ok_or(AppError::NoConfigDir)?;
            if path.exists() {
                return Err(AppError::ConfigExists(path));
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, Config::default().init_toml())?;
            println!("wrote {}", path.display());
        }
        ConfigAction::Check { fix: false } => {
            if !loaded.warnings.is_empty() {
                return Err(AppError::ConfigProblems);
            }
            loaded.config.validate()?;
            println!("config ok");
        }
        ConfigAction::Check { fix: true } => {
            let Some(path) = loaded.source.as_deref() else {
                if json {
                    let payload = FixOutput {
                        path: None,
                        changes: &[],
                    };
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                } else {
                    println!("config ok (no config file found)");
                }
                return Ok(());
            };
            let changes = fix_file(path)?;
            if json {
                let payload = FixOutput {
                    path: Some(path.display().to_string()),
                    changes: &changes,
                };
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else if changes.is_empty() {
                println!("config ok");
            } else {
                println!("fixed {}:", path.display());
                for change in &changes {
                    println!("  {}: {}", change.key, change.action);
                }
            }
        }
    }
    Ok(())
}
