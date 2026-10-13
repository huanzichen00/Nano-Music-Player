use super::{
    action::{Action, Category},
    artwork::ArtworkLoader,
    config::Settings,
    i18n::{self, Language, translate},
    navigation::{Filter, Group, Screen},
    state::{Effect, State, Target},
    thumbnails::{self, Prepared, Thumbnails},
};
use crate::{
    audio::{
        PlaybackStatus,
        engine::{AudioCommand, AudioEvent, Engine},
    },
    input::{Binding, Context, Dispatch, Dispatcher, Key, KeyPress, Keymap, Modifiers},
    library::{Track, playlists, romaji, scanner},
    ui::{AppWindow, I18n, LibraryRow, ListMetrics, PlaylistCandidate, ShortcutRow},
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, collections::HashSet, path::PathBuf, rc::Rc, sync::mpsc::Receiver};

// Closing a preview drops its receiver, but a blocking provider call may still be alive.
// Reserve capacity before spawning and retain it until that worker actually exits.
static GENIUS_WORKERS: crate::genius::RequestGate = crate::genius::RequestGate::new(1);

enum PendingMedia {
    Music {
        handoff: u64,
        command: AudioCommand,
    },
    Video {
        handoff: u64,
        command: crate::video::engine::Command,
    },
}

pub struct Controller {
    pub state: State,
    pub dispatcher: Dispatcher,
    engine: Engine,
    video_engine: crate::video::engine::Engine,
    video_loading: Option<Receiver<Result<Vec<crate::video::library::Video>, String>>>,
    video_import: Option<Receiver<crate::video::import::Event>>,
    pending_media: Option<PendingMedia>,
    pending_seek: Option<f64>,
    handoff: u64,
    last_landscape: bool,
    last_video_generation: u64,
    directory: PathBuf,
    settings: Settings,
    loading: Option<Receiver<Result<Prepared<Vec<Track>>, String>>>,
    playlist_loading: Option<Receiver<Result<Vec<playlists::Playlist>, String>>>,
    playlist_saving: Option<Receiver<Result<playlists::Playlist, String>>>,
    playlist_writer: Option<std::thread::JoinHandle<()>>,
    playlist_suspended: bool,
    playlist_edit_id: Option<i64>,
    playlist_original_paths: HashSet<PathBuf>,
    playlist_candidates: Vec<PlaylistCandidate>,
    playlist_paths: Vec<PathBuf>,
    importing: Option<Receiver<thumbnails::ImportEvent>>,
    deletion_events: Option<crate::library::deletion_monitor::Monitor>,
    lyrics_edit: Option<Receiver<Result<Option<scanner::LyricsUpdate>, String>>>,
    romaji_request: Option<romaji::Request>,
    romaji_store: romaji::Store,
    romaji_touched: HashSet<PathBuf>,
    translation_cache: crate::translation::Cache,
    translation_request: Option<
        Receiver<(
            crate::translation::Request,
            Result<crate::translation::Translation, crate::translation::Error>,
        )>,
    >,
    translation_request_id: u64,
    translation_active_request: Option<crate::translation::Request>,
    translation_visible: bool,
    translation_cancel: Option<crate::translation::Cancellation>,
    genius_seed: Option<PathBuf>,
    genius_request: Option<
        Receiver<(
            crate::genius::Request,
            Result<crate::genius::ResultSet, crate::genius::Error>,
        )>,
    >,
    genius_rows: Vec<crate::ui::GeniusRow>,
    genius_variation: u64,
    genius_cancel: Option<crate::genius::Cancellation>,
    lyrics_dirty: bool,
    dialog_open: bool,
    last_screen: Option<Screen>,
    last_navigation_depth: usize,
    last_query: String,
    rows_dirty: bool,
    last_track: Option<usize>,
    last_lyric: Option<usize>,
    last_selected: usize,
    last_scale: f32,
    last_window_size: slint::LogicalSize,
    resize_debouncer: super::geometry::ResizeDebouncer,
    resize_clock: std::time::Instant,
    last_notice: String,
    notice_since: std::time::Instant,
    letter_jump: Option<(std::time::Instant, Screen)>,
    thumbnails: Thumbnails,
    library_rows: Rc<VecModel<LibraryRow>>,
    row_artwork: Vec<Option<usize>>,
    artwork: ArtworkLoader,
    artwork_resolution: u32,
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
        state.language = settings.language;
        state.font = settings.font;
        state.always_on_top = settings.always_on_top;
        state.smooth_slides = settings.smooth_slides;
        state.romaji_visible = settings.romaji_visible;
        state.zoom = settings.zoom;
        state.device_mode = settings.device_mode;
        let keymap =
            Keymap::load_or_create(&directory.join("shortcuts.json")).unwrap_or_else(|message| {
                state.notice = format!("{message}; using default shortcuts");
                Keymap::default()
            });
        let database = directory.join("library.sqlite3");
        let (sender, receiver) = std::sync::mpsc::channel();
        let (library_sender, library_receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // Migrate before the scanner's large metadata SELECT can hold a read lock.
            // Both operations remain off the UI thread; a playlist error never blocks music.
            let _ = sender.send(playlists::load(&database));
            if let Ok(result) = thumbnails::load(database).recv() {
                let _ = library_sender.send(result);
            }
        });
        let loading = Some(library_receiver);
        let romaji_store = romaji::Store::new(directory.join("library.sqlite3"));
        let translation_cache =
            crate::translation::Cache::load(&directory.join("translations.json"))
                .unwrap_or_default();
        let video_loading = Some(crate::video::import::load(directory.join("videos.json")));
        Self {
            state,
            dispatcher: Dispatcher::new(keymap),
            engine: Engine::default(),
            video_engine: crate::video::engine::Engine::default(),
            video_loading,
            video_import: None,
            pending_media: None,
            pending_seek: None,
            handoff: 0,
            last_landscape: false,
            last_video_generation: 0,
            directory,
            settings,
            loading,
            playlist_loading: Some(receiver),
            playlist_saving: None,
            playlist_writer: None,
            playlist_suspended: false,
            playlist_edit_id: None,
            playlist_original_paths: HashSet::new(),
            playlist_candidates: Vec::new(),
            playlist_paths: Vec::new(),
            importing: None,
            deletion_events: None,
            lyrics_edit: None,
            romaji_request: None,
            romaji_store,
            romaji_touched: HashSet::new(),
            translation_cache,
            translation_request: None,
            translation_request_id: 0,
            translation_active_request: None,
            translation_visible: false,
            translation_cancel: None,
            genius_seed: None,
            genius_request: None,
            genius_rows: Vec::new(),
            genius_variation: 0,
            genius_cancel: None,
            lyrics_dirty: true,
            dialog_open: false,
            last_screen: None,
            last_navigation_depth: 1,
            last_query: String::new(),
            rows_dirty: true,
            last_track: None,
            last_lyric: None,
            last_selected: usize::MAX,
            last_scale: 0.0,
            last_window_size: slint::LogicalSize::new(0.0, 0.0),
            resize_debouncer: super::geometry::ResizeDebouncer::default(),
            resize_clock: std::time::Instant::now(),
            last_notice: String::new(),
            notice_since: std::time::Instant::now(),
            letter_jump: None,
            thumbnails: Thumbnails::default(),
            library_rows: Rc::new(VecModel::default()),
            row_artwork: Vec::new(),
            artwork: ArtworkLoader::default(),
            artwork_resolution: 0,
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
                    category: self.text(category.title()),
                    title: self.text(binding.action.title()),
                    keys: binding.label(cfg!(target_os = "macos")).into(),
                });
            }
        }
        ui.set_shortcuts(ModelRc::new(VecModel::from(reference)));
    }

    fn text(&self, key: &str) -> slint::SharedString {
        translate(self.state.language, key).into()
    }

    pub fn dispatch_modal_navigation(
        &mut self,
        ui: &AppWindow,
        event: slint::language::KeyEvent,
        editing: bool,
    ) -> bool {
        if self.dialog_open || self.shortcut_capture.is_some() {
            return false;
        }
        let Some(key) = Key::from_slint(event.text.as_str()) else {
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
        let resolved = self.dispatcher.dispatch(
            &press,
            Context {
                surface: crate::input::Surface::Library,
                text_input: editing,
                reference_open: self.state.reference_open,
                native_dialog: false,
            },
        );
        if resolved == Dispatch::Action(Action::Home) {
            ui.set_keyboard_navigation(true);
            self.execute(ui, Action::Home);
            return true;
        }
        if editing
            || self.state.reference_open
            || ui.get_deepseek_settings_open()
            || ui.get_playlist_editor_open()
            || ui.get_shortcut_editor_open()
            || !ui.get_genius_preview_open()
        {
            return false;
        }
        self.letter_jump = None;
        let command = match resolved {
            Dispatch::Action(Action::Back) => Some("close"),
            Dispatch::Action(Action::Activate) => Some(if ui.get_genius_songs().row_count() == 0 {
                "create"
            } else {
                "play"
            }),
            _ if !press.repeat && press.modifiers == Modifiers::default() => match press.key {
                Key::Letter('r') if ui.get_genius_preview_stage() > 0 => {
                    Some(if ui.get_genius_songs().row_count() == 0 {
                        "create"
                    } else {
                        "refresh"
                    })
                }
                Key::Letter('s') => Some("save"),
                _ => None,
            },
            _ => None,
        };
        if let Some(command) = command
            && ui.invoke_prepare_genius_action(command.into())
        {
            ui.set_keyboard_navigation(true);
            self.genius_command(ui, command);
        }
        true
    }

    fn dismiss_overlays_for_home(&mut self, ui: &AppWindow) {
        if ui.get_genius_preview_open() {
            self.genius_command(ui, "close");
            ui.set_genius_preview_open(false);
        }
        if ui.get_deepseek_settings_open() {
            ui.set_deepseek_api_key("".into());
            ui.set_deepseek_settings_open(false);
        }
        if ui.get_playlist_editor_open() {
            // Home hides the draft; reopening New Playlist resumes it unchanged.
            self.playlist_suspended = true;
            ui.set_playlist_editor_open(false);
        }
        self.shortcut_edit = None;
        self.shortcut_capture = None;
        self.sync_editor(ui);
        self.state.reference_open = false;
    }

    pub fn dispatch(
        &mut self,
        ui: &AppWindow,
        event: slint::language::KeyEvent,
        text_input: bool,
    ) -> bool {
        let key = Key::from_slint(event.text.as_str());
        if ui.get_deepseek_settings_open() && !self.state.reference_open {
            if key == Some(Key::Escape) {
                self.deepseek_command(ui, "cancel");
                return true;
            }
            return false;
        }
        if let Some(action) = self.shortcut_edit
            && !self.state.reference_open
        {
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
        if text_input || self.dialog_open || self.state.reference_open {
            self.letter_jump = None;
        }
        if self.letter_jump.as_ref().is_some_and(|(since, screen)| {
            since.elapsed().as_secs_f32() >= 3.0
                || *screen != self.state.navigation.current().screen
        }) {
            self.letter_jump = None;
        }
        if self.letter_jump.is_some() && press.repeat {
            return true;
        }
        if self.letter_jump.take().is_some()
            && !press.modifiers.primary
            && !press.modifiers.secondary
            && !press.modifiers.alt
        {
            self.state.notice.clear();
            if let Key::Letter(letter) = press.key {
                if !self.state.jump_to_letter(letter) {
                    self.state.notice = format!("No entries for {}", letter.to_ascii_uppercase());
                }
                ui.set_keyboard_navigation(true);
            }
            self.sync(ui);
            return true;
        }
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
        if action == Action::Home {
            if self.dialog_open {
                return;
            }
            self.dismiss_overlays_for_home(ui);
        }
        if matches!(action, Action::ShowShortcuts | Action::Shortcuts) {
            self.prepare_portrait_dialog(ui);
        }
        self.letter_jump = if action == Action::JumpToLetter && self.state.supports_letter_jump() {
            Some((
                std::time::Instant::now(),
                self.state.navigation.current().screen.clone(),
            ))
        } else {
            None
        };
        let effects = self.state.apply(action);
        // Start audio immediately; focus effects still need the new screen to exist.
        let (audio, effects): (Vec<_>, Vec<_>) = effects
            .into_iter()
            .partition(|effect| matches!(effect, Effect::Audio(_) | Effect::Video(_)));
        self.effects(ui, audio);
        self.sync(ui);
        self.effects(ui, effects);
    }

    fn effects(&mut self, ui: &AppWindow, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Audio(command) => self.send_music(command),
                Effect::Video(command) => self.send_video(command),
                Effect::ImportVideos => {
                    self.dialog_open = true;
                    self.video_import = Some(crate::video::import::start(
                        self.directory.join("videos.json"),
                        translate(self.state.language, "Import Videos…"),
                        None,
                    ));
                }
                Effect::Import => {
                    self.dialog_open = true;
                    self.importing = Some(thumbnails::import(
                        self.directory.join("library.sqlite3"),
                        translate(self.state.language, "Import Music Folder…"),
                    ));
                }
                Effect::FocusSearch => ui.invoke_focus_search(),
                Effect::FocusScreen => ui.invoke_focus_screen(),
                Effect::LanguageChanged => {
                    self.rows_dirty = true;
                    self.refresh_shortcuts(ui);
                    self.sync(ui);
                    self.save_settings(ui);
                }
                Effect::FontChanged | Effect::WindowPreferencesChanged => {
                    self.rows_dirty = true;
                    self.sync(ui);
                    self.save_settings(ui);
                }
                Effect::RomajiVisibilityChanged => {
                    self.rows_dirty = true;
                    self.sync(ui);
                    self.save_settings(ui);
                }
                Effect::ConfigureDeepSeek => {
                    self.prepare_portrait_dialog(ui);
                    let settings =
                        crate::ai::DeepSeekSettings::load(&self.directory.join("deepseek.json"));
                    let message = settings.as_ref().err().cloned().unwrap_or_default();
                    let settings = settings.unwrap_or_default();
                    ui.set_deepseek_key_configured(settings.has_key());
                    ui.set_deepseek_api_key("".into());
                    ui.set_deepseek_model(settings.model.into());
                    ui.set_deepseek_message(self.text(&message));
                    ui.set_deepseek_settings_open(true);
                }
                Effect::GeniusPreview => {
                    self.prepare_portrait_dialog(ui);
                    ui.invoke_open_genius_preview();
                }
                Effect::NewPlaylist => self.open_playlist_draft(ui, None),
                Effect::EditPlaylist { id } => self.open_playlist_draft(ui, Some(id)),
                Effect::GenerateRomaji { id } => self.start_romaji(ui, id),
                Effect::CancelRomaji => {
                    if self.romaji_request.take().is_some() {
                        self.state.romaji_generating = None;
                        self.state.notice = "Romaji generation cancelled".into();
                    }
                    self.sync(ui);
                }
                Effect::RemoveRomaji { id } => {
                    if let Some(track) = self.state.tracks.get(id) {
                        let path = track.path.clone();
                        if self
                            .romaji_request
                            .as_ref()
                            .is_some_and(|request| request.path == path)
                        {
                            self.romaji_request = None;
                            self.state.romaji_generating = None;
                        }
                        self.romaji_touched.insert(path.clone());
                        self.state.romaji.remove(&path);
                        self.state.notice = match self.romaji_store.remove(path) {
                            Ok(()) => "Romaji removed".into(),
                            Err(error) => format!("Romaji storage failed: {error}"),
                        };
                        self.lyrics_dirty = true;
                        self.sync(ui);
                    }
                }
                Effect::Resize => {
                    self.rows_dirty = true;
                    self.resize(ui);
                    self.sync(ui);
                    self.save_settings(ui);
                }
                Effect::EditShortcut(action) => {
                    self.prepare_portrait_dialog(ui);
                    self.shortcut_edit = Some(action);
                    self.shortcut_capture = None;
                    ui.set_shortcut_message(self.text("Choose how to edit this action."));
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
                            translate(self.state.language, "Import Lyrics"),
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

    fn prepare_portrait_dialog(&mut self, ui: &AppWindow) {
        if self.state.landscape_video() {
            // Use Done/Back semantics: pause, preserve list selection, and restore zoom.
            self.execute(ui, Action::Back);
        }
    }

    fn media_error(&mut self, message: String) {
        self.pending_media = None;
        self.pending_seek = None;
        if self.state.video_active {
            self.state.video_status = crate::video::Status::Error;
        } else {
            self.state.status = PlaybackStatus::Error;
        }
        self.state.notice = format!("Playback error: {message}");
    }

    fn send_music(&mut self, command: AudioCommand) {
        if matches!(command, AudioCommand::Load { .. }) {
            self.handoff += 1;
            self.pending_seek = None;
            self.pending_media = Some(PendingMedia::Music {
                handoff: self.handoff,
                command,
            });
            if let Err(error) = self.video_engine.send(crate::video::engine::Command::Stop {
                handoff: self.handoff,
            }) {
                self.media_error(error);
            }
        } else if self.pending_media.is_some() {
            if let AudioCommand::Seek(seconds) = command {
                self.pending_seek = Some(seconds);
            }
            // Pause and volume are read from authoritative reducer state at the acknowledgement.
        } else if let Err(error) = self.engine.send(command) {
            self.media_error(error);
        }
    }

    fn send_video(&mut self, command: crate::video::engine::Command) {
        use crate::video::engine::Command;
        if matches!(command, Command::Load { .. }) {
            self.handoff += 1;
            self.pending_seek = None;
            self.pending_media = Some(PendingMedia::Video {
                handoff: self.handoff,
                command,
            });
            if let Err(error) = self.engine.send(AudioCommand::Stop {
                handoff: self.handoff,
            }) {
                self.media_error(error);
            }
        } else if self.pending_media.is_some() {
            if let Command::Seek { seconds, .. } = command {
                self.pending_seek = Some(seconds);
            }
        } else if let Err(error) = self.video_engine.send(command) {
            self.media_error(error);
        }
    }

    fn finish_video_handoff(&mut self, handoff: u64) {
        if !matches!(&self.pending_media, Some(PendingMedia::Video { handoff: expected, .. }) if *expected == handoff)
        {
            return;
        }
        let Some(PendingMedia::Video { mut command, .. }) = self.pending_media.take() else {
            return;
        };
        if let crate::video::engine::Command::Load {
            volume,
            paused,
            start,
            ..
        } = &mut command
        {
            *volume = self.state.effective_volume();
            *paused = self.state.video_paused;
            if let Some(seconds) = self.pending_seek.take() {
                *start = seconds;
            }
        }
        if let Err(error) = self.video_engine.send(command) {
            self.media_error(error);
        }
    }

    fn finish_music_handoff(&mut self, handoff: u64) {
        if !matches!(&self.pending_media, Some(PendingMedia::Music { handoff: expected, .. }) if *expected == handoff)
        {
            return;
        }
        let Some(PendingMedia::Music { mut command, .. }) = self.pending_media.take() else {
            return;
        };
        if let AudioCommand::Load { volume, paused, .. } = &mut command {
            *volume = self.state.effective_volume();
            *paused = self.state.paused;
        }
        if let Err(error) = self.engine.send(command) {
            self.media_error(error);
            return;
        }
        if let Some(seconds) = self.pending_seek.take() {
            self.send_music(AudioCommand::Seek(seconds));
        }
    }

    /// Import original local files without changing their contents. This is also
    /// the generated-clip QA entry point, avoiding native picker automation.
    pub fn import_videos(&mut self, paths: Vec<PathBuf>) {
        if self.state.video_importing {
            return;
        }
        self.state.video_importing = true;
        self.video_import = Some(crate::video::import::start(
            self.directory.join("videos.json"),
            String::new(),
            Some(paths),
        ));
    }

    fn poll_videos(&mut self, ui: &AppWindow) {
        if let Some(result) = self
            .video_loading
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.video_loading = None;
            match result {
                Ok(videos) => self.state.merge_videos(videos),
                Err(error) => self.state.notice = format!("Video library error: {error}"),
            }
            self.rows_dirty = true;
        }
        let imports: Vec<_> = self
            .video_import
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for event in imports {
            match event {
                crate::video::import::Event::SelectionClosed => self.dialog_open = false,
                crate::video::import::Event::Completed(result) => {
                    self.video_import = None;
                    self.state.video_importing = false;
                    match result {
                        Ok(Some(result)) => {
                            self.state.merge_videos(result.videos);
                            self.state.notice = if result.errors.is_empty() {
                                format!("Imported {} videos", result.imported)
                            } else {
                                format!(
                                    "Imported {} videos · {} errors: {}",
                                    result.imported,
                                    result.errors.len(),
                                    result.errors[0]
                                )
                            };
                        }
                        Ok(None) => self.state.notice = "Import cancelled".into(),
                        Err(error) => self.state.notice = format!("Video import failed: {error}"),
                    }
                    self.rows_dirty = true;
                    ui.invoke_focus_screen();
                }
            }
        }
        while let Ok(event) = self.video_engine.events.try_recv() {
            match event {
                crate::video::engine::Event::Stopped { handoff } => {
                    self.finish_music_handoff(handoff)
                }
                crate::video::engine::Event::Finished(generation)
                    if self.state.video_active && generation == self.state.video_generation =>
                {
                    self.state.video_status = crate::video::Status::Finished;
                    self.state.video_paused = true;
                    self.state.video_position = self.state.duration();
                    self.state.video_overlay = true;
                }
                crate::video::engine::Event::Error {
                    generation,
                    message,
                } if self.state.video_active && generation == self.state.video_generation => {
                    self.state.video_status = crate::video::Status::Error;
                    self.state.video_paused = true;
                    self.state.notice = format!("Playback error: {message}");
                    ui.set_video_frame(slint::Image::default());
                }
                _ => {}
            }
        }
        if let Some(progress) = self.video_engine.take_progress()
            && self.state.video_active
            && self.pending_media.is_none()
            && progress.generation == self.state.video_generation
        {
            self.state.video_position = progress.position;
            self.state.video_status = match progress.status {
                crate::video::Status::Playing | crate::video::Status::Paused
                    if self.state.video_paused =>
                {
                    crate::video::Status::Paused
                }
                status => status,
            };
        }
    }

    /// A lightweight 60 Hz frame pump; metadata/navigation remain on the 50 ms poll.
    pub fn poll_video_frame(&mut self, ui: &AppWindow) {
        if let Some(frame) = self.video_engine.take_frame()
            && self.state.video_active
            && self.pending_media.is_none()
            && frame.generation == self.state.video_generation
            && self.state.navigation.current().screen == Screen::VideoPlaying
            && self.state.video_status != crate::video::Status::Error
        {
            let pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                &frame.rgba,
                frame.width,
                frame.height,
            );
            ui.set_video_frame(slint::Image::from_rgba8(pixels));
        }
    }

    fn base_size(&self) -> (f32, f32) {
        crate::video::orientation::window_base(self.state.device_mode, self.state.landscape_video())
    }

    fn resize(&mut self, ui: &AppWindow) {
        self.resize_debouncer.reset();
        ui.set_device_mode(self.state.device_mode);
        ui.set_video_landscape(self.state.landscape_video());
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
        let content_height = ui.get_content_height();
        if let Some(zoom) = self.resize_debouncer.observe(
            self.resize_clock.elapsed().as_millis() as u64,
            (size.width, size.height),
            (self.last_window_size.width, self.last_window_size.height),
            content_height,
            self.base_size(),
            (size.height - content_height).max(0.0),
        ) {
            self.state.zoom = zoom;
            self.resize(ui);
        }
    }

    pub fn save_settings(&mut self, ui: &AppWindow) {
        let size = ui.window().size().to_logical(ui.window().scale_factor());
        let (saved_width, saved_height) = if self.state.landscape_video() {
            crate::video::orientation::saved_size(
                self.state.device_mode,
                self.state.zoom,
                (size.height - ui.get_content_height()).max(0.0),
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
            zoom: self.state.zoom,
            device_mode: self.state.device_mode,
            monitor_deleted_files: self.settings.monitor_deleted_files,
            monitor_roots: self.settings.monitor_roots.clone(),
            window_width: Some(saved_width),
            window_height: Some(saved_height),
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
                Ok(prepared) => {
                    self.state.merge_library(prepared.value);
                    if self.settings.monitor_deleted_files {
                        let missing: Vec<_> = self
                            .state
                            .tracks
                            .iter()
                            .filter(|track| {
                                !track.path.is_file()
                                    && track.path.parent().is_some_and(|parent| parent.is_dir())
                            })
                            .map(|track| track.path.clone())
                            .collect();
                        for path in missing {
                            self.state.mark_track_available(&path, false);
                        }
                    }
                    self.start_deletion_monitor();
                    self.lyrics_dirty = true;
                    self.thumbnails.clear();
                    self.thumbnails.seed(&self.state.tracks, prepared.previews);
                    self.artwork.clear();
                    self.last_track = None;
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
            let thumbnails::ImportEvent::Completed(result) = event else {
                self.dialog_open = false;
                self.state.notice = "Scanning music folders…".to_owned();
                ui.invoke_focus_screen();
                continue;
            };
            self.importing = None;
            self.state.importing = false;
            self.dialog_open = false;
            match result {
                Ok(Some(prepared)) => {
                    let result = prepared.value;
                    let (present, missing): (Vec<_>, Vec<_>) = result
                        .tracks
                        .into_iter()
                        .partition(|track| track.path.is_file());
                    if !missing.is_empty() {
                        let database = self.directory.join("library.sqlite3");
                        std::thread::spawn(move || {
                            if let Ok(database) =
                                crate::library::database::Database::open(&database)
                            {
                                for track in missing {
                                    let _ = database.evict_track(&track.path);
                                }
                            }
                        });
                    }
                    self.state.merge_library(present);
                    self.start_deletion_monitor();
                    self.lyrics_dirty = true;
                    self.thumbnails.clear();
                    self.thumbnails.seed(&self.state.tracks, prepared.previews);
                    self.artwork.clear();
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
                        self.lyrics_dirty = true;
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
        self.poll_romaji(ui);
        self.poll_deletions();
        self.poll_translation(ui);
        self.poll_genius(ui);
        self.poll_playlists(ui);
        self.poll_videos(ui);
        while let Ok(event) = self.engine.events.try_recv() {
            match event {
                AudioEvent::Stopped { handoff } => self.finish_video_handoff(handoff),
                AudioEvent::Progress {
                    id,
                    status,
                    position,
                } if !self.state.video_active
                    && self.pending_media.is_none()
                    && self.state.current == Some(id) =>
                {
                    self.state.position = position;
                    self.state.status = if self.state.paused {
                        PlaybackStatus::Paused
                    } else {
                        status
                    };
                }
                AudioEvent::Finished(id)
                    if !self.state.video_active
                        && self.pending_media.is_none()
                        && self.state.current == Some(id) =>
                {
                    let effects = self.state.advance(false);
                    self.effects(ui, effects);
                }
                AudioEvent::Error { id, message }
                    if !self.state.video_active
                        && self.pending_media.is_none()
                        && (id.is_none() || id == self.state.current) =>
                {
                    self.state.status = PlaybackStatus::Error;
                    self.state.notice = format!("Playback error: {message}");
                }
                _ => {}
            }
        }
        self.sync(ui);
        if let Some(artwork) = self.artwork.poll() {
            ui.set_artwork(artwork);
        }
    }

    fn start_deletion_monitor(&mut self) {
        let missing: Vec<_> = self
            .state
            .tracks
            .iter()
            .filter(|track| !self.state.track_available(track))
            .map(|track| track.path.clone())
            .collect();
        if !missing.is_empty() {
            let db = self.directory.join("library.sqlite3");
            std::thread::spawn(move || {
                if let Ok(database) = crate::library::database::Database::open(&db) {
                    for path in missing {
                        let _ = database.evict_track(&path);
                    }
                }
            });
        }
        if !self.settings.monitor_deleted_files {
            return;
        }
        let mut roots: Vec<_> = self
            .state
            .tracks
            .iter()
            .filter_map(|track| track.path.parent().map(PathBuf::from))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        roots.extend(self.settings.monitor_roots.iter().cloned());
        roots.sort();
        roots.dedup();
        for root in &roots {
            if !self.settings.monitor_roots.contains(root) {
                self.settings.monitor_roots.push(root.clone());
            }
        }
        if let Some(monitor) = &self.deletion_events {
            monitor.add_roots(roots);
        } else if !roots.is_empty() {
            self.deletion_events = Some(crate::library::deletion_monitor::watch(roots));
        }
        let _ = self.settings.save(&self.directory.join("settings.json"));
    }

    fn poll_deletions(&mut self) {
        let Some(events) = self.deletion_events.as_ref().map(|monitor| &monitor.events) else {
            return;
        };
        let mut removed = Vec::new();
        let mut restored = Vec::new();
        let mut changed_roots = Vec::new();
        let mut errors = Vec::new();
        while let Ok(event) = events.try_recv() {
            match event {
                crate::library::deletion_monitor::EventMessage::Removed(path) => removed.push(path),
                crate::library::deletion_monitor::EventMessage::Restored(path) => {
                    restored.push(path)
                }
                crate::library::deletion_monitor::EventMessage::RootChanged(path) => {
                    changed_roots.push(path)
                }
                crate::library::deletion_monitor::EventMessage::Error(error) => errors.push(error),
            }
        }
        for path in restored {
            if let Some(track_path) = self
                .state
                .tracks
                .iter()
                .find(|track| crate::library::deletion_monitor::normalize(&track.path) == path)
                .map(|track| track.path.clone())
            {
                self.state.mark_track_available(&track_path, true);
                self.rows_dirty = true;
            }
        }
        for root in changed_roots {
            let missing: Vec<_> = self
                .state
                .tracks
                .iter()
                .enumerate()
                .filter(|(_, track)| self.state.track_available(track) && !track.path.is_file())
                .filter(|(_, track)| {
                    crate::library::deletion_monitor::normalize(&track.path).starts_with(&root)
                })
                .map(|(id, track)| (id, track.path.clone()))
                .collect();
            for (id, path) in missing {
                self.state.mark_track_available(&path, false);
                if self.state.current == Some(id) {
                    self.handoff += 1;
                    let _ = self.engine.send(AudioCommand::Stop {
                        handoff: self.handoff,
                    });
                    self.state.current = None;
                    self.state.status = PlaybackStatus::Stopped;
                    self.state.position = 0.0;
                    self.last_track = None;
                    self.lyrics_dirty = true;
                }
                let db = self.directory.join("library.sqlite3");
                std::thread::spawn(move || {
                    if let Ok(database) = crate::library::database::Database::open(&db) {
                        let _ = database.evict_track(&path);
                    }
                });
                self.rows_dirty = true;
            }
            let restored: Vec<_> = self
                .state
                .tracks
                .iter()
                .filter(|track| !self.state.track_available(track) && track.path.is_file())
                .filter(|track| {
                    crate::library::deletion_monitor::normalize(&track.path).starts_with(&root)
                })
                .map(|track| track.path.clone())
                .collect();
            for path in restored {
                self.state.mark_track_available(&path, true);
                self.rows_dirty = true;
            }
        }
        for path in removed {
            if let Some((id, track_path)) = self
                .state
                .tracks
                .iter()
                .enumerate()
                .find(|(_, track)| crate::library::deletion_monitor::normalize(&track.path) == path)
                .map(|(id, track)| (id, track.path.clone()))
            {
                self.state.mark_track_available(&track_path, false);
                if self.state.current == Some(id) {
                    self.handoff += 1;
                    let _ = self.engine.send(AudioCommand::Stop {
                        handoff: self.handoff,
                    });
                    self.state.current = None;
                    self.state.status = PlaybackStatus::Stopped;
                    self.state.position = 0.0;
                    self.last_track = None;
                    self.lyrics_dirty = true;
                }
                let db = self.directory.join("library.sqlite3");
                std::thread::spawn(move || {
                    if let Ok(database) = crate::library::database::Database::open(&db) {
                        let _ = database.evict_track(&track_path);
                    }
                });
                self.rows_dirty = true;
            }
        }
        if !errors.is_empty() {
            self.state.notice = format!("File monitoring paused after an error: {}", errors[0]);
        }
    }

    fn sync(&mut self, ui: &AppWindow) {
        ui.global::<I18n>()
            .set_language(self.state.language.index() as i32);
        let current_screen = self.state.navigation.current().screen.clone();
        let screen = &current_screen;
        let screen_changed = self.last_screen.as_ref() != Some(screen);
        let home_page = self.state.home_page as i32;
        if matches!(screen, Screen::Home) && ui.get_home_page() != home_page {
            ui.set_home_transition_direction(if home_page > ui.get_home_page() {
                1
            } else {
                -1
            });
        }
        let navigation_depth = self.state.navigation.depth();
        let transition_direction = if navigation_depth >= self.last_navigation_depth {
            1
        } else {
            -1
        };
        ui.set_smooth_slides(self.state.smooth_slides);
        if !self.state.smooth_slides {
            ui.invoke_cancel_screen_transition();
        }
        ui.set_transition_direction(transition_direction);
        if screen_changed && self.last_screen.is_some() {
            ui.set_previous_screen(ui.get_screen());
            ui.set_previous_screen_title(ui.get_screen_title());
            ui.set_previous_selected(ui.get_selected());
            ui.set_previous_home_page(ui.get_home_page());
            ui.set_previous_home_transition_direction(ui.get_home_transition_direction());
            ui.set_previous_rows(ui.get_rows());
            ui.set_previous_playlist_list(ui.get_playlist_list());
            ui.set_previous_playlist_content(ui.get_playlist_content());
            ui.set_previous_shuffle_list(ui.get_shuffle_list());
            ui.set_previous_list_scroll_y(ui.get_list_scroll_y());
            ui.set_previous_lyric_scroll_y(ui.get_lyric_scroll_y());
        }
        let landscape = self.state.landscape_video();
        ui.set_video_landscape(landscape);
        if landscape != self.last_landscape {
            self.last_landscape = landscape;
            self.resize(ui);
        }
        if self.last_video_generation != self.state.video_generation {
            self.last_video_generation = self.state.video_generation;
            ui.set_video_frame(slint::Image::default());
        }
        ui.set_video_overlay(self.state.video_overlay);
        ui.set_video_title(
            self.state
                .video_current
                .and_then(|id| self.state.videos.get(id))
                .map_or("", |v| v.title.as_str())
                .into(),
        );
        ui.set_screen(match screen {
            Screen::Home => 0,
            Screen::NowPlaying => 2,
            Screen::VideoPlaying => 9,
            Screen::Search => 3,
            Screen::Placeholder(_) => 4,
            Screen::Settings => 5,
            Screen::Shortcuts => 6,
            Screen::Lyrics(_) => 7,
            Screen::Languages | Screen::Fonts => 8,
            _ => 1,
        });
        ui.set_screen_title(match screen {
            Screen::Artist(_)
            | Screen::Songs(Filter::Album { .. } | Filter::Genre(_) | Filter::Playlist { .. }) => {
                screen.title().into()
            }
            _ => self.text(screen.title()),
        });
        ui.set_device_mode(self.state.device_mode);
        ui.set_playlist_list(matches!(screen, Screen::Groups(Group::Playlists)));
        ui.set_playlist_content(matches!(screen, Screen::Songs(Filter::Playlist { .. })));
        ui.set_shuffle_list(self.state.supports_list_shuffle());
        ui.set_ui_font_family(self.state.font.family().into());
        ui.set_keep_on_top(self.state.always_on_top);
        ui.set_selected(self.state.navigation.selected() as i32);
        ui.set_home_page(self.state.home_page as i32);
        ui.set_clock_text(chrono::Local::now().format("%-I:%M %p").to_string().into());
        ui.set_playing(if self.state.video_active {
            self.state.video_status == crate::video::Status::Playing
        } else {
            self.state.status == PlaybackStatus::Playing
        });
        ui.set_has_track(self.state.current.is_some());
        ui.set_track_path(
            self.state
                .current
                .and_then(|id| self.state.tracks.get(id))
                .map_or_else(String::new, |track| {
                    track.path.to_string_lossy().into_owned()
                })
                .into(),
        );
        ui.set_volume(self.state.volume);
        ui.set_muted(self.state.muted);
        ui.set_shuffle(self.state.shuffle);
        ui.set_repeat_mode(self.state.repeat.label().into());
        if self.last_notice != self.state.notice {
            self.last_notice.clone_from(&self.state.notice);
            self.notice_since = std::time::Instant::now();
        }
        ui.set_notice(
            if self.notice_since.elapsed().as_secs() < 3
                || self.state.importing
                || self.state.video_importing
                || self.state.romaji_generating.is_some()
            {
                i18n::notice(self.state.language, &self.state.notice).into()
            } else {
                "".into()
            },
        );
        ui.set_reference_open(self.state.reference_open);
        ui.set_lyrics_visible(self.state.lyrics_visible);
        ui.set_romaji_visible(self.state.romaji_visible);
        ui.set_translation_visible(self.translation_visible);
        ui.set_overlay_visible(self.state.overlay_visible);
        ui.set_track_count(if self.state.queue.order.is_empty() {
            "".into()
        } else {
            translate(self.state.language, "{0} of {1}")
                .replace("{0}", &(self.state.queue.position + 1).to_string())
                .replace("{1}", &self.state.queue.order.len().to_string())
                .into()
        });
        let duration = self.state.duration();
        ui.set_progress(if duration > 0.0 {
            (self.state.active_position() / duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        });
        ui.set_elapsed(format_time(self.state.active_position()).into());
        ui.set_duration(format_time(duration).into());
        ui.set_remaining(
            format!(
                "−{}",
                format_time((duration - self.state.active_position()).max(0.0))
            )
            .into(),
        );
        if screen_changed || self.rows_dirty || self.last_query != self.state.query {
            let metadata_rows = matches!(
                screen,
                Screen::Groups(Group::Artists | Group::Albums | Group::Genres)
                    | Screen::Artist(_)
                    | Screen::Songs(_)
                    | Screen::Search
                    | Screen::Videos
            );
            let source_rows = self.state.rows();
            self.row_artwork = source_rows.iter().map(|row| row.artwork_id).collect();
            let rows = source_rows
                .into_iter()
                .map(|row| LibraryRow {
                    title: if matches!(&row.target, Target::PlayVideo(_))
                        || (metadata_rows && !matches!(&row.target, Target::Action(_)))
                        || matches!(
                            &row.target,
                            Target::Screen(Screen::Songs(Filter::Playlist { .. }))
                        ) {
                        row.title.into()
                    } else {
                        self.text(&row.title)
                    },
                    subtitle: if let Target::Shortcut(action) = row.target {
                        self.text(&self.dispatcher.keymap.labels(action))
                    } else if matches!(screen, Screen::Groups(Group::Playlists)) {
                        i18n::notice(self.state.language, &row.subtitle).into()
                    } else {
                        if metadata_rows
                            || matches!(
                                screen,
                                Screen::Lyrics(_) | Screen::Languages | Screen::Fonts
                            )
                        {
                            row.subtitle.into()
                        } else {
                            self.text(&row.subtitle)
                        }
                    },
                    artwork: row
                        .artwork_id
                        .and_then(|id| self.thumbnails.cached(id))
                        .unwrap_or_default(),
                    show_artwork: !matches!(row.target, Target::Action(_))
                        && matches!(
                            screen,
                            Screen::Groups(
                                super::navigation::Group::Artists
                                    | super::navigation::Group::Albums
                            ) | Screen::Artist(_)
                        ),
                    chevron: !matches!(
                        row.target,
                        Target::Play(_)
                            | Target::Action(Action::ShuffleList)
                            | Target::PlayVideo(_)
                            | Target::Language(_)
                            | Target::Font(_)
                    ),
                    font_family: if let Target::Font(font) = row.target {
                        font.family().into()
                    } else {
                        "".into()
                    },
                })
                .collect::<Vec<_>>();
            self.library_rows = Rc::new(VecModel::from(rows));
            ui.set_rows(self.library_rows.clone().into());
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
                    | Screen::Videos
                    | Screen::Groups(_)
                    | Screen::Artist(_)
                    | Screen::Songs(_)
                    | Screen::Search
                    | Screen::Settings
                    | Screen::Shortcuts
                    | Screen::Lyrics(_)
                    | Screen::Languages
                    | Screen::Fonts
            ) {
                let row_height = ui.get_list_row_height();
                let playlist_list = *screen == Screen::Groups(Group::Playlists);
                let row_index = self.state.navigation.selected().saturating_sub(
                    if matches!(screen, Screen::Songs(Filter::Playlist { .. })) {
                        2
                    } else {
                        usize::from(playlist_list || self.state.supports_list_shuffle())
                    },
                );
                let row_y = row_index as f32 * row_height;
                let visible_height = ui.get_list_viewport_height() - ui.get_list_header_height();
                let scroll = ui.get_list_scroll_y();
                if row_y + scroll < 0.0 {
                    ui.set_list_scroll_y(-row_y);
                } else if row_y + row_height + scroll > visible_height {
                    ui.set_list_scroll_y(visible_height - row_y - row_height);
                }
            }
            self.last_selected = self.state.navigation.selected();
        }
        self.refresh_thumbnails(ui, screen);
        if screen_changed && self.last_screen.is_some() {
            ui.set_screen_transition_generation(ui.get_screen_transition_generation() + 1);
        }
        self.last_screen = Some(screen.clone());
        self.last_navigation_depth = navigation_depth;
        let track = self.state.current.and_then(|id| self.state.tracks.get(id));
        let artwork_resolution = (240.0 * scale * ui.window().scale_factor()).ceil() as u32;
        let track_changed = self.last_track != self.state.current;
        if track_changed || artwork_resolution != self.artwork_resolution {
            let artwork = track
                .zip(self.state.current)
                .map(|(track, id)| {
                    let preview = self.thumbnails.cached(id).or_else(|| {
                        self.thumbnails
                            .cached_images()
                            .find_map(|(cached_id, image)| {
                                self.state
                                    .tracks
                                    .get(cached_id)
                                    .filter(|cached| {
                                        image.size().width > 0 && cached.artwork == track.artwork
                                    })
                                    .map(|_| image.clone())
                            })
                    });
                    self.artwork.seed_preview(id, preview);
                    self.artwork.request(id, &track.artwork, artwork_resolution)
                })
                .unwrap_or_else(|| {
                    self.artwork.cancel();
                    slint::Image::default()
                });
            // Keep the existing cover visible while a sharper version is prepared on resize.
            if track_changed || artwork.size().width > 0 {
                ui.set_artwork(artwork);
            }
            self.artwork_resolution = artwork_resolution;
        }
        if self.last_track != self.state.current || self.lyrics_dirty {
            ui.set_track_title(track.map_or("", |t| t.title.as_str()).into());
            ui.set_track_artist(track.map_or("", |t| t.artist.as_str()).into());
            ui.set_track_album(track.map_or("", |t| t.album.as_str()).into());
            ui.set_lyric_lines(ModelRc::new(VecModel::from(
                track.map_or_else(Vec::new, |t| {
                    t.lyrics.lines().into_iter().map(Into::into).collect()
                }),
            )));
            ui.set_timed_lyrics(track.is_some_and(|t| !t.lyrics.timed.is_empty()));
            let translated = track.and_then(|track| {
                let request = crate::translation::Request {
                    request_id: 0,
                    track_id: track.path.clone(),
                    target: self.settings.translation_target,
                    provider_version: self.translation_provider_version(),
                    source: Self::translation_source(track),
                };
                self.translation_cache.get(&request.cache_key())
            });
            ui.set_translated_lines(ModelRc::new(VecModel::from(translated.map_or_else(
                Vec::new,
                |translation| {
                    translation
                        .lines
                        .iter()
                        .map(|line| line.as_str().into())
                        .collect()
                },
            ))));
            let romaji = track.and_then(|track| {
                self.state
                    .romaji
                    .get(&track.path)
                    .filter(|romaji| romaji.matches(&track.lyrics))
            });
            ui.set_romaji_lines(ModelRc::new(VecModel::from(romaji.map_or_else(
                Vec::new,
                |romaji| {
                    romaji
                        .lines
                        .iter()
                        .map(|line| line.as_str().into())
                        .collect()
                },
            ))));
            ui.set_lyric_scroll_y(0.0);
            self.last_lyric = None;
            self.last_track = self.state.current;
            self.lyrics_dirty = false;
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

    fn refresh_thumbnails(&mut self, ui: &AppWindow, screen: &Screen) {
        if self.thumbnails.poll() {
            for (index, id) in self.row_artwork.iter().enumerate() {
                if let Some(id) = id
                    && let Some(mut row) = self.library_rows.row_data(index)
                {
                    // Release evicted large images even when a synthetic track has no preview.
                    let image = self.thumbnails.cached(*id).unwrap_or_default();
                    if row.artwork != image {
                        row.artwork = image;
                        self.library_rows.set_row_data(index, row);
                    }
                }
            }
        }
        let resolution = ((ui.global::<ListMetrics>().get_artwork_row_height()
            - 2.0 * ui.global::<ListMetrics>().get_artwork_padding())
            * ui.get_display_scale()
            * ui.window().scale_factor())
        .ceil() as u32;
        let ids = if matches!(
            screen,
            Screen::Groups(Group::Albums | Group::Artists) | Screen::Artist(_)
        ) {
            let height = ui.get_list_row_height().max(1.0);
            let top = (-ui.get_list_scroll_y()).max(0.0);
            let prefix = usize::from(self.state.supports_list_shuffle());
            let start = (top / height).floor() as usize + prefix;
            let end = ((top + ui.get_list_viewport_height() - ui.get_list_header_height()) / height)
                .ceil() as usize
                + prefix;
            self.row_artwork
                .iter()
                .take(end.saturating_add(1))
                .skip(start.saturating_sub(1))
                .filter_map(|id| *id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        self.thumbnails
            .set_visible(&self.state.tracks, &ids, resolution);
    }

    fn refresh_playlist_candidates(&mut self) {
        let mut songs: Vec<_> = self
            .state
            .tracks
            .iter()
            .filter(|song| self.state.track_available(song))
            .collect();
        songs.sort_by_cached_key(|song| {
            (
                song.title.to_lowercase(),
                song.title.clone(),
                song.path.clone(),
            )
        });
        self.playlist_candidates = songs
            .into_iter()
            .map(|song| PlaylistCandidate {
                id: song.path.to_string_lossy().as_ref().into(),
                title: song.title.as_str().into(),
                artist: song.artist.as_str().into(),
                selected: false,
            })
            .collect();
    }

    fn open_playlist_draft(&mut self, ui: &AppWindow, id: Option<i64>) {
        if ui.get_playlist_editor_open()
            || ui.get_deepseek_settings_open()
            || ui.get_genius_preview_open()
            || ui.get_shortcut_editor_open()
            || self.state.reference_open
            || self.dialog_open
        {
            return;
        }
        self.prepare_portrait_dialog(ui);
        if self.playlist_suspended {
            self.playlist_suspended = false;
            ui.set_playlist_editor_open(true);
            ui.invoke_focus_screen();
            return;
        }
        if let Some(id) = id {
            let Some(playlist) = self.state.playlists.iter().find(|p| p.id == id).cloned() else {
                return;
            };
            self.playlist_edit_id = Some(id);
            self.playlist_paths = playlist.tracks;
            self.playlist_original_paths = self.playlist_paths.iter().cloned().collect();
            self.refresh_playlist_candidates();
            ui.set_playlist_editing(true);
            ui.set_playlist_name(playlist.name.into());
            ui.set_playlist_persisted(false);
            ui.set_playlist_message("Changes not saved".into());
            self.sync_playlist_draft(ui);
            ui.invoke_open_saved_playlist_editor();
        } else {
            self.playlist_command(ui, "new", "");
            ui.invoke_open_playlist_editor();
        }
    }

    fn sync_playlist_draft(&self, ui: &AppWindow) {
        let selected: HashSet<_> = self.playlist_paths.iter().collect();
        let candidates: Vec<_> = self
            .playlist_candidates
            .iter()
            .cloned()
            .map(|mut song| {
                song.selected = selected.contains(&PathBuf::from(song.id.as_str()));
                song
            })
            .collect();
        let by_path: std::collections::HashMap<_, _> = candidates
            .iter()
            .map(|song| (PathBuf::from(song.id.as_str()), song))
            .collect();
        let draft = self
            .playlist_paths
            .iter()
            .map(|path| {
                by_path
                    .get(path)
                    .map(|song| (*song).clone())
                    .unwrap_or_else(|| PlaylistCandidate {
                        id: path.to_string_lossy().as_ref().into(),
                        title: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .as_ref()
                            .into(),
                        artist: self.text("Unavailable song"),
                        selected: true,
                    })
            })
            .collect::<Vec<_>>();
        ui.set_playlist_candidates(ModelRc::new(VecModel::from(candidates)));
        ui.set_playlist_draft(ModelRc::new(VecModel::from(draft)));
    }

    pub fn playlist_command(&mut self, ui: &AppWindow, command: &str, id: &str) {
        if self.playlist_saving.is_some() {
            return;
        }
        if !matches!(command, "new" | "cancel") && !ui.get_playlist_editor_open() {
            return;
        }
        let path = PathBuf::from(id);
        if let Some(target) = command
            .strip_prefix("move:")
            .and_then(|index| index.parse::<usize>().ok())
        {
            if let Some(source) = self.playlist_paths.iter().position(|p| p == &path) {
                let target = target.min(self.playlist_paths.len() - 1);
                let path = self.playlist_paths.remove(source);
                self.playlist_paths.insert(target, path);
                ui.set_playlist_message(
                    if self.playlist_edit_id.is_some() {
                        "Changes not saved"
                    } else {
                        "Draft only · not saved"
                    }
                    .into(),
                );
                self.sync_playlist_draft(ui);
            }
            return;
        }
        match command {
            "new" => {
                self.playlist_edit_id = None;
                self.playlist_original_paths.clear();
                ui.set_playlist_editing(false);
                self.playlist_paths.clear();
                self.refresh_playlist_candidates();
                ui.set_playlist_saving(false);
                ui.set_playlist_persisted(false);
                ui.set_playlist_library_loading(
                    self.state.tracks.is_empty() && self.loading.is_some(),
                );
            }
            "toggle" => {
                if !self.playlist_candidates.iter().any(|song| song.id == id) {
                    return;
                }
                if let Some(index) = self.playlist_paths.iter().position(|p| *p == path) {
                    self.playlist_paths.remove(index);
                } else {
                    self.playlist_paths.push(path);
                }
            }
            "review" => {
                if ui.get_playlist_name().trim().is_empty() && self.playlist_edit_id.is_none() {
                    let prefix = self.text("New Playlist");
                    let mut number = 1;
                    let name = loop {
                        let name = format!("{prefix} {number}");
                        if !self
                            .state
                            .playlists
                            .iter()
                            .any(|playlist| playlist.name.to_lowercase() == name.to_lowercase())
                        {
                            break name;
                        }
                        number += 1;
                    };
                    ui.set_playlist_name(name.into());
                }
            }
            "up" | "down" => {
                if let Some(index) = self.playlist_paths.iter().position(|p| *p == path) {
                    let next = if command == "up" {
                        index.saturating_sub(1)
                    } else {
                        (index + 1).min(self.playlist_paths.len() - 1)
                    };
                    self.playlist_paths.swap(index, next);
                }
            }
            "remove" => self.playlist_paths.retain(|p| *p != path),
            "cancel" => {
                self.playlist_edit_id = None;
                self.playlist_original_paths.clear();
                self.playlist_suspended = false;
                self.playlist_paths.clear();
                self.playlist_candidates.clear();
            }
            "create" => {
                let name = ui.get_playlist_name().trim().to_owned();
                let paths = self.playlist_paths.clone();
                let edit_id = self.playlist_edit_id;
                let original = self.playlist_original_paths.clone();
                let available: HashSet<_> = self
                    .state
                    .tracks
                    .iter()
                    .map(|song| song.path.clone())
                    .collect();
                let database = self.directory.join("library.sqlite3");
                let (sender, receiver) = std::sync::mpsc::channel();
                self.playlist_writer = Some(std::thread::spawn(move || {
                    let result = if paths
                        .iter()
                        .any(|p| !original.contains(p) && (!available.contains(p) || !p.is_file()))
                    {
                        Err("selected songs are unavailable".into())
                    } else if let Some(id) = edit_id {
                        playlists::update(&database, id, &name, &paths)
                    } else {
                        playlists::create(&database, &name, &paths)
                    };
                    let _ = sender.send(result);
                }));
                self.playlist_saving = Some(receiver);
                ui.set_playlist_saving(true);
                ui.set_playlist_message("Saving playlist…".into());
                return;
            }
            _ => return,
        }
        ui.set_playlist_message(
            if self.playlist_edit_id.is_some() {
                "Changes not saved"
            } else {
                "Draft only · not saved"
            }
            .into(),
        );
        self.sync_playlist_draft(ui);
    }

    fn poll_playlists(&mut self, ui: &AppWindow) {
        if let Some(result) = self
            .playlist_loading
            .as_ref()
            .and_then(|r| r.try_recv().ok())
        {
            self.playlist_loading = None;
            match result {
                Ok(playlists) => {
                    for playlist in playlists {
                        if !self.state.playlists.iter().any(|p| p.id == playlist.id) {
                            self.state.playlists.push(playlist);
                        }
                    }
                    self.rows_dirty = true;
                }
                Err(_) => self.state.notice = "Could not load playlists".into(),
            }
        }
        if ui.get_playlist_editor_open() {
            ui.set_playlist_library_loading(self.state.tracks.is_empty() && self.loading.is_some());
            if self.playlist_candidates.is_empty() && !self.state.tracks.is_empty() {
                self.refresh_playlist_candidates();
                self.sync_playlist_draft(ui);
            }
        }
        if let Some(result) = self
            .playlist_saving
            .as_ref()
            .and_then(|r| r.try_recv().ok())
        {
            self.playlist_saving = None;
            if let Some(worker) = self.playlist_writer.take() {
                let _ = worker.join();
            }
            ui.set_playlist_saving(false);
            match result {
                Ok(playlist) => {
                    let screen = Screen::Songs(Filter::Playlist {
                        id: playlist.id,
                        name: playlist.name.clone(),
                    });
                    let editing = self.playlist_edit_id.take().is_some();
                    self.state
                        .navigation
                        .rename_playlist(playlist.id, &playlist.name);
                    if let Some(existing) = self
                        .state
                        .playlists
                        .iter_mut()
                        .find(|p| p.id == playlist.id)
                    {
                        *existing = playlist;
                    } else {
                        self.state.playlists.push(playlist);
                    }
                    self.rows_dirty = true;
                    ui.set_playlist_persisted(true);
                    if ui.get_playlist_editor_open() && !editing {
                        self.state.navigation.open(Screen::Groups(Group::Playlists));
                        self.state.navigation.open(screen);
                    }
                    ui.set_playlist_editor_open(false);
                    self.playlist_suspended = false;
                    self.state.notice = "Playlist saved".into();
                    self.playlist_paths.clear();
                    self.playlist_original_paths.clear();
                    self.playlist_candidates.clear();
                    ui.invoke_focus_screen();
                }
                Err(error) => {
                    let message = match error.as_str() {
                        "playlist name cannot be empty" => "Choose a playlist name",
                        "playlist name cannot exceed 100 characters" => {
                            "Playlist name must be 100 characters or fewer"
                        }
                        "playlist name contains control characters" => {
                            "Use a single-line playlist name"
                        }
                        "playlist must contain at least one track" => "Choose at least one song",
                        "a playlist with that name already exists" => {
                            "A playlist with this name already exists"
                        }
                        "selected songs are unavailable" => "Some selected songs are unavailable",
                        "playlist no longer exists" => "Playlist no longer exists",
                        _ => "Could not save playlist. Try again.",
                    };
                    ui.set_playlist_message(message.into());
                    if !ui.get_playlist_editor_open() {
                        self.state.notice = message.into();
                    }
                }
            }
        }
    }

    fn start_romaji(&mut self, ui: &AppWindow, id: usize) {
        if self.romaji_request.is_some() {
            self.state.notice = "Romaji generation is already running".into();
            self.sync(ui);
            return;
        }
        let result = (|| {
            let track = self.state.tracks.get(id).ok_or("Choose a song first")?;
            let settings =
                crate::ai::DeepSeekSettings::load(&self.directory.join("deepseek.json"))?;
            romaji::Request::start(
                track.path.clone(),
                track.lyrics.clone(),
                track.title.clone(),
                track.artist.clone(),
                settings,
            )
        })();
        match result {
            Ok(request) => {
                self.state.romaji_generating = Some(request.path.clone());
                self.romaji_request = Some(request);
                self.state.notice = "Generating romaji…".into();
            }
            Err(error) => self.state.notice = error,
        }
        self.sync(ui);
    }

    fn poll_romaji(&mut self, ui: &AppWindow) {
        while let Ok(event) = self.romaji_store.events.try_recv() {
            match event {
                romaji::StoreEvent::Loaded(Ok(cache)) => {
                    for (path, romaji) in cache {
                        if !self.romaji_touched.contains(&path) {
                            self.state.romaji.entry(path).or_insert(romaji);
                        }
                    }
                    self.lyrics_dirty = true;
                }
                romaji::StoreEvent::Loaded(Err(error))
                | romaji::StoreEvent::Written(Err(error)) => {
                    self.state.notice = format!("Romaji storage failed: {error}");
                }
                romaji::StoreEvent::Written(Ok(())) => {}
            }
        }
        while let Some(event) = self
            .romaji_request
            .as_ref()
            .and_then(|request| request.events.try_recv().ok())
        {
            match event {
                romaji::Event::Progress { done, total } => {
                    self.state.notice = format!("Generating romaji… {done}/{total}");
                }
                romaji::Event::Completed(result) => {
                    let Some(request) = self.romaji_request.take() else {
                        break;
                    };
                    self.state.romaji_generating = None;
                    let still_current =
                        self.state.tracks.iter().any(|track| {
                            track.path == request.path && track.lyrics == request.source
                        });
                    if !still_current {
                        self.state.notice = "Lyrics changed; romaji result discarded".into();
                        continue;
                    }
                    match result {
                        Ok(romaji) => {
                            self.romaji_touched.insert(request.path.clone());
                            let saved =
                                self.romaji_store.save(request.path.clone(), romaji.clone());
                            self.state.romaji.insert(request.path.clone(), romaji);
                            self.state.romaji_visible = true;
                            self.lyrics_dirty = true;
                            self.rows_dirty = true;
                            self.state.notice = match saved {
                                Ok(()) => "Romaji generated".into(),
                                Err(error) => format!("Romaji storage failed: {error}"),
                            };
                            self.save_settings(ui);
                        }
                        Err(error) => {
                            self.state.notice = format!("Romaji generation failed: {error}")
                        }
                    }
                }
            }
        }
    }

    fn translation_source(track: &Track) -> Vec<crate::translation::SourceLine> {
        if track.lyrics.timed.is_empty() {
            track
                .lyrics
                .plain
                .iter()
                .map(|text| crate::translation::SourceLine {
                    at_ms: None,
                    text: text.clone(),
                })
                .collect()
        } else {
            track
                .lyrics
                .timed
                .iter()
                .map(|line| crate::translation::SourceLine {
                    at_ms: Some(line.at_ms),
                    text: line.text.clone(),
                })
                .collect()
        }
    }

    fn start_translation(&mut self, ui: &AppWindow) {
        if self.translation_request.is_some() {
            self.state.notice = "Translation is already running".into();
            return;
        }
        let Some(track) = self
            .state
            .current
            .and_then(|id| self.state.tracks.get(id))
            .cloned()
        else {
            self.state.notice = "Choose a song first".into();
            self.sync(ui);
            return;
        };
        if track.lyrics.is_empty() {
            self.state.notice = "No lyrics available".into();
            self.sync(ui);
            return;
        }
        let settings = crate::ai::DeepSeekSettings::load(&self.directory.join("deepseek.json"));
        let model = settings
            .as_ref()
            .map(|settings| settings.model.as_str())
            .unwrap_or(crate::ai::DEFAULT_MODEL);
        self.translation_request_id = self.translation_request_id.wrapping_add(1);
        let request = crate::translation::Request {
            request_id: self.translation_request_id,
            track_id: track.path.clone(),
            target: self.settings.translation_target,
            provider_version: format!("{}:{model}", crate::translation::PROMPT_VERSION),
            source: Self::translation_source(&track),
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let active_request = request.clone();
        let cancellation = crate::translation::Cancellation::default();
        let worker_cancellation = cancellation.clone();
        std::thread::spawn(move || {
            let result = settings
                .map_err(|_| crate::translation::Error::Provider)
                .and_then(|settings| {
                    crate::ai::DeepSeek::new(&settings)
                        .map_err(|_| crate::translation::Error::Provider)
                })
                .and_then(|provider| {
                    crate::translation::generate(&request, &provider, &worker_cancellation)
                });
            let _ = sender.send((request, result));
        });
        self.translation_request = Some(receiver);
        self.translation_active_request = Some(active_request);
        self.translation_cancel = Some(cancellation);
        self.state.notice = "Generating translation…".into();
        self.sync(ui);
    }

    fn poll_translation(&mut self, ui: &AppWindow) {
        let Some(receiver) = self.translation_request.as_ref() else {
            return;
        };
        let Ok((request, result)) = receiver.try_recv() else {
            return;
        };
        self.translation_request = None;
        self.translation_active_request = None;
        self.translation_cancel = None;
        self.state.notice = self.apply_translation_result(request, result).into();
        self.lyrics_dirty = true;
        self.sync(ui);
    }

    fn apply_translation_result(
        &mut self,
        request: crate::translation::Request,
        result: Result<crate::translation::Translation, crate::translation::Error>,
    ) -> &'static str {
        let current = self.state.current.and_then(|id| self.state.tracks.get(id));
        let current_source = current.map(Self::translation_source).unwrap_or_default();
        match result {
            Ok(translation)
                if current.is_some_and(|track| {
                    request.accepts(
                        &translation,
                        self.translation_request_id,
                        &track.path,
                        self.settings.translation_target,
                        &current_source,
                        &self.translation_provider_version(),
                    )
                }) =>
            {
                let previous = self.translation_cache.clone();
                self.translation_cache.insert(translation);
                match self
                    .translation_cache
                    .save(&self.directory.join("translations.json"))
                {
                    Ok(()) => {
                        self.translation_visible = true;
                        "Translation ready"
                    }
                    Err(_) => {
                        self.translation_cache = previous;
                        "Could not save translation cache"
                    }
                }
            }
            Ok(_) => "Lyrics or target changed; translation discarded",
            Err(crate::translation::Error::Cancelled) => "Translation cancelled",
            Err(_) => "Translation failed; try again",
        }
    }

    fn set_translation_target(
        &mut self,
        ui: &AppWindow,
        target: crate::translation::TargetLanguage,
    ) {
        self.settings.translation_target = target;
        if let Some(token) = self.translation_cancel.take() {
            token.cancel();
        }
        self.translation_visible = false;
        self.lyrics_dirty = true;
        self.save_settings(ui);
        self.sync(ui);
    }

    fn translation_provider_version(&self) -> String {
        let model = crate::ai::DeepSeekSettings::load(&self.directory.join("deepseek.json"))
            .map(|settings| settings.model)
            .unwrap_or_else(|_| crate::ai::DEFAULT_MODEL.into());
        format!("{}:{model}", crate::translation::PROMPT_VERSION)
    }

    fn remove_translation_for_current(&mut self) -> Result<(), String> {
        let track = self
            .state
            .current
            .and_then(|id| self.state.tracks.get(id))
            .ok_or_else(|| "Choose a song first".to_owned())?;
        let request = crate::translation::Request {
            request_id: 0,
            track_id: track.path.clone(),
            target: self.settings.translation_target,
            provider_version: self.translation_provider_version(),
            source: Self::translation_source(track),
        };
        if self
            .translation_active_request
            .as_ref()
            .is_some_and(|active| active.cache_key() == request.cache_key())
        {
            if let Some(token) = self.translation_cancel.as_ref() {
                token.cancel();
            }
            self.translation_request_id = self.translation_request_id.wrapping_add(1);
            self.translation_active_request = None;
        }
        let previous = self.translation_cache.clone();
        self.translation_cache.remove(&request.cache_key());
        if let Err(error) = self
            .translation_cache
            .save(&self.directory.join("translations.json"))
        {
            self.translation_cache = previous;
            return Err(format!("Could not remove translation: {error:?}"));
        }
        self.translation_visible = false;
        self.lyrics_dirty = true;
        Ok(())
    }

    fn start_genius(&mut self, ui: &AppWindow, refresh: bool) {
        if self.genius_request.is_some() {
            return;
        }
        let Some(seed) = self.prepare_genius_seed(refresh) else {
            ui.set_genius_message(self.text("Choose a song first"));
            return;
        };
        let revision = self.library_revision();
        let seed_signature = self
            .state
            .tracks
            .iter()
            .find(|track| track.path == seed)
            .map(|track| {
                format!(
                    "{}\0{}\0{}",
                    track.title.trim().to_lowercase(),
                    track.artist.trim().to_lowercase(),
                    track.album.trim().to_lowercase()
                )
            });
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        for (index, track) in self.state.tracks.iter().enumerate() {
            if !track.path.is_file() {
                continue;
            }
            let signature = format!(
                "{}\0{}\0{}",
                track.title.trim().to_lowercase(),
                track.artist.trim().to_lowercase(),
                track.album.trim().to_lowercase()
            );
            if track.path != seed
                && (Some(&signature) == seed_signature.as_ref() || !seen.insert(signature.clone()))
            {
                continue;
            }
            seen.insert(signature);
            candidates.push(crate::genius::Candidate {
                id: format!("candidate-{index}"),
                path: track.path.clone(),
                title: track.title.chars().take(160).collect(),
                artist: track.artist.chars().take(160).collect(),
                album: track.album.chars().take(160).collect(),
                genre: track.genre.chars().take(80).collect(),
                duration_seconds: track.duration.max(0.0),
            });
        }
        let request = crate::genius::Request {
            request_id: self.genius_variation + 1,
            seed_path: seed,
            library_revision: revision,
            variation: self.genius_variation,
            limit: 24,
            candidates,
        };
        if request.validate().is_err() {
            ui.set_genius_message(self.text("Not enough available local songs"));
            return;
        }
        let Ok(permit) = GENIUS_WORKERS.try_acquire() else {
            ui.set_genius_message(self.text("Genius failed; retry is available"));
            return;
        };
        let settings = crate::ai::DeepSeekSettings::load(&self.directory.join("deepseek.json"));
        let (tx, rx) = std::sync::mpsc::channel();
        let cancellation = crate::genius::Cancellation::default();
        let worker_cancellation = cancellation.clone();
        std::thread::spawn(move || {
            let _permit = permit;
            let result = settings
                .map_err(|_| crate::genius::Error::Provider)
                .and_then(|settings| {
                    crate::ai::DeepSeek::new(&settings).map_err(|_| crate::genius::Error::Provider)
                })
                .and_then(|provider| {
                    crate::genius::generate(&request, &provider, &worker_cancellation)
                });
            let _ = tx.send((request, result));
        });
        self.genius_request = Some(rx);
        self.genius_cancel = Some(cancellation);
        ui.set_genius_busy(true);
        ui.set_genius_message(self.text("Generating…"));
    }

    fn prepare_genius_seed(&mut self, refresh: bool) -> Option<PathBuf> {
        if self.genius_seed.is_none() {
            self.genius_seed = self
                .state
                .current
                .and_then(|id| self.state.tracks.get(id))
                .map(|track| track.path.clone());
        }
        if refresh {
            self.genius_variation = self.genius_variation.wrapping_add(1);
        } else {
            self.genius_variation = 0;
            self.genius_rows.clear();
        }
        self.genius_seed.clone()
    }

    fn library_revision(&self) -> u64 {
        Self::library_revision_for(&self.state.tracks)
    }

    fn library_revision_for(tracks: &[Track]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for track in tracks {
            track.path.hash(&mut h);
            track.title.hash(&mut h);
            track.artist.hash(&mut h);
            track.album.hash(&mut h);
            track.genre.hash(&mut h);
            track.duration.to_bits().hash(&mut h);
            track.path.is_file().hash(&mut h);
        }
        h.finish()
    }

    fn poll_genius(&mut self, ui: &AppWindow) {
        let Some(receiver) = &self.genius_request else {
            return;
        };
        let Ok((request, result)) = receiver.try_recv() else {
            return;
        };
        self.genius_request = None;
        self.genius_cancel = None;
        ui.set_genius_busy(false);
        let message = match self.apply_genius_result(&request, result) {
            Ok(sparse) => {
                ui.set_genius_songs(slint::ModelRc::new(slint::VecModel::from(
                    self.genius_rows.clone(),
                )));
                if sparse {
                    "Short local playlist"
                } else {
                    "Genius playlist ready"
                }
            }
            Err(message) => message,
        };
        ui.set_genius_message(self.text(message));
    }

    fn apply_genius_result(
        &mut self,
        request: &crate::genius::Request,
        result: Result<crate::genius::ResultSet, crate::genius::Error>,
    ) -> Result<bool, &'static str> {
        if !self.genius_context_matches(request) {
            return Err("Library or seed changed; result discarded");
        }
        let result = result.map_err(|_| "Genius failed; retry is available")?;
        self.genius_rows.clear();
        if let Some(seed) = self
            .state
            .tracks
            .iter()
            .find(|track| track.path == request.seed_path)
        {
            self.genius_rows.push(crate::ui::GeniusRow {
                title: seed.title.clone().into(),
                artist: seed.artist.clone().into(),
                path: seed.path.to_string_lossy().into_owned().into(),
            });
        }
        for path in result.ordered_paths {
            if let Some(track) = self
                .state
                .tracks
                .iter()
                .find(|track| track.path == path && track.path.is_file())
            {
                self.genius_rows.push(crate::ui::GeniusRow {
                    title: track.title.clone().into(),
                    artist: track.artist.clone().into(),
                    path: track.path.to_string_lossy().into_owned().into(),
                });
            }
        }
        Ok(result.sparse)
    }

    fn genius_context_matches(&self, request: &crate::genius::Request) -> bool {
        self.genius_seed.as_deref() == Some(request.seed_path.as_path())
            && self.library_revision() == request.library_revision
            && request.candidates.iter().all(|candidate| {
                self.state.tracks.iter().any(|track| {
                    track.path == candidate.path
                        && track.title.chars().take(160).collect::<String>() == candidate.title
                        && track.artist.chars().take(160).collect::<String>() == candidate.artist
                        && track.album.chars().take(160).collect::<String>() == candidate.album
                        && track.genre.chars().take(80).collect::<String>() == candidate.genre
                        && track.duration.max(0.0) == candidate.duration_seconds
                })
            })
    }

    fn genius_command(&mut self, ui: &AppWindow, command: &str) {
        match command {
            "create" => {
                self.genius_seed = Some(PathBuf::from(ui.get_genius_seed_path().as_str()));
                self.genius_rows.clear();
                self.start_genius(ui, false);
            }
            "refresh" => self.start_genius(ui, true),
            "close" => {
                if let Some(token) = self.genius_cancel.take() {
                    token.cancel();
                }
                self.genius_request = None;
                self.genius_seed = None;
                self.genius_rows.clear();
                self.genius_variation = 0;
                ui.set_genius_busy(false);
                ui.set_genius_songs(ModelRc::default());
                ui.set_genius_message("".into());
            }
            "play" => {
                let ids = self.genius_playable_indices();
                if ids.is_empty() || ids.len() != self.genius_rows.len() {
                    ui.set_genius_message(self.text("Library or seed changed; result discarded"));
                    return;
                }
                let (start, keep, position) = self.install_genius_queue(ids);
                if keep.is_some() {
                    self.state.position = position;
                    self.send_music(crate::audio::engine::AudioCommand::Seek(position));
                    self.send_music(crate::audio::engine::AudioCommand::Pause(false));
                } else if let Some(effect) = self.state.start(start).into_iter().next() {
                    self.effects(ui, vec![effect]);
                }
                self.sync(ui);
            }
            "save" => {
                match self.save_genius_playlist() {
                    Ok(()) => {
                        ui.set_genius_message(self.text("Genius playlist saved"));
                    }
                    Err(_) => {
                        ui.set_genius_message(self.text("Could not save Genius playlist; retry"))
                    }
                }
                self.sync(ui);
            }
            _ => {}
        }
    }

    fn genius_playable_indices(&self) -> Vec<usize> {
        self.genius_rows
            .iter()
            .filter_map(|row| {
                self.state.tracks.iter().position(|track| {
                    track.path.to_string_lossy() == row.path.as_str() && track.path.is_file()
                })
            })
            .collect()
    }

    fn install_genius_queue(&mut self, ids: Vec<usize>) -> (usize, Option<usize>, f64) {
        let keep = self.state.current.filter(|id| {
            *id == ids[0]
                && !self.state.video_active
                && self.pending_media.is_none()
                && matches!(
                    self.state.status,
                    PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Loading
                )
        });
        let start = keep.unwrap_or(ids[0]);
        let position = self.state.position;
        self.state.queue.set(ids, start, false);
        if self.state.shuffle {
            self.state.queue.shuffle();
        }
        if keep.is_some() {
            self.state.position = position;
            self.state.paused = false;
            if self.state.status == PlaybackStatus::Paused {
                self.state.status = PlaybackStatus::Playing;
            }
        }
        (start, keep, position)
    }

    fn save_genius_playlist(&mut self) -> Result<(), String> {
        let paths: Vec<PathBuf> = self
            .genius_playable_indices()
            .into_iter()
            .filter_map(|id| self.state.tracks.get(id).map(|track| track.path.clone()))
            .collect();
        if paths.is_empty() || paths.len() != self.genius_rows.len() {
            return Err("Some selected songs are unavailable".into());
        }
        let seed = self
            .genius_seed
            .as_ref()
            .and_then(|path| self.state.tracks.iter().find(|track| &track.path == path));
        let name = seed
            .map(|track| format!("{} - {}", track.artist, track.title))
            .unwrap_or_else(|| "Genius Mix".into());
        let playlist = playlists::create(&self.directory.join("library.sqlite3"), &name, &paths)?;
        self.state.playlists.push(playlist);
        Ok(())
    }

    pub fn deepseek_command(&mut self, ui: &AppWindow, command: &str) {
        if !ui.get_deepseek_settings_open() {
            return;
        }
        match command {
            "cancel" => {
                ui.set_deepseek_api_key("".into());
                ui.set_deepseek_settings_open(false);
                ui.invoke_focus_screen();
            }
            "save" => {
                let path = self.directory.join("deepseek.json");
                let mut settings = crate::ai::DeepSeekSettings::load(&path).unwrap_or_default();
                let key = ui.get_deepseek_api_key().trim().to_owned();
                if !key.is_empty() {
                    settings.api_key = key;
                }
                settings.model = ui.get_deepseek_model().trim().to_owned();
                let result = if settings.has_key() {
                    settings.save(&path)
                } else {
                    Err("Enter a DeepSeek API key".into())
                };
                match result {
                    Ok(()) => {
                        ui.set_deepseek_api_key("".into());
                        ui.set_deepseek_key_configured(true);
                        ui.set_deepseek_settings_open(false);
                        self.state.notice = "DeepSeek settings saved".into();
                        ui.invoke_focus_screen();
                        self.sync(ui);
                    }
                    Err(error) => ui.set_deepseek_message(self.text(&error)),
                }
            }
            _ => {}
        }
    }

    fn sync_editor(&self, ui: &AppWindow) {
        ui.set_shortcut_editor_open(self.shortcut_edit.is_some());
        ui.set_shortcut_recording(self.shortcut_capture.is_some());
        if let Some(action) = self.shortcut_edit {
            ui.set_shortcut_title(self.text(action.title()));
            ui.set_shortcut_keys(self.text(&self.dispatcher.keymap.labels(action)));
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
                ui.set_shortcut_message(self.text("Saved. Your shortcuts are active."));
                self.sync(ui);
                true
            }
            Err(message) => {
                ui.set_shortcut_message(i18n::notice(self.state.language, &message).into());
                false
            }
        }
    }

    pub fn shortcut_command(&mut self, ui: &AppWindow, command: &str) {
        if command == "cancel" {
            if self.shortcut_capture.take().is_some() {
                ui.set_shortcut_message(self.text("Recording cancelled."));
            } else {
                self.shortcut_edit = None;
            }
        } else if let Some(action) = self.shortcut_edit {
            match command {
                "replace" | "add" => {
                    self.shortcut_capture = Some(command == "replace");
                    ui.set_shortcut_message(self.text("Press a key combination. Escape cancels."));
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

impl Drop for Controller {
    fn drop(&mut self) {
        // Finish an explicit Create before shutdown, so a successful write survives restart.
        if let Some(worker) = self.playlist_writer.take() {
            let _ = worker.join();
        }
    }
}

pub fn connect(ui: &AppWindow, controller: &Rc<RefCell<Controller>>) {
    ui.global::<I18n>().on_translate(|language, key| {
        translate(
            Language::ALL
                .get(language as usize)
                .copied()
                .unwrap_or_default(),
            &key,
        )
        .into()
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_open_genius_requested(move || {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().prepare_portrait_dialog(&ui);
            ui.invoke_open_genius_preview();
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_genius_command(move |command| {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().genius_command(&ui, &command);
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_playlist_command(move |command, id| {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().playlist_command(&ui, &command, &id);
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_deepseek_command(move |command| {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().deepseek_command(&ui, &command);
        }
    });
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
    ui.on_modal_key_event(move |event, editing| {
        weak.upgrade().is_some_and(|ui| {
            state
                .borrow_mut()
                .dispatch_modal_navigation(&ui, event, editing)
        })
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_command(move |name| {
        if let Some(ui) = weak.upgrade() {
            let mut controller = state.borrow_mut();
            match name.as_str() {
                "translate_lyrics" | "regenerate_translation" => controller.start_translation(&ui),
                "cancel_translation" => {
                    if let Some(token) = controller.translation_cancel.as_ref() {
                        token.cancel();
                    }
                    controller.translation_request_id =
                        controller.translation_request_id.wrapping_add(1);
                    controller.state.notice = "Translation cancellation requested".into();
                    controller.sync(&ui);
                }
                "toggle_translation" => {
                    controller.translation_visible = !controller.translation_visible;
                    controller.lyrics_dirty = true;
                    controller.sync(&ui);
                }
                "remove_translation" => {
                    controller.state.notice = match controller.remove_translation_for_current() {
                        Ok(()) => "Translation removed".into(),
                        Err(error) => error,
                    };
                    controller.sync(&ui);
                }
                "translation_target_en" => controller
                    .set_translation_target(&ui, crate::translation::TargetLanguage::English),
                "translation_target_zh" => controller.set_translation_target(
                    &ui,
                    crate::translation::TargetLanguage::SimplifiedChinese,
                ),
                "translation_target_ja" => controller
                    .set_translation_target(&ui, crate::translation::TargetLanguage::Japanese),
                _ if Action::parse(&name).is_some() => {
                    ui.set_keyboard_navigation(false);
                    controller.execute(&ui, Action::parse(&name).unwrap());
                }
                _ => {}
            }
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

#[cfg(test)]
mod ai_controller_tests {
    use super::*;
    use crate::{
        library::{Track, lyrics::Lyrics},
        translation::{Request, TargetLanguage},
    };

    fn make_track(path: PathBuf, title: &str, lyrics: Lyrics) -> Track {
        Track {
            path,
            title: title.into(),
            artist: "Artist".into(),
            album: "Album".into(),
            genre: "Indie".into(),
            number: 1,
            duration: 123.0,
            artwork: Vec::new(),
            lyrics,
        }
    }

    fn translation_request(track: &Track, request_id: u64, target: TargetLanguage) -> Request {
        Request {
            request_id,
            track_id: track.path.clone(),
            target,
            provider_version: format!(
                "{}:{}",
                crate::translation::PROMPT_VERSION,
                crate::ai::DEFAULT_MODEL
            ),
            source: Controller::translation_source(track),
        }
    }

    fn fixture_translation(
        request: &Request,
        answer: serde_json::Value,
    ) -> crate::translation::Translation {
        struct Fixture(serde_json::Value);
        impl crate::translation::JsonProvider for Fixture {
            fn json(&self, _: &str, _: &serde_json::Value) -> Result<serde_json::Value, String> {
                Ok(self.0.clone())
            }
        }
        crate::translation::generate(
            request,
            &Fixture(answer),
            &crate::translation::Cancellation::default(),
        )
        .unwrap()
    }

    #[test]
    fn genius_frozen_snapshot_rejects_in_place_metadata_edits_and_refresh_keeps_seed() {
        let dir = tempfile::tempdir().unwrap();
        let mut controller = Controller::new(dir.path().to_owned());
        let seed = dir.path().join("seed.flac");
        let song = dir.path().join("song.flac");
        std::fs::write(&seed, b"seed").unwrap();
        std::fs::write(&song, b"song").unwrap();
        controller.state.tracks = vec![
            make_track(seed.clone(), "Seed", Lyrics::default()),
            make_track(song.clone(), "Song", Lyrics::default()),
        ];
        controller.genius_seed = Some(seed.clone());
        controller.state.current = Some(1);
        assert_eq!(controller.prepare_genius_seed(false), Some(seed.clone()));
        assert_eq!(controller.prepare_genius_seed(true), Some(seed.clone()));
        assert_eq!(controller.genius_variation, 1);
        let request = crate::genius::Request {
            request_id: 4,
            seed_path: seed.clone(),
            library_revision: controller.library_revision(),
            variation: controller.genius_variation,
            limit: 1,
            candidates: controller
                .state
                .tracks
                .iter()
                .enumerate()
                .map(|(i, t)| crate::genius::Candidate {
                    id: format!("c{i}"),
                    path: t.path.clone(),
                    title: t.title.clone(),
                    artist: t.artist.clone(),
                    album: t.album.clone(),
                    genre: t.genre.clone(),
                    duration_seconds: t.duration,
                })
                .collect(),
        };
        assert!(controller.genius_context_matches(&request));
        struct Fixture;
        impl crate::genius::JsonProvider for Fixture {
            fn json(&self, _: &str, _: &serde_json::Value) -> Result<serde_json::Value, String> {
                Ok(serde_json::json!({"ids":["c1"]}))
            }
        }
        let generated =
            crate::genius::generate(&request, &Fixture, &crate::genius::Cancellation::default())
                .unwrap();
        assert_eq!(generated.ordered_paths, [song]);
        assert!(
            !controller
                .apply_genius_result(&request, Ok(generated))
                .unwrap()
        );
        assert_eq!(
            controller
                .genius_rows
                .iter()
                .map(|row| row.title.as_str())
                .collect::<Vec<_>>(),
            ["Seed", "Song"]
        );
        let refreshed = crate::genius::Request {
            request_id: 5,
            variation: 3,
            ..request.clone()
        };
        assert_eq!(refreshed.seed_path, request.seed_path);
        assert_eq!(refreshed.candidates, request.candidates);
        let base_tracks = controller.state.tracks.clone();
        for edit in 0..5 {
            controller.state.tracks = base_tracks.clone();
            let changed = &mut controller.state.tracks[1];
            match edit {
                0 => changed.title.push('!'),
                1 => changed.artist.push('!'),
                2 => changed.album.push('!'),
                3 => changed.genre.push('!'),
                _ => changed.duration += 1.0,
            }
            assert!(
                !controller.genius_context_matches(&request),
                "metadata field {edit} was not revisioned"
            );
        }
    }

    #[test]
    fn remove_translation_invalidates_pending_result_and_persists_removal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.mp3");
        std::fs::write(&path, b"fixture").unwrap();
        let mut controller = Controller::new(dir.path().to_owned());
        let track = make_track(path, "Song", Lyrics::parse("[00:01]hello\n[00:02]world"));
        controller.state.tracks = vec![track.clone()];
        controller.state.current = Some(0);
        let request = translation_request(&track, 21, TargetLanguage::English);
        let translation = fixture_translation(
            &request,
            serde_json::json!({"translations":["hola","mundo"]}),
        );
        controller.translation_cache.insert(translation.clone());
        controller
            .translation_cache
            .save(&dir.path().join("translations.json"))
            .unwrap();
        let token = crate::translation::Cancellation::default();
        controller.translation_cancel = Some(token.clone());
        controller.translation_active_request = Some(request.clone());
        controller.translation_request_id = request.request_id;
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send((request.clone(), Ok(translation.clone()))).unwrap();
        controller.translation_request = Some(rx);

        controller.remove_translation_for_current().unwrap();
        assert!(token.is_cancelled());
        assert!(
            !controller
                .translation_cache
                .get(&request.cache_key())
                .is_some()
        );
        let _ = controller
            .translation_request
            .take()
            .unwrap()
            .try_recv()
            .unwrap();
        let notice = controller.apply_translation_result(request.clone(), Ok(translation));
        assert_eq!(notice, "Lyrics or target changed; translation discarded");
        let persisted =
            crate::translation::Cache::load(&dir.path().join("translations.json")).unwrap();
        assert!(persisted.get(&request.cache_key()).is_none());
    }

    #[test]
    fn translation_result_rejects_target_and_source_switches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.mp3");
        let mut controller = Controller::new(dir.path().to_owned());
        let track = make_track(path, "Song", Lyrics::parse("[00:01]hello"));
        controller.state.tracks = vec![track.clone()];
        controller.state.current = Some(0);
        let request = translation_request(&track, 9, TargetLanguage::English);
        let translated =
            fixture_translation(&request, serde_json::json!({"translations":["hola"]}));
        controller.translation_request_id = 9;
        controller.settings.translation_target = TargetLanguage::Japanese;
        assert_eq!(
            controller.apply_translation_result(request.clone(), Ok(translated.clone())),
            "Lyrics or target changed; translation discarded"
        );
        controller.settings.translation_target = TargetLanguage::English;
        controller.state.tracks[0].lyrics = Lyrics::parse("[00:01]changed");
        assert_eq!(
            controller.apply_translation_result(request, Ok(translated)),
            "Lyrics or target changed; translation discarded"
        );
        assert!(controller.translation_cache.is_empty());
    }

    #[test]
    fn failed_translation_regeneration_preserves_the_previous_cached_value() {
        let dir = tempfile::tempdir().unwrap();
        let mut controller = Controller::new(dir.path().to_owned());
        let track = make_track(
            dir.path().join("song.flac"),
            "Song",
            Lyrics {
                plain: vec!["hello".into()],
                ..Default::default()
            },
        );
        let request = translation_request(&track, 2, TargetLanguage::English);
        let previous = fixture_translation(&request, serde_json::json!({"translations":["old"]}));
        let replacement =
            fixture_translation(&request, serde_json::json!({"translations":["new"]}));
        controller.state.tracks = vec![track];
        controller.state.current = Some(0);
        controller.translation_request_id = request.request_id;
        controller.translation_visible = true;
        controller.translation_cache.insert(previous.clone());
        std::fs::create_dir(dir.path().join("translations.json")).unwrap();
        assert_eq!(
            controller.apply_translation_result(request.clone(), Ok(replacement)),
            "Could not save translation cache"
        );
        assert_eq!(
            controller.translation_cache.get(&request.cache_key()),
            Some(&previous)
        );
        assert!(controller.translation_visible);
    }

    #[test]
    fn failed_translation_removal_reports_error_and_keeps_cached_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.mp3");
        let mut controller = Controller::new(dir.path().to_owned());
        let track = make_track(path, "Song", Lyrics::parse("[00:01]hello"));
        controller.state.tracks = vec![track.clone()];
        controller.state.current = Some(0);
        let request = translation_request(&track, 2, TargetLanguage::English);
        let value = fixture_translation(&request, serde_json::json!({"translations":["hola"]}));
        controller.translation_cache.insert(value);
        std::fs::create_dir(dir.path().join("translations.json")).unwrap();
        assert!(controller.remove_translation_for_current().is_err());
        assert!(
            controller
                .translation_cache
                .get(&request.cache_key())
                .is_some()
        );
    }

    #[test]
    fn genius_queue_and_save_use_preview_order_and_retry_after_duplicate_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut controller = Controller::new(dir.path().to_owned());
        let seed = dir.path().join("seed.mp3");
        let next = dir.path().join("next.mp3");
        std::fs::write(&seed, b"seed").unwrap();
        std::fs::write(&next, b"next").unwrap();
        controller.state.tracks = vec![
            make_track(seed.clone(), "Seed", Lyrics::default()),
            make_track(next.clone(), "Next", Lyrics::default()),
        ];
        controller.state.current = Some(0);
        controller.state.position = 37.25;
        controller.state.status = PlaybackStatus::Paused;
        controller.state.paused = true;
        controller.state.shuffle = true;
        controller.genius_seed = Some(seed.clone());
        controller.genius_rows = vec![
            crate::ui::GeniusRow {
                title: "Seed".into(),
                artist: "Artist".into(),
                path: seed.to_string_lossy().into_owned().into(),
            },
            crate::ui::GeniusRow {
                title: "Next".into(),
                artist: "Artist".into(),
                path: next.to_string_lossy().into_owned().into(),
            },
        ];
        let ids = controller.genius_playable_indices();
        let (start, kept, position) = controller.install_genius_queue(ids);
        assert_eq!(start, 0);
        assert_eq!(kept, Some(0));
        assert_eq!(position, 37.25);
        controller.state.queue.unshuffle();
        assert_eq!(controller.state.queue.order, [0, 1]);
        assert_eq!(controller.state.current, Some(0));
        assert_eq!(controller.state.position, 37.25);
        assert!(!controller.state.paused);
        assert_eq!(controller.state.status, PlaybackStatus::Playing);
        controller.save_genius_playlist().unwrap();
        assert_eq!(
            playlists::load(&dir.path().join("library.sqlite3")).unwrap()[0].tracks,
            [seed.clone(), next.clone()]
        );
        assert!(controller.save_genius_playlist().is_err());
        assert_eq!(
            playlists::load(&dir.path().join("library.sqlite3"))
                .unwrap()
                .len(),
            1
        );
        controller.state.current = Some(1);
        assert_eq!(controller.install_genius_queue(vec![0, 1]).1, None);
        controller.state.current = Some(0);
        controller.state.video_active = true;
        assert_eq!(controller.install_genius_queue(vec![0, 1]).1, None);
        std::fs::remove_file(next).unwrap();
        controller.state.tracks[0].title = "A different name".into();
        assert_eq!(
            controller.save_genius_playlist().unwrap_err(),
            "Some selected songs are unavailable"
        );
        assert_eq!(
            playlists::load(&dir.path().join("library.sqlite3"))
                .unwrap()
                .len(),
            1
        );
    }
}
