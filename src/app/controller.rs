use super::{
    action::{Action, Category},
    config::Settings,
    navigation::Screen,
    state::{Effect, State, Target},
};
use crate::{
    audio::{
        PlaybackStatus,
        engine::{AudioEvent, Engine},
    },
    input::{Binding, Context, Dispatch, Dispatcher, Key, KeyPress, Keymap, Modifiers},
    library::{Track, scanner},
    ui::{AppWindow, LibraryRow, ShortcutRow},
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::mpsc::Receiver};

pub struct Controller {
    pub state: State,
    pub dispatcher: Dispatcher,
    engine: Engine,
    directory: PathBuf,
    settings: Settings,
    loading: Option<Receiver<Result<Vec<Track>, String>>>,
    importing: Option<Receiver<scanner::ImportEvent>>,
    lyrics_edit: Option<Receiver<Result<Option<scanner::LyricsUpdate>, String>>>,
    dialog_open: bool,
    last_screen: Option<Screen>,
    last_query: String,
    rows_dirty: bool,
    last_track: Option<usize>,
    last_lyric: Option<usize>,
    last_selected: usize,
    last_scale: f32,
    last_window_size: slint::LogicalSize,
    last_notice: String,
    notice_since: std::time::Instant,
    shortcut_edit: Option<Action>,
    shortcut_capture: Option<bool>,
}

impl Controller {
    pub fn new(directory: PathBuf) -> Self {
        let mut state = State::default();
        let settings = Settings::load(&directory.join("settings.json")).unwrap_or_else(|message| {
            state.notice = message;
            Settings::default()
        });
        state.zoom = settings.zoom;
        state.device_mode = settings.device_mode;
        let keymap =
            Keymap::load_or_create(&directory.join("shortcuts.json")).unwrap_or_else(|message| {
                state.notice = format!("{message}; using default shortcuts");
                Keymap::default()
            });
        let loading = Some(scanner::load(directory.join("library.sqlite3")));
        Self {
            state,
            dispatcher: Dispatcher::new(keymap),
            engine: Engine::default(),
            directory,
            settings,
            loading,
            importing: None,
            lyrics_edit: None,
            dialog_open: false,
            last_screen: None,
            last_query: String::new(),
            rows_dirty: true,
            last_track: None,
            last_lyric: None,
            last_selected: usize::MAX,
            last_scale: 0.0,
            last_window_size: slint::LogicalSize::new(0.0, 0.0),
            last_notice: String::new(),
            notice_since: std::time::Instant::now(),
            shortcut_edit: None,
            shortcut_capture: None,
        }
    }

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

    fn refresh_shortcuts(&self, ui: &AppWindow) {
        let mut reference = Vec::new();
        for category in [
            Category::Playback,
            Category::Navigation,
            Category::Application,
        ] {
            for binding in self
                .dispatcher
                .keymap
                .bindings
                .iter()
                .filter(|b| b.action.category() == category)
            {
                reference.push(ShortcutRow {
                    category: category.title().into(),
                    title: binding.action.title().into(),
                    keys: binding.label(cfg!(target_os = "macos")).into(),
                });
            }
        }
        ui.set_shortcuts(ModelRc::new(VecModel::from(reference)));
    }

    pub fn dispatch(
        &mut self,
        ui: &AppWindow,
        event: slint::language::KeyEvent,
        text_input: bool,
    ) -> bool {
        let key = Key::from_slint(event.text.as_str());
        if let Some(action) = self.shortcut_edit {
            if event.repeat {
                return true;
            }
            if key == Some(Key::Escape) {
                self.shortcut_command(ui, "cancel");
            } else if let (Some(replace), Some(key)) = (self.shortcut_capture, key.clone()) {
                let binding = Binding {
                    action,
                    key,
                    optional_shift: false,
                    modifiers: Modifiers {
                        primary: event.modifiers.control,
                        secondary: event.modifiers.meta,
                        alt: event.modifiers.alt,
                        shift: event.modifiers.shift,
                    },
                };
                let result = self.dispatcher.keymap.edit(action, Some(binding), replace);
                if self.commit_keymap(ui, result) {
                    self.shortcut_capture = None;
                }
            } else if self.shortcut_capture.is_none() && key == Some(Key::Enter) {
                self.shortcut_command(ui, "replace");
            }
            self.sync_editor(ui);
            return true;
        }
        let Some(key) = key else {
            return false;
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
        let context = Context {
            surface: self.state.surface(),
            text_input,
            reference_open: self.state.reference_open,
            native_dialog: self.dialog_open,
        };
        match self.dispatcher.dispatch(&press, context) {
            Dispatch::Ignored => false,
            Dispatch::Consumed => true,
            Dispatch::Action(action) => {
                if action.category() == Category::Navigation {
                    ui.set_keyboard_navigation(true);
                }
                self.execute(ui, action);
                true
            }
        }
    }

    pub fn execute(&mut self, ui: &AppWindow, action: Action) {
        let effects = self.state.apply(action);
        self.sync(ui);
        self.effects(ui, effects);
    }

    fn effects(&mut self, ui: &AppWindow, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Audio(command) => {
                    if let Err(message) = self.engine.send(command) {
                        self.state.notice = message;
                        self.state.status = PlaybackStatus::Error;
                    }
                }
                Effect::Import => {
                    self.dialog_open = true;
                    self.importing = Some(scanner::import(self.directory.join("library.sqlite3")));
                }
                Effect::FocusSearch => ui.invoke_focus_search(),
                Effect::FocusScreen => ui.invoke_focus_screen(),
                Effect::Resize => {
                    self.rows_dirty = true;
                    self.resize(ui);
                    self.sync(ui);
                    self.save_settings(ui);
                }
                Effect::EditShortcut(action) => {
                    self.shortcut_edit = Some(action);
                    self.shortcut_capture = None;
                    ui.set_shortcut_message("Choose how to edit this action.".into());
                    self.sync_editor(ui);
                    ui.invoke_focus_screen();
                }
                Effect::EditLyrics { id, command } => {
                    if self.dialog_open || self.lyrics_edit.is_some() {
                        continue;
                    }
                    if let Some(track) = self.state.tracks.get(id) {
                        self.dialog_open = true;
                        self.lyrics_edit = Some(scanner::edit_lyrics(
                            self.directory.join("library.sqlite3"),
                            track.path.clone(),
                            command,
                            "Import Lyrics".into(),
                        ));
                    }
                }
                Effect::RestoreShortcuts => {
                    if self.commit_keymap(ui, Ok(Keymap::default())) {
                        self.state.notice = "Default shortcuts restored".into();
                        self.sync(ui);
                    }
                }
            }
        }
    }

    fn base_size(&self) -> (f32, f32) {
        if self.state.device_mode {
            (296.0, 296.0 * 76.5 / 39.6)
        } else {
            (240.0, 432.0)
        }
    }

    fn resize(&mut self, ui: &AppWindow) {
        ui.set_device_mode(self.state.device_mode);
        let (width, height) = self.base_size();
        let current_size = ui.window().size().to_logical(ui.window().scale_factor());
        let menu_height = (current_size.height - ui.get_content_height()).max(0.0);
        self.last_window_size = slint::LogicalSize::new(
            width * self.state.zoom,
            height * self.state.zoom + menu_height,
        );
        ui.window().set_size(self.last_window_size);
    }

    fn maintain_aspect_ratio(&mut self, ui: &AppWindow) {
        let size = ui.window().size().to_logical(ui.window().scale_factor());
        let (width, height) = self.base_size();
        let dx = (size.width - self.last_window_size.width).abs() / width;
        let dy = (size.height - self.last_window_size.height).abs() / height;
        let content_height = ui.get_content_height();
        let mismatch = (content_height - size.width / width * height).abs();
        if dx < 0.004 && dy < 0.004 && mismatch < 2.0 {
            return;
        }
        self.state.zoom = if dx >= dy {
            size.width / width
        } else {
            content_height / height
        }
        .clamp(0.75, 3.0);
        self.resize(ui);
    }

    pub fn save_settings(&mut self, ui: &AppWindow) {
        let size = ui.window().size().to_logical(ui.window().scale_factor());
        self.settings = Settings {
            zoom: self.state.zoom,
            device_mode: self.state.device_mode,
            window_width: Some(size.width),
            window_height: Some(size.height),
        };
        if let Err(message) = self.settings.save(&self.directory.join("settings.json")) {
            self.state.notice = format!("Cannot save window settings: {message}");
        }
    }

    pub fn poll(&mut self, ui: &AppWindow) {
        self.maintain_aspect_ratio(ui);
        if let Some(result) = self
            .loading
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.loading = None;
            match result {
                Ok(tracks) => {
                    self.state.merge_library(tracks);
                    self.rows_dirty = true;
                }
                Err(message) => self.state.notice = message,
            }
        }
        while let Some(event) = self
            .importing
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            let scanner::ImportEvent::Completed(result) = event else {
                self.dialog_open = false;
                self.state.notice = "Scanning music folders…".to_owned();
                ui.invoke_focus_screen();
                continue;
            };
            self.importing = None;
            self.state.importing = false;
            self.dialog_open = false;
            match result {
                Ok(Some(result)) => {
                    self.state.merge_library(result.tracks);
                    self.rows_dirty = true;
                    self.last_track = None;
                    self.state.notice = format!(
                        "{} songs in library{}",
                        self.state.tracks.len(),
                        if result.warnings.is_empty() {
                            String::new()
                        } else {
                            format!(" · {} files skipped", result.warnings.len())
                        }
                    );
                }
                Ok(None) => self.state.notice = "Import cancelled".to_owned(),
                Err(message) => self.state.notice = format!("Import failed: {message}"),
            }
        }
        if let Some(result) = self
            .lyrics_edit
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.lyrics_edit = None;
            self.dialog_open = false;
            match result {
                Ok(Some(update)) => {
                    if let Some((id, track)) = self
                        .state
                        .tracks
                        .iter_mut()
                        .enumerate()
                        .find(|(_, track)| track.path == update.path)
                    {
                        track.lyrics = update.lyrics;
                        if self.state.current == Some(id) {
                            self.state.lyrics_visible = !track.lyrics.is_empty();
                            self.state.overlay_visible = self.state.lyrics_visible;
                            self.last_track = None;
                        }
                    }
                    self.state.notice = "Lyrics updated".into();
                }
                Ok(None) => self.state.notice = "Import cancelled".into(),
                Err(error) => self.state.notice = format!("Lyrics import failed: {error}"),
            }
            self.rows_dirty = true;
            ui.invoke_focus_screen();
        }
        while let Ok(event) = self.engine.events.try_recv() {
            match event {
                AudioEvent::Progress {
                    id,
                    status,
                    position,
                } if self.state.current == Some(id) => {
                    self.state.position = position;
                    self.state.status = if self.state.paused {
                        PlaybackStatus::Paused
                    } else {
                        status
                    };
                }
                AudioEvent::Finished(id) if self.state.current == Some(id) => {
                    let effects = self.state.advance(false);
                    self.effects(ui, effects);
                }
                AudioEvent::Error { id, message } if id.is_none() || id == self.state.current => {
                    self.state.status = PlaybackStatus::Error;
                    self.state.notice = format!("Playback error: {message}");
                }
                _ => {}
            }
        }
        self.sync(ui);
    }

    fn sync(&mut self, ui: &AppWindow) {
        let screen = &self.state.navigation.current().screen;
        let screen_changed = self.last_screen.as_ref() != Some(screen);
        ui.set_screen(match screen {
            Screen::Home => 0,
            Screen::NowPlaying => 2,
            Screen::Search => 3,
            Screen::Placeholder(_) => 4,
            Screen::Settings => 5,
            Screen::Shortcuts => 6,
            Screen::Lyrics(_) => 7,
            _ => 1,
        });
        ui.set_screen_title(screen.title().into());
        ui.set_device_mode(self.state.device_mode);
        ui.set_selected(self.state.navigation.selected() as i32);
        ui.set_home_page(self.state.home_page as i32);
        ui.set_clock_text(chrono::Local::now().format("%-I:%M %p").to_string().into());
        ui.set_playing(self.state.status == PlaybackStatus::Playing);
        ui.set_has_track(self.state.current.is_some());
        ui.set_volume(self.state.volume);
        ui.set_muted(self.state.muted);
        ui.set_shuffle(self.state.shuffle);
        ui.set_repeat_mode(self.state.repeat.label().into());
        if self.last_notice != self.state.notice {
            self.last_notice.clone_from(&self.state.notice);
            self.notice_since = std::time::Instant::now();
        }
        ui.set_notice(
            if self.notice_since.elapsed().as_secs() < 3 || self.state.importing {
                self.state.notice.clone().into()
            } else {
                "".into()
            },
        );
        ui.set_reference_open(self.state.reference_open);
        ui.set_lyrics_visible(self.state.lyrics_visible);
        ui.set_overlay_visible(self.state.overlay_visible);
        ui.set_track_count(if self.state.queue.order.is_empty() {
            "".into()
        } else {
            format!(
                "{} of {}",
                self.state.queue.position + 1,
                self.state.queue.order.len()
            )
            .into()
        });
        let duration = self.state.duration();
        ui.set_progress(if duration > 0.0 {
            (self.state.position / duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        });
        ui.set_elapsed(format_time(self.state.position).into());
        ui.set_duration(format_time(duration).into());
        ui.set_remaining(
            format!(
                "−{}",
                format_time((duration - self.state.position).max(0.0))
            )
            .into(),
        );
        if screen_changed || self.rows_dirty || self.last_query != self.state.query {
            let rows = self
                .state
                .rows()
                .into_iter()
                .map(|row| LibraryRow {
                    title: row.title.into(),
                    subtitle: if let Target::Shortcut(action) = row.target {
                        self.dispatcher.keymap.labels(action).into()
                    } else {
                        row.subtitle.into()
                    },
                    chevron: !matches!(row.target, Target::Play(_)),
                })
                .collect::<Vec<_>>();
            ui.set_rows(ModelRc::new(VecModel::from(rows)));
            self.last_query.clone_from(&self.state.query);
            self.rows_dirty = false;
            if screen_changed {
                ui.set_list_scroll_y(0.0);
            }
        }
        let scale = ui.get_display_scale();
        if screen_changed
            || self.last_selected != self.state.navigation.selected()
            || (scale - self.last_scale).abs() > 0.01
        {
            if matches!(
                screen,
                Screen::Music
                    | Screen::Groups(_)
                    | Screen::Artist(_)
                    | Screen::Songs(_)
                    | Screen::Search
                    | Screen::Settings
                    | Screen::Shortcuts
                    | Screen::Lyrics(_)
            ) {
                let row_y = self.state.navigation.selected() as f32 * 48.0 * scale;
                let visible_height = if *screen == Screen::Search {
                    324.0
                } else {
                    368.0
                } * scale;
                let scroll = ui.get_list_scroll_y();
                if row_y + scroll < 0.0 {
                    ui.set_list_scroll_y(-row_y);
                } else if row_y + 48.0 * scale + scroll > visible_height {
                    ui.set_list_scroll_y(visible_height - row_y - 48.0 * scale);
                }
            }
            self.last_selected = self.state.navigation.selected();
        }
        self.last_screen = Some(screen.clone());
        let track = self.state.current.and_then(|id| self.state.tracks.get(id));
        if self.last_track != self.state.current {
            ui.set_track_title(track.map_or("", |t| t.title.as_str()).into());
            ui.set_track_artist(track.map_or("", |t| t.artist.as_str()).into());
            ui.set_track_album(track.map_or("", |t| t.album.as_str()).into());
            let artwork = track
                .and_then(|t| image::load_from_memory(&t.artwork).ok())
                .map(|image| {
                    let image = image.thumbnail(480, 480).to_rgba8();
                    slint::Image::from_rgba8(
                        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                            image.as_raw(),
                            image.width(),
                            image.height(),
                        ),
                    )
                })
                .unwrap_or_default();
            ui.set_artwork(artwork);
            ui.set_lyric_lines(ModelRc::new(VecModel::from(
                track.map_or_else(Vec::new, |t| {
                    t.lyrics.lines().into_iter().map(Into::into).collect()
                }),
            )));
            ui.set_timed_lyrics(track.is_some_and(|t| !t.lyrics.timed.is_empty()));
            ui.set_lyric_scroll_y(0.0);
            self.last_lyric = None;
            self.last_track = self.state.current;
        }
        let active = track.and_then(|t| t.lyrics.active(self.state.position));
        ui.set_active_lyric(active.map_or(-1, |index| index as i32));
        if self.state.lyrics_visible
            && (active != self.last_lyric || (scale - self.last_scale).abs() > 0.01)
        {
            if active.is_none() {
                ui.set_lyric_scroll_y(0.0);
            }
            // Slint scrolls to the measured row position, including wrapped plain/LRC lines.
            self.last_lyric = active;
        }
        self.last_scale = scale;
    }

    fn sync_editor(&self, ui: &AppWindow) {
        ui.set_shortcut_editor_open(self.shortcut_edit.is_some());
        ui.set_shortcut_recording(self.shortcut_capture.is_some());
        if let Some(action) = self.shortcut_edit {
            ui.set_shortcut_title(action.title().into());
            ui.set_shortcut_keys(self.dispatcher.keymap.labels(action).into());
        }
    }

    fn commit_keymap(&mut self, ui: &AppWindow, map: Result<Keymap, String>) -> bool {
        match map.and_then(|map| {
            map.save(&self.directory.join("shortcuts.json"))?;
            Ok(map)
        }) {
            Ok(map) => {
                self.dispatcher.keymap = map;
                self.rows_dirty = true;
                self.refresh_shortcuts(ui);
                ui.set_shortcut_message("Saved. Your shortcuts are active.".into());
                self.sync(ui);
                true
            }
            Err(message) => {
                ui.set_shortcut_message(message.into());
                false
            }
        }
    }

    pub fn shortcut_command(&mut self, ui: &AppWindow, command: &str) {
        if command == "cancel" {
            if self.shortcut_capture.take().is_some() {
                ui.set_shortcut_message("Recording cancelled.".into());
            } else {
                self.shortcut_edit = None;
            }
        } else if let Some(action) = self.shortcut_edit {
            match command {
                "replace" | "add" => {
                    self.shortcut_capture = Some(command == "replace");
                    ui.set_shortcut_message("Press a key combination. Escape cancels.".into());
                }
                "clear" => {
                    self.commit_keymap(ui, self.dispatcher.keymap.edit(action, None, true));
                }
                "restore" => {
                    self.commit_keymap(ui, self.dispatcher.keymap.restore_action(action));
                }
                "done" => {
                    self.shortcut_edit = None;
                    self.shortcut_capture = None;
                }
                _ => {}
            }
        }
        self.sync_editor(ui);
        ui.invoke_focus_screen();
    }
}

pub fn connect(ui: &AppWindow, controller: &Rc<RefCell<Controller>>) {
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_shortcut_command(move |command| {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().shortcut_command(&ui, &command);
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_key_event(move |event, editing| {
        weak.upgrade()
            .is_some_and(|ui| state.borrow_mut().dispatch(&ui, event, editing))
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_command(move |name| {
        if let (Some(ui), Some(action)) = (weak.upgrade(), Action::parse(&name)) {
            ui.set_keyboard_navigation(false);
            state.borrow_mut().execute(&ui, action);
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_item_clicked(move |index| {
        if let Some(ui) = weak.upgrade() {
            let mut state = state.borrow_mut();
            if index >= 0 && (index as usize) < state.state.selection_count() {
                ui.set_keyboard_navigation(false);
                state.state.navigation.select(index as usize);
                state.execute(&ui, Action::Activate);
            }
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_query_changed(move |text| {
        if let Some(ui) = weak.upgrade() {
            let mut state = state.borrow_mut();
            state.state.query = text.to_string();
            state.state.navigation.select(0);
            state.sync(&ui);
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_volume_changed(move |value| {
        if let Some(ui) = weak.upgrade() {
            let mut state = state.borrow_mut();
            let effect = state.state.set_volume(value);
            state.effects(&ui, vec![effect]);
            state.sync(&ui);
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_seek_changed(move |value| {
        if let Some(ui) = weak.upgrade() {
            let mut state = state.borrow_mut();
            let seconds = f64::from(value) * state.state.duration();
            let effect = state.state.seek(seconds);
            state.effects(&ui, vec![effect]);
            state.sync(&ui);
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_home_swipe(move |next| {
        if let Some(ui) = weak.upgrade() {
            let mut state = state.borrow_mut();
            state.state.home_page = usize::from(next);
            state.state.navigation.select(0);
            state.sync(&ui);
        }
    });
}

fn format_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
