//! Desktop practice transport, UI projection and ordered background persistence.
use super::*;
use crate::app::practice::{PracticeStore, format_time, parse_time};
use std::{path::Path, sync::mpsc, thread, time::Instant};

pub(super) struct PracticeSaves {
    sender: Option<mpsc::Sender<PracticeStore>>,
    errors: mpsc::Receiver<String>,
    worker: Option<thread::JoinHandle<()>>,
}

impl PracticeSaves {
    pub(super) fn new(path: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel::<PracticeStore>();
        let (errors, error_receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            for mut store in receiver {
                if let Err(error) = store.save(&path) {
                    let _ = errors.send(error);
                }
            }
        });
        Self {
            sender: Some(sender),
            errors: error_receiver,
            worker: Some(worker),
        }
    }

    fn save(&self, store: &PracticeStore) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(store.clone());
        }
    }
}

impl Drop for PracticeSaves {
    fn drop(&mut self) {
        // Closing the channel drains accepted edits before application exit.
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Controller {
    fn practice_track(&self) -> Option<&crate::library::Track> {
        if self.state.video_active || self.state.podcasts.active.is_some() {
            return None;
        }
        self.state
            .current
            .and_then(|id| self.state.tracks.get(id))
            .filter(|track| self.state.track_available(track))
    }

    pub(super) fn prepare_practice_load(&mut self, command: &mut AudioCommand) {
        match command {
            AudioCommand::Load { path, paused, .. }
            | AudioCommand::LoadWithGain { path, paused, .. }
                if self.practice.enabled() =>
            {
                *paused = true;
                self.state.paused = true;
                self.practice.cancel();
                let _ = self.practice.select(path, None, 0.0);
                let _ = self
                    .practice
                    .begin(path, 0.0, self.practice_store.delay_seconds());
            }
            AudioCommand::LoadAt { .. } => self.practice.set_enabled(false),
            AudioCommand::Stop { .. } | AudioCommand::Shutdown => {
                self.practice.cancel();
            }
            _ => {}
        }
    }

    pub(super) fn practice_progress(
        &mut self,
        id: usize,
        status: PlaybackStatus,
        position: f64,
        now: Instant,
    ) {
        if status != PlaybackStatus::Paused || !self.state.paused {
            return;
        }
        let Some(track) = self.state.tracks.get(id) else {
            return;
        };
        let Some(request) = self.practice.pending() else {
            return;
        };
        // Engine events have already passed generation/seek epoch filtering.
        if request.path == crate::library::paths::normalize(&track.path)
            && position.is_finite()
            && (position - request.position).abs() <= 0.05
        {
            self.practice.audio_ready(request.token, &track.path, now);
        }
    }

    pub(super) fn poll_practice(&mut self, ui: &AppWindow, now: Instant) {
        while let Ok(error) = self.practice_saves.errors.try_recv() {
            ui.set_practice_error(
                format!("{}: {error}", self.text("Cannot save practice data")).into(),
            );
        }
        if let Some(request) = self
            .practice
            .pending()
            .filter(|_| self.practice.is_waiting())
        {
            let (token, since) = self.practice_wait.get_or_insert((request.token, now));
            if *token != request.token {
                *token = request.token;
                *since = now;
            }
            if now.saturating_duration_since(*since) >= std::time::Duration::from_secs(10) {
                self.practice.cancel();
                ui.set_practice_error(self.text("Audio preparation timed out. Try again."));
            }
        } else {
            self.practice_wait = None;
        }
        let path = self.practice_track().map(|track| track.path.clone());
        if let Some(path) = path {
            if let Some(request) = self.practice.tick(&path, now)
                && self.state.paused
                && self.pending_media.is_none()
                && self.state.status == PlaybackStatus::Paused
                && request.path == crate::library::paths::normalize(&path)
            {
                self.state.paused = false;
                self.state.status = PlaybackStatus::Playing;
                self.send_music(AudioCommand::Pause(false));
            }
        } else {
            self.practice.cancel();
            if self.state.video_active || self.state.podcasts.active.is_some() {
                self.practice.set_enabled(false);
                ui.set_practice_open(false);
            }
        }
    }

    pub(super) fn sync_practice(&mut self, ui: &AppWindow) {
        let path = self.practice_track().map(|track| track.path.clone());
        if self.practice_path != path {
            self.practice_path = path;
            if ui.get_practice_open() && ui.get_practice_editing() {
                ui.invoke_close_practice_editor();
            }
            ui.set_practice_error("".into());
        }
        if self.practice.enabled()
            && self.state.status == PlaybackStatus::Error
            && ui.get_practice_error().is_empty()
        {
            ui.set_practice_error(self.text(&self.state.notice));
        }
        let track = self.practice_track();
        let rows = track
            .map(|track| {
                self.practice_store
                    .markers(&track.path)
                    .iter()
                    .map(|marker| crate::ui::PracticeMarkerRow {
                        id: marker.id.to_string().into(),
                        name: marker.name.as_str().into(),
                        time: format_time(marker.position).into(),
                        progress: if track.duration > 0.0 {
                            (marker.position / track.duration).clamp(0.0, 1.0) as f32
                        } else {
                            0.0
                        },
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        ui.set_practice_has_track(track.is_some());
        ui.set_practice_title(track.map_or("", |track| track.title.as_str()).into());
        ui.set_practice_position(format_time(self.state.position).into());
        ui.set_practice_enabled(self.practice.enabled());
        ui.set_practice_pending(self.practice.pending().is_some());
        ui.set_practice_countdown(self.practice.remaining(Instant::now()).unwrap_or(0) as i32);
        ui.set_practice_delay(self.practice_store.delay_seconds() as f32);
        ui.set_practice_lead_in(self.practice_store.lead_in_seconds() as f32);
        if rows != self.practice_rows {
            self.practice_rows = rows.clone();
            ui.set_practice_markers(ModelRc::new(VecModel::from(rows)));
        }
    }

    pub(super) fn handle_practice_action(&mut self, ui: &AppWindow, action: Action) -> bool {
        let command = match action {
            Action::PracticeMode => Some(if ui.get_practice_open() {
                "close"
            } else {
                "open"
            }),
            Action::PracticeAddMarker => Some("add"),
            Action::PracticePreviousMarker => Some("previous"),
            Action::PracticeNextMarker => Some("next"),
            Action::PracticeRestart => Some("restart"),
            Action::TogglePlay if self.practice.pending().is_some() => Some("cancel"),
            Action::Back if ui.get_practice_open() => Some("close"),
            _ => None,
        };
        if let Some(command) = command {
            self.practice_command(ui, command, "", "");
            true
        } else {
            false
        }
    }

    pub(super) fn dispatch_practice(
        &mut self,
        ui: &AppWindow,
        event: slint::language::KeyEvent,
        editing: bool,
    ) -> bool {
        let key = Key::from_slint(event.text.as_str());
        if key == Some(Key::Escape) {
            if ui.get_practice_editing() {
                ui.invoke_close_practice_editor();
            } else {
                self.practice_command(ui, "close", "", "");
            }
            return true;
        }
        if editing || ui.get_practice_editor_focused() {
            return false;
        }
        let Some(key) = key else {
            return true;
        };
        let press = KeyPress {
            key,
            repeat: event.repeat,
            modifiers: Modifiers {
                primary: event.modifiers.control,
                secondary: event.modifiers.meta,
                alt: event.modifiers.alt,
                shift: event.modifiers.shift,
            },
        };
        if let Dispatch::Action(action) = self.dispatcher.dispatch(
            &press,
            Context {
                surface: crate::input::Surface::Library,
                text_input: false,
                reference_open: false,
                native_dialog: false,
            },
        ) {
            if action == Action::Home
                || action.category() == Category::Playback
                || Action::PRACTICE.contains(&action)
            {
                self.execute(ui, action);
            }
        }
        true
    }

    fn practice_overlay_blocked(&self, ui: &AppWindow) -> bool {
        self.dialog_open
            || self.state.reference_open
            || ui.get_playlist_editor_open()
            || ui.get_genius_preview_open()
            || ui.get_shortcut_editor_open()
            || ui.get_deepseek_settings_open()
            || ui.get_podcast_subscribe_open()
    }

    pub fn practice_command(&mut self, ui: &AppWindow, command: &str, id: &str, payload: &str) {
        if command != "cancel" && command != "close" && self.practice_overlay_blocked(ui) {
            return;
        }
        ui.set_practice_error("".into());
        let result = self.apply_practice_command(ui, command, id, payload);
        if let Err(error) = result {
            ui.set_practice_error(self.text(&error));
        }
        self.sync(ui);
        self.publish_media_snapshot();
    }

    fn apply_practice_command(
        &mut self,
        ui: &AppWindow,
        command: &str,
        id: &str,
        payload: &str,
    ) -> Result<(), String> {
        match command {
            "open" => {
                self.prepare_portrait_dialog(ui);
                ui.set_track_details_open(false);
                ui.set_native_status_open(false);
                ui.set_practice_open(true);
                ui.invoke_focus_screen();
                return Ok(());
            }
            "close" => {
                self.practice.cancel();
                ui.set_practice_open(false);
                ui.invoke_close_practice_editor();
                ui.invoke_focus_screen();
                return Ok(());
            }
            "cancel" => {
                self.practice.cancel();
                return Ok(());
            }
            "delay" | "lead_in" => {
                let seconds = payload
                    .parse::<u32>()
                    .map_err(|_| "Invalid practice settings".to_owned())?;
                if command == "delay" {
                    self.practice_store.set_delay_seconds(seconds)?;
                } else {
                    self.practice_store.set_lead_in_seconds(seconds)?;
                }
                self.practice_saves.save(&self.practice_store);
                return Ok(());
            }
            "toggle" if self.practice.enabled() => {
                self.practice.set_enabled(false);
                return Ok(());
            }
            "toggle" => {
                if self.state.video_active || self.state.podcasts.active.is_some() {
                    return Err("Choose a song first".into());
                }
                self.practice.set_enabled(true);
                self.cancel_gapless_prefetch();
                if self.practice_track().is_some() {
                    self.state.paused = true;
                    if self.state.status == PlaybackStatus::Playing {
                        self.state.status = PlaybackStatus::Paused;
                    }
                    self.send_music(AudioCommand::Pause(true));
                }
                return Ok(());
            }
            _ => {}
        }
        let track = self.practice_track().ok_or("Choose a song first")?;
        let path = track.path.clone();
        let duration = track.duration;
        match command {
            "start" => {
                let anchor = self.practice.anchor_position(&path);
                let prepared_target =
                    (anchor - f64::from(self.practice_store.lead_in_seconds())).max(0.0);
                let reuse_anchor = self.state.status == PlaybackStatus::Stopped
                    || (self.state.paused && (self.state.position - prepared_target).abs() <= 0.05);
                let (position, marker) = if reuse_anchor {
                    (anchor, self.practice.selected_marker())
                } else {
                    (self.state.position, None)
                };
                self.start_practice(&path, position, marker)?;
            }
            "restart" => {
                let position = self.practice.anchor_position(&path);
                self.start_practice(&path, position, self.practice.selected_marker())?;
            }
            "add" => {
                let name = format!(
                    "{} {}",
                    self.text("Marker"),
                    self.practice_store.markers(&path).len() + 1
                );
                self.practice_store
                    .add_marker(&path, &name, self.state.position, duration)?;
                self.practice_saves.save(&self.practice_store);
            }
            "save" => {
                let (time, name) = payload
                    .split_once('\n')
                    .ok_or("Enter a marker name and time")?;
                let position = parse_time(time)?;
                if id.is_empty() {
                    self.practice_store
                        .add_marker(&path, name, position, duration)?;
                } else {
                    let marker_id = id.parse().map_err(|_| "Practice marker no longer exists")?;
                    self.practice_store
                        .update_marker(&path, marker_id, name, position, duration)?;
                    if self.practice.selected_marker() == Some(marker_id) {
                        self.practice.select(&path, Some(marker_id), position)?;
                    }
                }
                self.practice_saves.save(&self.practice_store);
            }
            "delete" => {
                let marker_id = id.parse().map_err(|_| "Practice marker no longer exists")?;
                if !self.practice_store.remove_marker(&path, marker_id) {
                    return Err("Practice marker no longer exists".into());
                }
                if self.practice.selected_marker() == Some(marker_id) {
                    self.practice.select(&path, None, 0.0)?;
                }
                self.practice_saves.save(&self.practice_store);
            }
            "jump" | "locate" | "previous" | "next" => {
                let markers = self.practice_store.markers(&path);
                let marker = if command == "previous" || command == "next" {
                    let selected = self
                        .practice
                        .selected_marker()
                        .and_then(|id| markers.iter().position(|m| m.id == id));
                    match (command, selected) {
                        ("previous", Some(index)) => markers.get(index.saturating_sub(1)),
                        ("next", Some(index)) => {
                            markers.get((index + 1).min(markers.len().saturating_sub(1)))
                        }
                        ("previous", None) => markers
                            .iter()
                            .rev()
                            .find(|m| m.position < self.state.position - 0.05)
                            .or(markers.first()),
                        _ => markers
                            .iter()
                            .find(|m| m.position > self.state.position + 0.05)
                            .or(markers.last()),
                    }
                } else {
                    let marker_id = id
                        .parse::<u64>()
                        .map_err(|_| "Practice marker no longer exists")?;
                    markers.iter().find(|m| m.id == marker_id)
                }
                .cloned()
                .ok_or("No practice markers yet")?;
                if command == "locate" {
                    self.practice.cancel();
                    self.pause_and_seek(marker.position);
                    self.practice
                        .select(&path, Some(marker.id), marker.position)?;
                } else {
                    self.start_practice(&path, marker.position, Some(marker.id))?;
                }
            }
            _ => return Err("Unknown practice command".into()),
        }
        Ok(())
    }

    fn pause_and_seek(&mut self, target: f64) {
        if matches!(
            self.state.status,
            PlaybackStatus::Stopped | PlaybackStatus::Error
        ) {
            if let Some(id) = self.state.current {
                let effects = self.state.start(id);
                for effect in effects {
                    if let Effect::Audio(command) = effect {
                        self.send_music(command);
                    }
                }
            }
        }
        self.state.paused = true;
        if self.state.status != PlaybackStatus::Loading {
            self.state.status = PlaybackStatus::Paused;
        }
        self.send_music(AudioCommand::Pause(true));
        if self.state.duration() <= 0.0 {
            self.state.position = target;
            self.send_music(AudioCommand::Seek(target));
        } else if let Effect::Audio(command) = self.state.seek(target) {
            self.send_music(command);
        }
        self.cancel_gapless_prefetch();
    }

    pub(super) fn start_practice(
        &mut self,
        path: &Path,
        anchor: f64,
        marker: Option<u64>,
    ) -> Result<(), String> {
        let duration = self.state.duration();
        if !anchor.is_finite() || anchor < 0.0 || (duration > 0.0 && anchor >= duration) {
            return Err("Marker must be before the end of the song".into());
        }
        self.practice.set_enabled(true);
        let target = (anchor - f64::from(self.practice_store.lead_in_seconds())).max(0.0);
        self.pause_and_seek(target);
        self.practice.select(path, marker, anchor)?;
        self.practice
            .begin(path, target, self.practice_store.delay_seconds())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn controller() -> (tempfile::TempDir, AppWindow, Controller) {
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        let ui = AppWindow::new().unwrap();
        let mut controller = Controller::new(directory.path().to_owned());
        for name in ["first", "second"] {
            controller.state.tracks.push(crate::library::Track {
                path: directory.path().join(format!("{name}.wav")),
                title: name.into(),
                artist: "Practice".into(),
                album: "Tests".into(),
                genre: String::new(),
                number: 1,
                duration: 180.0,
                artwork: Vec::new(),
                lyrics: Default::default(),
            });
        }
        controller.state.current = Some(0);
        controller.state.status = PlaybackStatus::Paused;
        controller.state.paused = true;
        (directory, ui, controller)
    }

    #[test]
    fn ready_countdown_lead_in_and_quick_jumps_follow_latest_request() {
        let (_directory, ui, mut c) = controller();
        c.practice_command(&ui, "save", "", "01:02.500\nChorus");
        c.practice_command(&ui, "save", "", "00:18.000\nVerse");
        let path = c.state.tracks[0].path.clone();
        let ids: Vec<_> = c
            .practice_store
            .markers(&path)
            .iter()
            .map(|m| m.id)
            .collect();
        c.practice_command(&ui, "lead_in", "", "2");
        c.practice_command(&ui, "jump", &ids[1].to_string(), "");
        assert_eq!(c.state.position, 60.5);
        c.practice_command(&ui, "cancel", "", "");
        c.practice_command(&ui, "start", "", "");
        assert_eq!(
            c.state.position, 60.5,
            "lead-in must not accumulate on retry"
        );
        assert_eq!(c.practice.selected_marker(), Some(ids[1]));
        assert!(c.state.paused);
        assert!(c.practice.is_waiting());
        let old = c.practice.pending().unwrap().clone();
        let now = Instant::now();
        c.poll_practice(&ui, now + Duration::from_secs(50));
        assert!(
            c.state.paused,
            "waiting for audio never expires into playback"
        );
        c.practice_command(&ui, "jump", &ids[0].to_string(), "");
        let current = c.practice.pending().unwrap().clone();
        assert_ne!(old.token, current.token);
        assert_eq!(current.position, 16.0);
        assert!(!c.practice.audio_ready(old.token, &path, now));
        c.practice_progress(0, PlaybackStatus::Paused, 60.5, now);
        assert!(c.practice.is_waiting());
        c.practice_progress(0, PlaybackStatus::Playing, 16.0, now);
        assert!(c.practice.is_waiting());
        c.practice_progress(0, PlaybackStatus::Paused, 16.0, now);
        assert_eq!(c.practice.remaining(now), Some(3));
        c.poll_practice(&ui, now + Duration::from_secs(2));
        assert!(c.state.paused);
        c.state.status = PlaybackStatus::Paused;
        c.poll_practice(&ui, now + Duration::from_secs(3));
        assert!(!c.state.paused);
        assert!(c.practice.pending().is_none());
        c.execute(&ui, Action::TogglePlay);
        assert!(c.state.paused);
        c.execute(&ui, Action::TogglePlay);
        assert!(!c.state.paused, "ordinary pause/resume has no countdown");
    }

    #[test]
    fn cancellation_seek_media_pause_output_loss_and_track_change_never_resume() {
        use crate::platform::macos_media::RemoteCommand;
        let (_directory, ui, mut c) = controller();
        let now = Instant::now();
        for reason in 0..6 {
            c.state.current = Some(0);
            c.state.position = 12.0;
            c.state.status = PlaybackStatus::Paused;
            c.practice_command(&ui, "start", "", "");
            c.practice_progress(0, PlaybackStatus::Paused, 12.0, now);
            assert!(c.practice.pending().is_some());
            match reason {
                0 => c.execute(&ui, Action::TogglePlay),
                1 => c.handle_media_command(&ui, RemoteCommand::Pause),
                2 => c.pause_for_output_change(&ui),
                3 => {
                    let effect = c.state.seek(24.0);
                    c.effects(&ui, vec![effect]);
                }
                4 => {
                    c.state.current = Some(1);
                }
                _ => c.practice_command(&ui, "toggle", "", ""),
            }
            c.poll_practice(&ui, now + Duration::from_secs(20));
            assert!(c.state.paused, "cancel reason {reason} resumed audio");
            assert!(c.practice.pending().is_none());
        }
    }

    #[test]
    fn unknown_duration_seek_timeout_and_modal_cancel_remain_safe() {
        let (_directory, ui, mut c) = controller();
        c.state.tracks[0].duration = 0.0;
        c.practice_command(&ui, "save", "", "00:12.000\nUnknown duration");
        let id = c.practice_store.markers(&c.state.tracks[0].path)[0]
            .id
            .to_string();
        c.practice_command(&ui, "jump", &id, "");
        assert_eq!(c.state.position, 12.0);
        assert_eq!(c.practice.pending().unwrap().position, 12.0);
        let now = Instant::now();
        c.poll_practice(&ui, now);
        c.poll_practice(&ui, now + Duration::from_secs(10));
        assert!(c.practice.pending().is_none());
        assert!(c.state.paused);
        assert!(ui.get_practice_error().contains("timed out"));
        c.practice_command(&ui, "jump", &id, "");
        c.practice_progress(0, PlaybackStatus::Paused, 12.0, now);
        ui.set_genius_preview_open(true);
        c.handle_media_command(&ui, crate::platform::macos_media::RemoteCommand::Toggle);
        assert!(c.practice.pending().is_none());
        c.poll_practice(&ui, now + Duration::from_secs(30));
        assert!(c.state.paused);
    }

    #[test]
    fn natural_finish_stops_even_with_repeat_and_gapless_enabled() {
        let (_directory, ui, mut c) = controller();
        c.practice.set_enabled(true);
        c.state.queue.set(vec![0, 1], 0, false);
        c.state.repeat = crate::audio::Repeat::All;
        c.state.gapless_playback = true;
        c.state.paused = false;
        c.state.status = PlaybackStatus::Playing;
        c.schedule_gapless_prefetch();
        assert!(c.prefetched.is_none());
        c.handle_audio_event(&ui, AudioEvent::Finished(0));
        assert_eq!(c.state.current, Some(0));
        assert_eq!(c.state.queue.position, 0);
        assert_eq!(c.state.position, 180.0);
        assert!(c.state.paused);
        assert_eq!(c.state.status, PlaybackStatus::Stopped);
    }

    #[test]
    fn practice_can_be_armed_before_selecting_the_first_song() {
        let (_directory, ui, mut c) = controller();
        c.state.current = None;
        c.practice_command(&ui, "open", "", "");
        c.practice_command(&ui, "toggle", "", "");
        c.poll_practice(&ui, Instant::now());
        assert!(c.practice.enabled());
        assert!(ui.get_practice_open());
        assert!(ui.get_practice_error().is_empty());
        c.practice_command(&ui, "close", "", "");
        c.state.current = Some(0);
        let mut load = AudioCommand::Load {
            id: 0,
            path: c.state.tracks[0].path.clone(),
            volume: 0.0,
            paused: false,
        };
        c.prepare_practice_load(&mut load);
        assert!(matches!(load, AudioCommand::Load { paused: true, .. }));
        assert!(c.practice.is_waiting());
    }

    #[test]
    fn new_music_load_is_paused_and_podcast_cancels_countdown() {
        let (_directory, _ui, mut c) = controller();
        c.practice.set_enabled(true);
        let path = c.state.tracks[0].path.clone();
        let mut load = AudioCommand::Load {
            id: 0,
            path,
            volume: 0.0,
            paused: false,
        };
        c.prepare_practice_load(&mut load);
        assert!(matches!(load, AudioCommand::Load { paused: true, .. }));
        assert!(c.practice.is_waiting());
        assert!(c.state.paused);
        let mut podcast = AudioCommand::LoadAt {
            id: usize::MAX,
            path: "podcast.mp3".into(),
            volume: 0.0,
            paused: true,
            start: 0.0,
            speed: 1.0,
        };
        c.prepare_practice_load(&mut podcast);
        assert!(!c.practice.enabled());
        assert!(c.practice.pending().is_none());
    }

    #[test]
    fn ordered_background_saves_restore_per_song_markers_and_settings() {
        let (directory, ui, mut c) = controller();
        c.practice_command(&ui, "delay", "", "5");
        c.practice_command(&ui, "lead_in", "", "2");
        c.practice_command(&ui, "save", "", "00:45.125\nChorus");
        let path = c.state.tracks[0].path.clone();
        let id = c.practice_store.markers(&path)[0].id.to_string();
        c.practice_command(&ui, "save", &id, "00:44.250\nSolo");
        c.practice_command(&ui, "save", &id, "garbage\nInvalid");
        assert!(!ui.get_practice_error().is_empty());
        c.state.current = Some(1);
        c.practice_command(&ui, "save", "", "00:03.000\nIntro");
        let second_path = c.state.tracks[1].path.clone();
        drop(c); // the save worker drains the last accepted edit
        let store = PracticeStore::load(&directory.path().join("practice.json")).unwrap();
        assert_eq!(store.delay_seconds(), 5);
        assert_eq!(store.lead_in_seconds(), 2);
        assert_eq!(store.markers(&path)[0].name, "Solo");
        assert_eq!(store.markers(&path)[0].position, 44.25);
        assert_eq!(store.markers(&second_path)[0].name, "Intro");
        let mut restored = Controller::new(directory.path().to_owned());
        restored.state.tracks.push(crate::library::Track {
            path,
            title: "Restored".into(),
            artist: String::new(),
            album: String::new(),
            genre: String::new(),
            number: 1,
            duration: 60.0,
            artwork: vec![],
            lyrics: Default::default(),
        });
        restored.state.current = Some(0);
        restored.sync_practice(&ui);
        assert_eq!(ui.get_practice_markers().row_count(), 1);
        assert_eq!(ui.get_practice_markers().row_data(0).unwrap().name, "Solo");
        assert!(
            !restored.practice.enabled(),
            "launch never arms an unattended practice session"
        );
    }
}
