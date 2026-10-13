//! Input recognition for library navigation, kept independent of UI state.

use std::time::Duration;

const WHEEL_THRESHOLD: f32 = 48.0;
const WHEEL_DOMINANCE: f32 = 1.35;
const WHEEL_TIMEOUT: Duration = Duration::from_millis(450);
const POINTER_THRESHOLD: f32 = 32.0;
const POINTER_DOMINANCE: f32 = 1.4;

/// Accumulates horizontal scroll deltas into a single Back decision.
#[derive(Debug, Default)]
pub struct HorizontalScrollBack {
    horizontal: f32,
    vertical: f32,
    elapsed: Duration,
}

impl HorizontalScrollBack {
    /// Add one native scroll event. Positive `delta_x` means rightward motion.
    pub fn push(&mut self, delta_x: f32, delta_y: f32, elapsed: Duration) -> bool {
        if elapsed > WHEEL_TIMEOUT {
            self.reset();
        }
        self.elapsed += elapsed;
        self.horizontal += delta_x;
        self.vertical += delta_y;

        if self.horizontal.abs() >= WHEEL_THRESHOLD {
            let qualifies = self.horizontal > 0.0
                && self.horizontal.abs() >= self.vertical.abs() * WHEEL_DOMINANCE;
            self.reset();
            return qualifies;
        }
        if self.vertical.abs() >= WHEEL_THRESHOLD
            || self.horizontal.signum() != 0.0
                && self.vertical.abs() > self.horizontal.abs() / WHEEL_DOMINANCE
        {
            self.reset();
        }
        false
    }

    pub fn reset(&mut self) {
        self.horizontal = 0.0;
        self.vertical = 0.0;
        self.elapsed = Duration::ZERO;
    }
}

/// Tracks a pointer drag over successive frames and recognizes a deliberate right swipe.
#[derive(Debug, Default)]
pub struct PointerBack {
    start: Option<(f32, f32)>,
}

impl PointerBack {
    pub fn begin(&mut self, x: f32, y: f32) {
        self.start = Some((x, y));
    }

    /// Returns true only once the gesture ends with a sufficiently dominant rightward drag.
    pub fn finish(&mut self, x: f32, y: f32) -> bool {
        let Some((start_x, start_y)) = self.start.take() else {
            return false;
        };
        let dx = x - start_x;
        let dy = y - start_y;
        dx >= POINTER_THRESHOLD && dx >= dy.abs() * POINTER_DOMINANCE
    }

    pub fn cancel(&mut self) {
        self.start = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_accumulates_across_frames_and_resets_after_back() {
        let mut recognizer = HorizontalScrollBack::default();
        assert!(!recognizer.push(20.0, 0.0, Duration::from_millis(16)));
        assert!(!recognizer.push(20.0, 0.0, Duration::from_millis(16)));
        assert!(recognizer.push(12.0, 0.0, Duration::from_millis(16)));
        assert!(!recognizer.push(20.0, 0.0, Duration::from_millis(16)));
    }

    #[test]
    fn vertical_diagonal_leftward_and_short_scrolls_do_not_go_back() {
        let mut recognizer = HorizontalScrollBack::default();
        assert!(!recognizer.push(20.0, 0.0, Duration::ZERO));
        assert!(!recognizer.push(0.0, 40.0, Duration::from_millis(5)));
        assert!(!recognizer.push(50.0, 40.0, Duration::ZERO));
        assert!(!recognizer.push(-80.0, 0.0, Duration::ZERO));
        assert!(!recognizer.push(30.0, 0.0, Duration::ZERO));
    }

    #[test]
    fn scroll_timeout_clears_old_delta() {
        let mut recognizer = HorizontalScrollBack::default();
        assert!(!recognizer.push(30.0, 0.0, Duration::ZERO));
        assert!(!recognizer.push(30.0, 0.0, Duration::from_millis(451)));
        assert!(recognizer.push(20.0, 0.0, Duration::from_millis(1)));
    }

    #[test]
    fn pointer_requires_real_dominant_rightward_drag_and_clears_state() {
        let mut recognizer = PointerBack::default();
        recognizer.begin(10.0, 10.0);
        assert!(recognizer.finish(52.0, 18.0));
        assert!(!recognizer.finish(90.0, 10.0));
        recognizer.begin(10.0, 10.0);
        assert!(!recognizer.finish(50.0, 50.0));
        recognizer.begin(10.0, 10.0);
        assert!(!recognizer.finish(39.0, 10.0));
        recognizer.begin(10.0, 10.0);
        recognizer.cancel();
        assert!(!recognizer.finish(90.0, 10.0));
    }
}
