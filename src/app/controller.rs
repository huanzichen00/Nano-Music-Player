use super::{
    action::{Action, Category},
    artwork::ArtworkLoader,
    config::Settings,
    i18n::{self, Language, translate},
    navigation::{Filter, Group, Screen},
    state::{Effect, State, Target},
};
use crate::{
    audio::{
        PlaybackStatus,
        engine::{AudioEvent, Engine},
    },
    input::{Binding, Context, Dispatch, Dispatcher, Key, KeyPress, Keymap, Modifiers},
    library::{Track, romaji, scanner},
    ui::{AppWindow, I18n, LibraryRow, ListMetrics, ShortcutRow},
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::mpsc::Receiver,
};

pub struct Controller {
    pub state: State,
    pub dispatcher: Dispatcher,
    engine: Engine,
    directory: PathBuf,
    settings: Settings,
    loading: Option<Receiver<Result<Vec<Track>, String>>>,
    importing: Option<Receiver<scanner::ImportEvent>>,
    lyrics_edit: Option<Receiver<Result<Option<scanner::LyricsUpdate>, String>>>,
    romaji_request: Option<romaji::Request>,
    romaji_store: romaji::Store,
    romaji_touched: HashSet<PathBuf>,
    lyrics_dirty: bool,
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
    letter_jump: Option<(std::time::Instant, Screen)>,
    thumbnails: HashMap<usize, slint::Image>,
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
        state.romaji_visible = settings.romaji_visible;
        state.zoom = settings.zoom;
        state.device_mode = settings.device_mode;
        let keymap =
            Keymap::load_or_create(&directory.join("shortcuts.json")).unwrap_or_else(|message| {
                state.notice = format!("{message}; using default shortcuts");
                Keymap::default()
            });
        let loading = Some(scanner::load(directory.join("library.sqlite3")));
        let romaji_store = romaji::Store::new(directory.join("library.sqlite3"));
        Self {
            state,
            dispatcher: Dispatcher::new(keymap),
            engine: Engine::default(),
            directory,
            settings,
            loading,
            importing: None,
            lyrics_edit: None,
            romaji_request: None,
            romaji_store,
            romaji_touched: HashSet::new(),
            lyrics_dirty: true,
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
            letter_jump: None,
            thumbnails: HashMap::new(),
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

    pub fn dispatch(
        &mut self,
        ui: &AppWindow,
        event: slint::language::KeyEvent,
        text_input: bool,
    ) -> bool {
        let key = Key::from_slint(event.text.as_str());
        if ui.get_deepseek_settings_open() {
            if key == Some(Key::Escape) {
                self.deepseek_command(ui, "cancel");
                return true;
            }
            return false;
        }
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
            .partition(|effect| matches!(effect, Effect::Audio(_)));
        self.effects(ui, audio);
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
                    self.importing = Some(scanner::import(
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
                Effect::RomajiVisibilityChanged => {
                    self.rows_dirty = true;
                    self.sync(ui);
                    self.save_settings(ui);
                }
                Effect::ConfigureDeepSeek => {
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
            language: self.state.language,
            romaji_visible: self.state.romaji_visible,
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
                    self.lyrics_dirty = true;
                    self.thumbnails.clear();
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
                    self.lyrics_dirty = true;
                    self.thumbnails.clear();
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
        if let Some(artwork) = self.artwork.poll() {
            ui.set_artwork(artwork);
        }
    }

    fn sync(&mut self, ui: &AppWindow) {
        ui.global::<I18n>()
            .set_language(self.state.language.index() as i32);
        let current_screen = self.state.navigation.current().screen.clone();
        let screen = &current_screen;
        let screen_changed = self.last_screen.as_ref() != Some(screen);
        ui.set_screen(match screen {
            Screen::Home => 0,
            Screen::NowPlaying => 2,
            Screen::Search => 3,
            Screen::Placeholder(_) => 4,
            Screen::Settings => 5,
            Screen::Shortcuts => 6,
            Screen::Lyrics(_) => 7,
            Screen::Languages => 8,
            _ => 1,
        });
        ui.set_screen_title(match screen {
            Screen::Artist(_) | Screen::Songs(Filter::Album { .. } | Filter::Genre(_)) => {
                screen.title().into()
            }
            _ => self.text(screen.title()),
        });
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
            if self.notice_since.elapsed().as_secs() < 3
                || self.state.importing
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
            let metadata_rows = matches!(
                screen,
                Screen::Groups(Group::Artists | Group::Albums | Group::Genres)
                    | Screen::Artist(_)
                    | Screen::Songs(_)
                    | Screen::Search
            );
            let rows = self
                .state
                .rows()
                .into_iter()
                .map(|row| LibraryRow {
                    title: if metadata_rows {
                        row.title.into()
                    } else {
                        self.text(&row.title)
                    },
                    subtitle: if let Target::Shortcut(action) = row.target {
                        self.text(&self.dispatcher.keymap.labels(action))
                    } else {
                        if metadata_rows || matches!(screen, Screen::Lyrics(_) | Screen::Languages)
                        {
                            row.subtitle.into()
                        } else {
                            self.text(&row.subtitle)
                        }
                    },
                    artwork: row
                        .artwork_id
                        .map(|id| {
                            self.thumbnails
                                .entry(id)
                                .or_insert_with(|| {
                                    decode_artwork(
                                        &self.state.tracks[id].artwork,
                                        ((ui.global::<ListMetrics>().get_artwork_row_height()
                                            - 2.0
                                                * ui.global::<ListMetrics>().get_artwork_padding())
                                            * 3.0
                                            * ui.window().scale_factor())
                                        .ceil() as u32,
                                    )
                                })
                                .clone()
                        })
                        .unwrap_or_default(),
                    show_artwork: matches!(
                        screen,
                        Screen::Groups(
                            super::navigation::Group::Artists | super::navigation::Group::Albums
                        ) | Screen::Artist(_)
                    ),
                    chevron: !matches!(row.target, Target::Play(_) | Target::Language(_)),
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
                    | Screen::Languages
            ) {
                let row_height = ui.get_list_row_height();
                let row_y = self.state.navigation.selected() as f32 * row_height;
                let visible_height = ui.get_list_viewport_height()
                    - if *screen == Screen::Search {
                        44.0 * scale
                    } else {
                        0.0
                    };
                let scroll = ui.get_list_scroll_y();
                if row_y + scroll < 0.0 {
                    ui.set_list_scroll_y(-row_y);
                } else if row_y + row_height + scroll > visible_height {
                    ui.set_list_scroll_y(visible_height - row_y - row_height);
                }
            }
            self.last_selected = self.state.navigation.selected();
        }
        self.last_screen = Some(screen.clone());
        let track = self.state.current.and_then(|id| self.state.tracks.get(id));
        let artwork_resolution = (240.0 * scale * ui.window().scale_factor()).ceil() as u32;
        let track_changed = self.last_track != self.state.current;
        if track_changed || artwork_resolution != self.artwork_resolution {
            let artwork = track
                .zip(self.state.current)
                .map(|(track, id)| {
                    let preview = self.thumbnails.get(&id).cloned().or_else(|| {
                        self.thumbnails.iter().find_map(|(cached_id, image)| {
                            self.state
                                .tracks
                                .get(*cached_id)
                                .filter(|cached| cached.artwork == track.artwork)
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

fn decode_artwork(bytes: &[u8], resolution: u32) -> slint::Image {
    image::load_from_memory(bytes)
        .ok()
        .map(|image| {
            let image = image.thumbnail(resolution, resolution).to_rgba8();
            slint::Image::from_rgba8(
                slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                    image.as_raw(),
                    image.width(),
                    image.height(),
                ),
            )
        })
        .unwrap_or_default()
}
