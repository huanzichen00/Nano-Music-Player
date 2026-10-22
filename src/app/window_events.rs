//! Native resize evidence shared with the window-event observer through Cells.
//! No controller borrow is needed when set_size dispatches a synchronous event.

use super::geometry::ResizeContext;
use slint::winit_030::{WinitWindowAccessor, winit::event::WindowEvent};
use std::cell::Cell;

#[derive(Default)]
pub struct NativeResizeSignals {
    occluded: Cell<bool>,
    revision: Cell<u64>,
    live_resize_seen: Cell<bool>,
}

impl NativeResizeSignals {
    /// Observe without consuming or changing Slint's native event dispatch.
    pub fn observe(&self, window: &slint::Window, event: &WindowEvent) {
        match event {
            WindowEvent::Focused(_) | WindowEvent::ScaleFactorChanged { .. } => self.invalidate(),
            WindowEvent::Occluded(occluded) => {
                self.occluded.set(*occluded);
                self.invalidate();
            }
            WindowEvent::Resized(_) => {
                // Capture this in the event hook: AppKit can finish the drag
                // before the next controller timer observes inLiveResize.
                if native_live_resize(window) == Some(true) {
                    self.live_resize_seen.set(true);
                }
            }
            _ => {}
        }
    }

    pub fn context(&self, window: &slint::Window) -> ResizeContext {
        let focused = window
            .with_winit_window(|native| native.has_focus())
            .unwrap_or(true);
        let seen = self.live_resize_seen.replace(false);
        ResizeContext {
            focused,
            occluded: self.occluded.get(),
            scale_factor: window.scale_factor(),
            native_user_resize: native_live_resize(window).map(|live| live || seen),
        }
    }

    /// Changes even if a focus/occlusion transition returns to its original
    /// state between polls, so stale aspect-resize candidates can be discarded.
    pub fn revision(&self) -> u64 {
        self.revision.get()
    }

    pub fn clear_live_resize(&self) {
        self.live_resize_seen.set(false);
    }

    fn invalidate(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
        self.clear_live_resize();
    }
}

#[cfg(target_os = "macos")]
fn native_live_resize(window: &slint::Window) -> Option<bool> {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    unsafe extern "C" {
        fn nm_desktop_view_is_live_resizing(view: *mut std::ffi::c_void) -> bool;
    }

    window.with_winit_window(|native| {
        let Ok(handle) = native.window_handle() else {
            return false;
        };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return false;
        };
        // SAFETY: winit lends its valid NSView only for this callback. The bridge
        // checks the main thread and reads inLiveResize without retaining it.
        unsafe { nm_desktop_view_is_live_resizing(handle.ns_view.as_ptr()) }
    })
}

#[cfg(not(target_os = "macos"))]
fn native_live_resize(_window: &slint::Window) -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::ComponentHandle;

    #[test]
    fn software_backend_uses_fallback_and_keeps_focus_and_occlusion_revision_evidence() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = crate::ui::AppWindow::new().unwrap();
        let signals = NativeResizeSignals::default();
        let initial = signals.context(ui.window());
        assert!(initial.focused);
        assert!(!initial.occluded);
        assert!(initial.native_user_resize.is_none());
        assert_eq!(signals.revision(), 0);

        signals.live_resize_seen.set(true);
        signals.observe(ui.window(), &WindowEvent::Focused(false));
        assert!(!signals.live_resize_seen.get());
        signals.observe(ui.window(), &WindowEvent::Focused(true));
        assert_eq!(signals.revision(), 2);
        // A software backend has no native focus state. Preserve the documented
        // active fallback instead of treating a synthesized event as authority.
        assert!(signals.context(ui.window()).focused);

        signals.observe(ui.window(), &WindowEvent::Occluded(true));
        assert_eq!(signals.revision(), 3);
        assert!(signals.context(ui.window()).occluded);
        signals.observe(ui.window(), &WindowEvent::Occluded(false));
        assert_eq!(signals.revision(), 4);
        assert!(!signals.context(ui.window()).occluded);
        signals.observe(
            ui.window(),
            &WindowEvent::Resized(slint::winit_030::winit::dpi::PhysicalSize::new(600, 1000)),
        );
        assert_eq!(signals.revision(), 4);
        assert!(!signals.live_resize_seen.get());
        assert!(signals.context(ui.window()).native_user_resize.is_none());
    }

    #[test]
    fn revision_wrap_and_live_resize_clear_are_safe_without_controller_state() {
        let signals = NativeResizeSignals::default();
        signals.revision.set(u64::MAX);
        signals.live_resize_seen.set(true);
        signals.invalidate();
        assert_eq!(signals.revision(), 0);
        assert!(!signals.live_resize_seen.get());
        signals.live_resize_seen.set(true);
        signals.clear_live_resize();
        assert!(!signals.live_resize_seen.get());
    }
}
