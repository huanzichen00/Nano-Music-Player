//! Desktop window geometry and settings persistence coordination.
//!
//! Resize guards, accepted logical sizes, and progress saves retain their
//! existing controller ownership and lifecycle.
use super::*;

impl Controller {
    pub fn initialize(&mut self, ui: &AppWindow) {
        let (width, height) = self.base_size();
        if let Some(scale) = self
            .settings
            .window_width
            .map(|v| v / width)
            .or_else(|| self.settings.window_height.map(|v| v / height))
        {
            self.state.zoom = scale.clamp(0.75, 3.0);
        }
        self.resize(ui);
        self.refresh_shortcuts(ui);
        self.sync(ui);
    }

    pub(super) fn prepare_portrait_dialog(&mut self, ui: &AppWindow) {
        if self.state.presentation_mode {
            self.set_presentation_mode(ui, false);
        }
        if self.state.landscape_video() {
            // Use Done/Back semantics: pause, preserve list selection, and restore zoom.
            self.execute(ui, Action::Back);
        }
    }

    pub(super) fn base_size(&self) -> (f32, f32) {
        if self.state.presentation_mode {
            presentation::BASE_SIZE
        } else {
            crate::video::orientation::window_base(
                self.state.device_mode,
                self.state.landscape_video(),
            )
        }
    }

    pub(super) fn resize(&mut self, ui: &AppWindow) {
        self.resize_debouncer.reset();
        self.resize_guard.reset();
        self.native_resize.clear_live_resize();
        ui.set_presentation_mode(self.state.presentation_mode);
        ui.set_device_mode(self.state.device_mode);
        ui.set_video_landscape(self.state.landscape_video());
        let (width, height) = self.base_size();
        let scale_factor = ui.window().scale_factor();
        self.last_window_scale_factor = scale_factor;
        // Before our first request there is no pending resize. Synchronous
        // adapters already have a matching native/layout pair (and may draw
        // their MenuBar inside the window); retain that initial inset.
        if self.window_menu_height.is_none() && self.last_window_size.width == 0.0 {
            let native = ui.window().size().to_logical(scale_factor);
            let content = ui.get_content_height();
            if native.width > 0.0 && native.height > 0.0 && content > 0.0 {
                self.window_menu_height = Some((native.height - content).max(0.0));
            }
        }
        let menu_height = self.window_menu_height.unwrap_or(0.0);
        self.last_window_size = slint::LogicalSize::new(
            width * self.state.zoom,
            height * self.state.zoom + menu_height,
        );
        ui.window().set_size(self.last_window_size);
    }

    pub(super) fn maintain_aspect_ratio(&mut self, ui: &AppWindow) {
        let now_ms = self.resize_clock.elapsed().as_millis() as u64;
        let revision = self.native_resize.revision();
        if revision != self.native_resize_revision {
            self.native_resize_revision = revision;
            self.resize_guard.invalidate(now_ms);
            self.resize_debouncer.reset();
        }
        let context = self.native_resize.context(ui.window());
        let scale_factor = ui.window().scale_factor();
        if self.last_window_scale_factor > 0.0
            && (scale_factor - self.last_window_scale_factor).abs() > f32::EPSILON
        {
            // Moving a window between backing scales can report the new DPI
            // before its physical size catches up. Preserve our last logical
            // request instead of treating that intermediate size as a drag.
            // Reapply it directly: mixing the old layout's content height with
            // the new DPI here can also invent extra menu chrome.
            self.last_window_scale_factor = scale_factor;
            self.resize_guard.invalidate(now_ms);
            self.resize_debouncer.reset();
            ui.window().set_size(self.last_window_size);
            return;
        }
        self.last_window_scale_factor = scale_factor;
        let size = ui.window().size().to_logical(scale_factor);
        // Slint's Window item excludes its in-window MenuBar. Measure it once
        // from an acknowledged application request, then keep that inset out
        // of all workspace/window-manager measurements.
        let rounding = (1.0 / scale_factor).max(0.5) + 0.01;
        if self.window_menu_height.is_none()
            && (size.width - self.last_window_size.width).abs() <= rounding
            && (size.height - self.last_window_size.height).abs() <= rounding
        {
            let inset = (size.height - ui.get_content_height()).max(0.0);
            if ui.get_content_height() > 0.0 && inset.is_finite() {
                let (since, previous) = self.menu_measurement.get_or_insert((now_ms, inset));
                if (inset - *previous).abs() > 0.5 {
                    *since = now_ms;
                    *previous = inset;
                } else if now_ms.saturating_sub(*since)
                    >= crate::app::geometry::ResizeDebouncer::QUIET_MS
                {
                    self.window_menu_height = Some(inset);
                    self.menu_measurement = None;
                    if inset > 0.0 {
                        self.resize(ui);
                        return;
                    }
                }
            }
        } else if self.window_menu_height.is_none() {
            self.menu_measurement = None;
        }
        let menu_height = self.window_menu_height.unwrap_or(0.0);
        match self.resize_guard.observe(
            now_ms,
            (size.width, size.height),
            (self.last_window_size.width, self.last_window_size.height),
            context,
        ) {
            crate::app::geometry::ResizeAction::Ignore => {
                self.resize_debouncer.reset();
                return;
            }
            crate::app::geometry::ResizeAction::RestoreRequested => {
                self.resize_debouncer.reset();
                ui.window().set_size(self.last_window_size);
                return;
            }
            crate::app::geometry::ResizeAction::ObserveUserResize => {}
        }
        let content_height = (size.height - menu_height).max(0.0);
        if let Some(zoom) = self.resize_debouncer.observe(
            self.resize_clock.elapsed().as_millis() as u64,
            (size.width, size.height),
            (self.last_window_size.width, self.last_window_size.height),
            content_height,
            self.base_size(),
            menu_height,
        ) {
            self.state.zoom = zoom;
            self.rows_dirty = true;
            self.resize(ui);
        }
    }

    pub fn save_settings(&mut self, _ui: &AppWindow) {
        self.save_video_progress(true);
        self.save_podcast_progress();
        // Persist an accepted logical request, never an intermediate WM frame.
        let size = if self.last_window_size.width > 0.0 && self.last_window_size.height > 0.0 {
            self.last_window_size
        } else {
            // Commands can save settings before initialize() creates the first
            // native frame (including the headless controller tests).
            let (width, height) = self.base_size();
            slint::LogicalSize::new(
                width * self.state.zoom,
                height * self.state.zoom + self.window_menu_height.unwrap_or(0.0),
            )
        };
        let saved_zoom = self.presentation_restore_zoom.unwrap_or(self.state.zoom);
        let (saved_width, saved_height) =
            if self.state.presentation_mode || self.state.landscape_video() {
                crate::video::orientation::saved_size(
                    self.state.device_mode,
                    saved_zoom,
                    self.window_menu_height.unwrap_or(0.0),
                )
            } else {
                (size.width, size.height)
            };
        self.settings = Settings {
            language: self.state.language,
            font: self.state.font,
            always_on_top: self.state.always_on_top,
            smooth_slides: self.state.smooth_slides,
            romaji_visible: self.state.romaji_visible,
            translation_target: self.settings.translation_target,
            translation_visible: self.lyrics.visible(),
            lyrics_visible: self.settings.lyrics_visible,
            zoom: saved_zoom,
            device_mode: self.state.device_mode,
            legacy_monitor_setting: None,
            library_roots: self.settings.library_roots.clone(),
            gapless_playback: self.state.gapless_playback,
            replay_gain_mode: self.state.replay_gain_mode,
            replay_gain_preamp_db: self.state.replay_gain_preamp_db,
            replay_gain_prevent_clipping: self.state.replay_gain_prevent_clipping,
            window_width: Some(saved_width),
            window_height: Some(saved_height),
        };
        if let Err(message) = self.settings.save(&self.directory.join("settings.json")) {
            self.state.notice = format!("Cannot save window settings: {message}");
        }
    }
}
