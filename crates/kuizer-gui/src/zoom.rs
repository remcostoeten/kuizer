use std::path::{Path, PathBuf};

use gpui::{Pixels, px};
use kuizer::config::{self, Config, DEFAULT_ZOOM, MAX_ZOOM, MIN_ZOOM, SettingValue};

use crate::theme;

pub const STEP: u16 = 10;

/// @name snap
/// @description Rounds an interface scale to the nearest step of ten inside the allowed range,
/// so `appearance.zoom = 73` opens at 70 percent.
///
/// @example
/// assert_eq!(snap(73), 70);
pub fn snap(percent: u16) -> u16 {
    let rounded = percent.saturating_add(STEP / 2) / STEP * STEP;
    rounded.clamp(MIN_ZOOM, MAX_ZOOM)
}

pub fn zoomed_in(percent: u16) -> u16 {
    snap(snap(percent).saturating_add(STEP))
}

pub fn zoomed_out(percent: u16) -> u16 {
    snap(snap(percent).saturating_sub(STEP))
}

pub fn rem_size(percent: u16) -> Pixels {
    px(theme::REM * f32::from(snap(percent)) / 100.)
}

pub fn legacy_file() -> Option<PathBuf> {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .map(|dir| dir.join("kuizer").join("gui-zoom"))
}

/// @name migrate
/// @description Moves the interface scale from the old GUI state file into `appearance.zoom`.
/// A scale the config file already sets wins. The old file is removed once the config file holds
/// the scale; when the config file cannot be written, the old scale still applies to this session
/// and the move is tried again on the next launch.
///
/// @example
/// zoom::migrate(&mut config, config::default_path().as_deref(), zoom::legacy_file().as_deref());
pub fn migrate(config: &mut Config, config_file: Option<&Path>, legacy: Option<&Path>) {
    let (Some(config_file), Some(legacy)) = (config_file, legacy) else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(legacy) else {
        return;
    };
    let moved = match config::has_setting(config_file, "appearance", "zoom") {
        Ok(true) => true,
        Ok(false) => match text.trim().parse::<u16>().map(snap) {
            Ok(DEFAULT_ZOOM) | Err(_) => true,
            Ok(percent) => {
                config.appearance.zoom = percent;
                let value = SettingValue::Integer(i64::from(percent));
                config::set_setting(config_file, "appearance", "zoom", value).is_ok()
            }
        },
        Err(_) => {
            if let Ok(percent) = text.trim().parse::<u16>() {
                config.appearance.zoom = snap(percent);
            }
            false
        }
    };
    if moved {
        let _ = std::fs::remove_file(legacy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_stay_within_the_range() {
        let mut percent = DEFAULT_ZOOM;
        for _ in 0..20 {
            percent = zoomed_in(percent);
        }
        assert_eq!(percent, MAX_ZOOM);
        for _ in 0..20 {
            percent = zoomed_out(percent);
        }
        assert_eq!(percent, MIN_ZOOM);
        assert_eq!(snap(73), 70);
        assert_eq!(rem_size(120), px(19.2));
    }

    #[test]
    fn the_old_state_file_moves_into_the_config_once() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.toml");
        let legacy = dir.path().join("gui-zoom");
        std::fs::write(&file, "# mine\n[appearance]\nmotion = false\n").unwrap();
        std::fs::write(&legacy, "130\n").unwrap();
        let mut config = Config::default();
        migrate(&mut config, Some(&file), Some(&legacy));
        assert_eq!(config.appearance.zoom, 130);
        assert!(!legacy.exists());
        let loaded = Config::load(Some(&file)).unwrap().config;
        assert_eq!(loaded.appearance.zoom, 130);
        assert!(!loaded.appearance.motion);

        std::fs::write(&legacy, "60\n").unwrap();
        let mut config = loaded;
        migrate(&mut config, Some(&file), Some(&legacy));
        assert_eq!(config.appearance.zoom, 130, "the config file wins");
        assert!(!legacy.exists());
    }
}
