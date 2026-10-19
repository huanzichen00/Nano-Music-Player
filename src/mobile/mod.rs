//! A touch-first controller sharing the Rust audio and library core.
mod annotations;
mod artwork;
#[cfg(debug_assertions)]
mod audit;
mod gapless;
mod genius;
#[path = "../app/controller/genius.rs"]
#[allow(dead_code)]
mod genius_service;
#[path = "../app/controller/lyrics.rs"]
#[allow(dead_code)]
mod lyrics_service;
mod navigation;
mod playback;
#[path = "../app/controller/playlists.rs"]
#[allow(dead_code)]
mod playlist_service;
mod playlists;
mod statistics;
#[path = "../app/controller/statistics.rs"]
#[allow(dead_code)]
mod statistics_service;
pub mod storage;
#[path = "../app/thumbnails.rs"]
#[allow(dead_code)]
mod thumbnails;
pub mod transfer;

#[cfg(test)]
mod feedback_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod touch_tests;
#[cfg(test)]
mod visual_tests;
#[cfg(test)]
mod workflow_tests;

use crate::{
    audio::{
        PlaybackStatus,
        engine::{AudioCommand, AudioEvent, Engine},
    },
    library::{Track, scanner::ScanResult},
    platform::{
        ios::{self, Bridge, Event},
        macos_media::{MediaSnapshot, PlaybackState, RemoteCommand},
    },
    shared::{
        action::Action,
        config::Settings,
        i18n::{self, Language},
        settings::{Target as Setting, Values as SettingValues},
    },
    ui::{I18n, LibraryRow, MobileWindow},
};
use navigation::{Navigation, Page, Selection};
use playback::{Load, Playback};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::mpsc::{self, Receiver},
    time::Duration,
};
use storage::Storage;
use transfer::{Transfer, TransferEvent};

struct Controller {
    storage: Storage,
    engine: Engine,
    bridge: Bridge,
    playback: Playback,
    tracks: Vec<Track>,
    navigation: Navigation,
    entries: Vec<navigation::Entry>,
    visible: Vec<usize>,
    query: String,
    notice: String,
    job: Option<Receiver<Result<ScanResult, String>>>,
    picker_open: bool,
    artwork: artwork::Artwork,
    thumbnails: thumbnails::Thumbnails,
    row_artwork: Vec<Option<usize>>,
    row_model: Rc<VecModel<LibraryRow>>,
    media_artwork: Vec<u8>,
    media_marker: Option<(usize, PlaybackStatus, i64)>,
    deferred_load: Option<Load>,
    output_needs_reload: bool,
    audio_disconnected: bool,
    diagnostic_progress: Option<(usize, PlaybackStatus, bool)>,
    transfer: Option<Transfer>,
    transfer_status: String,
    pending_refresh: bool,
    canonical_paths: Vec<PathBuf>,
    published_urls: Vec<String>,
    deferred_seek: Option<f64>,
    import_notice: Option<String>,
    transfer_refresh_pending: bool,
    volume: f32,
    muted: bool,
    settings: Settings,
    playlists: playlist_service::PlaylistsService,
    saved_playlists: Vec<crate::library::playlists::Playlist>,
    playlist_jobs: playlists::PlaylistJobs,
    genius: genius_service::GeniusService,
    gapless: gapless::Gapless,
    annotations: annotations::Annotations,
    statistics: statistics_service::StatisticsService,
    ratings: std::collections::HashMap<PathBuf, u8>,
    ratings_ready: bool,
    smart_playlists:
        std::collections::HashMap<crate::library::statistics::SmartPlaylist, Vec<PathBuf>>,
    listening: Option<crate::library::statistics::ListeningSession>,
    listening_id: Option<usize>,
    listening_clock: std::time::Instant,
}

type ControllerHandle = Rc<RefCell<Controller>>;

impl Controller {
    fn new(storage: Storage) -> Result<Self, String> {
        let settings = Settings::load(&storage.settings_path())?;
        let (sender, loading) = mpsc::channel();
        let database = storage.database_path().to_owned();
        std::thread::spawn(move || {
            let _ = sender.send(crate::library::playlists::load(&database));
        });
        let annotations = annotations::Annotations::new(
            storage.support_dir().into(),
            settings.translation_visible,
        );
        let statistics =
            statistics_service::StatisticsService::new(storage.database_path().to_owned());
        let mut controller = Self {
            storage,
            engine: Engine::default(),
            bridge: Bridge::new(),
            playback: Playback::default(),
            tracks: Vec::new(),
            navigation: Navigation::default(),
            entries: Vec::new(),
            visible: Vec::new(),
            query: String::new(),
            notice: String::new(),
            job: None,
            picker_open: false,
            artwork: artwork::Artwork::default(),
            thumbnails: thumbnails::Thumbnails::default(),
            row_artwork: Vec::new(),
            row_model: Rc::new(VecModel::default()),
            media_artwork: Vec::new(),
            media_marker: None,
            deferred_load: None,
            output_needs_reload: false,
            audio_disconnected: false,
            diagnostic_progress: None,
            transfer: None,
            transfer_status: String::new(),
            pending_refresh: false,
            canonical_paths: Vec::new(),
            published_urls: Vec::new(),
            deferred_seek: None,
            import_notice: None,
            transfer_refresh_pending: false,
            volume: 0.8,
            muted: false,
            settings,
            playlists: playlist_service::PlaylistsService::new(loading),
            saved_playlists: Vec::new(),
            playlist_jobs: playlists::PlaylistJobs::default(),
            genius: genius_service::GeniusService::default(),
            gapless: gapless::Gapless::default(),
            annotations,
            statistics,
            ratings: Default::default(),
            ratings_ready: false,
            smart_playlists: Default::default(),
            listening: None,
            listening_id: None,
            listening_clock: std::time::Instant::now(),
        };
        controller
            .navigation
            .set_language(controller.settings.language);
        controller.scan(None);
        Ok(controller)
    }

    fn sync_preferences(&mut self, ui: &MobileWindow) {
        self.navigation.set_language(self.settings.language);
        ui.global::<I18n>()
            .set_language(self.settings.language.index() as i32);
        ui.set_device_mode(self.settings.device_mode);
        ui.set_smooth_slides(self.settings.smooth_slides);
        ui.set_lyrics_visible(self.settings.lyrics_visible);
        self.present_annotations(ui);
    }

    fn apply_setting(&mut self, ui: &MobileWindow, setting: Setting) {
        match &setting {
            Setting::Action(Action::ConfigureDeepSeek) => {
                self.open_api_settings(ui);
                return;
            }
            Setting::Translation(command) => {
                self.translation_command(ui, *command);
                return;
            }
            _ => {}
        }
        if let Setting::Screen(page) = setting {
            let languages = page == Page::Languages;
            self.navigate(ui, page);
            if languages {
                self.navigation.select(self.settings.language.index());
                self.rebuild_rows(ui);
            }
            return;
        }
        let gain_changed = match setting {
            Setting::Language(language) => {
                self.settings.language = language;
                false
            }
            Setting::DisplayMode(body) => {
                self.settings.device_mode = body;
                false
            }
            Setting::Action(Action::ToggleSmoothSlides) => {
                self.settings.smooth_slides = !self.settings.smooth_slides;
                false
            }
            Setting::Action(Action::ToggleLyrics) => {
                self.settings.lyrics_visible = !self.settings.lyrics_visible;
                false
            }
            Setting::Action(Action::ToggleReplayGainClipping) => {
                self.settings.replay_gain_prevent_clipping =
                    !self.settings.replay_gain_prevent_clipping;
                true
            }
            Setting::Action(Action::ToggleGaplessPlayback) => {
                self.cancel_gapless();
                self.settings.gapless_playback = !self.settings.gapless_playback;
                false
            }
            Setting::Action(Action::ToggleTranslation) => {
                self.settings.translation_visible = !self.settings.translation_visible;
                false
            }
            Setting::Action(Action::ToggleRomaji) => {
                self.settings.romaji_visible = !self.settings.romaji_visible;
                false
            }
            Setting::ReplayGain(mode) => {
                self.settings.replay_gain_mode = mode;
                true
            }
            Setting::ReplayGainPreamp(db) => {
                self.settings.replay_gain_preamp_db = db;
                true
            }
            _ => return,
        };
        self.notice.clear();
        let save_error = self.settings.save(&self.storage.settings_path()).err();
        self.sync_preferences(ui);
        if gain_changed {
            self.cancel_gapless();
            let position = self.playback.position;
            if let Some(load) = self.playback.reload() {
                self.load(ui, load);
                self.seek(ui, position);
            }
        }
        if let Some(error) = save_error {
            self.notice = format!("Settings could not be saved: {error}");
        }
        self.present_track(ui);
        self.rebuild_rows(ui);
        self.present(ui);
    }

    fn scan(&mut self, files: Option<Vec<PathBuf>>) {
        let storage = self.storage.clone();
        let (sender, receiver) = mpsc::channel();
        self.job = Some(receiver);
        std::thread::spawn(move || {
            let result = match files {
                Some(files) => storage.import_files(&files),
                None => storage.refresh(),
            };
            let _ = sender.send(result);
        });
    }

    fn current(&self) -> Option<&Track> {
        self.playback.current.and_then(|id| self.tracks.get(id))
    }

    fn rebuild_rows(&mut self, ui: &MobileWindow) {
        let mut entries = self
            .annotation_rows()
            .or_else(|| self.stats_rows())
            .unwrap_or_else(|| {
                self.navigation
                    .rows(&self.tracks, &self.query, &self.saved_playlists)
            });
        self.merge_stats_menu(&mut entries);
        if matches!(self.navigation.current(), Page::Music | Page::Settings)
            && self.current().is_some()
        {
            entries.push(navigation::Entry {
                title: i18n::translate(self.settings.language, "Song Options"),
                subtitle: String::new(),
                icon: 0,
                chevron: true,
                show_artwork: false,
                track: None,
                action: Selection::Command("song-options"),
            });
        }
        self.entries = entries;
        // Group covers reference a representative song without treating that
        // menu as a playable row. Build this index once per navigation change.
        let mut artists = std::collections::HashMap::new();
        let mut albums = std::collections::HashMap::new();
        for (id, track) in self.tracks.iter().enumerate() {
            if !track.artwork.is_empty() {
                artists.entry(track.artist.as_str()).or_insert(id);
                albums.entry(track.album.as_str()).or_insert(id);
            }
        }
        self.row_artwork = self
            .entries
            .iter()
            .map(|entry| {
                if !entry.show_artwork {
                    return None;
                }
                entry.track.or_else(|| match &entry.action {
                    Selection::Open(Page::Artist(name)) => artists.get(name.as_str()).copied(),
                    Selection::Open(Page::Songs(navigation::Filter::Album { name, .. })) => {
                        albums.get(name.as_str()).copied()
                    }
                    _ => None,
                })
            })
            .collect();
        let settings = SettingValues::from(&self.settings);
        self.visible = self
            .entries
            .iter()
            .filter_map(|entry| entry.track)
            .collect();
        let rows = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let target = match &entry.action {
                    Selection::Setting(target) => Some(target),
                    _ => None,
                };
                LibraryRow {
                    title: entry.title.as_str().into(),
                    subtitle: entry.subtitle.as_str().into(),
                    chevron: entry.chevron,
                    icon: if entry.track.is_some() && entry.track == self.playback.current {
                        8
                    } else {
                        entry.icon
                    },
                    show_artwork: entry.show_artwork,
                    artwork: self.row_artwork[index]
                        .and_then(|id| self.thumbnails.cached(id))
                        .unwrap_or_default(),
                    setting: self.navigation.is_settings_list(),
                    toggle: target.and_then(|target| settings.toggle(target)).is_some(),
                    checked: target.is_some_and(|target| {
                        settings.selected(target) || settings.toggle(target).unwrap_or(false)
                    }),
                    value: target
                        .map(|target| {
                            i18n::translate(self.settings.language, &settings.value(target))
                        })
                        .unwrap_or_default()
                        .into(),
                    ..Default::default()
                }
            })
            .collect::<Vec<_>>();
        let selected = if rows.is_empty() {
            if *self.navigation.current() == Page::Home {
                0
            } else {
                -1
            }
        } else {
            self.entries
                .iter()
                .position(|entry| entry.track.is_some() && entry.track == self.playback.current)
                .unwrap_or_else(|| self.navigation.selected().min(rows.len() - 1))
                as i32
        };
        ui.set_navigation_key(format!("{:?}", self.navigation.current()).into());
        self.row_model = Rc::new(VecModel::from(rows));
        ui.set_rows(self.row_model.clone().into());
        ui.set_selected(selected);
        ui.set_screen(self.navigation.screen());
        ui.set_screen_title(self.navigation.title().into());
        ui.set_library_count(self.tracks.len() as i32);
    }

    fn navigate(&mut self, ui: &MobileWindow, page: Page) {
        self.prepare_annotation_page(&page);
        if page == Page::Search {
            self.query.clear();
            ui.set_search_query("".into());
        }
        self.navigation.open(page);
        self.rebuild_rows(ui);
        self.present(ui);
    }

    fn activate(&mut self, ui: &MobileWindow, index: usize) {
        let Some(selection) = self.entries.get(index).map(|entry| entry.action.clone()) else {
            return;
        };
        self.navigation.select(index);
        match selection {
            Selection::Open(page) => self.navigate(ui, page),
            Selection::Play {
                track,
                order,
                shuffle,
            } => {
                self.select(ui, track, order.to_vec(), shuffle);
            }
            Selection::Command(command) => self.command(ui, command),
            Selection::Setting(setting) => self.apply_setting(ui, setting),
            Selection::EditPlaylist(id) => self.open_playlist_editor(ui, Some(id)),
            Selection::DeletePlaylist(id) => {
                self.playlist_command(ui, "delete-request", &id.to_string())
            }
        }
    }

    fn library_loaded(&mut self, ui: &MobileWindow, result: ScanResult) {
        self.cancel_gapless();
        let path = self.current().map(|track| track.path.clone());
        let queued_paths = self
            .playback
            .queue
            .order
            .iter()
            .filter_map(|id| self.tracks.get(*id))
            .map(|track| track.path.clone())
            .collect::<Vec<_>>();
        self.thumbnails.clear();
        self.tracks = result.tracks;
        if ui.get_playlist_editor_open() {
            self.refresh_playlist_candidates(ui);
        }
        if let Some(path) = path {
            if let Some(id) = self.tracks.iter().position(|track| track.path == path) {
                self.playback.current = Some(id);
                // Title sorting may change indexes. Preserve a search-scoped
                // run rather than unexpectedly adding every imported song.
                let mut order = queued_paths
                    .iter()
                    .filter_map(|path| self.tracks.iter().position(|track| &track.path == path))
                    .collect::<Vec<_>>();
                if !order.contains(&id) {
                    order.push(id);
                }
                let mut canonical = self
                    .canonical_paths
                    .iter()
                    .filter_map(|path| self.tracks.iter().position(|track| &track.path == path))
                    .collect::<Vec<_>>();
                if !canonical.contains(&id) {
                    canonical.push(id);
                }
                self.playback.queue.set(canonical, id, false);
                self.playback.queue.position =
                    order.iter().position(|queued| *queued == id).unwrap_or(0);
                self.playback.queue.order = order;
                if let Some(load) = self.deferred_load.as_mut() {
                    load.track = id;
                }
            } else {
                let _ = self.engine.send(AudioCommand::Stop { handoff: 0 });
                self.playback.clear();
                self.deferred_load = None;
                self.deferred_seek = None;
                self.media_artwork.clear();
                ui.set_artwork(slint::Image::default());
            }
        }
        self.on_stats_library_loaded();
        self.notice = if let Some(message) = self.import_notice.take() {
            format!("Library updated · {} songs; {message}", self.tracks.len())
        } else if result.warnings.is_empty() {
            format!("Library updated · {} songs", self.tracks.len())
        } else {
            format!("Loaded {} songs; {}", self.tracks.len(), result.warnings[0])
        };
        self.media_marker = None;
        self.rebuild_rows(ui);
        self.present_track(ui);
    }

    fn select(&mut self, ui: &MobileWindow, id: usize, order: Vec<usize>, shuffle: bool) {
        if id >= self.tracks.len() {
            return;
        }
        self.playback.shuffle = shuffle || self.playback.shuffle;
        self.canonical_paths = order
            .iter()
            .map(|id| self.tracks[*id].path.clone())
            .collect();
        let load = self.playback.select(id, order);
        self.navigation.open(Page::NowPlaying);
        self.load_user(ui, load);
    }

    fn load(&mut self, ui: &MobileWindow, load: Load) {
        self.load_from(ui, load, None, false);
    }

    fn load_user(&mut self, ui: &MobileWindow, load: Load) {
        self.load_from(ui, load, None, true);
    }

    fn load_from(&mut self, ui: &MobileWindow, load: Load, position: Option<f64>, explicit: bool) {
        crate::shared::diagnostics::record(
            "load_requested",
            serde_json::json!({
                "session": load.session, "track": load.track, "explicit": explicit,
                "interrupted": self.playback.interrupted(), "resume_position": position
            }),
        );
        self.cancel_gapless();
        self.on_stats_track_change();
        ui.set_artwork(slint::Image::default());
        self.media_artwork.clear();
        self.media_marker = None;
        self.notice.clear();
        self.deferred_seek = position;
        self.deferred_load = None;
        if let Some(position) = position {
            self.playback.position = position;
        }
        if let Some(track) = self.tracks.get(load.track) {
            self.artwork.request(load.session, track.artwork.clone());
        }
        self.present_track(ui);
        self.rebuild_rows(ui);
        if self.playback.interrupted() && !explicit {
            // Do not open a new audio output while iOS owns the interruption.
            self.deferred_load = Some(load);
            self.playback.status = PlaybackStatus::Paused;
            crate::shared::diagnostics::record(
                "load_deferred_for_interruption",
                serde_json::json!({"session": load.session}),
            );
        } else {
            self.dispatch_load(load);
        }
    }

    fn dispatch_load(&mut self, mut load: Load) {
        crate::shared::diagnostics::record(
            "session_activate_begin",
            serde_json::json!({"session": load.session}),
        );
        if let Err(message) = self.bridge.prepare_playback() {
            self.fail(message);
            return;
        }
        crate::shared::diagnostics::record(
            "session_activate_ok",
            serde_json::json!({"session": load.session}),
        );
        // This path is reached during an interruption only for an explicit user
        // request. A successful native activation supersedes a missing Ended.
        self.playback.session_reactivated();
        if self.audio_disconnected {
            self.engine = Engine::default();
            self.audio_disconnected = false;
        }
        if self.output_needs_reload {
            self.engine.reopen_on_next_load();
        }
        load.paused = !self.playback.should_play();
        let Some(track) = self.tracks.get(load.track) else {
            return;
        };
        let command = AudioCommand::LoadWithGain {
            id: load.session,
            path: track.path.clone(),
            volume: if self.muted { 0.0 } else { self.volume },
            paused: load.paused,
            gain: self.audio_gain(),
        };
        match self.engine.send(command) {
            Ok(()) => {
                self.output_needs_reload = false;
                self.diagnostic_progress = None;
                crate::shared::diagnostics::record(
                    "audio_load_sent",
                    serde_json::json!({"session": load.session, "paused": load.paused}),
                );
                if let Some(seconds) = self.deferred_seek.take() {
                    let _ = self.engine.send(AudioCommand::Seek(seconds));
                }
            }
            Err(message) => self.fail(message),
        }
    }

    fn fail(&mut self, message: String) {
        crate::shared::diagnostics::record(
            "playback_error",
            serde_json::json!({"session": self.playback.session, "message": message.chars().take(512).collect::<String>()}),
        );
        self.cancel_gapless();
        let _ = self.engine.send(AudioCommand::Pause(true));
        self.playback.user_intent(false);
        self.playback.status = PlaybackStatus::Error;
        self.notice = message;
        self.deferred_load = None;
        self.deferred_seek = None;
        self.media_marker = None;
    }

    fn set_playing(&mut self, ui: &MobileWindow, playing: bool) {
        if playing && self.playback.current.is_none() {
            if let Some(id) = self
                .visible
                .first()
                .copied()
                .or_else(|| (!self.tracks.is_empty()).then_some(0))
            {
                let order = if self.visible.is_empty() {
                    (0..self.tracks.len()).collect()
                } else {
                    self.visible.clone()
                };
                self.select(ui, id, order, false);
            }
            return;
        }
        if playing
            && matches!(
                self.playback.status,
                PlaybackStatus::Stopped | PlaybackStatus::Error
            )
        {
            if let Some(load) = self.playback.restart() {
                self.load_user(ui, load);
            }
            return;
        }
        self.playback.user_intent(playing);
        self.pause_stats();
        if playing
            && (self.playback.interrupted() || self.output_needs_reload || self.audio_disconnected)
        {
            if let Some(load) = self.deferred_load.take() {
                self.dispatch_load(load);
            } else {
                let position = self.deferred_seek.unwrap_or(self.playback.position);
                if let Some(load) = self.playback.reload() {
                    self.load_from(ui, load, Some(position), true);
                }
            }
        } else if self.playback.should_play() {
            if let Some(load) = self.deferred_load.take() {
                self.dispatch_load(load);
            } else if let Err(message) = self
                .bridge
                .prepare_playback()
                .and_then(|_| self.engine.send(AudioCommand::Pause(false)))
            {
                self.fail(message);
            }
        } else {
            let _ = self.engine.send(AudioCommand::Pause(true));
        }
    }

    fn command(&mut self, ui: &MobileWindow, command: &str) {
        if matches!(command, "back" | "home") {
            if ui.get_api_settings_open() {
                self.api_command(ui, "cancel");
                return;
            }
            if ui.get_lyric_editor_open() {
                self.lyric_editor_command(ui, "cancel");
                return;
            }
            if ui.get_playlist_editor_open() {
                self.playlist_command(ui, "cancel", "");
                return;
            }
            if ui.get_genius_preview_open() {
                self.genius_command(ui, "close");
                return;
            }
        }
        if self.annotation_command(ui, command) {
            return;
        }
        match command {
            "dismiss-notice" => self.notice.clear(),
            "home" => {
                self.notice.clear();
                self.navigation.home();
                ui.set_home_page(0);
                self.rebuild_rows(ui);
            }
            "back" => {
                self.notice.clear();
                self.navigation.back();
                self.rebuild_rows(ui);
            }
            "music" => self.navigate(ui, Page::Music),
            "songs" => self.navigate(ui, Page::Songs(navigation::Filter::All)),
            "search" => self.navigate(ui, Page::Search),
            "settings" => self.navigate(ui, Page::Settings),
            "song-options" => {
                if let Some(id) = self.playback.current {
                    self.navigate(ui, Page::Lyrics(id));
                }
            }
            "computer-transfer" => {
                self.navigation.open_transfer();
                self.rebuild_rows(ui);
            }
            "refresh" => {
                self.pending_refresh = true;
                self.notice = "Refreshing library…".into();
            }
            "device-mode" => {
                self.apply_setting(ui, Setting::DisplayMode(!self.settings.device_mode));
            }
            "lyrics" => self.apply_setting(ui, Setting::Action(Action::ToggleLyrics)),
            "genius" => self.open_genius(ui),
            "new-playlist" => self.open_playlist_editor(ui, None),
            "edit-playlist" => self.playlist_command(ui, "edit", ""),
            "toggle-playlist-editing" => self.playlist_command(ui, "toggle-list-editing", ""),
            "playlist-empty" => self.notice = "Empty playlist".into(),
            "playlist-editing" => {
                self.navigation.toggle_playlist_editing();
                self.rebuild_rows(ui);
            }
            "mute" => {
                self.muted = !self.muted;
                let _ = self.engine.send(AudioCommand::Volume(if self.muted {
                    0.0
                } else {
                    self.volume
                }));
            }
            "import" if self.job.is_none() && !self.picker_open => {
                match self.bridge.pick_music(self.storage.music_dir()) {
                    Ok(()) => {
                        self.picker_open = true;
                        self.notice = "Choose music to import".into();
                    }
                    Err(message) => self.notice = message,
                }
            }
            "toggle" => self.set_playing(ui, !self.playback.should_play()),
            "play" => self.set_playing(ui, true),
            "pause" => self.set_playing(ui, false),
            "next" => {
                if let Some(load) = self.playback.advance(true) {
                    self.load_user(ui, load);
                }
            }
            "previous" => {
                if self.playback.position > 3.0 {
                    self.seek(ui, 0.0);
                } else if let Some(load) = self.playback.previous() {
                    self.load_user(ui, load);
                }
            }
            "shuffle" => {
                self.cancel_gapless();
                self.playback.toggle_shuffle();
            }
            "repeat" => {
                self.cancel_gapless();
                self.playback.repeat = self.playback.repeat.cycle();
            }
            "transfer-start" if self.transfer.is_none() => {
                match Transfer::start(self.storage.clone()) {
                    Ok(transfer) => {
                        self.transfer = Some(transfer);
                        self.bridge.set_transfer_active(true);
                        self.transfer_status =
                            "You can switch apps briefly to share this address. If iOS pauses background transfer, return here to resume with the same address. Keep the app open for large uploads.".into();
                    }
                    Err(message) => self.transfer_status = message,
                }
            }
            "transfer-stop" => self.stop_transfer("Computer transfer stopped"),
            _ => {}
        }
        self.present(ui);
    }

    fn set_volume(&mut self, ui: &MobileWindow, volume: f32) {
        if !volume.is_finite() {
            return;
        }
        self.volume = volume.clamp(0.0, 1.0);
        self.muted = false;
        if let Err(message) = self.engine.send(AudioCommand::Volume(self.volume)) {
            self.notice = message;
        }
        self.present(ui);
    }

    fn seek(&mut self, ui: &MobileWindow, seconds: f64) {
        if !seconds.is_finite() {
            return;
        }
        let Some(duration) = self.current().map(|track| track.duration) else {
            return;
        };
        let seconds = seconds.clamp(0.0, duration.max(0.0));
        self.cancel_gapless();
        self.seek_stats();
        if self.playback.status == PlaybackStatus::Stopped
            && let Some(load) = self.playback.restart()
        {
            self.load(ui, load);
        }
        self.playback.position = seconds;
        if self.deferred_load.is_none() {
            if let Err(message) = self.engine.send(AudioCommand::Seek(seconds)) {
                self.notice = message;
            }
        } else {
            self.deferred_seek = Some(seconds);
        }
        self.media_marker = None;
        self.present(ui);
    }

    fn native_event(&mut self, ui: &MobileWindow, event: Event) {
        match event {
            Event::FilesSelected(files) => {
                self.picker_open = false;
                if files.is_empty() {
                    self.notice = "Import cancelled".into();
                } else {
                    self.notice = "Copying and reading music…".into();
                    self.scan(Some(files));
                }
            }
            Event::ImportFinished { imported, errors } => {
                self.picker_open = false;
                self.notice = if imported == 0 && errors.is_empty() {
                    "Import cancelled".into()
                } else if errors.is_empty() {
                    format!("Imported {imported} files; updating library…")
                } else {
                    format!("Imported {imported} files; {}", errors[0])
                };
                self.import_notice = (imported > 0).then(|| errors.first().cloned()).flatten();
                if imported > 0 {
                    self.scan(None);
                }
            }
            Event::Remote(command) => match command {
                RemoteCommand::Play => self.command(ui, "play"),
                RemoteCommand::Pause => self.command(ui, "pause"),
                RemoteCommand::Toggle => self.command(ui, "toggle"),
                RemoteCommand::Next => self.command(ui, "next"),
                RemoteCommand::Previous => self.command(ui, "previous"),
                RemoteCommand::SeekTo(seconds) => self.seek(ui, seconds),
            },
            Event::InterruptionBegan { .. } => {
                crate::shared::diagnostics::record(
                    "interruption_began",
                    serde_json::json!({"session": self.playback.session, "position": self.playback.position}),
                );
                self.cancel_gapless();
                self.output_needs_reload = true;
                self.playback.interruption_began();
                self.pause_stats();
                let _ = self.engine.send(AudioCommand::Pause(true));
            }
            Event::InterruptionEnded { should_resume, .. } => {
                crate::shared::diagnostics::record(
                    "interruption_ended",
                    serde_json::json!({"session": self.playback.session, "should_resume": should_resume}),
                );
                let was_interrupted = self.playback.interrupted();
                self.playback.interruption_ended(should_resume);
                self.pause_stats();
                if was_interrupted && self.playback.should_play() {
                    if let Some(load) = self.deferred_load.take() {
                        self.dispatch_load(load);
                    } else if self.output_needs_reload {
                        let position = self.playback.position;
                        if let Some(load) = self.playback.reload() {
                            self.load_from(ui, load, Some(position), false);
                        }
                    } else if let Err(message) = self
                        .bridge
                        .prepare_playback()
                        .and_then(|_| self.engine.send(AudioCommand::Pause(false)))
                    {
                        self.fail(message);
                    }
                }
            }
            Event::OutputDisconnected => {
                crate::shared::diagnostics::record(
                    "output_disconnected",
                    serde_json::json!({"session": self.playback.session}),
                );
                self.playback.user_intent(false);
                self.pause_stats();
                let _ = self.engine.send(AudioCommand::Pause(true));
            }
            Event::EnteredBackground => {
                crate::shared::diagnostics::record(
                    "entered_background",
                    serde_json::json!({"session": self.playback.session, "status": format!("{:?}", self.playback.status)}),
                );
                if self.transfer.is_some() {
                    self.transfer_status = "The address remains valid while you switch apps briefly. Background transfer pauses when the iOS allowance expires; returning here resumes the same address.".into();
                }
            }
            Event::TransferPaused => {
                if self.transfer.is_some() {
                    self.transfer_status = "Computer transfer paused after the background allowance expired. Return to the app to resume with the same address.".into();
                }
            }
            Event::TransferResumed => {
                if self.transfer.is_some() {
                    self.transfer_status = "Computer transfer resumed with the same address. Keep the app open for large uploads.".into();
                }
            }
        }
    }

    fn stop_transfer(&mut self, message: &str) {
        if let Some(mut transfer) = self.transfer.take() {
            transfer.stop();
            self.bridge.set_transfer_active(false);
            self.transfer_status = message.into();
            self.pending_refresh = true;
        }
    }

    fn poll(&mut self, ui: &MobileWindow) {
        self.poll_playlists(ui);
        self.poll_genius(ui);
        self.poll_annotations(ui);
        self.poll_statistics(ui);
        while let Some(event) = self.bridge.poll_event() {
            self.native_event(ui, event);
        }
        while let Some(event) = self.transfer.as_ref().and_then(Transfer::poll_event) {
            match event {
                TransferEvent::LibraryChanged(_) => {
                    // A server worker can complete after an earlier UI scan.
                    // Refresh after consuming it instead of applying snapshots
                    // that might arrive out of order across the two channels.
                    self.pending_refresh = true;
                    self.transfer_refresh_pending = true;
                    self.transfer_status = "Music received; updating library…".into();
                }
                TransferEvent::Notice(message) => self.transfer_status = message,
            }
        }
        if self
            .transfer
            .as_ref()
            .is_some_and(|transfer| !transfer.is_running())
        {
            let message = self.transfer_status.clone();
            self.stop_transfer(&message);
        }
        let loaded = self
            .job
            .as_ref()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Library worker stopped; import again".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = loaded {
            self.job = None;
            match result {
                Ok(result) => self.library_loaded(ui, result),
                Err(message) => self.notice = message,
            }
            if self.transfer_refresh_pending && !self.pending_refresh {
                self.transfer_status = self.notice.clone();
                self.transfer_refresh_pending = false;
            }
        }
        if self.pending_refresh && self.job.is_none() {
            self.pending_refresh = false;
            self.scan(None);
        }
        loop {
            let event = match self.engine.events.try_recv() {
                Ok(event) => event,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !self.audio_disconnected {
                        self.audio_disconnected = true;
                        self.fail("Audio worker stopped; tap Play to retry".into());
                    }
                    break;
                }
            };
            match event {
                AudioEvent::Progress {
                    id,
                    status,
                    position,
                    observed_at,
                } => {
                    let marker = (id, status, position > 0.0);
                    if self.diagnostic_progress != Some(marker) {
                        crate::shared::diagnostics::record(
                            "audio_progress",
                            serde_json::json!({"session": id, "status": format!("{status:?}"), "position": position}),
                        );
                        self.diagnostic_progress = Some(marker);
                    }
                    self.observe_stats(id, status, position, observed_at);
                    self.playback.progress(id, status, position);
                }
                AudioEvent::Finished(id) => {
                    self.complete_stats(id);
                    if let Some(load) = self.playback.finish(id) {
                        self.load(ui, load);
                    }
                }
                AudioEvent::Error { id, message }
                    if id.is_none_or(|id| id == self.playback.session) =>
                {
                    self.fail(message)
                }
                AudioEvent::Transitioned { from, to, token } => {
                    self.gapless_transition(ui, from, to, token);
                }
                AudioEvent::PrefetchFailed { id, token, .. } => {
                    self.gapless_failed(id, token);
                }
                _ => {}
            }
        }
        self.schedule_gapless();
        self.poll_thumbnails(ui);
        if let Some(cover) = self.artwork.poll(self.playback.session) {
            ui.set_artwork(cover.image);
            self.media_artwork = cover.media_bytes;
            self.media_marker = None;
        }
        self.present(ui);
    }

    fn poll_thumbnails(&mut self, ui: &MobileWindow) {
        let first = ui.get_first_visible_row().max(0) as usize;
        let count = ui.get_visible_row_count().max(0) as usize;
        let ids = if ui.get_library_visible() {
            self.row_artwork
                .iter()
                .skip(first.saturating_sub(1))
                .take(count + 2)
                .filter_map(|id| *id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        self.thumbnails.set_visible(&self.tracks, &ids, 192);
        if self.thumbnails.poll() {
            for (row, id) in self.row_artwork.iter().enumerate() {
                if let Some(image) = id.and_then(|id| self.thumbnails.cached(id))
                    && let Some(mut data) = self.row_model.row_data(row)
                    && data.artwork != image
                {
                    data.artwork = image;
                    self.row_model.set_row_data(row, data);
                }
            }
        }
    }

    fn present_track(&self, ui: &MobileWindow) {
        self.present_annotations(ui);
        let track = self.current();
        ui.set_track_title(
            track
                .map(title)
                .map(str::to_owned)
                .unwrap_or_else(|| i18n::translate(self.settings.language, "Choose a song"))
                .into(),
        );
        ui.set_track_artist(
            track
                .map(|track| track.artist.as_str())
                .filter(|artist| !artist.trim().is_empty())
                .unwrap_or("nano Music")
                .into(),
        );
        ui.set_track_album(
            track
                .map(|track| track.album.as_str())
                .unwrap_or_default()
                .into(),
        );
        ui.set_lyric_lines(ModelRc::new(VecModel::from(
            track
                .map(|track| track.lyrics.lines())
                .unwrap_or_default()
                .into_iter()
                .map(slint::SharedString::from)
                .collect::<Vec<_>>(),
        )));
        ui.set_timed_lyrics(track.is_some_and(|track| !track.lyrics.timed.is_empty()));
    }

    fn present(&mut self, ui: &MobileWindow) {
        self.present_playlists(ui);
        self.sync_statistics_ui(ui);
        let insets = ios::safe_area();
        ui.set_safe_top(insets.top);
        ui.set_safe_right(insets.right);
        ui.set_safe_bottom(insets.bottom);
        ui.set_safe_left(insets.left);
        ui.set_keyboard_height(ios::keyboard_height());
        let duration = self
            .current()
            .map(|track| track.duration)
            .unwrap_or(0.0)
            .max(0.0);
        let position = self.playback.position.clamp(0.0, duration);
        ui.set_has_track(self.current().is_some());
        ui.set_playing(
            self.playback.should_play()
                && matches!(
                    self.playback.status,
                    PlaybackStatus::Playing | PlaybackStatus::Loading
                ),
        );
        ui.set_loading(self.playback.status == PlaybackStatus::Loading);
        ui.set_elapsed(time(position).into());
        ui.set_duration(time(duration).into());
        ui.set_progress(if duration > 0.0 {
            (position / duration) as f32
        } else {
            0.0
        });
        ui.set_active_lyric(
            self.current()
                .and_then(|track| track.lyrics.active(position))
                .map(|id| id as i32)
                .unwrap_or(-1),
        );
        ui.set_busy(self.job.is_some() || self.picker_open);
        ui.set_shuffle(self.playback.shuffle);
        ui.set_repeat_mode(self.playback.repeat.label().into());
        ui.set_volume(self.volume);
        ui.set_muted(self.muted);
        ui.set_clock_text(chrono::Local::now().format("%-I:%M %p").to_string().into());
        ui.set_track_count(
            if self.playback.current.is_some() && !self.playback.queue.order.is_empty() {
                format!(
                    "{} / {}",
                    self.playback.queue.position + 1,
                    self.playback.queue.order.len()
                )
            } else {
                String::new()
            }
            .into(),
        );
        ui.set_notice(i18n::notice(self.settings.language, &self.notice).into());
        ui.set_transfer_running(self.transfer.is_some());
        ui.set_transfer_status(i18n::notice(self.settings.language, &self.transfer_status).into());
        let urls = self
            .transfer
            .as_ref()
            .map(Transfer::urls)
            .unwrap_or_default();
        if self.published_urls != urls {
            self.published_urls = urls.to_vec();
            ui.set_transfer_urls(ModelRc::new(VecModel::from(
                urls.iter()
                    .map(|url| slint::SharedString::from(url.as_str()))
                    .collect::<Vec<_>>(),
            )));
        }
        let marker = (self.playback.session, self.playback.status, position as i64);
        if self.media_marker != Some(marker) {
            self.media_marker = Some(marker);
            self.bridge
                .publish(self.current().map(|track| MediaSnapshot {
                    title: title(track).into(),
                    artist: track.artist.clone(),
                    album: track.album.clone(),
                    artwork: (!self.media_artwork.is_empty()).then(|| self.media_artwork.clone()),
                    duration_seconds: (duration > 0.0).then_some(duration),
                    elapsed_seconds: position,
                    playback_state: if self.playback.should_play()
                        && self.playback.status != PlaybackStatus::Error
                    {
                        PlaybackState::Playing
                    } else if self.playback.status == PlaybackStatus::Stopped {
                        PlaybackState::Stopped
                    } else {
                        PlaybackState::Paused
                    },
                }));
        }
    }

    fn audio_gain(&self) -> crate::audio::gain::GainConfig {
        crate::audio::gain::GainConfig {
            mode: self.settings.replay_gain_mode,
            preamp_db: self.settings.replay_gain_preamp_db,
            prevent_clipping: self.settings.replay_gain_prevent_clipping,
        }
    }
}

fn title(track: &Track) -> &str {
    if track.title.trim().is_empty() {
        track
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Untitled song")
    } else {
        &track.title
    }
}

fn time(seconds: f64) -> String {
    let seconds = if seconds.is_finite() {
        seconds.max(0.0) as u64
    } else {
        0
    };
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn connect_controller(ui: &MobileWindow, controller: ControllerHandle) {
    controller.borrow_mut().sync_preferences(ui);
    {
        let weak = ui.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_rating_changed(move |stars| {
            if let Some(ui) = weak.upgrade()
                && let Ok(stars) = u8::try_from(stars)
            {
                controller.borrow_mut().rate_track(&ui, stars);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_api_command(move |command| {
            if let Some(ui) = weak.upgrade() {
                controller.borrow_mut().api_command(&ui, &command);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_lyric_editor_command(move |command| {
            if let Some(ui) = weak.upgrade() {
                controller.borrow_mut().lyric_editor_command(&ui, &command);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_playlist_command(move |command, id| {
            if let Some(ui) = weak.upgrade() {
                controller.borrow_mut().playlist_command(&ui, &command, &id);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_genius_command(move |command| {
            if let Some(ui) = weak.upgrade() {
                controller.borrow_mut().genius_command(&ui, &command);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_copy_text(move |text| {
            if let Some(ui) = weak.upgrade() {
                let mut controller = controller.borrow_mut();
                controller.transfer_status = match ios::copy_text(ui.window(), &text) {
                    Ok(()) => "Address copied".into(),
                    Err(error) => format!("Could not copy address: {error}"),
                };
                controller.present(&ui);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_command(move |command| {
            if let Some(ui) = weak.upgrade() {
                controller.borrow_mut().command(&ui, &command);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_item_clicked(move |id| {
            if let Some(ui) = weak.upgrade()
                && id >= 0
            {
                controller.borrow_mut().activate(&ui, id as usize);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_home_activated(move |page, index| {
            if let Some(ui) = weak.upgrade() {
                let mut controller = controller.borrow_mut();
                controller.navigation.open_home(page, index);
                controller.rebuild_rows(&ui);
                controller.present(&ui);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_volume_changed(move |volume| {
            if let Some(ui) = weak.upgrade() {
                controller.borrow_mut().set_volume(&ui, volume);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_query_changed(move |query| {
            if let Some(ui) = weak.upgrade() {
                let mut controller = controller.borrow_mut();
                controller.query = query.to_string();
                controller.rebuild_rows(&ui);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_seek_changed(move |fraction| {
            if let Some(ui) = weak.upgrade() {
                let mut controller = controller.borrow_mut();
                let duration = controller
                    .current()
                    .map(|track| track.duration)
                    .unwrap_or(0.0);
                controller.seek(&ui, fraction as f64 * duration);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_lyric_clicked(move |id| {
            if let Some(ui) = weak.upgrade() {
                let mut controller = controller.borrow_mut();
                let seconds = usize::try_from(id)
                    .ok()
                    .and_then(|id| controller.current()?.lyrics.timed.get(id))
                    .map(|line| line.at_ms as f64 / 1000.0);
                if let Some(seconds) = seconds {
                    controller.seek(&ui, seconds);
                }
            }
        });
    }
    controller.borrow_mut().present(ui);
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    crate::platform::select_ui_backend()?;
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg| arg == "--ui-audit") {
        return audit::run();
    }
    let ui = MobileWindow::new()?;
    connect_translations(&ui);
    ui.set_library_initializing(true);
    ui.set_notice("Preparing music library…".into());
    let insets = ios::safe_area();
    ui.set_safe_top(insets.top);
    ui.set_safe_right(insets.right);
    ui.set_safe_bottom(insets.bottom);
    ui.set_safe_left(insets.left);
    let documents = ios::documents_dir().map_err(std::io::Error::other)?;
    let (sender, storage) = mpsc::channel();
    // Package hashing, audio copies and database migrations must finish before
    // services open caches, while the UI event loop stays responsive to UIKit.
    std::thread::Builder::new()
        .name("nano-mobile-storage".into())
        .spawn(move || {
            let result = Storage::new(documents);
            if let Ok(storage) = &result {
                crate::shared::diagnostics::init(storage.support_dir());
            }
            let _ = sender.send(result);
        })?;
    let mut storage = Some(storage);
    let controller: Rc<RefCell<Option<ControllerHandle>>> = Default::default();
    let timer = slint::Timer::default();
    let weak = ui.as_weak();
    let polling = Rc::clone(&controller);
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(50),
        move || {
            if let Some(ui) = weak.upgrade() {
                let result = storage.as_ref().map(Receiver::try_recv);
                match result {
                    Some(Err(mpsc::TryRecvError::Empty)) => {
                        ui.set_clock_text(
                            chrono::Local::now().format("%-I:%M %p").to_string().into(),
                        );
                        return;
                    }
                    Some(result) => {
                        storage = None;
                        let result = result
                            .map_err(|_| "The music library worker stopped unexpectedly".to_owned())
                            .and_then(|result| result)
                            .and_then(Controller::new);
                        match result {
                            Ok(ready) => {
                                let ready = Rc::new(RefCell::new(ready));
                                connect_controller(&ui, Rc::clone(&ready));
                                *polling.borrow_mut() = Some(ready);
                            }
                            Err(error) => ui.set_notice(error.into()),
                        }
                        ui.set_library_initializing(false);
                    }
                    None => {}
                }
                if let Some(controller) = polling.borrow().as_ref() {
                    controller.borrow_mut().poll(&ui);
                }
            }
        },
    );
    ui.run()?;
    timer.stop();
    if let Some(controller) = controller.borrow().as_ref() {
        controller.borrow().bridge.publish(None);
    }
    Ok(())
}

fn connect_translations(ui: &MobileWindow) {
    ui.global::<I18n>()
        .set_language(Language::English.index() as i32);
    ui.global::<I18n>().on_translate(|language, key| {
        i18n::translate(
            Language::ALL
                .get(language as usize)
                .copied()
                .unwrap_or_default(),
            &key,
        )
        .into()
    });
}
