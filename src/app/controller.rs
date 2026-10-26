mod artist_images;
mod genius;
mod library;
mod library_ui;
mod lyrics;
mod media_events;
mod native_status;
mod playback;
#[path = "controller/playlists.rs"]
mod playlist_service;
mod podcasts;
mod practice;
mod statistics;
mod ui_sync;
mod video;
mod window;

use library::{LibraryTasks, LoadEvent as LibraryLoadEvent};
use lyrics::{LookupEvent, LyricsService, RomajiEvent};

use super::{
    action::{Action, Category},
    artwork::ArtworkLoader,
    config::Settings,
    i18n::{self, Language, translate},
    navigation::{Filter, Group, Screen, StatisticsPage},
    state::{Effect, State, Target},
    thumbnails::{self, Prepared, Thumbnails},
};
use crate::{
    audio::{
        PlaybackStatus,
        engine::{AudioCommand, AudioEvent, Engine},
    },
    input::{Binding, Context, Dispatch, Dispatcher, Key, KeyPress, Keymap, Modifiers},
    library::{playlists, scanner},
    ui::{
        AppWindow, BackScroll, I18n, LibraryRow, PlaylistCandidate, StatisticsItem, SubtitleChoice,
        WheelGesture,
    },
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use statistics::{Event as StatisticsEvent, StatisticsService};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::mpsc::Receiver,
};

use genius::GeniusService;

use playback::PendingMedia;
use playlist_service::PlaylistsService;

pub struct Controller {
    pub state: State,
    pub dispatcher: Dispatcher,
    engine: Engine,
    practice: super::practice::PracticeSession,
    practice_store: super::practice::PracticeStore,
    practice_saves: practice::PracticeSaves,
    practice_rows: Vec<crate::ui::PracticeMarkerRow>,
    practice_path: Option<PathBuf>,
    practice_wait: Option<(u64, std::time::Instant)>,
    podcasts: podcasts::Podcasts,
    video_engine: crate::video::engine::Engine,
    video_tasks: video::VideoTasks,
    video_progress_saved: std::time::Instant,
    video_thumbnails: super::video_thumbnails::VideoThumbnails,
    subtitle_tracks: Vec<crate::video::engine::SubtitleTrack>,
    subtitle_cursor: usize,
    pending_media: Option<PendingMedia>,
    pending_subtitle: Option<crate::video::subtitles::SubtitlePreference>,
    pending_seek: Option<f64>,
    handoff: u64,
    last_landscape: bool,
    last_video_generation: u64,
    directory: PathBuf,
    iphone_sync: Option<super::iphone_sync::SyncPanel>,
    settings: Settings,
    media_bridge: crate::platform::macos_media::MediaBridge,
    device_status: crate::platform::desktop_status::Monitor,
    media_marker: Option<(Option<usize>, PlaybackStatus, i64)>,
    statistics: StatisticsService,
    artist_images: crate::library::artist_images::ArtistImages,
    artist_images_enabled: bool,
    artist_photos: artist_images::ArtistPhotoCache,
    playback_session: Option<crate::library::statistics::ListeningSession>,
    prefetched: Option<(usize, usize, u64)>,
    prefetch_token: u64,
    failed_prefetch: Option<(usize, usize)>,
    listening_clock: std::time::Instant,
    library_tasks: LibraryTasks,
    playlists: PlaylistsService,
    playlist_candidates: Vec<PlaylistCandidate>,
    lyrics_edit: Option<Receiver<Result<Option<scanner::LyricsUpdate>, String>>>,
    lyrics: LyricsService,
    genius: GeniusService,
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
    last_window_scale_factor: f32,
    resize_debouncer: super::geometry::ResizeDebouncer,
    resize_guard: super::geometry::ResizeGuard,
    native_resize: Rc<super::window_events::NativeResizeSignals>,
    native_resize_revision: u64,
    window_menu_height: Option<f32>,
    menu_measurement: Option<(u64, f32)>,
    resize_clock: std::time::Instant,
    last_notice: String,
    notice_since: std::time::Instant,
    letter_jump: Option<(std::time::Instant, Screen)>,
    thumbnails: Thumbnails,
    library_rows: Rc<VecModel<LibraryRow>>,
    statistics_items: Rc<VecModel<StatisticsItem>>,
    details_path: Option<PathBuf>,
    last_statistics_minute: i64,
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
        state.translation_visible = settings.translation_visible;
        state.zoom = settings.zoom;
        state.device_mode = settings.device_mode;
        state.gapless_playback = settings.gapless_playback;
        state.replay_gain_mode = settings.replay_gain_mode;
        state.replay_gain_preamp_db = settings.replay_gain_preamp_db;
        state.replay_gain_prevent_clipping = settings.replay_gain_prevent_clipping;
        let keymap =
            Keymap::load_or_create(&directory.join("shortcuts.json")).unwrap_or_else(|message| {
                state.notice = format!("{message}; using default shortcuts");
                Keymap::default()
            });
        let database = directory.join("library.sqlite3");
        let statistics = StatisticsService::new(database.clone());
        let (library_tasks, receiver) = LibraryTasks::start(database);
        let video_tasks = video::VideoTasks::new(directory.clone());
        match crate::video::progress::WatchProgress::load(&directory.join("video-progress.json")) {
            Ok(progress) => state.video_progress = progress,
            Err(error) => state.notice = format!("Video progress error: {error}"),
        }
        match crate::video::subtitles::SubtitleAssociations::load(
            &directory.join("video-subtitles.json"),
        ) {
            Ok(associations) => state.video_subtitles = associations,
            Err(error) => state.notice = format!("Subtitle preferences error: {error}"),
        }
        let lyrics = LyricsService::new(directory.clone(), settings.translation_visible);
        let practice_store = super::practice::PracticeStore::load(&directory.join("practice.json"))
            .unwrap_or_else(|error| {
                state.notice = format!("Practice data error: {error}");
                super::practice::PracticeStore::default()
            });
        let practice_saves = practice::PracticeSaves::new(directory.join("practice.json"));
        Self {
            practice: Default::default(),
            practice_store,
            practice_saves,
            practice_rows: Vec::new(),
            practice_path: None,
            practice_wait: None,
            state,
            dispatcher: Dispatcher::new(keymap),
            engine: Engine::default(),
            podcasts: podcasts::Podcasts::new(directory.clone()),
            video_engine: crate::video::engine::Engine::default(),
            video_tasks,
            video_progress_saved: std::time::Instant::now(),
            video_thumbnails: super::video_thumbnails::VideoThumbnails::default(),
            subtitle_tracks: Vec::new(),
            subtitle_cursor: 0,
            pending_media: None,
            pending_subtitle: None,
            pending_seek: None,
            handoff: 0,
            last_landscape: false,
            last_video_generation: 0,
            artist_images: crate::library::artist_images::ArtistImages::new(directory.clone()),
            artist_images_enabled: false,
            artist_photos: Default::default(),
            directory,
            settings,
            iphone_sync: None,
            media_bridge: crate::platform::macos_media::MediaBridge::default(),
            device_status: crate::platform::desktop_status::Monitor::new(),
            media_marker: None,
            statistics,
            playback_session: None,
            prefetched: None,
            prefetch_token: 0,
            failed_prefetch: None,
            listening_clock: std::time::Instant::now(),
            library_tasks,
            playlists: PlaylistsService::new(receiver),
            playlist_candidates: Vec::new(),
            lyrics_edit: None,
            lyrics,
            genius: GeniusService::default(),
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
            last_window_scale_factor: 0.0,
            resize_debouncer: super::geometry::ResizeDebouncer::default(),
            resize_guard: super::geometry::ResizeGuard::default(),
            native_resize: Rc::new(super::window_events::NativeResizeSignals::default()),
            native_resize_revision: 0,
            window_menu_height: None,
            menu_measurement: None,
            resize_clock: std::time::Instant::now(),
            last_notice: String::new(),
            notice_since: std::time::Instant::now(),
            letter_jump: None,
            thumbnails: Thumbnails::default(),
            library_rows: Rc::new(VecModel::default()),
            statistics_items: Rc::new(VecModel::default()),
            details_path: None,
            last_statistics_minute: 0,
            row_artwork: Vec::new(),
            artwork: ArtworkLoader::default(),
            artwork_resolution: 0,
            shortcut_edit: None,
            shortcut_capture: None,
        }
    }

    pub fn dispatch_modal_navigation(
        &mut self,
        ui: &AppWindow,
        event: slint::language::KeyEvent,
        editing: bool,
    ) -> bool {
        if ui.get_practice_open() {
            return self.dispatch_practice(ui, event, editing);
        }
        if self.dialog_open || self.shortcut_capture.is_some() {
            return false;
        }
        if ui.get_subtitle_menu_open() {
            if editing {
                return true;
            }
            let Some(key) = Key::from_slint(event.text.as_str()) else {
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
            let resolved = self.dispatcher.dispatch(
                &press,
                Context {
                    surface: crate::input::Surface::SubtitleMenu,
                    text_input: false,
                    reference_open: false,
                    native_dialog: false,
                },
            );
            let mut choices = self.subtitle_choices();
            match resolved {
                Dispatch::Action(Action::Home) => {
                    ui.set_subtitle_menu_open(false);
                    self.execute(ui, Action::Home);
                }
                Dispatch::Action(Action::Back | Action::ToggleSubtitles) => {
                    ui.set_subtitle_menu_open(false);
                    ui.invoke_focus_screen();
                }
                Dispatch::Action(Action::SelectNext)
                | Dispatch::Action(Action::NavigateRight)
                | Dispatch::Action(Action::Right) => {
                    self.subtitle_cursor = (self.subtitle_cursor + 1) % choices.len().max(1);
                    for (index, choice) in choices.iter_mut().enumerate() {
                        choice.selected = index == self.subtitle_cursor;
                    }
                    ui.set_subtitle_menu_selected(self.subtitle_cursor as i32);
                    ui.set_subtitle_choices(ModelRc::new(VecModel::from(choices)));
                }
                Dispatch::Action(Action::SelectPrevious)
                | Dispatch::Action(Action::NavigateLeft)
                | Dispatch::Action(Action::Left) => {
                    self.subtitle_cursor = self
                        .subtitle_cursor
                        .checked_sub(1)
                        .unwrap_or(choices.len().saturating_sub(1));
                    for (index, choice) in choices.iter_mut().enumerate() {
                        choice.selected = index == self.subtitle_cursor;
                    }
                    ui.set_subtitle_menu_selected(self.subtitle_cursor as i32);
                    ui.set_subtitle_choices(ModelRc::new(VecModel::from(choices)));
                }
                Dispatch::Action(Action::Activate) => {
                    if let Some(choice) = choices.get(self.subtitle_cursor) {
                        self.choose_video_subtitle(choice.id);
                    }
                    ui.set_subtitle_menu_open(false);
                    self.sync(ui);
                }
                _ => {}
            }
            return true;
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
        ui.set_practice_open(false);
        self.practice.cancel();
        if ui.get_podcast_subscribe_open() {
            self.podcast_command(ui, "close");
        }
        ui.set_subtitle_menu_open(false);
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
            self.playlists.suspend();
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
        if ui.get_practice_open() {
            return self.dispatch_practice(ui, event, text_input);
        }
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
        if (ui.get_track_details_open() || ui.get_native_status_open())
            && self.shortcut_edit.is_none()
            && !text_input
        {
            self.letter_jump = None;
            match self.dispatcher.dispatch(&press, context) {
                Dispatch::Action(
                    action @ (Action::Back
                    | Action::Home
                    | Action::TrackDetails
                    | Action::DeviceInformation),
                ) => {
                    self.execute(ui, action);
                    return true;
                }
                Dispatch::Action(action) if action.category() == Category::Navigation => {
                    return true;
                }
                _ => {}
            }
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
            Dispatch::Ignored
                if !text_input
                    && !self.dialog_open
                    && !self.state.reference_open
                    && press.modifiers == Modifiers::default()
                    && !press.repeat =>
            {
                if let Key::Digit(digit @ '0'..='5') = press.key
                    && let Some(path) = self.state.rating_target().map(|track| track.path.clone())
                {
                    self.set_rating(ui, &path, (digit as u8) - b'0');
                    true
                } else {
                    false
                }
            }
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
        if self.handle_practice_action(ui, action) {
            return;
        }
        if action == Action::Back && (ui.get_track_details_open() || ui.get_native_status_open()) {
            ui.set_track_details_open(false);
            ui.set_native_status_open(false);
            self.details_path = None;
            ui.invoke_focus_screen();
            return;
        }
        if action == Action::Home {
            ui.set_track_details_open(false);
            ui.set_native_status_open(false);
            self.details_path = None;
        }
        if matches!(action, Action::DeviceInformation | Action::TrackDetails)
            && (self.dialog_open
                || self.state.reference_open
                || ui.get_playlist_editor_open()
                || ui.get_genius_preview_open()
                || ui.get_shortcut_editor_open()
                || ui.get_deepseek_settings_open()
                || ui.get_podcast_subscribe_open())
        {
            return;
        }
        if action == Action::ToggleSubtitles {
            if !self.dialog_open
                && self.state.video_active
                && self.state.navigation.current().screen == Screen::VideoPlaying
                && !self.state.reference_open
            {
                if ui.get_subtitle_menu_open() {
                    ui.set_subtitle_menu_open(false);
                } else {
                    self.open_subtitle_menu(ui);
                }
                ui.invoke_focus_screen();
            }
            return;
        }
        if (action == Action::RefreshLibrary
            || (action == Action::ImportFolder
                && !matches!(
                    self.state.navigation.current().screen,
                    Screen::Videos | Screen::VideoPlaying
                )))
            && self.library_tasks.busy()
        {
            self.state.notice = if self.library_tasks.is_importing() {
                "Scanning music folders…"
            } else {
                "Refreshing library…"
            }
            .into();
            self.sync(ui);
            return;
        }
        if action == Action::ImportFolder && self.lyrics_edit.is_some() {
            self.state.notice = "A lyric request is already running".into();
            self.sync(ui);
            return;
        }
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
        let previous_gain = self.gain_config();
        if action == Action::ToggleTranslation {
            let current = self.state.current.and_then(|id| self.state.tracks.get(id));
            if self
                .lyrics
                .shortcut_should_translate(current, self.settings.translation_target)
            {
                self.lyrics.mark_shortcut(current);
                self.translation_command(ui, crate::translation::Command::Translate);
                return;
            }
        }
        if action == Action::ToggleRomaji
            && self.state.navigation.current().screen == Screen::NowPlaying
            && !self.state.video_active
            && self.state.podcasts.active.is_none()
            && let Some(id) = self.state.current
            && let Some(track) = self.state.tracks.get(id)
            && !self
                .state
                .romaji
                .get(&track.path)
                .is_some_and(|r| r.matches(&track.lyrics))
        {
            self.start_romaji(ui, id);
            if self.state.romaji_generating.as_ref()
                == self
                    .state
                    .current
                    .and_then(|id| self.state.tracks.get(id))
                    .map(|t| &t.path)
            {
                self.state.romaji_visible = true;
                self.lyrics_dirty = true;
            }
            self.sync(ui);
            return;
        }
        let effects = self.state.apply(action);
        if self.gain_config() != previous_gain {
            self.cancel_gapless_prefetch();
            self.failed_prefetch = None;
        }
        // Start audio immediately; focus effects still need the new screen to exist.
        let (audio, effects): (Vec<_>, Vec<_>) = effects
            .into_iter()
            .partition(|effect| matches!(effect, Effect::Audio(_) | Effect::Video(_)));
        self.effects(ui, audio);
        self.sync(ui);
        self.effects(ui, effects);
        self.schedule_gapless_prefetch();
        self.publish_media_snapshot();
    }

    fn effects(&mut self, ui: &AppWindow, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::RetryArtistImages => {
                    self.artist_images.retry_failed();
                }
                Effect::DeviceInformation => {
                    ui.set_track_details_open(false);
                    self.details_path = None;
                    ui.set_native_status_open(!ui.get_native_status_open());
                    ui.invoke_focus_screen();
                }
                Effect::TrackDetails => self.toggle_track_details(ui),
                Effect::RefreshStatistics => {
                    self.refresh_statistics(true);
                    self.sync(ui);
                }
                Effect::Podcast(action) => self.podcast_action(ui, action),
                Effect::Audio(command) => {
                    if matches!(command, AudioCommand::Seek(_)) {
                        self.practice.cancel();
                    }
                    self.send_music(command);
                }
                Effect::Video(command) => self.send_video(command),
                Effect::ImportVideos => {
                    self.dialog_open = true;
                    self.video_tasks
                        .start_import(translate(self.state.language, "Import Videos…"), None);
                }
                Effect::RefreshLibrary => {
                    self.start_library_refresh();
                    ui.set_library_busy(self.library_tasks.busy());
                }
                Effect::Import => {
                    if self
                        .library_tasks
                        .start_import(translate(self.state.language, "Import Music Folder…"))
                    {
                        self.dialog_open = true;
                        ui.set_library_busy(true);
                    } else {
                        self.state.importing = false;
                        self.state.notice = "Refreshing library…".into();
                    }
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
                Effect::Translation(command) => self.translation_command(ui, command),
                Effect::FindLyrics { id } => self.start_online_lyrics(ui, id, false),
                Effect::GeniusPreview => {
                    self.prepare_portrait_dialog(ui);
                    ui.invoke_open_genius_preview();
                }
                Effect::NewPlaylist => self.open_playlist_draft(ui, None),
                Effect::EditPlaylist { id } => self.open_playlist_draft(ui, Some(id)),
                Effect::GenerateRomaji { id } => self.start_romaji(ui, id),
                Effect::CancelRomaji => {
                    if self.lyrics.cancel_romaji() {
                        self.state.romaji_generating = None;
                        self.state.notice = "Romaji generation cancelled".into();
                    }
                    self.sync(ui);
                }
                Effect::RemoveRomaji { id } => {
                    if let Some(track) = self.state.tracks.get(id) {
                        let path = track.path.clone();
                        let removal = self.lyrics.remove_romaji(path.clone());
                        if removal.cancelled {
                            self.state.romaji_generating = None;
                        }
                        self.state.romaji.remove(&path);
                        self.state.notice = match removal.result {
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
                    self.cancel_online_lyrics();
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

    /// Import original local files without changing their contents. This is also
    /// the generated-clip QA entry point, avoiding native picker automation.
    pub fn import_videos(&mut self, paths: Vec<PathBuf>) {
        if self.state.video_importing {
            return;
        }
        self.state.video_importing = true;
        self.video_tasks.start_import(String::new(), Some(paths));
    }

    /// Persist the user's explicit subtitle choice for the active video and
    /// apply it to the current generation. UI menus call this for Off,
    /// Automatic, embedded tracks, and imported/replaced sidecars.
    pub fn set_video_subtitle(&mut self, preference: crate::video::subtitles::SubtitlePreference) {
        let Some(path) = self
            .state
            .video_current
            .and_then(|id| self.state.videos.get(id))
            .map(|video| video.path.clone())
        else {
            return;
        };
        let preference = match preference {
            crate::video::subtitles::SubtitlePreference::External(path) => {
                let valid_extension = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "ass" | "ssa"));
                if !valid_extension || !path.is_file() {
                    self.state.notice = format!(
                        "Subtitle file is missing or unsupported: {}",
                        path.display()
                    );
                    return;
                }
                match std::fs::canonicalize(path) {
                    Ok(path) => crate::video::subtitles::SubtitlePreference::External(path),
                    Err(error) => {
                        self.state.notice = format!("Could not open subtitle file: {error}");
                        return;
                    }
                }
            }
            other => other,
        };
        self.state.video_subtitles.set(&path, preference.clone());
        if let Err(error) = self
            .state
            .video_subtitles
            .save(&self.directory.join("video-subtitles.json"))
        {
            self.state.notice = format!("Could not save subtitle preference: {error}");
        }
        if self.state.video_active {
            self.send_video(crate::video::engine::Command::Subtitle {
                generation: self.state.video_generation,
                preference,
            });
        }
    }

    fn subtitle_choices(&self) -> Vec<SubtitleChoice> {
        let selected = self
            .state
            .video_current
            .and_then(|id| self.state.videos.get(id))
            .map(|video| self.state.video_subtitles.get(&video.path))
            .unwrap_or_default();
        let mut choices = vec![
            SubtitleChoice {
                id: -2,
                label: self.text("Automatic"),
                selected: selected == crate::video::subtitles::SubtitlePreference::Automatic,
            },
            SubtitleChoice {
                id: -1,
                label: self.text("Off"),
                selected: selected == crate::video::subtitles::SubtitlePreference::Off,
            },
        ];
        choices.extend(self.subtitle_tracks.iter().map(|track| {
            let preference = if track.external {
                track.external_path.clone().map_or(
                    crate::video::subtitles::SubtitlePreference::Embedded(track.id),
                    crate::video::subtitles::SubtitlePreference::External,
                )
            } else {
                crate::video::subtitles::SubtitlePreference::Embedded(track.id)
            };
            SubtitleChoice {
                id: track.id as i32,
                label: if track.title.is_empty() {
                    if track.language.is_empty() {
                        format!(
                            "{}{}",
                            self.text(if track.external { "External" } else { "Track" }),
                            track.id
                        )
                        .into()
                    } else {
                        format!("{} · {}", track.language, track.id).into()
                    }
                } else {
                    format!("{} · {}", track.title, track.id).into()
                },
                selected: selected == preference,
            }
        }));
        choices
    }

    fn choose_video_subtitle(&mut self, id: i32) {
        let preference = match id {
            -2 => crate::video::subtitles::SubtitlePreference::Automatic,
            -1 => crate::video::subtitles::SubtitlePreference::Off,
            _ => {
                let Some(track) = self
                    .subtitle_tracks
                    .iter()
                    .find(|track| track.id == i64::from(id))
                else {
                    return;
                };
                if track.external {
                    track.external_path.clone().map_or(
                        crate::video::subtitles::SubtitlePreference::Embedded(track.id),
                        crate::video::subtitles::SubtitlePreference::External,
                    )
                } else {
                    crate::video::subtitles::SubtitlePreference::Embedded(track.id)
                }
            }
        };
        self.set_video_subtitle(preference);
    }

    fn open_subtitle_menu(&mut self, ui: &AppWindow) {
        let choices = self.subtitle_choices();
        self.subtitle_cursor = choices
            .iter()
            .position(|choice| choice.selected)
            .unwrap_or(0);
        ui.set_subtitle_menu_selected(self.subtitle_cursor as i32);
        ui.set_subtitle_choices(ModelRc::new(VecModel::from(choices)));
        ui.set_subtitle_menu_open(true);
    }

    fn begin_subtitle_import(&mut self) {
        if self.video_tasks.subtitle_pending() || self.dialog_open {
            return;
        }
        let Some((generation, video_path)) = self
            .state
            .video_current
            .filter(|_| self.state.video_active)
            .and_then(|id| {
                self.state
                    .videos
                    .get(id)
                    .map(|video| (self.state.video_generation, video.path.clone()))
            })
        else {
            return;
        };
        let title = self.text("Import ASS/SSA…").to_string();
        self.video_tasks
            .start_subtitle_import(generation, video_path, title);
        self.dialog_open = true;
    }

    fn subtitle_import_matches(&self, generation: u64, video_path: &std::path::Path) -> bool {
        video::subtitle_import_matches(
            generation,
            video_path,
            self.state.video_active,
            self.state.video_generation,
            self.state
                .video_current
                .and_then(|id| self.state.videos.get(id))
                .map(|video| video.path.as_path()),
        )
    }

    fn poll_videos(&mut self, ui: &AppWindow) {
        let previous_status = self.state.video_status;
        if let Some(result) = self.video_tasks.poll_subtitle() {
            self.dialog_open = false;
            if let video::SubtitleSelection {
                generation,
                video_path,
                selected: Some(path),
            } = result
                && self.subtitle_import_matches(generation, &video_path)
            {
                self.set_video_subtitle(crate::video::subtitles::SubtitlePreference::External(
                    path,
                ));
                ui.set_subtitle_choices(ModelRc::new(VecModel::from(self.subtitle_choices())));
            }
        }
        if let Some(result) = self.video_tasks.poll_loading() {
            match result {
                Ok(videos) => {
                    self.video_thumbnails.clear();
                    self.state.merge_videos(videos);
                }
                Err(error) => self.state.notice = format!("Video library error: {error}"),
            }
            self.rows_dirty = true;
        }
        while let Some(event) = self.video_tasks.poll_import() {
            match event {
                crate::video::import::Event::SelectionClosed => self.dialog_open = false,
                crate::video::import::Event::RemovedMissing { path, result } => match result {
                    Ok(true) => {
                        let active_removed = self.state.video_active
                            && self
                                .state
                                .video_current
                                .and_then(|id| self.state.videos.get(id))
                                .is_some_and(|video| video.path == path);
                        self.save_video_progress(true);
                        self.state.remove_video(&path);
                        if active_removed {
                            self.pending_media = None;
                            self.pending_subtitle = None;
                            self.pending_seek = None;
                            let _ = self
                                .video_engine
                                .send(crate::video::engine::Command::Stop { handoff: 0 });
                            ui.set_video_frame(slint::Image::default());
                        }
                        self.video_thumbnails.clear();
                        self.rows_dirty = true;
                        self.state.notice = "Removed missing video".into();
                    }
                    Ok(false) => self.state.notice.clear(),
                    Err(error) => self.state.notice = format!("Video library error: {error}"),
                },
                crate::video::import::Event::Completed(result) => {
                    self.state.video_importing = false;
                    match result {
                        Ok(Some(result)) => {
                            self.video_thumbnails.clear();
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
        self.poll_video_events(ui);
        self.save_video_progress(
            previous_status != crate::video::Status::Finished
                && self.state.video_status == crate::video::Status::Finished,
        );
    }

    fn save_video_progress(&mut self, force: bool) {
        self.state.remember_video_progress();
        if !self.state.video_progress.is_dirty()
            || (!force && self.video_progress_saved.elapsed() < std::time::Duration::from_secs(5))
        {
            return;
        }
        // Retry failed saves at the same bounded interval; leave the in-memory history dirty.
        self.video_progress_saved = std::time::Instant::now();
        if let Err(error) = self
            .state
            .video_progress
            .save(&self.directory.join("video-progress.json"))
        {
            self.state.notice = format!("Video progress error: {error}");
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

    pub fn poll(&mut self, ui: &AppWindow) {
        self.poll_artist_images();
        if self.device_status.poll() {
            self.pause_for_output_change(ui);
        }
        self.maintain_aspect_ratio(ui);
        self.poll_library_loading();
        self.poll_library_imports(ui);
        if let Some(result) = self
            .lyrics_edit
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.lyrics_edit = None;
            let (online_path, translate_after) = self.lyrics.take_storage_context();
            if online_path.is_none() {
                self.dialog_open = false;
            }
            let mut translated_path = None;
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
                            if translate_after.as_ref() == Some(&update.path) {
                                translated_path = Some(update.path.clone());
                            }
                        }
                    }
                    self.state.notice = "Lyrics updated".into();
                }
                Ok(None) => self.state.notice = "Import cancelled".into(),
                Err(error) => self.state.notice = format!("Lyrics import failed: {error}"),
            }
            if let Some(path) = online_path {
                if translated_path.is_some() || self.state.notice == "Lyrics updated" {
                    self.state.notice = "Found on LRCLIB".into();
                }
                self.state
                    .lyrics_lookup_status
                    .insert(path, self.state.notice.clone());
            }
            self.rows_dirty = true;
            ui.invoke_focus_screen();
            if translated_path.is_some() {
                self.start_translation(ui);
            }
        }
        self.poll_romaji(ui);
        self.poll_podcasts(ui);
        self.poll_statistics();
        self.poll_library_refresh();
        self.poll_translation(ui);
        self.poll_online_lyrics(ui);
        self.poll_genius(ui);
        self.poll_playlists(ui);
        self.poll_videos(ui);
        self.poll_audio_events(ui);
        self.poll_media_commands(ui);
        self.poll_practice(ui, std::time::Instant::now());
        self.schedule_gapless_prefetch();
        self.sync(ui);
        self.publish_media_snapshot();
        if let Some(artwork) = self.artwork.poll() {
            ui.set_artwork(artwork);
        }
    }

    pub fn set_rating(&mut self, ui: &AppWindow, path: &std::path::Path, stars: u8) {
        if stars > 5
            || !self.state.ratings_ready
            || self
                .state
                .rating_target()
                .is_none_or(|track| track.path != path)
            || self.dialog_open
            || self.state.reference_open
            || ui.get_playlist_editor_open()
            || ui.get_genius_preview_open()
            || ui.get_shortcut_editor_open()
            || ui.get_deepseek_settings_open()
        {
            return;
        }
        let old = self.state.ratings.get(path).copied().unwrap_or(0);
        if old == stars {
            return;
        }
        self.state.ratings.insert(path.to_path_buf(), stars);
        self.rows_dirty = true;
        if let Err(previous) = self.statistics.rate(path, stars, old) {
            self.state.ratings.insert(path.to_path_buf(), previous);
            self.state.notice = "Could not save rating".into();
        }
        self.sync(ui);
    }

    fn refresh_statistics(&mut self, force: bool) {
        let tracks = self
            .state
            .tracks
            .iter()
            .filter(|t| self.state.track_available(t))
            .map(|t| crate::library::statistics::StatisticsTrack {
                path: t.path.clone(),
                title: t.title.clone(),
                artist: t.artist.clone(),
                album: t.album.clone(),
            })
            .collect();
        self.state.statistics.failed = false;
        match self
            .statistics
            .query(self.state.statistics.range, tracks, force)
        {
            Ok(Some(snapshot)) => {
                self.state.statistics.snapshot = Some(snapshot);
                self.state.statistics.loading = false;
            }
            Ok(None) => self.state.statistics.loading = true,
            Err(error) => {
                self.state.statistics.loading = false;
                self.state.statistics.failed = true;
                eprintln!("Statistics query failed: {error}");
            }
        }
        self.state.clamp_statistics_selection();
        self.rows_dirty = true;
    }

    fn poll_statistics(&mut self) {
        while let Some(event) = self.statistics.poll(&self.state.ratings) {
            match event {
                StatisticsEvent::Statistics { range, snapshot } => {
                    if range == self.state.statistics.range {
                        self.state.statistics.snapshot = Some(snapshot);
                        self.state.statistics.loading = false;
                        self.state.statistics.failed = false;
                        self.state.clamp_statistics_selection();
                        self.rows_dirty |= self.state.navigation.current().screen.is_statistics();
                    }
                }
                StatisticsEvent::StatisticsFailed { range, error } => {
                    if range == self.state.statistics.range {
                        self.state.statistics.loading = false;
                        self.state.statistics.failed = true;
                        self.rows_dirty |= self.state.navigation.current().screen.is_statistics();
                        eprintln!("Statistics query failed: {error}");
                    }
                }
                StatisticsEvent::RatingsLoaded(ratings) => {
                    self.state.ratings = ratings;
                    self.state.ratings_ready = true;
                    self.rows_dirty = true;
                }
                StatisticsEvent::RatingReverted { path, stars } => {
                    self.state.ratings.insert(path, stars);
                    self.rows_dirty = true;
                    self.state.notice = "Could not save rating".into();
                }
                StatisticsEvent::SmartPlaylists(playlists) => {
                    if self.state.smart_playlists != playlists {
                        self.state.smart_playlists = playlists;
                        self.rows_dirty |= matches!(
                            self.state.navigation.current().screen,
                            Screen::Groups(Group::Playlists)
                                | Screen::Songs(Filter::SmartPlaylist(_))
                        );
                    }
                }
                StatisticsEvent::Unavailable(error) => {
                    self.state.notice = format!("Playback statistics unavailable: {error}");
                }
            }
        }
    }

    fn refresh_playlist_candidates(&mut self) {
        self.playlists.refresh_candidates(
            self.state
                .tracks
                .iter()
                .filter(|song| self.state.track_available(song)),
        );
        self.playlist_candidates = self
            .playlists
            .candidates()
            .iter()
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
        if self.playlists.resume() {
            ui.set_playlist_editor_open(true);
            ui.invoke_focus_screen();
            return;
        }
        if let Some(id) = id {
            let Some(playlist) = self.state.playlists.iter().find(|p| p.id == id).cloned() else {
                return;
            };
            self.playlists.begin_edit(&playlist);
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
        let selected: HashSet<_> = self.playlists.paths().iter().collect();
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
            .playlists
            .paths()
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
        if self.playlists.is_saving() {
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
            if self.playlists.move_to(&path, target) {
                ui.set_playlist_message(
                    if self.playlists.is_editing() {
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
                self.playlists.begin_new();
                ui.set_playlist_editing(false);
                self.refresh_playlist_candidates();
                ui.set_playlist_saving(false);
                ui.set_playlist_persisted(false);
                ui.set_playlist_library_loading(
                    self.state.tracks.is_empty() && !self.library_tasks.ready(),
                );
            }
            "toggle" => {
                if !self.playlists.toggle(&path) {
                    return;
                }
            }
            "review" => {
                if ui.get_playlist_name().trim().is_empty() && !self.playlists.is_editing() {
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
            "up" | "down" => self.playlists.nudge(&path, command == "down"),
            "remove" => self.playlists.remove(&path),
            "cancel" => {
                self.playlists.cancel();
                self.playlist_candidates.clear();
            }
            "create" => {
                self.playlists.save(
                    &self.directory.join("library.sqlite3"),
                    ui.get_playlist_name().as_str(),
                    &self.state.tracks,
                );
                ui.set_playlist_saving(true);
                ui.set_playlist_message("Saving playlist…".into());
                return;
            }
            _ => return,
        }
        ui.set_playlist_message(
            if self.playlists.is_editing() {
                "Changes not saved"
            } else {
                "Draft only · not saved"
            }
            .into(),
        );
        self.sync_playlist_draft(ui);
    }

    fn poll_playlists(&mut self, ui: &AppWindow) {
        if let Some(result) = self.playlists.poll_load() {
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
            ui.set_playlist_library_loading(
                self.state.tracks.is_empty() && !self.library_tasks.ready(),
            );
            if self.playlists.candidates().is_empty() && !self.state.tracks.is_empty() {
                self.refresh_playlist_candidates();
                self.sync_playlist_draft(ui);
            }
        }
        if let Some(result) = self.playlists.poll_save() {
            ui.set_playlist_saving(false);
            match result {
                Ok(saved) => {
                    let playlist = saved.playlist;
                    let screen = Screen::Songs(Filter::Playlist {
                        id: playlist.id,
                        name: playlist.name.clone(),
                    });
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
                    if ui.get_playlist_editor_open() && !saved.editing {
                        self.state.navigation.open(Screen::Groups(Group::Playlists));
                        self.state.navigation.open(screen);
                    }
                    ui.set_playlist_editor_open(false);
                    self.state.notice = "Playlist saved".into();
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
        match self.lyrics.start_romaji(self.state.tracks.get(id)) {
            Ok(path) => {
                self.state.romaji_generating = Some(path);
                self.state.notice = "Generating romaji…".into();
            }
            Err(error) => self.state.notice = error,
        }
        self.sync(ui);
    }

    fn poll_romaji(&mut self, ui: &AppWindow) {
        for event in self.lyrics.poll_romaji(&self.state.tracks) {
            match event {
                RomajiEvent::Loaded(cache) => {
                    for (path, romaji) in cache {
                        self.state.romaji.entry(path).or_insert(romaji);
                    }
                    self.lyrics_dirty = true;
                }
                RomajiEvent::StorageFailed(error) => {
                    self.state.notice = format!("Romaji storage failed: {error}");
                }
                RomajiEvent::Progress { done, total } => {
                    self.state.notice = format!("Generating romaji… {done}/{total}");
                }
                RomajiEvent::Generated {
                    path,
                    romaji,
                    storage,
                } => {
                    self.state.romaji_generating = None;
                    self.state.romaji.insert(path, romaji);
                    self.state.romaji_visible = true;
                    self.lyrics_dirty = true;
                    self.rows_dirty = true;
                    self.state.notice = match storage {
                        Ok(()) => "Romaji generated".into(),
                        Err(error) => format!("Romaji storage failed: {error}"),
                    };
                    self.save_settings(ui);
                }
                RomajiEvent::Finished { notice } => {
                    self.state.romaji_generating = None;
                    self.state.notice = notice;
                }
            }
        }
    }

    #[cfg(test)]
    fn translation_source(track: &crate::library::Track) -> Vec<crate::translation::SourceLine> {
        LyricsService::source(track)
    }

    fn cancel_online_lyrics(&mut self) {
        if let Some(path) = self.lyrics.cancel_lookup() {
            self.state
                .lyrics_lookup_status
                .insert(path, "Online lyric search cancelled".into());
            self.rows_dirty = true;
        }
    }

    fn start_online_lyrics(&mut self, ui: &AppWindow, id: usize, translate_after: bool) {
        if self.lyrics.lookup_busy()
            || self.lyrics_edit.is_some()
            || self.lyrics.translation_busy()
            || self.library_tasks.is_importing()
        {
            self.state.notice = "A lyric request is already running".into();
            self.sync(ui);
            return;
        }
        match self
            .lyrics
            .start_lookup(self.state.tracks.get(id), translate_after)
        {
            Ok(path) => {
                self.state
                    .lyrics_lookup_status
                    .insert(path, "Searching online lyrics…".into());
                self.rows_dirty = true;
                self.state.notice = "Searching online lyrics…".into();
            }
            Err(message) => {
                if let Some(track) = self.state.tracks.get(id) {
                    self.state
                        .lyrics_lookup_status
                        .insert(track.path.clone(), message.clone());
                    self.rows_dirty = true;
                }
                self.state.notice = message;
            }
        }
        self.sync(ui);
    }

    #[cfg(test)]
    fn accepts_online_lyrics(&self, request: &crate::library::online_lyrics::Request) -> bool {
        self.lyrics.accepts_lookup(
            request,
            &self.state.tracks,
            &self.state.unavailable_tracks,
            self.lyrics_edit.is_some(),
            self.state.current.and_then(|id| self.state.tracks.get(id)),
        )
    }

    fn poll_online_lyrics(&mut self, ui: &AppWindow) {
        let Some(event) = self.lyrics.poll_lookup(
            &self.state.tracks,
            &self.state.unavailable_tracks,
            self.lyrics_edit.is_some(),
            self.state.current.and_then(|id| self.state.tracks.get(id)),
        ) else {
            return;
        };
        match event {
            LookupEvent::Found { request, found } => {
                self.lyrics_edit = Some(scanner::store_online_lyrics(
                    self.directory.join("library.sqlite3"),
                    request.path,
                    found,
                    (!request.previous_lyrics.is_empty()).then_some(request.previous_lyrics),
                ));
                self.state.notice = "Saving online lyrics…".into();
                if let Some(path) = self.lyrics.lookup_path() {
                    self.state
                        .lyrics_lookup_status
                        .insert(path.to_owned(), self.state.notice.clone());
                }
            }
            LookupEvent::Failed { path, notice } => {
                self.state.notice = notice;
                if let Some(path) = path {
                    self.state
                        .lyrics_lookup_status
                        .insert(path, self.state.notice.clone());
                }
            }
        }
        self.rows_dirty = true;
        self.sync(ui);
    }

    fn current_translation_cached(&self) -> bool {
        self.state
            .current
            .and_then(|id| self.state.tracks.get(id))
            .is_some_and(|track| {
                self.lyrics
                    .cached_translation(track, self.settings.translation_target)
                    .is_some()
            })
    }

    fn translation_command(&mut self, ui: &AppWindow, command: crate::translation::Command) {
        use crate::translation::Command;
        match command {
            Command::Translate => {
                self.lyrics.set_visible(true);
                self.lyrics_dirty = true;
                self.save_settings(ui);
                if self.current_translation_cached() {
                    self.state.notice = "Translation ready".into();
                    self.sync(ui);
                } else {
                    self.start_translation(ui);
                }
            }
            Command::Regenerate => {
                self.lyrics.set_visible(true);
                self.lyrics_dirty = true;
                self.save_settings(ui);
                self.start_translation(ui);
            }
            Command::Cancel => {
                let finding = self.lyrics.lookup_path().is_some();
                self.cancel_online_lyrics();
                self.lyrics.cancel_translation();
                self.state.notice = if finding {
                    "Online lyric search cancelled"
                } else {
                    "Translation cancellation requested"
                }
                .into();
                self.sync(ui);
            }
            Command::Toggle => {
                self.lyrics.toggle_visible();
                self.save_settings(ui);
                self.lyrics_dirty = true;
                self.sync(ui);
            }
            Command::Remove => {
                self.cancel_online_lyrics();
                self.state.notice = match self.remove_translation_for_current() {
                    Ok(()) => {
                        self.save_settings(ui);
                        "Translation removed".into()
                    }
                    Err(error) => error,
                };
                self.sync(ui);
            }
            Command::SetTarget(target) => self.set_translation_target(ui, target),
            Command::FindLyrics => {
                if let Some(id) = self.state.current {
                    self.start_online_lyrics(ui, id, false);
                } else {
                    self.state.notice = "Choose a song first".into();
                    self.sync(ui);
                }
            }
        }
    }

    fn start_translation(&mut self, ui: &AppWindow) {
        if self.lyrics.translation_busy() {
            self.state.notice = "Translation is already running".into();
            self.sync(ui);
            return;
        }
        let Some(track) = self.state.current.and_then(|id| self.state.tracks.get(id)) else {
            self.state.notice = "Choose a song first".into();
            self.sync(ui);
            return;
        };
        if track.lyrics.is_empty() {
            self.start_online_lyrics(ui, self.state.current.unwrap(), true);
            return;
        }
        self.state.notice = match self
            .lyrics
            .start_translation(Some(track), self.settings.translation_target)
        {
            Ok(()) => "Generating translation…".into(),
            Err(error) => error,
        };
        self.sync(ui);
    }

    fn poll_translation(&mut self, ui: &AppWindow) {
        if let Some(notice) = self.lyrics.poll_translation(
            self.state.current.and_then(|id| self.state.tracks.get(id)),
            self.settings.translation_target,
        ) {
            self.state.notice = notice.into();
            self.lyrics_dirty = true;
            self.sync(ui);
        }
    }

    #[cfg(test)]
    fn apply_translation_result(
        &mut self,
        request: crate::translation::Request,
        result: Result<crate::translation::Translation, crate::translation::Error>,
    ) -> &'static str {
        self.lyrics.apply_translation_result(
            request,
            result,
            self.state.current.and_then(|id| self.state.tracks.get(id)),
            self.settings.translation_target,
        )
    }

    fn set_translation_target(
        &mut self,
        ui: &AppWindow,
        target: crate::translation::TargetLanguage,
    ) {
        if let Some(path) = self
            .lyrics
            .change_target(self.settings.translation_target, target)
        {
            self.state
                .lyrics_lookup_status
                .insert(path, "Online lyric search cancelled".into());
            self.rows_dirty = true;
        }
        self.settings.translation_target = target;
        self.lyrics_dirty = true;
        self.save_settings(ui);
        self.sync(ui);
    }

    fn remove_translation_for_current(&mut self) -> Result<(), String> {
        self.lyrics.remove_translation(
            self.state.current.and_then(|id| self.state.tracks.get(id)),
            self.settings.translation_target,
        )?;
        self.settings.translation_visible = false;
        self.lyrics_dirty = true;
        Ok(())
    }

    fn start_genius(&mut self, ui: &AppWindow, refresh: bool) {
        let current = self.state.current.and_then(|id| self.state.tracks.get(id));
        match self
            .genius
            .start(&self.directory, &self.state.tracks, current, refresh)
        {
            Ok(true) => {
                ui.set_genius_busy(true);
                ui.set_genius_message(self.text("Generating…"));
            }
            Ok(false) => {}
            Err(message) => ui.set_genius_message(self.text(message)),
        }
    }

    #[cfg(test)]
    fn prepare_genius_seed(&mut self, refresh: bool) -> Option<PathBuf> {
        let current = self.state.current.and_then(|id| self.state.tracks.get(id));
        self.genius.prepare_seed(current, refresh)
    }

    #[cfg(test)]
    fn library_revision(&self) -> u64 {
        GeniusService::library_revision_for(&self.state.tracks)
    }

    fn poll_genius(&mut self, ui: &AppWindow) {
        let Some(result) = self.genius.poll(&self.state.tracks) else {
            return;
        };
        ui.set_genius_busy(false);
        let message = match result {
            Ok(sparse) => {
                let rows = self
                    .genius
                    .entries()
                    .iter()
                    .map(|entry| crate::ui::GeniusRow {
                        title: entry.title.as_str().into(),
                        artist: entry.artist.as_str().into(),
                        path: entry.path.to_string_lossy().into_owned().into(),
                    })
                    .collect::<Vec<_>>();
                ui.set_genius_songs(ModelRc::new(VecModel::from(rows)));
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

    #[cfg(test)]
    fn apply_genius_result(
        &mut self,
        request: &crate::genius::Request,
        result: Result<crate::genius::ResultSet, crate::genius::Error>,
    ) -> Result<bool, &'static str> {
        self.genius
            .apply_result(request, result, &self.state.tracks)
    }

    #[cfg(test)]
    fn genius_context_matches(&self, request: &crate::genius::Request) -> bool {
        self.genius.context_matches(request, &self.state.tracks)
    }

    fn genius_command(&mut self, ui: &AppWindow, command: &str) {
        match command {
            "create" => {
                self.genius
                    .set_seed(PathBuf::from(ui.get_genius_seed_path().as_str()));
                self.start_genius(ui, false);
            }
            "refresh" => self.start_genius(ui, true),
            "close" => {
                self.genius.close();
                ui.set_genius_busy(false);
                ui.set_genius_songs(ModelRc::default());
                ui.set_genius_message("".into());
            }
            "play" => {
                let ids = self.genius_playable_indices();
                if ids.is_empty() || ids.len() != self.genius.entries().len() {
                    ui.set_genius_message(self.text("Library or seed changed; result discarded"));
                    return;
                }
                let (start, keep, position) = self.install_genius_queue(ids);
                if keep.is_some() {
                    if self.practice.enabled() {
                        let path = self.state.tracks[start].path.clone();
                        if let Err(error) = self.start_practice(&path, position, None) {
                            self.state.notice = error;
                        }
                    } else {
                        self.state.position = position;
                        self.send_music(crate::audio::engine::AudioCommand::Seek(position));
                        self.send_music(crate::audio::engine::AudioCommand::Pause(false));
                    }
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
        self.genius.playable_indices(&self.state.tracks)
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
        let (name, paths) = self.genius.playlist_snapshot(&self.state.tracks)?;
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
        self.save_video_progress(true);
        self.save_podcast_progress();
        self.cancel_online_lyrics();
        // Drain queued user ratings/statistics before process exit.
        self.statistics.shutdown();
        // Finish an explicit Create before shutdown, so a successful write survives restart.
        self.playlists.shutdown();
    }
}

pub fn connect(ui: &AppWindow, controller: &Rc<RefCell<Controller>>) {
    use slint::winit_030::{
        EventResult, WinitWindowAccessor,
        winit::event::{MouseScrollDelta, TouchPhase, WindowEvent},
    };
    // Native wheel activity must survive temporarily disabled/translating views.
    // Observe only: ordinary scrolling and Slint event dispatch remain unchanged.
    let scroll_activity_ui = ui.as_weak();
    let native_resize = controller.borrow().native_resize.clone();
    ui.window().on_winit_window_event(move |window, event| {
        native_resize.observe(window, event);
        let Some(ui) = scroll_activity_ui.upgrade() else {
            return EventResult::Propagate;
        };
        if let WindowEvent::MouseWheel { delta, phase, .. } = event {
            // TouchArea omits phases. Hold elastic motion through finger input
            // and OS momentum, then release with a short handoff guard.
            // Other backends may send Started without a matching Ended for a
            // mouse wheel, so retain their quiet-timer fallback.
            if cfg!(target_os = "macos") {
                match phase {
                    TouchPhase::Started => ui.global::<WheelGesture>().set_active(true),
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        ui.global::<WheelGesture>().set_active(false);
                    }
                    TouchPhase::Moved => {}
                }
            }
            let (x, y) = match delta {
                MouseScrollDelta::LineDelta(x, y) => (f64::from(*x), f64::from(*y)),
                MouseScrollDelta::PixelDelta(p) => (p.x, p.y),
            };
            if x.abs() > 1.5 * y.abs() {
                ui.global::<BackScroll>().invoke_activity();
            }
        } else if cfg!(target_os = "macos")
            && matches!(
                event,
                WindowEvent::Focused(false) | WindowEvent::CursorLeft { .. }
            )
        {
            ui.global::<WheelGesture>().set_active(false);
        }
        EventResult::Propagate
    });
    // Keep scroll-burst ownership independent of view/transition lifetimes.
    let scroll_timeout = slint::Timer::default();
    let scroll_ui = ui.as_weak();
    ui.global::<BackScroll>().on_activity(move || {
        if let Some(ui) = scroll_ui.upgrade()
            && ui.global::<BackScroll>().get_consumed()
        {
            let weak = ui.as_weak();
            scroll_timeout.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(40),
                move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.global::<BackScroll>().set_consumed(false);
                    }
                },
            );
        }
    });
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
    ui.on_podcast_details(move |index| {
        if let Some(ui) = weak.upgrade() {
            state
                .borrow_mut()
                .podcast_details(&ui, index.max(0) as usize);
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_practice_command(move |command, id, payload| {
        if let Some(ui) = weak.upgrade() {
            state
                .borrow_mut()
                .practice_command(&ui, &command, &id, &payload);
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_podcast_command(move |command| {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().podcast_command(&ui, &command);
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
    ui.on_subtitle_command(move |name| {
        if let Some(ui) = weak.upgrade() {
            let mut controller = state.borrow_mut();
            match name.as_str() {
                "open" if controller.state.video_active => {
                    controller.open_subtitle_menu(&ui);
                }
                "close" => ui.set_subtitle_menu_open(false),
                "import" if controller.state.video_active => {
                    ui.set_subtitle_menu_open(false);
                    controller.begin_subtitle_import();
                }
                "remove" if controller.state.video_active => {
                    controller
                        .set_video_subtitle(crate::video::subtitles::SubtitlePreference::Automatic);
                    ui.set_subtitle_choices(ModelRc::new(VecModel::from(
                        controller.subtitle_choices(),
                    )));
                }
                _ => {}
            }
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_subtitle_select(move |id| {
        if let Some(ui) = weak.upgrade() {
            let mut controller = state.borrow_mut();
            if controller.state.video_active {
                controller.choose_video_subtitle(id);
                ui.set_subtitle_choices(ModelRc::new(VecModel::from(
                    controller.subtitle_choices(),
                )));
                ui.set_subtitle_menu_open(false);
            }
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_command(move |name| {
        if let Some(ui) = weak.upgrade() {
            let mut controller = state.borrow_mut();
            if matches!(name.as_str(), "home" | "back") && ui.get_subtitle_menu_open() {
                ui.set_subtitle_menu_open(false);
            }
            match name.as_str() {
                "bluetooth_settings" => {
                    #[cfg(target_os = "macos")]
                    if let Err(error) = std::process::Command::new("/usr/bin/open")
                        .arg("x-apple.systempreferences:com.apple.BluetoothSettings")
                        .spawn()
                        .map(|mut child| std::thread::spawn(move || child.wait()))
                    {
                        controller.state.notice =
                            format!("Could not open Bluetooth settings: {error}");
                    }
                }
                "sync_iphone" => {
                    if controller.iphone_sync.is_none() {
                        match super::iphone_sync::SyncPanel::new(
                            controller.directory.clone(),
                            controller.state.language,
                        ) {
                            Ok(panel) => controller.iphone_sync = Some(panel),
                            Err(error) => controller.state.notice = error,
                        }
                    }
                    if let Some(panel) = &controller.iphone_sync
                        && let Err(error) = panel.show(controller.state.language)
                    {
                        controller.state.notice = error;
                    }
                    controller.sync(&ui);
                }
                _ if crate::translation::Command::parse(&name).is_some() => controller
                    .translation_command(&ui, crate::translation::Command::parse(&name).unwrap()),
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
    ui.on_statistics_range_selected(move |index| {
        if let Some(ui) = weak.upgrade() {
            let mut c = state.borrow_mut();
            if !c.state.navigation.current().screen.is_statistics() {
                return;
            }
            let range = match index {
                0 => crate::library::statistics::StatisticsRange::Last7Days,
                1 => crate::library::statistics::StatisticsRange::Last30Days,
                2 => crate::library::statistics::StatisticsRange::AllTime,
                _ => return,
            };
            if c.state.statistics.range == range {
                return;
            }
            c.state.statistics.range = range;
            c.state.statistics.snapshot = None;
            c.state.navigation.select(0);
            c.refresh_statistics(false);
            ui.set_list_scroll_y(0.0);
            c.sync(&ui);
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_statistics_open_page(move |index| {
        if let Some(ui) = weak.upgrade() {
            let mut c = state.borrow_mut();
            if !c.state.navigation.current().screen.is_statistics() {
                return;
            }
            let page = match index {
                0 => StatisticsPage::Home,
                1 => StatisticsPage::Overview,
                2 => StatisticsPage::Artists,
                3 => StatisticsPage::Albums,
                4 => StatisticsPage::Tracks,
                5 => StatisticsPage::Recent,
                6 => StatisticsPage::Daily,
                7 => StatisticsPage::Range,
                _ => return,
            };
            c.state.navigation.open(Screen::Statistics(page));
            c.sync(&ui);
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_statistics_refresh(move || {
        if let Some(ui) = weak.upgrade() {
            let mut c = state.borrow_mut();
            if c.state.navigation.current().screen.is_statistics() {
                c.refresh_statistics(true);
                c.sync(&ui);
            }
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_statistics_photo_credits(move || {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().open_artist_photo_credits();
            ui.invoke_focus_screen();
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_details_close(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_track_details_open(false);
            state.borrow_mut().details_path = None;
            ui.invoke_focus_screen();
        }
    });
    let weak = ui.as_weak();
    let state = controller.clone();
    ui.on_rating_changed(move |path, stars| {
        if let Some(ui) = weak.upgrade()
            && (0..=5).contains(&stars)
        {
            state
                .borrow_mut()
                .set_rating(&ui, std::path::Path::new(path.as_str()), stars as u8);
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

#[cfg(test)]
mod subtitle_import_controller_tests {
    use super::*;

    fn video(path: PathBuf, title: &str) -> crate::video::library::Video {
        crate::video::library::Video {
            path,
            title: title.into(),
            duration: 4.0,
            width: 320,
            height: 180,
            display_aspect: 16.0 / 9.0,
            video_codec: "h264".into(),
            audio_codec: None,
        }
    }

    #[test]
    fn delayed_subtitle_picker_result_requires_same_video_generation_and_path() {
        let directory = tempfile::tempdir().unwrap();
        let mut controller = Controller::new(directory.path().to_path_buf());
        let first = directory.path().join("first.mp4");
        let second = directory.path().join("second.mp4");
        controller
            .state
            .merge_videos(vec![video(first.clone(), "First")]);
        controller.state.video_current = Some(0);
        controller.state.video_active = true;
        controller.state.video_generation = 8;
        assert!(controller.subtitle_import_matches(8, &first));
        controller
            .state
            .merge_videos(vec![video(second.clone(), "Second")]);
        controller.state.video_current = Some(1);
        controller.state.video_generation = 9;
        assert!(!controller.subtitle_import_matches(8, &first));
        assert!(!controller.subtitle_import_matches(9, &first));
        assert!(controller.subtitle_import_matches(9, &second));
    }
}

#[cfg(test)]
#[path = "local_video_profile.rs"]
mod local_video_profile;

#[cfg(test)]
mod cached_library_controller_tests {
    use super::*;

    #[test]
    fn cached_publication_keeps_newer_imports_and_refresh_does_not_block_playlist_creation() {
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("song.mp3");
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/quiet.mp3"),
            &source,
        )
        .unwrap();
        let track = crate::library::scanner::read_track(source).unwrap();
        crate::library::database::Database::open(&directory.path().join("library.sqlite3"))
            .unwrap()
            .save(&track, 1)
            .unwrap();
        let events = thumbnails::load(directory.path().join("library.sqlite3"));
        let cached = events
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(matches!(cached, thumbnails::LoadEvent::Cached(_)));
        let mut controller = Controller::new(directory.path().to_owned());
        let mut imported = track;
        imported.title = "Newer imported title".into();
        controller.state.tracks = vec![imported];
        controller.state.navigation.open(Screen::Songs(Filter::All));
        let (sender, receiver) = std::sync::mpsc::channel();
        controller.library_tasks.replace_loading(receiver, false);
        sender.send(cached).unwrap();
        let ui = AppWindow::new().unwrap();
        controller.initialize(&ui);
        controller.poll(&ui);
        assert_eq!(controller.state.tracks[0].title, "Newer imported title");
        assert!(controller.library_tasks.ready());
        assert!(
            controller.library_tasks.loading_active(),
            "source refresh may still be running"
        );
        controller.execute(&ui, Action::NewPlaylist);
        assert!(!ui.get_playlist_library_loading());
        sender.send(thumbnails::LoadEvent::Complete).unwrap();
        controller.poll(&ui);
        assert!(!controller.library_tasks.loading_active());

        // A worker that stops before publication must not leave an endless
        // loading state. Existing imported tracks still remain available.
        let (sender, receiver) = std::sync::mpsc::channel::<thumbnails::LoadEvent>();
        drop(sender);
        controller.library_tasks.replace_loading(receiver, false);
        controller.poll(&ui);
        assert!(controller.library_tasks.ready());
        assert!(!controller.library_tasks.loading_active());
        assert!(controller.state.notice.contains("cached tracks"));
        assert!(!ui.get_playlist_library_loading());
        assert_eq!(controller.state.tracks[0].title, "Newer imported title");
    }
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
    fn cached_translation_is_available_from_settings_without_a_provider_request() {
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        let mut controller = Controller::new(directory.path().to_owned());
        let track = make_track(
            directory.path().join("song.wav"),
            "Song",
            Lyrics::plain_text("hello"),
        );
        let request = translation_request(&track, 1, TargetLanguage::English);
        controller
            .lyrics
            .insert_cached_for_test(fixture_translation(
                &request,
                serde_json::json!({"translations":["hello translated"]}),
            ));
        controller.state.tracks = vec![track];
        controller.state.current = Some(0);
        let ui = AppWindow::new().unwrap();
        controller.translation_command(&ui, crate::translation::Command::Translate);
        assert!(controller.lyrics.visible());
        assert!(!controller.lyrics.translation_busy());
        assert!(!controller.lyrics.lookup_busy());
        assert_eq!(
            ui.get_translated_lines().row_data(0).unwrap(),
            "hello translated"
        );
        assert_eq!(
            ui.get_displayed_lyric_lines().row_data(0).unwrap(),
            "hello",
            "translation must stay below the original lyric"
        );
        assert!(!ui.get_romaji_displayed());
        assert!(
            controller.state.romaji_visible,
            "translation must preserve R's preference"
        );
        controller.translation_command(&ui, crate::translation::Command::Toggle);
        assert_eq!(ui.get_displayed_lyric_lines().row_data(0).unwrap(), "hello");
        assert!(ui.get_romaji_displayed());
        assert!(
            !Settings::load(&directory.path().join("settings.json"))
                .unwrap()
                .translation_visible
        );
        controller.state.romaji_visible = false;
        controller.execute(&ui, Action::ToggleTranslation);
        assert!(
            !controller.lyrics.translation_busy(),
            "T must reuse the cache offline"
        );
        assert!(!controller.lyrics.lookup_busy());
        controller.execute(&ui, Action::ToggleTranslation);
        assert!(
            !ui.get_romaji_displayed(),
            "T off cannot enable a disabled R preference"
        );
    }

    #[test]
    fn t_starts_missing_translation_once_and_late_completion_keeps_it_hidden() {
        i_slint_backend_testing::init_no_event_loop();
        for initially_enabled in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            // Parsing fails before any provider can use an environment credential.
            std::fs::write(
                directory.path().join("deepseek.json"),
                "invalid test configuration",
            )
            .unwrap();
            let mut controller = Controller::new(directory.path().to_owned());
            let track = make_track(
                directory.path().join("song.wav"),
                "Song",
                Lyrics::plain_text("hello"),
            );
            controller.state.tracks = vec![track];
            controller.state.current = Some(0);
            controller.lyrics.set_visible(initially_enabled);
            let ui = AppWindow::new().unwrap();
            controller.execute(&ui, Action::ToggleTranslation);
            assert!(ui.get_translation_visible());
            assert!(controller.lyrics.translation_busy());
            assert!(!controller.lyrics.lookup_busy());
            let request = controller
                .lyrics
                .active_translation_for_test()
                .cloned()
                .unwrap();
            assert_eq!(request.source[0].text, "hello");
            assert!(ui.get_notice().contains("Generating translation"));
            assert!(!ui.get_romaji_displayed());
            assert!(controller.state.romaji_visible);

            controller.execute(&ui, Action::ToggleTranslation);
            assert!(!ui.get_translation_visible());
            assert!(ui.get_romaji_displayed());
            assert_eq!(
                controller.lyrics.translation_generation_for_test(),
                request.request_id
            );
            assert!(controller.lyrics.translation_busy());
            let translation =
                fixture_translation(&request, serde_json::json!({"translations":["translated"]}));
            assert_eq!(
                controller.apply_translation_result(request, Ok(translation)),
                "Translation ready"
            );
            controller.sync(&ui);
            assert!(!ui.get_translation_visible());
            assert!(
                !Settings::load(&directory.path().join("settings.json"))
                    .unwrap()
                    .translation_visible
            );
            assert_eq!(ui.get_displayed_lyric_lines().row_data(0).unwrap(), "hello");
        }
    }

    #[test]
    fn t_without_source_lyrics_enters_the_existing_online_lookup_path() {
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("deepseek.json"),
            "invalid test configuration",
        )
        .unwrap();
        let mut controller = Controller::new(directory.path().to_owned());
        let track = make_track(directory.path().join("song.wav"), "Song", Lyrics::default());
        let path = track.path.clone();
        controller.state.tracks = vec![track];
        controller.state.current = Some(0);
        let ui = AppWindow::new().unwrap();
        controller.execute(&ui, Action::ToggleTranslation);
        assert!(ui.get_translation_visible());
        assert!(controller.state.lyrics_lookup_status.contains_key(&path));
        assert!(!controller.lyrics.translation_busy());
        assert!(
            !controller.lyrics.lookup_busy(),
            "invalid credentials must prevent HTTP"
        );
        controller.execute(&ui, Action::ToggleTranslation);
        assert!(!ui.get_translation_visible());
    }

    #[test]
    fn translation_completion_cannot_override_a_later_t_off() {
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        let mut controller = Controller::new(directory.path().to_owned());
        let track = make_track(
            directory.path().join("song.wav"),
            "Song",
            Lyrics::plain_text("hello"),
        );
        let request = translation_request(&track, 8, TargetLanguage::English);
        let translation =
            fixture_translation(&request, serde_json::json!({"translations":["translated"]}));
        controller.state.tracks = vec![track];
        controller.state.current = Some(0);
        controller.lyrics.set_translation_generation_for_test(8);
        controller.lyrics.set_visible(true);
        let ui = AppWindow::new().unwrap();
        controller.translation_command(&ui, crate::translation::Command::Toggle);
        assert_eq!(
            controller.apply_translation_result(request, Ok(translation)),
            "Translation ready"
        );
        assert!(!controller.lyrics.visible());
        assert!(
            !Settings::load(&directory.path().join("settings.json"))
                .unwrap()
                .translation_visible
        );
        controller.sync(&ui);
        assert_eq!(ui.get_displayed_lyric_lines().row_data(0).unwrap(), "hello");
    }

    #[test]
    fn online_lyrics_reject_skips_metadata_changes_edits_and_cancellation_before_storage() {
        use crate::library::{
            database::{Database, LyricsAssociation},
            online_lyrics::{Found, Request},
        };
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        let mut controller = Controller::new(directory.path().to_owned());
        let track = make_track(directory.path().join("song.wav"), "Song", Lyrics::default());
        let request = Request::new(&track);
        controller
            .lyrics
            .set_lookup_continuation_for_test(Some(track.path.clone()));
        controller.state.tracks = vec![track.clone()];
        controller.state.current = Some(0);
        assert!(controller.accepts_online_lyrics(&request));
        controller.state.current = None;
        assert!(!controller.accepts_online_lyrics(&request));
        controller.lyrics.set_lookup_continuation_for_test(None);
        assert!(
            controller.accepts_online_lyrics(&request),
            "explicit Find can target a song that is not playing"
        );
        controller
            .lyrics
            .set_lookup_continuation_for_test(Some(request.path.clone()));
        controller.state.current = Some(0);
        controller.state.tracks[0].title = "different recording".into();
        assert!(!controller.accepts_online_lyrics(&request));
        controller.state.tracks[0] = track.clone();
        controller.state.tracks[0].lyrics = Lyrics::plain_text("manual edit");
        assert!(!controller.accepts_online_lyrics(&request));
        controller.state.tracks[0] = track;
        let cancellation = crate::translation::Cancellation::default();
        cancellation.cancel();
        controller.lyrics.install_lookup_completion(
            request.clone(),
            Ok(Found {
                lyrics: Lyrics::plain_text("found online"),
                source_url: "https://lrclib.net/api/get/1".into(),
            }),
            cancellation,
        );
        let ui = AppWindow::new().unwrap();
        controller.poll_online_lyrics(&ui);
        assert!(controller.lyrics_edit.is_none());
        assert!(controller.state.tracks[0].lyrics.is_empty());
        let db = Database::open(&directory.path().join("library.sqlite3")).unwrap();
        assert!(matches!(
            db.lyrics_association(&request.path).unwrap(),
            LyricsAssociation::Automatic
        ));
    }

    #[test]
    fn downloaded_plain_lyrics_can_upgrade_without_overwriting_a_manual_edit() {
        use crate::library::{
            database::Database,
            online_lyrics::{Found, Request},
        };
        i_slint_backend_testing::init_no_event_loop();
        for manual_edit in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let db = Database::open(&directory.path().join("library.sqlite3")).unwrap();
            let previous = Lyrics::plain_text("original first");
            let track = make_track(directory.path().join("song.wav"), "Song", previous.clone());
            db.set_online_lyrics(&track.path, "https://lrclib.net/api/get/1", &previous)
                .unwrap();
            let mut controller = Controller::new(directory.path().to_owned());
            controller.state.tracks = vec![track.clone()];
            controller.state.current = Some(0);
            let synced = Lyrics::parse("[00:01]original first");
            controller.lyrics.install_lookup_completion(
                Request::new(&track),
                Ok(Found {
                    lyrics: synced.clone(),
                    source_url: "https://lrclib.net/api/get/2".into(),
                }),
                crate::translation::Cancellation::default(),
            );
            if manual_edit {
                db.set_manual_lyrics(&track.path, &directory.path().join("manual.txt"), &previous)
                    .unwrap();
            }
            let ui = AppWindow::new().unwrap();
            controller.poll_online_lyrics(&ui);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while controller.lyrics_edit.is_some() && std::time::Instant::now() < deadline {
                controller.poll(&ui);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(controller.lyrics_edit.is_none());
            let expected = if manual_edit { previous } else { synced };
            assert_eq!(controller.state.tracks[0].lyrics, expected);
            let mut reloaded = track;
            db.apply_lyrics(&mut reloaded).unwrap();
            assert_eq!(reloaded.lyrics, expected);
            assert!(!controller.lyrics.translation_busy());
        }
    }

    #[test]
    fn accepted_online_lyrics_publish_after_storage_and_survive_restart() {
        use crate::library::{
            database::Database,
            online_lyrics::{Found, Request},
        };
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        let mut controller = Controller::new(directory.path().to_owned());
        let track = make_track(directory.path().join("song.wav"), "Song", Lyrics::default());
        let request = Request::new(&track);
        controller.state.tracks = vec![track.clone()];
        controller.state.current = Some(0);
        let lyrics = Lyrics::parse("[00:01]original first\n[00:02]original second");
        controller.lyrics.install_lookup_completion(
            request,
            Ok(Found {
                lyrics: lyrics.clone(),
                source_url: "https://lrclib.net/api/get/1".into(),
            }),
            crate::translation::Cancellation::default(),
        );
        let ui = AppWindow::new().unwrap();
        controller.poll_online_lyrics(&ui);
        assert!(
            !controller.dialog_open,
            "background lyric storage must not block playback keys"
        );
        assert!(
            controller.state.tracks[0].lyrics.is_empty(),
            "publish only after durable storage"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while controller.lyrics_edit.is_some() && std::time::Instant::now() < deadline {
            controller.poll(&ui);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(controller.lyrics_edit.is_none());
        assert_eq!(controller.state.tracks[0].lyrics, lyrics);
        assert_eq!(ui.get_lyric_lines().row_count(), 2);
        assert!(
            !controller.lyrics.translation_busy(),
            "Find alone does not translate"
        );
        let mut reloaded = track;
        Database::open(&directory.path().join("library.sqlite3"))
            .unwrap()
            .apply_lyrics(&mut reloaded)
            .unwrap();
        assert_eq!(reloaded.lyrics, lyrics);
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
        controller.genius.set_seed(seed.clone());
        controller.state.current = Some(1);
        assert_eq!(controller.prepare_genius_seed(false), Some(seed.clone()));
        assert_eq!(controller.prepare_genius_seed(true), Some(seed.clone()));
        assert_eq!(controller.genius.variation(), 1);
        let request = crate::genius::Request {
            request_id: 4,
            seed_path: seed.clone(),
            library_revision: controller.library_revision(),
            variation: controller.genius.variation(),
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
                .genius
                .entries()
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
        controller
            .lyrics
            .insert_cached_for_test(translation.clone());
        controller.lyrics.save_cache_for_test().unwrap();
        let token = controller
            .lyrics
            .install_translation_completion(request.clone(), Ok(translation.clone()));

        controller.remove_translation_for_current().unwrap();
        assert!(token.is_cancelled());
        assert!(!controller.lyrics.cached_for_test(&request).is_some());
        let _ = controller
            .lyrics
            .take_translation_completion_for_test()
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
        controller.lyrics.set_translation_generation_for_test(9);
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
        assert!(controller.lyrics.cache_is_empty_for_test());
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
        controller
            .lyrics
            .set_translation_generation_for_test(request.request_id);
        controller.lyrics.set_visible(true);
        controller.lyrics.insert_cached_for_test(previous.clone());
        std::fs::create_dir(dir.path().join("translations.json")).unwrap();
        assert_eq!(
            controller.apply_translation_result(request.clone(), Ok(replacement)),
            "Could not save translation cache"
        );
        assert_eq!(controller.lyrics.cached_for_test(&request), Some(&previous));
        assert!(controller.lyrics.visible());
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
        controller.lyrics.insert_cached_for_test(value);
        std::fs::create_dir(dir.path().join("translations.json")).unwrap();
        assert!(controller.remove_translation_for_current().is_err());
        assert!(controller.lyrics.cached_for_test(&request).is_some());
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
        controller.genius.set_seed(seed.clone());
        controller.genius.set_preview(
            seed.clone(),
            vec![
                genius::PreviewEntry {
                    title: "Seed".into(),
                    artist: "Artist".into(),
                    path: seed.clone(),
                },
                genius::PreviewEntry {
                    title: "Next".into(),
                    artist: "Artist".into(),
                    path: next.clone(),
                },
            ],
        );
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

#[cfg(test)]
#[path = "controller/library_refresh_tests.rs"]
mod library_refresh_tests;

#[cfg(test)]
#[path = "controller/ui_sync_tests.rs"]
mod ui_sync_tests;
