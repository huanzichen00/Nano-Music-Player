//! Pure window-resize geometry used to keep the player at its logical aspect ratio.

/// Wait for native resize notifications to settle before correcting the aspect ratio.
/// The controller polls faster than typical window-manager resize sequences complete, so an
/// immediate `set_size` from each poll competes with the pointer drag. This state coalesces the
/// stream into one correction after a short quiet interval.
#[derive(Default)]
pub struct ResizeDebouncer {
    candidate: Option<(u64, (f32, f32), f32)>,
}

impl ResizeDebouncer {
    pub const QUIET_MS: u64 = 140;

    /// Discard pending measurements after an application-driven size or orientation change.
    pub fn reset(&mut self) {
        self.candidate = None;
    }

    /// Return the zoom to apply after the observed resize has remained still for `QUIET_MS`.
    ///
    /// `previous_window` is the last size requested by the application. `content_height`
    /// excludes native menu chrome, which keeps the vertical drag estimate independent of menus.
    pub fn observe(
        &mut self,
        now_ms: u64,
        size: (f32, f32),
        previous_window: (f32, f32),
        content_height: f32,
        base: (f32, f32),
        menu_height: f32,
    ) -> Option<f32> {
        let (width, height) = base;
        if ![
            size.0,
            size.1,
            previous_window.0,
            previous_window.1,
            content_height,
            width,
            height,
            menu_height,
        ]
        .into_iter()
        .all(f32::is_finite)
            || width <= 0.0
            || height <= 0.0
            || size.0 <= 0.0
            || content_height <= 0.0
            || menu_height < 0.0
        {
            self.candidate = None;
            return None;
        }

        let moved = self.candidate.is_none_or(|(_, old_size, old_content)| {
            (size.0 - old_size.0).abs() > 0.5
                || (size.1 - old_size.1).abs() > 0.5
                || (content_height - old_content).abs() > 0.5
        });
        if moved {
            self.candidate = Some((now_ms, size, content_height));
            return None;
        }
        let (since, _, _) = self.candidate?;
        if now_ms.saturating_sub(since) < Self::QUIET_MS {
            return None;
        }

        let desired_content_height = size.0 / width * height;
        let mismatch = (content_height - desired_content_height).abs();
        if mismatch < 1.0
            && (size.0 - previous_window.0).abs() < 1.0
            && (size.1 - previous_window.1).abs() < 1.0
        {
            self.candidate = None;
            return None;
        }

        let prior_content_height = (previous_window.1 - menu_height).max(0.0);
        let horizontal_drag = (size.0 - previous_window.0).abs() / width;
        let vertical_drag = (content_height - prior_content_height).abs() / height;
        let zoom = if horizontal_drag >= vertical_drag {
            size.0 / width
        } else {
            content_height / height
        };
        self.candidate = None;
        Some(zoom.clamp(0.75, 3.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (f32, f32) = (240.0, 432.0);

    #[test]
    fn stable_ratio_does_not_request_a_correction() {
        let mut debounce = ResizeDebouncer::default();
        assert_eq!(
            debounce.observe(0, (480.0, 884.0), (480.0, 884.0), 864.0, SCREEN, 20.0),
            None
        );
        assert_eq!(
            debounce.observe(150, (480.0, 884.0), (480.0, 884.0), 864.0, SCREEN, 20.0),
            None
        );
    }

    #[test]
    fn width_drag_keeps_width_and_restores_screen_ratio() {
        let mut debounce = ResizeDebouncer::default();
        assert_eq!(
            debounce.observe(0, (600.0, 884.0), (480.0, 884.0), 864.0, SCREEN, 20.0),
            None
        );
        assert_eq!(
            debounce.observe(140, (600.0, 884.0), (480.0, 884.0), 864.0, SCREEN, 20.0),
            Some(2.5)
        );
    }

    #[test]
    fn height_drag_uses_content_height_and_ignores_native_menu_chrome() {
        let mut debounce = ResizeDebouncer::default();
        assert_eq!(
            debounce.observe(0, (480.0, 1004.0), (480.0, 884.0), 984.0, SCREEN, 20.0),
            None
        );
        assert_eq!(
            debounce.observe(140, (480.0, 1004.0), (480.0, 884.0), 984.0, SCREEN, 20.0),
            Some(2.2777777)
        );
    }

    #[test]
    fn invalid_native_dimensions_are_ignored() {
        let mut debounce = ResizeDebouncer::default();
        assert_eq!(
            debounce.observe(0, (f32::NAN, 500.0), (480.0, 884.0), 480.0, SCREEN, 20.0),
            None
        );
        assert_eq!(
            debounce.observe(0, (480.0, 500.0), (480.0, 884.0), 480.0, (0.0, 432.0), 20.0),
            None
        );
    }

    #[test]
    fn proportional_drag_updates_zoom_even_without_aspect_mismatch() {
        let mut debounce = ResizeDebouncer::default();
        assert_eq!(
            debounce.observe(0, (600.0, 1100.0), (480.0, 884.0), 1080.0, SCREEN, 20.0),
            None
        );
        assert_eq!(
            debounce.observe(150, (600.0, 1100.0), (480.0, 884.0), 1080.0, SCREEN, 20.0),
            Some(2.5)
        );
    }

    #[test]
    fn mode_change_discards_pending_drag_and_uses_new_hardware_orientation() {
        let mut debounce = ResizeDebouncer::default();
        assert_eq!(
            debounce.observe(0, (600.0, 884.0), (480.0, 884.0), 864.0, SCREEN, 20.0),
            None
        );
        debounce.reset();
        let landscape = (296.0 * 76.5 / 39.6, 296.0);
        assert_eq!(
            debounce.observe(
                200,
                (1200.0, 612.0),
                (landscape.0 * 2.0, 612.0),
                592.0,
                landscape,
                20.0
            ),
            None
        );
        assert_eq!(
            debounce.observe(
                350,
                (1200.0, 612.0),
                (landscape.0 * 2.0, 612.0),
                592.0,
                landscape,
                20.0
            ),
            Some(1200.0 / landscape.0)
        );
        debounce.reset();
        assert_eq!(
            debounce.observe(500, (100.0, 452.0), (240.0, 452.0), 432.0, SCREEN, 20.0),
            None
        );
        assert_eq!(
            debounce.observe(650, (100.0, 452.0), (240.0, 452.0), 432.0, SCREEN, 20.0),
            Some(0.75)
        );
    }

    #[test]
    fn continued_drag_resets_quiet_timer_and_emits_once() {
        let mut debounce = ResizeDebouncer::default();
        assert_eq!(
            debounce.observe(0, (600.0, 884.0), (480.0, 884.0), 864.0, SCREEN, 20.0),
            None
        );
        assert_eq!(
            debounce.observe(100, (620.0, 884.0), (480.0, 884.0), 864.0, SCREEN, 20.0),
            None
        );
        assert_eq!(
            debounce.observe(200, (620.0, 884.0), (480.0, 884.0), 864.0, SCREEN, 20.0),
            None
        );
        assert_eq!(
            debounce.observe(240, (620.0, 884.0), (480.0, 884.0), 864.0, SCREEN, 20.0),
            Some(620.0 / 240.0)
        );
        assert_eq!(
            debounce.observe(400, (620.0, 884.0), (620.0, 884.0), 1116.0, SCREEN, 20.0),
            None
        );
    }
}
