//! Mobile routing around the desktop's source-bound lyric request service.
use super::{
    Controller, MobileWindow,
    lyrics_service::{LookupEvent, LyricsService, RomajiEvent},
    navigation::{Entry, Page, Selection},
};
use crate::{
    library::{
        Track,
        database::Database,
        lyrics::Lyrics,
        romaji,
        scanner::{self, LyricsUpdate},
    },
    shared::i18n,
    translation::Command,
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    sync::mpsc::{self, Receiver},
};

pub(super) struct Annotations {
    service: LyricsService,
    romaji: HashMap<PathBuf, romaji::Romaji>,
    saving: Option<Receiver<Result<Option<LyricsUpdate>, String>>>,
    target: Option<PathBuf>,
    editor_target: Option<PathBuf>,
}

impl Annotations {
    pub(super) fn new(directory: PathBuf, visible: bool) -> Self {
        Self {
            service: LyricsService::new(directory, visible),
            romaji: HashMap::new(),
            saving: None,
            target: None,
            editor_target: None,
        }
    }
}

fn entry(title: String, subtitle: String, action: Selection) -> Entry {
    Entry {
        title,
        subtitle,
        chevron: matches!(action, Selection::Open(_)),
        icon: 0,
        show_artwork: false,
        track: None,
        action,
    }
}

impl Controller {
    pub(super) fn prepare_annotation_page(&mut self, page: &Page) {
        match page {
            Page::Lyrics(id) | Page::LyricsSearch(id) => {
                self.annotations.target = self.tracks.get(*id).map(|track| track.path.clone())
            }
            Page::LyricSongs => self.annotations.target = None,
            _ => {}
        }
    }
    pub(super) fn annotation_rows(&self) -> Option<Vec<Entry>> {
        let tr = |key: &str| i18n::translate(self.settings.language, key);
        Some(match self.navigation.current() {
            Page::LyricSongs => self
                .tracks
                .iter()
                .enumerate()
                .map(|(id, track)| {
                    entry(
                        track.title.clone(),
                        track.artist.clone(),
                        Selection::Open(Page::Lyrics(id)),
                    )
                })
                .collect(),
            Page::Lyrics(id) | Page::LyricsSearch(id) => {
                let track = self
                    .annotations
                    .target
                    .as_ref()
                    .and_then(|path| self.tracks.iter().find(|track| &track.path == path))
                    .or_else(|| self.tracks.get(*id))?;
                [
                    ("Find Lyrics Online", "lyric-search"),
                    ("Edit Lyrics", "lyric-edit"),
                    ("Copy Lyrics", "lyric-copy"),
                    ("Remove Lyrics", "lyric-remove"),
                    ("Use Automatic Lyrics", "lyric-automatic"),
                    ("Generate Romaji", "romaji-generate"),
                    ("Cancel Romaji Generation", "romaji-cancel"),
                    ("Remove Romaji", "romaji-remove"),
                    ("Cancel Lyric Search", "lyric-search-cancel"),
                ]
                .into_iter()
                .map(|(title, command)| {
                    entry(tr(title), track.title.clone(), Selection::Command(command))
                })
                .collect()
            }
            _ => return None,
        })
    }

    pub(super) fn annotation_track_ref(&self) -> Option<&Track> {
        match self.navigation.current() {
            Page::Lyrics(id) | Page::LyricsSearch(id) => {
                let indexed = self.tracks.get(*id);
                match self.annotations.target.as_ref() {
                    Some(path) => indexed
                        .filter(|track| &track.path == path)
                        .or_else(|| self.tracks.iter().find(|track| &track.path == path))
                        .or(indexed),
                    None => indexed,
                }
            }
            _ => self.current(),
        }
    }

    pub(super) fn annotation_track(&self) -> Option<Track> {
        self.annotation_track_ref().cloned()
    }

    pub(super) fn open_api_settings(&mut self, ui: &MobileWindow) {
        let path = self.storage.support_dir().join("deepseek.json");
        match crate::ai::DeepSeekSettings::load(&path) {
            Ok(settings) => {
                ui.set_api_key_configured(settings.has_key());
                ui.set_api_key("".into());
                ui.set_api_model(settings.model.into());
                ui.set_api_message("".into());
                ui.set_api_settings_open(true);
            }
            Err(error) => self.notice = error,
        }
    }

    pub(super) fn api_command(&mut self, ui: &MobileWindow, command: &str) {
        if !ui.get_api_settings_open() {
            return;
        }
        if command == "paste-api-key" {
            match crate::platform::ios::paste_text(ui.window()) {
                Ok(text) if !text.trim().is_empty() => {
                    ui.set_api_key(text.trim().into());
                    ui.set_api_message("".into());
                }
                Ok(_) => ui.set_api_message(
                    i18n::translate(self.settings.language, "Clipboard has no text").into(),
                ),
                Err(error) => {
                    ui.set_api_message(i18n::translate(self.settings.language, &error).into())
                }
            }
            return;
        }
        if command == "cancel" {
            ui.set_api_key("".into());
            ui.set_api_settings_open(false);
            return;
        }
        if command != "save" {
            return;
        }
        let path = self.storage.support_dir().join("deepseek.json");
        let result = (|| {
            let mut settings = crate::ai::DeepSeekSettings::load(&path)?;
            let key = ui.get_api_key().trim().to_owned();
            if !key.is_empty() {
                settings.api_key = key;
            }
            settings.model = ui.get_api_model().trim().to_owned();
            if !settings.has_key() {
                return Err("Enter a DeepSeek API key".into());
            }
            settings.save(&path)
        })();
        match result {
            Ok(()) => {
                ui.set_api_key("".into());
                ui.set_api_settings_open(false);
                self.notice = "DeepSeek settings saved".into();
            }
            Err(error) => {
                ui.set_api_message(i18n::translate(self.settings.language, &error).into())
            }
        }
        self.present(ui);
    }

    pub(super) fn translation_command(&mut self, ui: &MobileWindow, command: Command) {
        let current = self.current().cloned();
        let result = match command {
            Command::Translate | Command::Regenerate => {
                if current
                    .as_ref()
                    .is_some_and(|track| track.lyrics.is_empty())
                {
                    self.annotations
                        .service
                        .start_lookup(current.as_ref(), true)
                        .map(|path| {
                            self.annotations.target = Some(path);
                        })
                } else {
                    self.annotations
                        .service
                        .start_translation(current.as_ref(), self.settings.translation_target)
                }
            }
            Command::Cancel => {
                self.annotations.service.cancel_translation();
                self.annotations.service.cancel_lookup();
                Ok(())
            }
            Command::Remove => self
                .annotations
                .service
                .remove_translation(current.as_ref(), self.settings.translation_target),
            Command::Toggle => {
                self.settings.translation_visible = !self.settings.translation_visible;
                self.settings.save(&self.storage.settings_path())
            }
            Command::FindLyrics => self
                .annotations
                .service
                .start_lookup(current.as_ref(), false)
                .map(|path| {
                    self.annotations.target = Some(path);
                }),
            Command::SetTarget(target) => {
                self.annotations
                    .service
                    .change_target(self.settings.translation_target, target);
                self.settings.translation_target = target;
                self.settings.save(&self.storage.settings_path())
            }
        };
        self.notice = result.err().unwrap_or_else(|| match command {
            Command::Translate | Command::Regenerate => "Translating lyrics…".into(),
            Command::Cancel => "Translation cancelled".into(),
            Command::Remove => "Translation removed".into(),
            Command::SetTarget(_) => String::new(),
            Command::Toggle | Command::FindLyrics => String::new(),
        });
        if matches!(command, Command::Translate | Command::Regenerate)
            && self.notice == "Translating lyrics…"
        {
            self.settings.translation_visible = true;
            self.annotations.service.set_visible(true);
            if let Err(error) = self.settings.save(&self.storage.settings_path()) {
                self.notice = error;
            }
        }
        self.present_annotations(ui);
        self.rebuild_rows(ui);
        self.present(ui);
    }

    pub(super) fn annotation_command(&mut self, ui: &MobileWindow, command: &str) -> bool {
        if !command.starts_with("lyric-") && !command.starts_with("romaji-") {
            return false;
        }
        crate::shared::diagnostics::record(
            "lyrics_action",
            serde_json::json!({"command": command.chars().take(40).collect::<String>(), "session": self.playback.session}),
        );
        let track = self.annotation_track();
        let Some(track) = track else {
            self.notice = "Choose a song first".into();
            return true;
        };
        if self.annotations.saving.is_some() {
            self.notice = "Saving…".into();
            return true;
        }
        self.notice = match command {
            "lyric-search" => match self.annotations.service.start_lookup(Some(&track), false) {
                Ok(path) => {
                    self.annotations.target = Some(path);
                    "Searching lyrics…".into()
                }
                Err(error) => error,
            },
            "lyric-search-cancel" => {
                self.annotations.service.cancel_lookup();
                "Search cancelled".into()
            }
            "lyric-edit" => {
                ui.set_lyric_editor_text(editable_lyrics(&track.lyrics).into());
                ui.set_lyric_editor_message("".into());
                self.annotations.editor_target = Some(track.path.clone());
                ui.set_lyric_editor_open(true);
                String::new()
            }
            "lyric-copy" => {
                match crate::platform::ios::copy_text(ui.window(), &editable_lyrics(&track.lyrics))
                {
                    Ok(()) => "Lyrics copied".into(),
                    Err(error) => error,
                }
            }
            "lyric-remove" | "lyric-automatic" => {
                self.start_lyric_write(track.path, None, command == "lyric-automatic");
                "Saving…".into()
            }
            "romaji-generate" => self
                .annotations
                .service
                .start_romaji(Some(&track))
                .map(|_| "Generating romaji…".into())
                .unwrap_or_else(|error| error),
            "romaji-cancel" => {
                self.annotations.service.cancel_romaji();
                "Romaji generation cancelled".into()
            }
            "romaji-remove" => {
                let removal = self.annotations.service.remove_romaji(track.path.clone());
                self.annotations.romaji.remove(&track.path);
                removal
                    .result
                    .err()
                    .unwrap_or_else(|| "Romaji removed".into())
            }
            _ => return false,
        };
        self.present_annotations(ui);
        self.present(ui);
        true
    }

    pub(super) fn lyric_editor_command(&mut self, ui: &MobileWindow, command: &str) {
        if !ui.get_lyric_editor_open() || self.annotations.saving.is_some() {
            return;
        }
        if command == "cancel" {
            ui.set_lyric_editor_open(false);
            self.annotations.editor_target = None;
            return;
        }
        if command != "save" {
            return;
        }
        let text = ui.get_lyric_editor_text().to_string();
        if text.len() > 512 * 1024 {
            ui.set_lyric_editor_message("Lyrics are too large".into());
            return;
        }
        let lyrics = parsed_lyrics(&text);
        if lyrics.is_empty() {
            ui.set_lyric_editor_message("Enter lyrics before saving".into());
            return;
        }
        if let Some(path) = self.annotations.editor_target.clone() {
            self.start_lyric_write(path, Some(text), false);
            ui.set_lyric_editor_saving(true);
        }
    }

    fn start_lyric_write(&mut self, path: PathBuf, text: Option<String>, automatic: bool) {
        self.annotations.service.cancel_lookup();
        self.annotations.service.cancel_translation();
        let database = self.storage.database_path().to_owned();
        let support = self.storage.support_dir().to_owned();
        let (sender, receiver) = mpsc::channel();
        self.annotations.saving = Some(receiver);
        std::thread::spawn(move || {
            let result = (|| {
                let db = Database::open(&database)?;
                if let Some(text) = text {
                    let source = support
                        .join("manual-lyrics")
                        .join(format!("{:016x}.lrc", rand::random::<u64>()));
                    std::fs::create_dir_all(source.parent().unwrap())
                        .map_err(|error| error.to_string())?;
                    std::fs::write(&source, &text).map_err(|error| error.to_string())?;
                    if let Err(error) = db.set_manual_lyrics(&path, &source, &parsed_lyrics(&text))
                    {
                        let _ = std::fs::remove_file(source);
                        return Err(error);
                    }
                } else if automatic {
                    db.use_automatic_lyrics(&path)?;
                } else {
                    db.remove_lyrics(&path)?;
                }
                let track = scanner::refresh_track(&db, &path)?;
                Ok(Some(LyricsUpdate {
                    path,
                    lyrics: track.lyrics,
                }))
            })();
            let _ = sender.send(result);
        });
    }

    pub(super) fn poll_annotations(&mut self, ui: &MobileWindow) {
        let mut changed = false;
        let lookup = if self.annotations.service.lookup_busy() {
            let selected = self
                .annotations
                .target
                .as_ref()
                .and_then(|path| self.tracks.iter().find(|track| &track.path == path));
            self.annotations.service.poll_lookup(
                &self.tracks,
                &BTreeSet::new(),
                self.annotations.saving.is_some(),
                selected,
            )
        } else {
            None
        };
        if let Some(event) = lookup {
            match event {
                LookupEvent::Found { request, found } => {
                    self.annotations.saving = Some(scanner::store_online_lyrics(
                        self.storage.database_path().into(),
                        request.path,
                        found,
                        (!request.previous_lyrics.is_empty()).then_some(request.previous_lyrics),
                    ))
                }
                LookupEvent::Failed { notice, .. } => self.notice = notice,
            }
            changed = true;
        }
        if let Some(result) = self
            .annotations
            .saving
            .as_ref()
            .and_then(|job| job.try_recv().ok())
        {
            self.annotations.saving = None;
            ui.set_lyric_editor_saving(false);
            match result {
                Ok(Some(update)) => {
                    if let Some(track) = self
                        .tracks
                        .iter_mut()
                        .find(|track| track.path == update.path)
                    {
                        track.lyrics = update.lyrics;
                    }
                    ui.set_lyric_editor_open(false);
                    self.annotations.editor_target = None;
                    self.notice = "Lyrics updated".into();
                    let (_, translate) = self.annotations.service.take_storage_context();
                    if translate.as_ref() == self.current().map(|track| &track.path)
                        && translate.is_some()
                    {
                        let current = self.playback.current.and_then(|id| self.tracks.get(id));
                        if let Err(error) = self
                            .annotations
                            .service
                            .start_translation(current, self.settings.translation_target)
                        {
                            self.notice = error;
                        }
                    }
                }
                Ok(None) => self.notice = "Import cancelled".into(),
                Err(error) => {
                    self.annotations.service.take_storage_context();
                    ui.set_lyric_editor_message(
                        i18n::translate(self.settings.language, &error).into(),
                    );
                    self.notice = error;
                }
            }
            changed = true;
        }
        let current = self.playback.current.and_then(|id| self.tracks.get(id));
        if let Some(notice) = self
            .annotations
            .service
            .poll_translation(current, self.settings.translation_target)
        {
            self.notice = notice.into();
            changed = true;
        }
        for event in self.annotations.service.poll_romaji(&self.tracks) {
            match event {
                RomajiEvent::Loaded(cache) => self.annotations.romaji.extend(cache),
                RomajiEvent::Generated {
                    path,
                    romaji,
                    storage,
                } => {
                    self.annotations.romaji.insert(path, romaji);
                    self.notice = storage.err().unwrap_or_else(|| "Romaji ready".into());
                }
                RomajiEvent::Progress { done, total } => {
                    self.notice = format!("Generating romaji… {done}/{total}")
                }
                RomajiEvent::StorageFailed(error) => self.notice = error,
                RomajiEvent::Finished { notice } => self.notice = notice,
            }
            changed = true;
        }
        ui.set_annotation_busy(
            self.annotations.service.translation_busy()
                || self.annotations.service.lookup_busy()
                || self.annotations.saving.is_some(),
        );
        if changed {
            self.present_track(ui);
            self.rebuild_rows(ui);
        }
    }

    pub(super) fn present_annotations(&self, ui: &MobileWindow) {
        let current = self.current();
        let translated = current.and_then(|track| {
            self.annotations
                .service
                .cached_translation(track, self.settings.translation_target)
        });
        ui.set_translated_lines(ModelRc::new(VecModel::from(
            translated
                .map(|value| {
                    value
                        .lines
                        .iter()
                        .map(|line| line.as_str().into())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        )));
        let romaji = current.and_then(|track| {
            self.annotations
                .romaji
                .get(&track.path)
                .filter(|value| value.matches(&track.lyrics))
        });
        ui.set_romaji_lines(ModelRc::new(VecModel::from(
            romaji
                .map(|value| {
                    value
                        .lines
                        .iter()
                        .map(|line| line.as_str().into())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        )));
        ui.set_translation_visible(self.settings.translation_visible && translated.is_some());
        ui.set_romaji_visible(self.settings.romaji_visible);
    }
}

fn parsed_lyrics(text: &str) -> Lyrics {
    let parsed = Lyrics::parse(text);
    if parsed.timed.is_empty() {
        Lyrics::plain_text(text)
    } else {
        parsed
    }
}

fn editable_lyrics(lyrics: &Lyrics) -> String {
    if lyrics.timed.is_empty() {
        return lyrics.plain.join("\n");
    }
    lyrics
        .timed
        .iter()
        .map(|line| {
            format!(
                "[{:02}:{:02}.{:03}]{}",
                line.at_ms / 60_000,
                line.at_ms / 1000 % 60,
                line.at_ms % 1000,
                line.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_preserves_lrc_timestamps_and_plain_text() {
        let timed = Lyrics::parse("[00:01.250]日本語\n[00:02.500]中文");
        assert_eq!(parsed_lyrics(&editable_lyrics(&timed)), timed);
        let plain = Lyrics::plain_text("one\n\n  two");
        assert_eq!(parsed_lyrics(&editable_lyrics(&plain)), plain);
    }
}
