use std::path::Path;
use std::sync::OnceLock;

/// @name ease_out
/// @description The strong UI ease-out, `cubic-bezier(0.23, 1, 0.32, 1)`, as a GPUI easing.
///
/// @example
/// assert_eq!(ease_out(1.0), 1.0);
pub fn ease_out(progress: f32) -> f32 {
    cubic_bezier(0.23, 1.0, 0.32, 1.0, progress)
}

/// @name cubic_bezier
/// @description Evaluates a CSS `cubic-bezier(x1, y1, x2, y2)` curve at `progress`, solving
/// for the curve parameter with Newton steps and falling back to bisection.
///
/// @example
/// assert!((cubic_bezier(0.0, 0.0, 1.0, 1.0, 0.5) - 0.5).abs() < 1e-4);
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    if progress == 0.0 || progress == 1.0 {
        return progress;
    }
    let curve = |a: f32, b: f32, t: f32| {
        let inverse = 1.0 - t;
        3.0 * inverse * inverse * t * a + 3.0 * inverse * t * t * b + t * t * t
    };
    let slope = |a: f32, b: f32, t: f32| {
        let inverse = 1.0 - t;
        3.0 * inverse * inverse * a + 6.0 * inverse * t * (b - a) + 3.0 * t * t * (1.0 - b)
    };
    let mut t = progress;
    for _ in 0..8 {
        let error = curve(x1, x2, t) - progress;
        if error.abs() < 1e-5 {
            return curve(y1, y2, t);
        }
        let derivative = slope(x1, x2, t);
        if derivative.abs() < 1e-6 {
            break;
        }
        t = (t - error / derivative).clamp(0.0, 1.0);
    }
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    t = progress;
    for _ in 0..32 {
        let x = curve(x1, x2, t);
        if (x - progress).abs() < 1e-5 {
            break;
        }
        if x < progress {
            low = t;
        } else {
            high = t;
        }
        t = (low + high) / 2.0;
    }
    curve(y1, y2, t)
}

/// @name reduced
/// @description Whether the desktop asks for less motion: KDE with its animation speed set
/// to instant, or GTK with animations turned off. Read once per process.
///
/// @example
/// let _ = reduced();
pub fn reduced() -> bool {
    static REDUCED: OnceLock<bool> = OnceLock::new();
    *REDUCED.get_or_init(|| {
        dirs::config_dir().is_some_and(|config| {
            let kde = read(&config.join("kdeglobals"), "AnimationDurationFactor");
            let gtk = read(
                &config.join("gtk-3.0/settings.ini"),
                "gtk-enable-animations",
            );
            kde.is_some_and(|factor| factor.parse::<f32>().is_ok_and(|factor| factor == 0.0))
                || gtk.is_some_and(|enabled| matches!(enabled.as_str(), "0" | "false"))
        })
    })
}

fn read(file: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    setting(&text, key)
}

fn setting(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_bezier_is_the_identity() {
        for step in 0..=10_u8 {
            let progress = f32::from(step) / 10.0;
            assert!((cubic_bezier(0.0, 0.0, 1.0, 1.0, progress) - progress).abs() < 1e-3);
        }
    }

    #[test]
    fn ease_out_front_loads_the_motion() {
        assert!(ease_out(0.0).abs() < f32::EPSILON);
        assert!((ease_out(1.0) - 1.0).abs() < f32::EPSILON);
        assert!(ease_out(0.2) > 0.6);
        let mut previous = 0.0;
        for step in 1..=20_u8 {
            let value = ease_out(f32::from(step) / 20.0);
            assert!(value >= previous);
            previous = value;
        }
    }

    #[test]
    fn settings_are_read_by_key() {
        let text = "[KDE]\nAnimationDurationFactor=0\nSingleClick=false\n";
        assert_eq!(
            setting(text, "AnimationDurationFactor").as_deref(),
            Some("0")
        );
        assert_eq!(setting(text, "Missing"), None);
    }
}
