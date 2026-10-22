//! Pure window-resize geometry used to keep the player at its logical aspect ratio.

/// Native context used to distinguish a pointer resize from window-manager and
/// backing-store changes. `Some` is for platforms with a reliable live-resize
/// signal; `None` preserves the ordinary resize behavior on other platforms.
#[derive(Debug, Clone, Copy)]
pub struct ResizeContext {
    pub focused: bool,
    pub occluded: bool,
    pub scale_factor: f32,
    pub native_user_resize: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeAction {
    /// Discard any pending aspect/zoom candidate without changing saved zoom.
    Ignore,
    /// The existing aspect debouncer may measure this user resize.
    ObserveUserResize,
    /// Reapply the last logical size requested by the app, without deriving a
    /// zoom or menu inset from this native measurement.
    RestoreRequested,
}

/// Tracks resize intent independently of the aspect correction calculation.
///
/// Size alone cannot identify a user gesture: a workspace switch or tiling
/// window manager may send an equally stable, even proportional, resize. A
/// native live-resize signal provides that distinction on macOS. Focus,
/// occlusion, and backing-scale changes additionally quarantine measurements
/// until the previous application-requested size has been restored.
#[derive(Default)]
pub struct ResizeGuard {
    context: Option<(bool, bool, f32)>,
    user_resize: bool,
    interrupted_at: Option<u64>,
    restore_candidate: Option<(u64, (f32, f32), bool)>,
}

impl ResizeGuard {
    /// Call for native focus/occlusion/scale events even when their final state
    /// returns to the old state between polls. This invalidates a pending drag.
    pub fn invalidate(&mut self, now_ms: u64) {
        self.user_resize = false;
        self.interrupted_at = Some(now_ms);
        self.restore_candidate = None;
    }

    /// Finish an accepted drag or discard it after an app-driven mode, zoom, or
    /// orientation change. Keep the native context for the next observation.
    pub fn reset(&mut self) {
        self.user_resize = false;
        self.interrupted_at = None;
        self.restore_candidate = None;
    }

    pub fn observe(
        &mut self,
        now_ms: u64,
        size: (f32, f32),
        requested: (f32, f32),
        context: ResizeContext,
    ) -> ResizeAction {
        if ![
            size.0,
            size.1,
            requested.0,
            requested.1,
            context.scale_factor,
        ]
        .into_iter()
        .all(|value| value.is_finite() && value > 0.0)
        {
            self.invalidate(now_ms);
            return ResizeAction::Ignore;
        }
        if self.context.is_some_and(|(focused, occluded, scale)| {
            focused != context.focused
                || occluded != context.occluded
                || (scale - context.scale_factor).abs() > 0.001
        }) {
            self.invalidate(now_ms);
        }
        self.context = Some((context.focused, context.occluded, context.scale_factor));
        if !context.focused || context.occluded {
            self.invalidate(now_ms);
            return ResizeAction::Ignore;
        }

        // Native sizes are whole physical pixels. A fractional logical request
        // can round by almost one physical pixel at the current backing scale.
        let rounding = (1.0 / context.scale_factor).max(0.5) + 0.01;
        let matches_requested =
            (size.0 - requested.0).abs() <= rounding && (size.1 - requested.1).abs() <= rounding;
        if context.native_user_resize == Some(true) {
            // Latch until the quiet-period aspect correction runs: AppKit ends
            // live resize before that debouncer applies the final measurement.
            self.user_resize = !matches_requested;
            self.interrupted_at = None;
            self.restore_candidate = None;
            return ResizeAction::ObserveUserResize;
        }
        if matches_requested {
            self.reset();
            return ResizeAction::Ignore;
        }
        // Platforms without native gesture evidence keep their existing resize
        // behavior, including a drag which itself focuses an inactive window.
        if self.user_resize || context.native_user_resize.is_none() {
            return ResizeAction::ObserveUserResize;
        }

        let candidate = self.restore_candidate.get_or_insert((now_ms, size, false));
        if !close_size(candidate.1, size) {
            *candidate = (now_ms, size, false);
        }
        let since = candidate.0.max(self.interrupted_at.unwrap_or_default());
        if !candidate.2 && now_ms.saturating_sub(since) >= ResizeDebouncer::QUIET_MS {
            // Issue one restoration per stable native size, avoiding a repeated
            // set_size loop if the window manager refuses the requested size.
            candidate.2 = true;
            return ResizeAction::RestoreRequested;
        }
        ResizeAction::Ignore
    }
}

fn close_size(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() <= 0.5 && (a.1 - b.1).abs() <= 0.5
}

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

    const REQUESTED: (f32, f32) = (480.0, 884.0);

    fn native_context(user_resize: Option<bool>) -> ResizeContext {
        ResizeContext {
            focused: true,
            occluded: false,
            scale_factor: 2.0,
            native_user_resize: user_resize,
        }
    }

    #[test]
    fn focused_window_manager_resize_does_not_become_user_zoom() {
        let mut guard = ResizeGuard::default();
        let context = native_context(Some(false));
        // A proportional resize is not evidence that the pointer caused it.
        let tiled = (600.0, 1100.0);
        assert_eq!(
            guard.observe(0, tiled, REQUESTED, context),
            ResizeAction::Ignore
        );
        assert_eq!(
            guard.observe(140, tiled, REQUESTED, context),
            ResizeAction::RestoreRequested
        );
        assert_eq!(
            guard.observe(500, tiled, REQUESTED, context),
            ResizeAction::Ignore
        );
        assert_eq!(
            guard.observe(1000, tiled, REQUESTED, context),
            ResizeAction::Ignore
        );
    }

    #[test]
    fn physical_pixel_rounding_acknowledges_fractional_logical_requests() {
        for scale_factor in [0.5_f32, 1.0, 1.5, 2.0] {
            let mut guard = ResizeGuard::default();
            let context = ResizeContext {
                scale_factor,
                ..native_context(None)
            };
            let requested = (370.3, 714.77);
            let actual = (
                (requested.0 * scale_factor).floor() / scale_factor,
                (requested.1 * scale_factor).floor() / scale_factor,
            );
            guard.invalidate(0);
            assert_eq!(
                guard.observe(150, actual, requested, context),
                ResizeAction::Ignore
            );
            assert_eq!(
                guard.observe(200, (430.0, actual.1), requested, context),
                ResizeAction::ObserveUserResize,
                "scale {scale_factor}"
            );
        }
    }

    #[test]
    fn native_user_drag_is_latched_through_quiet_period_after_live_resize_ends() {
        let mut guard = ResizeGuard::default();
        let dragged = (600.0, 884.0);
        assert_eq!(
            guard.observe(0, dragged, REQUESTED, native_context(Some(true))),
            ResizeAction::ObserveUserResize
        );
        assert_eq!(
            guard.observe(150, dragged, REQUESTED, native_context(Some(false))),
            ResizeAction::ObserveUserResize
        );
        guard.reset();
        assert_eq!(
            guard.observe(200, dragged, (600.0, 1100.0), native_context(Some(false))),
            ResizeAction::Ignore
        );
    }

    #[test]
    fn focus_loss_cancels_drag_and_waits_until_reactivation_before_restoring() {
        let mut guard = ResizeGuard::default();
        let resized = (600.0, 1100.0);
        guard.observe(0, resized, REQUESTED, native_context(Some(true)));
        let inactive = ResizeContext {
            focused: false,
            ..native_context(Some(false))
        };
        assert_eq!(
            guard.observe(50, resized, REQUESTED, inactive),
            ResizeAction::Ignore
        );
        assert_eq!(
            guard.observe(1000, resized, REQUESTED, inactive),
            ResizeAction::Ignore
        );
        assert_eq!(
            guard.observe(1050, resized, REQUESTED, native_context(Some(false))),
            ResizeAction::Ignore
        );
        assert_eq!(
            guard.observe(1190, resized, REQUESTED, native_context(Some(false))),
            ResizeAction::RestoreRequested
        );
    }

    #[test]
    fn occlusion_quarantines_workspace_sizes_without_requiring_focus_loss() {
        let mut guard = ResizeGuard::default();
        guard.observe(0, REQUESTED, REQUESTED, native_context(Some(false)));
        let hidden = ResizeContext {
            occluded: true,
            ..native_context(Some(false))
        };
        let tiled = (800.0, 1000.0);
        assert_eq!(
            guard.observe(10, tiled, REQUESTED, hidden),
            ResizeAction::Ignore
        );
        assert_eq!(
            guard.observe(20, tiled, REQUESTED, native_context(Some(false))),
            ResizeAction::Ignore
        );
        assert_eq!(
            guard.observe(160, tiled, REQUESTED, native_context(Some(false))),
            ResizeAction::RestoreRequested
        );
        // The quarantined size cannot later enter the fallback user path merely
        // because enough time has elapsed since returning to the workspace.
        assert_eq!(
            guard.observe(2000, tiled, REQUESTED, native_context(Some(false))),
            ResizeAction::Ignore
        );
        guard.observe(2050, REQUESTED, REQUESTED, native_context(Some(false)));
        assert_eq!(
            guard.observe(2100, (500.0, 884.0), REQUESTED, native_context(Some(true))),
            ResizeAction::ObserveUserResize
        );
    }

    #[test]
    fn event_invalidation_survives_focus_changes_between_poll_snapshots() {
        let mut guard = ResizeGuard::default();
        guard.observe(0, REQUESTED, REQUESTED, native_context(Some(false)));
        guard.invalidate(10);
        guard.invalidate(20);
        let tiled = (800.0, 1000.0);
        assert_eq!(
            guard.observe(50, tiled, REQUESTED, native_context(Some(false))),
            ResizeAction::Ignore
        );
        assert_eq!(
            guard.observe(190, tiled, REQUESTED, native_context(Some(false))),
            ResizeAction::RestoreRequested
        );
    }

    #[test]
    fn backing_scale_change_does_not_persist_intermediate_logical_size() {
        let mut guard = ResizeGuard::default();
        guard.observe(0, REQUESTED, REQUESTED, native_context(Some(false)));
        let changed = ResizeContext {
            scale_factor: 1.0,
            ..native_context(Some(false))
        };
        let transitional = (960.0, 1768.0);
        assert_eq!(
            guard.observe(50, transitional, REQUESTED, changed),
            ResizeAction::Ignore
        );
        assert_eq!(
            guard.observe(190, transitional, REQUESTED, changed),
            ResizeAction::RestoreRequested
        );
    }

    #[test]
    fn actual_user_resize_can_begin_immediately_after_workspace_reactivation() {
        let mut guard = ResizeGuard::default();
        guard.invalidate(0);
        assert_eq!(
            guard.observe(10, (500.0, 884.0), REQUESTED, native_context(Some(true))),
            ResizeAction::ObserveUserResize
        );
    }

    #[test]
    fn fallback_platform_accepts_a_drag_that_reactivates_the_window() {
        let mut guard = ResizeGuard::default();
        guard.invalidate(0);
        assert_eq!(
            guard.observe(10, (500.0, 884.0), REQUESTED, native_context(None)),
            ResizeAction::ObserveUserResize
        );
    }

    #[test]
    fn system_resize_stream_waits_for_quiet_and_does_not_fight_every_event() {
        let mut guard = ResizeGuard::default();
        let context = native_context(Some(false));
        guard.observe(0, (700.0, 1000.0), REQUESTED, context);
        assert_eq!(
            guard.observe(100, (800.0, 1000.0), REQUESTED, context),
            ResizeAction::Ignore
        );
        assert_eq!(
            guard.observe(200, (800.0, 1000.0), REQUESTED, context),
            ResizeAction::Ignore
        );
        assert_eq!(
            guard.observe(240, (800.0, 1000.0), REQUESTED, context),
            ResizeAction::RestoreRequested
        );
    }

    #[test]
    fn invalid_or_hidden_native_sizes_never_authorize_zoom() {
        let mut guard = ResizeGuard::default();
        let context = native_context(Some(true));
        for size in [
            (0.0, 884.0),
            (480.0, 0.0),
            (f32::NAN, 884.0),
            (480.0, f32::INFINITY),
        ] {
            assert_eq!(
                guard.observe(0, size, REQUESTED, context),
                ResizeAction::Ignore
            );
        }
        assert_eq!(
            guard.observe(0, REQUESTED, (0.0, 0.0), context),
            ResizeAction::Ignore
        );
        assert_eq!(
            guard.observe(
                0,
                REQUESTED,
                REQUESTED,
                ResizeContext {
                    occluded: true,
                    ..context
                }
            ),
            ResizeAction::Ignore
        );
    }

    #[test]
    fn platforms_without_live_resize_signal_keep_existing_drag_behavior() {
        let mut guard = ResizeGuard::default();
        assert_eq!(
            guard.observe(0, (600.0, 884.0), REQUESTED, native_context(None)),
            ResizeAction::ObserveUserResize
        );
        guard.reset();
        let landscape = (600.0, 296.0);
        assert_eq!(
            guard.observe(50, (650.0, 296.0), landscape, native_context(None)),
            ResizeAction::ObserveUserResize
        );
    }

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
