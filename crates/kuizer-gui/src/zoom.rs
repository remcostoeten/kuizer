use std::path::PathBuf;

use gpui::{Pixels, px};

use crate::theme;

const STEP: u16 = 10;
const MIN: u16 = 50;
const MAX: u16 = 200;
const DEFAULT: u16 = 100;

/// @name Zoom
/// @description The interface scale as a whole percentage between 50 and 200, changed in steps
/// of ten and kept in a plain text file so the next launch opens at the same size.
///
/// @example
/// let mut zoom = Zoom::for_user();
/// zoom.zoom_in();
pub struct Zoom {
    file: Option<PathBuf>,
    pub percent: u16,
}

impl Zoom {
    pub fn for_user() -> Self {
        Self::load(
            dirs::state_dir()
                .or_else(dirs::data_local_dir)
                .map(|dir| dir.join("kuizer").join("gui-zoom")),
        )
    }

    pub fn load(file: Option<PathBuf>) -> Self {
        let percent = file
            .as_deref()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| text.trim().parse::<u16>().ok())
            .map_or(DEFAULT, clamp);
        Self { file, percent }
    }

    pub fn zoom_in(&mut self) -> bool {
        self.set(self.percent.saturating_add(STEP))
    }

    pub fn zoom_out(&mut self) -> bool {
        self.set(self.percent.saturating_sub(STEP))
    }

    pub fn reset(&mut self) -> bool {
        self.set(DEFAULT)
    }

    pub fn rem_size(&self) -> Pixels {
        px(theme::REM * f32::from(self.percent) / 100.)
    }

    fn set(&mut self, percent: u16) -> bool {
        let percent = clamp(percent);
        if percent == self.percent {
            return false;
        }
        self.percent = percent;
        self.save();
        true
    }

    fn save(&self) {
        let Some(file) = &self.file else {
            return;
        };
        if let Some(parent) = file.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return;
        }
        if let Err(error) = std::fs::write(file, format!("{}\n", self.percent)) {
            eprintln!("kuizer-gui: cannot save the zoom level: {error}");
        }
    }
}

fn clamp(percent: u16) -> u16 {
    let rounded = (percent + STEP / 2) / STEP * STEP;
    rounded.clamp(MIN, MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_stay_within_the_range() {
        let mut zoom = Zoom::load(None);
        assert_eq!(zoom.percent, 100);
        for _ in 0..20 {
            zoom.zoom_in();
        }
        assert_eq!(zoom.percent, 200);
        assert!(!zoom.zoom_in());
        for _ in 0..20 {
            zoom.zoom_out();
        }
        assert_eq!(zoom.percent, 50);
        assert!(!zoom.zoom_out());
        assert!(zoom.reset());
        assert_eq!(zoom.percent, 100);
    }

    #[test]
    fn the_level_survives_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("state").join("gui-zoom");
        let mut zoom = Zoom::load(Some(file.clone()));
        zoom.zoom_in();
        zoom.zoom_in();
        assert_eq!(Zoom::load(Some(file.clone())).percent, 120);
        assert_eq!(Zoom::load(Some(file)).rem_size(), px(19.2));
    }

    #[test]
    fn a_broken_file_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("gui-zoom");
        std::fs::write(&file, "huge").unwrap();
        assert_eq!(Zoom::load(Some(file.clone())).percent, 100);
        std::fs::write(&file, "999").unwrap();
        assert_eq!(Zoom::load(Some(file.clone())).percent, 200);
        std::fs::write(&file, "73").unwrap();
        assert_eq!(Zoom::load(Some(file)).percent, 70);
    }
}
