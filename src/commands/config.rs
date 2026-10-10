use std::fs;

use super::AppError;
use crate::cli::ConfigAction;
use crate::config::{Config, Loaded, default_path};

pub fn run(loaded: &Loaded, action: Option<ConfigAction>, json: bool) -> Result<(), AppError> {
    match action.unwrap_or(ConfigAction::Show) {
        ConfigAction::Show if json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&loaded.config).unwrap_or_default()
            );
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
            fs::write(&path, Config::default().to_toml())?;
            println!("wrote {}", path.display());
        }
        ConfigAction::Check => {
            loaded.config.validate()?;
            println!("config ok");
        }
    }
    Ok(())
}
