//! Touch-facing adapter for the desktop playlist draft and persistence service.

use super::{
    Controller, MobileWindow,
    navigation::{Filter, Group, Page},
};
use crate::{
    library::playlists::{self, Playlist},
    shared::i18n,
    ui::PlaylistCandidate,
};
use slint::{ModelRc, VecModel};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
};

#[derive(Default)]
pub(super) struct PlaylistJobs {
    deleting: Option<Receiver<Result<i64, String>>>,
    pub(super) saving_genius: Option<Receiver<Result<Playlist, String>>>,
    deleted: HashSet<i64>,
}

impl PlaylistJobs {
    pub(super) fn busy(&self) -> bool {
        self.deleting.is_some() || self.saving_genius.is_some()
    }
}

impl Controller {
    pub(super) fn open_playlist_editor(&mut self, ui: &MobileWindow, id: Option<i64>) {
        crate::shared::diagnostics::record(
            "playlist_editor_open",
            serde_json::json!({"editing": id.is_some(), "session": self.playback.session}),
        );
        if self.playlists.is_saving()
            || self.playlist_jobs.busy()
            || ui.get_playlist_editor_open()
            || ui.get_genius_preview_open()
        {
            return;
        }
        if let Some(id) = id {
            let Some(playlist) = self
                .saved_playlists
                .iter()
                .find(|playlist| playlist.id == id)
            else {
                self.notice = "Playlist no longer exists".into();
                return;
            };
            self.playlists.begin_edit(playlist);
            ui.set_playlist_name(playlist.name.as_str().into());
            ui.set_playlist_editing(true);
            ui.set_playlist_message("Changes not saved".into());
        } else {
            self.playlists.begin_new();
            ui.set_playlist_name("".into());
            ui.set_playlist_editing(false);
            ui.set_playlist_message("Draft only · not saved".into());
        }
        ui.set_playlist_persisted(false);
        ui.set_playlist_saving(false);
        self.refresh_playlist_candidates(ui);
        if id.is_some() {
            ui.invoke_open_saved_playlist_editor();
        } else {
            ui.invoke_open_playlist_editor();
        }
    }

    pub(super) fn refresh_playlist_candidates(&mut self, ui: &MobileWindow) {
        self.playlists.refresh_candidates(&self.tracks);
        ui.set_playlist_library_loading(self.tracks.is_empty() && self.job.is_some());
        self.sync_playlist_draft(ui);
    }

    fn sync_playlist_draft(&self, ui: &MobileWindow) {
        let selected: HashSet<_> = self.playlists.paths().iter().collect();
        let candidates: Vec<_> = self
            .playlists
            .candidates()
            .iter()
            .map(|song| PlaylistCandidate {
                id: song.path.to_string_lossy().as_ref().into(),
                title: song.title.as_str().into(),
                artist: song.artist.as_str().into(),
                selected: selected.contains(&song.path),
            })
            .collect();
        let by_path: HashMap<_, _> = candidates
            .iter()
            .map(|song| (PathBuf::from(song.id.as_str()), song))
            .collect();
        let draft: Vec<_> = self
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
                        artist: i18n::translate(self.settings.language, "Unavailable song").into(),
                        selected: true,
                    })
            })
            .collect();
        ui.set_playlist_candidates(ModelRc::new(VecModel::from(candidates)));
        ui.set_playlist_draft(ModelRc::new(VecModel::from(draft)));
    }

    pub(super) fn playlist_command(&mut self, ui: &MobileWindow, command: &str, id: &str) {
        crate::shared::diagnostics::record(
            "playlist_action",
            serde_json::json!({"command": command.chars().take(40).collect::<String>(), "session": self.playback.session}),
        );
        match command {
            "new" => {
                self.open_playlist_editor(ui, None);
                return;
            }
            "edit" => {
                let current = match self.navigation.current() {
                    Page::Songs(Filter::Playlist { id, .. }) => Some(*id),
                    _ => None,
                };
                self.open_playlist_editor(ui, id.parse().ok().or(current));
                return;
            }
            "toggle-list-editing" => {
                self.navigation.toggle_playlist_editing();
                self.rebuild_rows(ui);
                self.present_playlists(ui);
                return;
            }
            "delete-request" => {
                if self.playlists.is_saving() || self.playlist_jobs.busy() {
                    return;
                }
                let Some(playlist) = id.parse::<i64>().ok().and_then(|id| {
                    self.saved_playlists
                        .iter()
                        .find(|playlist| playlist.id == id)
                }) else {
                    return;
                };
                ui.set_playlist_delete_name(playlist.name.as_str().into());
                ui.set_playlist_delete_id(playlist.id.to_string().into());
                ui.set_playlist_delete_open(true);
                return;
            }
            "delete-cancel" => {
                ui.set_playlist_delete_open(false);
                return;
            }
            "delete-confirm" => {
                if !ui.get_playlist_delete_open()
                    || ui.get_playlist_delete_id().as_str() != id
                    || self.playlist_jobs.busy()
                    || self.playlists.is_saving()
                {
                    return;
                }
                let Ok(id) = id.parse::<i64>() else {
                    return;
                };
                ui.set_playlist_delete_open(false);
                let database = self.storage.database_path().to_owned();
                let (sender, receiver) = mpsc::channel();
                self.playlist_jobs.deleting = Some(receiver);
                std::thread::spawn(move || {
                    let result = playlists::delete(&database, id).map(|()| id);
                    let _ = sender.send(result);
                });
                return;
            }
            _ => {}
        }
        if !ui.get_playlist_editor_open() || self.playlists.is_saving() {
            return;
        }
        let path = Path::new(id);
        if let Some(index) = command
            .strip_prefix("move:")
            .and_then(|index| index.parse().ok())
        {
            self.playlists.move_to(path, index);
        } else {
            match command {
                "toggle" => {
                    self.playlists.toggle(path);
                }
                "up" | "down" => self.playlists.nudge(path, command == "down"),
                "remove" => self.playlists.remove(path),
                "cancel" => {
                    self.playlists.cancel();
                    ui.set_playlist_editor_open(false);
                    return;
                }
                "review" => {
                    if ui.get_playlist_name().trim().is_empty() && !self.playlists.is_editing() {
                        let base = i18n::translate(self.settings.language, "New Playlist");
                        ui.set_playlist_name(available_name(&base, &self.saved_playlists).into());
                    }
                }
                "create" => {
                    if self.playlists.save(
                        self.storage.database_path(),
                        ui.get_playlist_name().as_str(),
                        &self.tracks,
                    ) {
                        ui.set_playlist_saving(true);
                        ui.set_playlist_message("Saving playlist…".into());
                    }
                    return;
                }
                _ => return,
            }
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

    pub(super) fn poll_playlists(&mut self, ui: &MobileWindow) {
        let mut changed = false;
        if let Some(result) = self.playlists.poll_load() {
            match result {
                Ok(playlists) => {
                    for playlist in playlists {
                        if !self.playlist_jobs.deleted.contains(&playlist.id)
                            && !self
                                .saved_playlists
                                .iter()
                                .any(|saved| saved.id == playlist.id)
                        {
                            self.saved_playlists.push(playlist);
                        }
                    }
                    changed = true;
                }
                Err(error) => self.notice = format!("Could not load playlists: {error}"),
            }
        }
        if ui.get_playlist_editor_open() {
            ui.set_playlist_library_loading(self.tracks.is_empty() && self.job.is_some());
            if self.playlists.candidates().is_empty() && !self.tracks.is_empty() {
                self.refresh_playlist_candidates(ui);
            }
        }
        if let Some(result) = self.playlists.poll_save() {
            ui.set_playlist_saving(false);
            match result {
                Ok(saved) => {
                    let playlist = saved.playlist;
                    self.navigation.rename_playlist(playlist.id, &playlist.name);
                    let page = Page::Songs(Filter::Playlist {
                        id: playlist.id,
                        name: playlist.name.clone(),
                    });
                    if let Some(existing) = self
                        .saved_playlists
                        .iter_mut()
                        .find(|saved| saved.id == playlist.id)
                    {
                        *existing = playlist;
                    } else {
                        self.saved_playlists.push(playlist);
                    }
                    ui.set_playlist_persisted(true);
                    ui.set_playlist_editor_open(false);
                    if !saved.editing {
                        self.navigation.open(Page::Groups(Group::Playlists));
                        self.navigation.open(page);
                    }
                    self.notice = "Playlist saved".into();
                    changed = true;
                }
                Err(error) => {
                    ui.set_playlist_message(playlist_error(&error).into());
                    self.notice = playlist_error(&error).into();
                }
            }
        }
        if let Some(result) = self
            .playlist_jobs
            .deleting
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.playlist_jobs.deleting = None;
            match result {
                Ok(id) => {
                    self.playlist_jobs.deleted.insert(id);
                    self.saved_playlists.retain(|playlist| playlist.id != id);
                    if matches!(self.navigation.current(), Page::Songs(Filter::Playlist { id: current, .. }) if *current == id)
                    {
                        self.navigation.back();
                    }
                    self.notice = "Playlist deleted".into();
                    changed = true;
                }
                Err(error) => self.notice = format!("Could not delete playlist: {error}"),
            }
        }
        if changed {
            self.rebuild_rows(ui);
        }
        self.present_playlists(ui);
    }

    pub(super) fn present_playlists(&self, ui: &MobileWindow) {
        ui.set_playlist_list(matches!(
            self.navigation.current(),
            Page::Groups(Group::Playlists)
        ));
        ui.set_playlist_content(matches!(
            self.navigation.current(),
            Page::Songs(Filter::Playlist { .. })
        ));
        ui.set_playlist_list_editing(self.navigation.playlist_editing());
    }
}

pub(super) fn available_name(base: &str, playlists: &[Playlist]) -> String {
    let base: String = base
        .chars()
        .filter(|character| !character.is_control())
        .take(100)
        .collect();
    let base = if base.trim().is_empty() {
        "New Playlist"
    } else {
        base.trim()
    };
    let mut number = 1;
    loop {
        let suffix = if number == 1 {
            String::new()
        } else {
            format!(" {number}")
        };
        let prefix: String = base
            .chars()
            .take(100usize.saturating_sub(suffix.chars().count()))
            .collect();
        let name = format!("{prefix}{suffix}");
        if !playlists
            .iter()
            .any(|playlist| playlist.name.to_lowercase() == name.to_lowercase())
        {
            return name;
        }
        number += 1;
    }
}

fn playlist_error(error: &str) -> &str {
    match error {
        "playlist name cannot be empty" => "Choose a playlist name",
        "playlist name cannot exceed 100 characters" => {
            "Playlist name must be 100 characters or fewer"
        }
        "playlist name contains control characters" => "Use a single-line playlist name",
        "playlist must contain at least one track" => "Choose at least one song",
        "a playlist with that name already exists" => "A playlist with this name already exists",
        "selected songs are unavailable" => "Some selected songs are unavailable",
        "playlist no longer exists" => "Playlist no longer exists",
        _ => "Could not save playlist. Try again.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        library::Track,
        mobile::playlist_service::{PlaylistsService, SavedPlaylist},
    };

    fn saved(service: &mut PlaylistsService) -> Result<SavedPlaylist, String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        loop {
            if let Some(result) = service.poll_save() {
                return result;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
    }

    #[test]
    fn desktop_draft_supports_mobile_reorder_rename_empty_retry_and_real_deletion() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("library.sqlite3");
        let tracks: Vec<_> = (0..3)
            .map(|id| {
                let path = directory.path().join(format!("{id}.wav"));
                std::fs::write(&path, include_bytes!("../../tests/fixtures/quiet.wav")).unwrap();
                Track {
                    path,
                    title: format!("Song {id}"),
                    artist: "Artist".into(),
                    album: "Album".into(),
                    genre: "Genre".into(),
                    number: id,
                    duration: 1.0,
                    artwork: Vec::new(),
                    lyrics: Default::default(),
                }
            })
            .collect();
        let (_sender, receiver) = mpsc::channel();
        let mut service = PlaylistsService::new(receiver);
        service.begin_new();
        service.refresh_candidates(&tracks);
        for track in &tracks {
            service.toggle(&track.path);
        }
        service.move_to(&tracks[2].path, 0);
        assert!(service.save(&database, "Trip", &tracks));
        let original = saved(&mut service).unwrap().playlist;
        assert_eq!(
            original.tracks,
            [
                tracks[2].path.clone(),
                tracks[0].path.clone(),
                tracks[1].path.clone()
            ]
        );
        playlists::create(&database, "Taken", &[tracks[0].path.clone()]).unwrap();
        service.begin_edit(&original);
        service.refresh_candidates(&tracks);
        service.nudge(&tracks[0].path, true);
        let reordered = service.paths().to_vec();
        assert!(service.save(&database, "Taken", &tracks));
        assert!(saved(&mut service).is_err());
        assert_eq!(
            service.paths(),
            reordered,
            "a failed save retains the recoverable draft"
        );
        assert!(service.save(&database, "Renamed", &tracks));
        let renamed = saved(&mut service).unwrap().playlist;
        assert_eq!(renamed.id, original.id);
        assert_eq!(renamed.tracks, reordered);
        assert_eq!(playlists::load(&database).unwrap()[0], renamed);
        service.begin_edit(&renamed);
        for path in renamed.tracks {
            service.remove(&path);
        }
        assert!(service.save(&database, "Empty", &tracks));
        let empty = saved(&mut service).unwrap().playlist;
        assert!(empty.tracks.is_empty());
        assert_eq!(playlists::load(&database).unwrap()[0], empty);
        playlists::delete(&database, empty.id).unwrap();
        assert!(
            playlists::load(&database)
                .unwrap()
                .iter()
                .all(|playlist| playlist.id != empty.id)
        );
        assert!(
            tracks.iter().all(|track| track.path.is_file()),
            "deleting a playlist never deletes music"
        );
    }
}
