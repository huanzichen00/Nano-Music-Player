//! Temporary horizontal lyrics display; playback and saved portrait geometry stay intact.
use super::*;

pub(super) const BASE_SIZE: (f32, f32) = (540.0, 180.0);

impl Controller {
    pub(super) fn set_presentation_mode(&mut self, ui: &AppWindow, enabled: bool) {
        if self.state.presentation_mode == enabled {
            return;
        }
        if enabled {
            self.presentation_restore_zoom = Some(self.state.zoom);
            self.state.zoom = self.presentation_zoom;
        } else {
            self.presentation_zoom = self.state.zoom;
            if let Some(zoom) = self.presentation_restore_zoom.take() {
                self.state.zoom = zoom;
            }
        }
        self.state.presentation_mode = enabled;
        self.resize(ui);
        ui.invoke_focus_screen();
    }

    pub(super) fn handle_presentation_action(&mut self, ui: &AppWindow, action: Action) -> bool {
        if action == Action::PresentationMode {
            if self.state.presentation_mode {
                self.set_presentation_mode(ui, false);
            } else if !self.dialog_open
                && !self.state.reference_open
                && !ui.get_practice_open()
                && !ui.get_track_details_open()
                && !ui.get_native_status_open()
                && !ui.get_genius_preview_open()
                && !ui.get_playlist_editor_open()
                && !ui.get_shortcut_editor_open()
                && !ui.get_deepseek_settings_open()
                && !ui.get_podcast_subscribe_open()
                && !self.state.video_active
                && self.state.podcasts.active.is_none()
            {
                self.set_presentation_mode(ui, true);
            }
            self.sync(ui);
            return true;
        }
        if !self.state.presentation_mode {
            return false;
        }
        if action == Action::Back {
            self.set_presentation_mode(ui, false);
            self.sync(ui);
            return true;
        }
        // Opening application/navigation UI returns to its regular geometry.
        // Playback, zoom and the existing on-top preference stay usable in place.
        if action == Action::PracticeMode
            || (action.category() != Category::Playback
                && !matches!(
                    action,
                    Action::ZoomIn | Action::ZoomOut | Action::ToggleAlwaysOnTop
                ))
        {
            self.set_presentation_mode(ui, false);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{Track, lyrics::Lyrics};

    #[test]
    fn presentation_preserves_playback_navigation_and_saved_portrait_geometry() {
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        let ui = AppWindow::new().unwrap();
        let mut c = Controller::new(directory.path().to_owned());
        c.state.tracks.push(Track {
            path: directory.path().join("song.wav"),
            title: "Song".into(),
            artist: String::new(),
            album: String::new(),
            genre: String::new(),
            number: 1,
            duration: 120.0,
            artwork: vec![],
            lyrics: Lyrics::parse("[00:01]First\n[00:02]Second"),
        });
        c.state.current = Some(0);
        c.state.position = 12.0;
        c.state.paused = false;
        c.state.status = PlaybackStatus::Playing;
        c.state.queue.set(vec![0], 0, false);
        c.state.navigation.open(Screen::Songs(Filter::All));
        c.initialize(&ui);
        let old_zoom = c.state.zoom;
        let old_size = c.last_window_size;
        let old_screen = c.state.navigation.current().screen.clone();
        c.execute(&ui, Action::PresentationMode);
        assert!(c.state.presentation_mode && ui.get_presentation_mode());
        assert_eq!(c.base_size(), BASE_SIZE);
        assert_eq!(c.state.position, 12.0);
        assert!(!c.state.paused);
        assert_eq!(c.state.queue.order, vec![0]);
        assert_eq!(c.state.navigation.current().screen, old_screen);
        c.execute(&ui, Action::ZoomIn);
        let display_zoom = c.state.zoom;
        assert!(display_zoom > 1.0);
        c.save_settings(&ui);
        let saved = Settings::load(&directory.path().join("settings.json")).unwrap();
        assert!((saved.zoom - old_zoom).abs() < 0.001);
        assert!((saved.window_width.unwrap() - old_size.width).abs() < 0.01);
        assert!((saved.window_height.unwrap() - old_size.height).abs() < 0.01);
        c.execute(&ui, Action::Back);
        assert!(!c.state.presentation_mode);
        assert!(!ui.get_presentation_mode());
        assert_eq!(c.state.zoom, old_zoom);
        assert_eq!(c.last_window_size, old_size);
        assert!(!c.state.paused);
        assert_eq!(c.state.navigation.current().screen, old_screen);
        c.execute(&ui, Action::PresentationMode);
        assert_eq!(
            c.state.zoom, display_zoom,
            "the display keeps its own temporary zoom"
        );
        c.execute(&ui, Action::Settings);
        assert!(!c.state.presentation_mode);
        assert_eq!(c.state.zoom, old_zoom);
    }

    #[test]
    fn presentation_routes_space_to_practice_in_place_and_escape_back_exits() {
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        let ui = AppWindow::new().unwrap();
        let mut c = Controller::new(directory.path().to_owned());
        c.state.tracks.push(Track {
            path: directory.path().join("song.wav"),
            title: "Song".into(),
            artist: String::new(),
            album: String::new(),
            genre: String::new(),
            number: 1,
            duration: 120.0,
            artwork: vec![],
            lyrics: Default::default(),
        });
        c.state.current = Some(0);
        c.state.status = PlaybackStatus::Paused;
        c.state.paused = true;
        c.state.position = 8.0;
        c.practice_command(&ui, "toggle", "", "");
        c.execute(&ui, Action::PresentationMode);
        let event = |text: &str| {
            let mut event = slint::language::KeyEvent::default();
            event.text = text.into();
            event
        };
        assert!(c.dispatch(&ui, event(" "), false));
        assert!(c.practice.pending().is_some());
        assert!(c.state.presentation_mode);
        assert!(!ui.get_practice_open());
        assert!(c.dispatch(&ui, event(" "), false));
        assert!(c.practice.pending().is_none());
        assert!(c.state.paused);
        assert!(c.state.presentation_mode);
        assert!(c.dispatch(&ui, event("\u{1b}"), false));
        assert!(!c.state.presentation_mode);
        assert_eq!(c.state.position, 8.0);
    }
}
