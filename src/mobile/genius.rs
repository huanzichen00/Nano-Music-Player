//! Mobile actions for the exact desktop Genius request and preview service.

use super::{Controller, MobileWindow, navigation::Page, playlists::available_name};
use crate::{
    library::playlists::{self, Playlist},
    shared::i18n,
    ui::GeniusRow,
};
use slint::{ModelRc, VecModel};
use std::{path::Path, sync::mpsc};

impl Controller {
    pub(super) fn open_genius(&mut self, ui: &MobileWindow) {
        if ui.get_playlist_editor_open()
            || ui.get_genius_preview_open()
            || self.playlist_jobs.busy()
        {
            return;
        }
        self.genius.close();
        let seed = self.current().cloned();
        if let Some(track) = &seed {
            self.genius.set_seed(track.path.clone());
        }
        ui.set_genius_seed_title(
            seed.as_ref()
                .map(|track| track.title.as_str())
                .unwrap_or("")
                .into(),
        );
        ui.set_genius_seed_artist(
            seed.as_ref()
                .map(|track| track.artist.as_str())
                .unwrap_or("")
                .into(),
        );
        ui.set_genius_seed_available(seed.is_some());
        ui.set_genius_stage(0);
        ui.set_genius_variation(0);
        ui.set_genius_busy(false);
        ui.set_genius_message("".into());
        ui.set_genius_songs(ModelRc::default());
        ui.set_genius_preview_open(true);
    }

    pub(super) fn genius_command(&mut self, ui: &MobileWindow, command: &str) {
        if !ui.get_genius_preview_open() {
            return;
        }
        match command {
            "close" => {
                self.genius.close();
                ui.set_genius_preview_open(false);
                ui.set_genius_busy(false);
                ui.set_genius_songs(ModelRc::default());
                ui.set_genius_message("".into());
            }
            "create" | "refresh" => {
                if self.playlist_jobs.saving_genius.is_some() {
                    return;
                }
                let current = self.current().cloned();
                match self.genius.start(
                    self.storage.support_dir(),
                    &self.tracks,
                    current.as_ref(),
                    command == "refresh",
                ) {
                    Ok(true) => {
                        ui.set_genius_busy(true);
                        ui.set_genius_message(
                            i18n::translate(self.settings.language, "Generating…").into(),
                        );
                    }
                    Ok(false) => {}
                    Err(message) => ui.set_genius_message(
                        i18n::translate(self.settings.language, message).into(),
                    ),
                }
            }
            "play" => {
                let ids = self.genius.playable_indices(&self.tracks);
                if ids.is_empty() || ids.len() != self.genius.entries().len() {
                    ui.set_genius_message(
                        i18n::translate(
                            self.settings.language,
                            "Library or seed changed; result discarded",
                        )
                        .into(),
                    );
                    return;
                }
                let first = ids[0];
                // Preserve an already playing seed, including its position,
                // while replacing only the queue with the validated preview.
                if self.playback.current == Some(first)
                    && matches!(
                        self.playback.status,
                        crate::audio::PlaybackStatus::Playing
                            | crate::audio::PlaybackStatus::Paused
                            | crate::audio::PlaybackStatus::Loading
                    )
                {
                    self.canonical_paths =
                        ids.iter().map(|&id| self.tracks[id].path.clone()).collect();
                    self.playback.shuffle = false;
                    self.playback.queue.set(ids, first, false);
                    self.set_playing(ui, true);
                    self.navigation.open(Page::NowPlaying);
                } else {
                    self.playback.shuffle = false;
                    self.select(ui, first, ids, false);
                }
                ui.set_genius_stage(2);
                ui.set_genius_message(
                    i18n::translate(self.settings.language, "Playing Genius playlist").into(),
                );
            }
            "save" => {
                if self.genius.is_busy() || self.playlist_jobs.busy() || self.playlists.is_saving()
                {
                    return;
                }
                let snapshot = self.genius.playlist_snapshot(&self.tracks);
                let (name, paths) = match snapshot {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        ui.set_genius_message(
                            i18n::translate(self.settings.language, &error).into(),
                        );
                        return;
                    }
                };
                let database = self.storage.database_path().to_owned();
                let (sender, receiver) = mpsc::channel();
                self.playlist_jobs.saving_genius = Some(receiver);
                ui.set_genius_busy(true);
                std::thread::spawn(move || {
                    let result = save_preview(&database, &name, &paths);
                    let _ = sender.send(result);
                });
            }
            _ => {}
        }
        self.rebuild_rows(ui);
        self.present(ui);
    }

    pub(super) fn poll_genius(&mut self, ui: &MobileWindow) {
        if let Some(result) = self.genius.poll(&self.tracks) {
            let message = match result {
                Ok(sparse) => {
                    let rows: Vec<_> = self
                        .genius
                        .entries()
                        .iter()
                        .map(|entry| GeniusRow {
                            title: entry.title.as_str().into(),
                            artist: entry.artist.as_str().into(),
                            path: entry.path.to_string_lossy().as_ref().into(),
                        })
                        .collect();
                    ui.set_genius_songs(ModelRc::new(VecModel::from(rows)));
                    ui.set_genius_stage(1);
                    if sparse {
                        "Short local playlist"
                    } else {
                        "Genius playlist ready"
                    }
                }
                Err(message) => message,
            };
            ui.set_genius_message(i18n::translate(self.settings.language, message).into());
        }
        if let Some(result) = self
            .playlist_jobs
            .saving_genius
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.playlist_jobs.saving_genius = None;
            match result {
                Ok(playlist) => {
                    self.saved_playlists.push(playlist);
                    ui.set_genius_stage(3);
                    ui.set_genius_message(
                        i18n::translate(self.settings.language, "Genius playlist saved").into(),
                    );
                    self.notice = "Playlist saved".into();
                    self.rebuild_rows(ui);
                }
                Err(error) => {
                    ui.set_genius_message(
                        i18n::translate(
                            self.settings.language,
                            "Could not save Genius playlist; retry",
                        )
                        .into(),
                    );
                    self.notice = format!("Could not save playlist: {error}");
                }
            }
        }
        ui.set_genius_busy(self.genius.is_busy() || self.playlist_jobs.saving_genius.is_some());
    }
}

fn save_preview(
    database: &Path,
    name: &str,
    paths: &[std::path::PathBuf],
) -> Result<Playlist, String> {
    if paths.is_empty() || paths.iter().any(|path| !path.is_file()) {
        return Err("Some selected songs are unavailable".into());
    }
    let existing = playlists::load(database)?;
    playlists::create(database, &available_name(name, &existing), paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Track;
    use crate::mobile::genius_service::{GeniusService, PreviewEntry};

    fn tracks(directory: &Path) -> Vec<Track> {
        (0..3)
            .map(|index| {
                let path = directory.join(format!("{index}.wav"));
                std::fs::write(&path, include_bytes!("../../tests/fixtures/quiet.wav")).unwrap();
                Track {
                    path,
                    title: format!("Song {index}"),
                    artist: "Artist".into(),
                    album: "Album".into(),
                    genre: "Genre".into(),
                    number: index,
                    duration: 1.0,
                    artwork: Vec::new(),
                    lyrics: Default::default(),
                }
            })
            .collect()
    }

    #[test]
    fn frozen_preview_saves_real_local_tracks_in_order_and_resolves_name_collisions() {
        let directory = tempfile::tempdir().unwrap();
        let tracks = tracks(directory.path());
        let database = directory.path().join("library.sqlite3");
        let mut service = GeniusService::default();
        service.set_preview(
            tracks[0].path.clone(),
            [0, 2, 1]
                .into_iter()
                .map(|id| PreviewEntry::from(&tracks[id]))
                .collect(),
        );
        let (name, paths) = service.playlist_snapshot(&tracks).unwrap();
        let first = save_preview(&database, &name, &paths).unwrap();
        let second = save_preview(&database, &name, &paths).unwrap();
        assert_ne!(first.id, second.id);
        assert_ne!(first.name, second.name);
        assert_eq!(first.tracks, paths);
        assert_eq!(playlists::load(&database).unwrap(), [first, second]);
        std::fs::remove_file(&tracks[2].path).unwrap();
        assert!(service.playlist_snapshot(&tracks).is_err());
        assert!(save_preview(&database, &name, &paths).is_err());
        assert_eq!(playlists::load(&database).unwrap().len(), 2);
    }

    #[test]
    fn invalid_provider_configuration_reports_failure_without_any_provider_call() {
        let directory = tempfile::tempdir().unwrap();
        // Invalid local JSON fails before consulting environment credentials or
        // constructing a provider, keeping this test unconditionally offline.
        std::fs::write(directory.path().join("deepseek.json"), b"not valid JSON").unwrap();
        let tracks = tracks(directory.path());
        let mut service = GeniusService::default();
        assert!(
            service
                .start(directory.path(), &tracks, Some(&tracks[0]), false)
                .unwrap()
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        loop {
            if let Some(result) = service.poll(&tracks) {
                assert!(result.is_err());
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(service.entries().is_empty());
    }
}
