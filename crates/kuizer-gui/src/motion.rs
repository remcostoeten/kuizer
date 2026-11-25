use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static OFF: AtomicBool = AtomicBool::new(false);

/// The longest a state change may take to animate.
pub const LIMIT: Duration = Duration::from_millis(150);

/// One step of a state change: a row blending between delete-flow states, the summary bar
/// rising in, the space map zooming into a folder.
pub const STEP: Duration = Duration::from_millis(140);

/// How long a tooltip takes to fade and rise in.
pub const TOOLTIP_IN: Duration = Duration::from_millis(125);

/// How long the pointer rests on an element before its tooltip opens.
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(250);

/// How soon after one tooltip closes the next opens at once, without delay or fade.
pub const TOOLTIP_WARM: Duration = Duration::from_millis(300);

/// One beat of the pulse on a chart that is still measuring. Progress loops show ongoing work,
/// not a state change, so `LIMIT` does not apply to them.
pub const PULSE: Duration = Duration::from_millis(1_400);

/// One pass of the sweep along a bar that is still measuring.
pub const SWEEP: Duration = Duration::from_millis(1_100);

/// The opacity the space map starts from when it zooms into or out of a folder.
pub const ZOOM_FROM: f32 = 0.35;

const _: () =
    assert!(STEP.as_millis() <= LIMIT.as_millis() && TOOLTIP_IN.as_millis() <= LIMIT.as_millis());

/// @name fade_from
/// @description The opacity at `progress` of a fade that starts at `from` and ends opaque.
///
/// @example
/// assert_eq!(fade_from(0.5, 1.0), 1.0);
pub fn fade_from(from: f32, progress: f32) -> f32 {
    from + (1.0 - from) * progress
}

/// @name ease_out
/// @description The strong UI ease-out, `cubic-bezier(0.23, 1, 0.32, 1)`, as a GPUI easing.
///
/// @example
/// assert_eq!(ease_out(1.0), 1.0);
pub fn ease_out(progress: f32) -> f32 {
    cubic_bezier(0.23, 1.0, 0.32, 1.0, progress)
}

/// @name ease_in_out
/// @description A symmetric ease, `cubic-bezier(0.65, 0, 0.35, 1)`, for motion that loops.
///
/// @example
/// assert_eq!(ease_in_out(0.0), 0.0);
pub fn ease_in_out(progress: f32) -> f32 {
    cubic_bezier(0.65, 0.0, 0.35, 1.0, progress)
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

/// @name configure
/// @description Applies `appearance.motion` from the config: `false` stills every animation.
///
/// @example
/// configure(config.appearance.motion);
pub fn configure(enabled: bool) {
    OFF.store(!enabled, Ordering::Relaxed);
}

/// @name reduced
/// @description Whether animations should stand still because `appearance.motion` is off.
///
/// @example
/// let _ = reduced();
pub fn reduced() -> bool {
    OFF.load(Ordering::Relaxed)
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
}
