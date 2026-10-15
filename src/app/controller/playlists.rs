use crate::library::{
    Track,
    playlists::{self, Playlist},
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc::Receiver,
    thread::JoinHandle,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Candidate {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
}

pub(super) struct SavedPlaylist {
    pub playlist: Playlist,
    pub editing: bool,
}

/// Owns a recoverable editing draft and its background persistence work.
pub(super) struct PlaylistsService {
    loading: Option<Receiver<Result<Vec<Playlist>, String>>>,
    saving: Option<Receiver<Result<Playlist, String>>>,
    writer: Option<JoinHandle<()>>,
    suspended: bool,
    edit_id: Option<i64>,
    original_paths: HashSet<PathBuf>,
    candidates: Vec<Candidate>,
    paths: Vec<PathBuf>,
}

impl PlaylistsService {
    /// The caller starts loading before scanning, preserving schema migration order.
    pub(super) fn new(loading: Receiver<Result<Vec<Playlist>, String>>) -> Self {
        Self {
            loading: Some(loading),
            saving: None,
            writer: None,
            suspended: false,
            edit_id: None,
            original_paths: HashSet::new(),
            candidates: Vec::new(),
            paths: Vec::new(),
        }
    }

    pub(super) fn refresh_candidates<'a>(&mut self, tracks: impl IntoIterator<Item = &'a Track>) {
        let mut songs: Vec<_> = tracks.into_iter().collect();
        songs.sort_by_cached_key(|song| {
            (
                song.title.to_lowercase(),
                song.title.clone(),
                song.path.clone(),
            )
        });
        self.candidates = songs
            .into_iter()
            .map(|song| Candidate {
                path: song.path.clone(),
                title: song.title.clone(),
                artist: song.artist.clone(),
            })
            .collect();
    }

    pub(super) fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    pub(super) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub(super) fn is_editing(&self) -> bool {
        self.edit_id.is_some()
    }

    pub(super) fn is_saving(&self) -> bool {
        self.saving.is_some()
    }

    pub(super) fn begin_new(&mut self) {
        if self.is_saving() {
            return;
        }
        self.edit_id = None;
        self.original_paths.clear();
        self.paths.clear();
    }

    pub(super) fn begin_edit(&mut self, playlist: &Playlist) {
        if self.is_saving() {
            return;
        }
        self.edit_id = Some(playlist.id);
        self.paths = playlist.tracks.clone();
        self.original_paths = self.paths.iter().cloned().collect();
    }

    pub(super) fn suspend(&mut self) {
        self.suspended = true;
    }

    pub(super) fn resume(&mut self) -> bool {
        std::mem::take(&mut self.suspended)
    }

    pub(super) fn move_to(&mut self, path: &Path, target: usize) -> bool {
        if self.is_saving() {
            return false;
        }
        let Some(source) = self.paths.iter().position(|p| p == path) else {
            return false;
        };
        let target = target.min(self.paths.len() - 1);
        let path = self.paths.remove(source);
        self.paths.insert(target, path);
        true
    }

    pub(super) fn toggle(&mut self, path: &Path) -> bool {
        // Commands carry the UI's lossy path string, including for non-Unicode paths.
        if self.is_saving()
            || !self
                .candidates
                .iter()
                .any(|song| song.path.to_string_lossy() == path.to_string_lossy())
        {
            return false;
        }
        if let Some(index) = self.paths.iter().position(|p| p == path) {
            self.paths.remove(index);
        } else {
            self.paths.push(path.to_owned());
        }
        true
    }

    pub(super) fn nudge(&mut self, path: &Path, down: bool) {
        if self.is_saving() {
            return;
        }
        if let Some(index) = self.paths.iter().position(|p| p == path) {
            let next = if down {
                (index + 1).min(self.paths.len() - 1)
            } else {
                index.saturating_sub(1)
            };
            self.paths.swap(index, next);
        }
    }

    pub(super) fn remove(&mut self, path: &Path) {
        if !self.is_saving() {
            self.paths.retain(|p| p != path);
        }
    }

    pub(super) fn cancel(&mut self) {
        if !self.is_saving() {
            self.clear_draft();
        }
    }

    fn clear_draft(&mut self) {
        self.edit_id = None;
        self.original_paths.clear();
        self.suspended = false;
        self.paths.clear();
        self.candidates.clear();
    }

    /// Capture user intent before spawning; the worker never borrows live state.
    pub(super) fn save(&mut self, database: &Path, name: &str, tracks: &[Track]) -> bool {
        if self.is_saving() {
            return false;
        }
        let name = name.trim().to_owned();
        let paths = self.paths.clone();
        let edit_id = self.edit_id;
        let original = self.original_paths.clone();
        let available: HashSet<_> = tracks.iter().map(|song| song.path.clone()).collect();
        let database = database.to_owned();
        let (sender, receiver) = std::sync::mpsc::channel();
        self.writer = Some(std::thread::spawn(move || {
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
        self.saving = Some(receiver);
        true
    }

    pub(super) fn poll_load(&mut self) -> Option<Result<Vec<Playlist>, String>> {
        let result = self.loading.as_ref()?.try_recv().ok()?;
        self.loading = None;
        Some(result)
    }

    pub(super) fn poll_save(&mut self) -> Option<Result<SavedPlaylist, String>> {
        let result = self.saving.as_ref()?.try_recv().ok()?;
        self.saving = None;
        self.shutdown();
        Some(result.map(|playlist| {
            let editing = self.is_editing();
            self.clear_draft();
            SavedPlaylist { playlist, editing }
        }))
    }

    /// Controller invokes this after draining statistics during application exit.
    pub(super) fn shutdown(&mut self) {
        if let Some(worker) = self.writer.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for PlaylistsService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn service() -> PlaylistsService {
        let (_sender, receiver) = std::sync::mpsc::channel();
        PlaylistsService::new(receiver)
    }

    fn tracks(directory: &Path) -> Vec<Track> {
        ["Zulu", "alpha", "Alpha"]
            .into_iter()
            .enumerate()
            .map(|(index, title)| {
                let path = directory.join(format!("{index}.wav"));
                std::fs::write(&path, b"offline fixture").unwrap();
                Track {
                    path,
                    title: title.into(),
                    artist: "Artist".into(),
                    album: "Album".into(),
                    genre: "Genre".into(),
                    number: index as u32,
                    duration: 60.0,
                    artwork: Vec::new(),
                    lyrics: Default::default(),
                }
            })
            .collect()
    }

    fn saved(service: &mut PlaylistsService) -> Result<SavedPlaylist, String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = service.poll_save() {
                return result;
            }
            assert!(Instant::now() < deadline, "playlist fixture timed out");
            std::thread::yield_now();
        }
    }

    #[test]
    fn startup_load_reports_once_and_draft_order_survives_suspend_resume() {
        let directory = tempfile::tempdir().unwrap();
        let tracks = tracks(directory.path());
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut service = PlaylistsService::new(receiver);
        let playlist = Playlist {
            id: 17,
            name: "Original".into(),
            tracks: vec![tracks[0].path.clone(), tracks[1].path.clone()],
        };
        assert!(service.poll_load().is_none());
        sender.send(Ok(vec![playlist.clone()])).unwrap();
        let loaded = service.poll_load().unwrap().unwrap();
        assert_eq!(loaded.as_slice(), std::slice::from_ref(&playlist));
        assert!(service.poll_load().is_none());
        service.begin_edit(&playlist);
        service.refresh_candidates(&tracks);
        assert_eq!(
            service
                .candidates()
                .iter()
                .map(|song| song.title.as_str())
                .collect::<Vec<_>>(),
            ["Alpha", "alpha", "Zulu"]
        );
        assert!(service.toggle(&tracks[2].path));
        assert!(service.move_to(&tracks[2].path, 0));
        service.nudge(&tracks[0].path, true);
        service.suspend();
        assert!(service.resume());
        assert!(!service.resume());
        assert!(service.is_editing());
        assert_eq!(
            service.paths(),
            [
                tracks[2].path.clone(),
                tracks[1].path.clone(),
                tracks[0].path.clone()
            ]
        );
        service.cancel();
        assert!(service.paths().is_empty());
        assert!(!service.is_editing());
    }

    #[test]
    fn failed_save_preserves_draft_and_success_retains_id_missing_originals_and_order() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("library.sqlite3");
        let tracks = tracks(directory.path());
        let missing = directory.path().join("missing.wav");
        let original = playlists::create(
            &database,
            "Original",
            &[missing.clone(), tracks[0].path.clone()],
        )
        .unwrap();
        playlists::create(&database, "Taken", &[tracks[1].path.clone()]).unwrap();
        let mut service = service();
        service.begin_edit(&original);
        service.refresh_candidates(&tracks);
        service.toggle(&tracks[2].path);
        service.move_to(&tracks[2].path, 0);
        let expected = service.paths().to_vec();
        assert!(service.save(&database, "Taken", &tracks));
        service.cancel();
        service.remove(&missing);
        assert!(!service.toggle(&tracks[1].path));
        assert_eq!(service.paths(), expected);
        assert_eq!(
            saved(&mut service).err().unwrap(),
            "a playlist with that name already exists"
        );
        assert_eq!(service.paths(), expected);
        assert!(service.is_editing());
        assert_eq!(playlists::load(&database).unwrap()[0], original);
        service.suspend();
        assert!(service.save(&database, "Renamed", &tracks));
        let saved = saved(&mut service).unwrap();
        assert!(saved.editing);
        assert_eq!(saved.playlist.id, original.id);
        assert_eq!(saved.playlist.tracks, expected);
        assert!(service.paths().is_empty());
        assert!(service.candidates().is_empty());
        assert!(!service.is_editing());
        assert!(!service.resume());
    }

    #[test]
    fn unavailable_addition_is_rejected_and_drop_finishes_explicit_save() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("library.sqlite3");
        let tracks = tracks(directory.path());
        let mut service = service();
        service.begin_new();
        service.refresh_candidates(&tracks);
        service.toggle(&tracks[0].path);
        std::fs::remove_file(&tracks[0].path).unwrap();
        service.save(&database, "Unavailable", &tracks);
        assert_eq!(
            saved(&mut service).err().unwrap(),
            "selected songs are unavailable"
        );
        assert_eq!(service.paths(), [tracks[0].path.clone()]);
        service.remove(&tracks[0].path);
        service.toggle(&tracks[1].path);
        assert!(service.save(&database, "Survives shutdown", &tracks));
        drop(service);
        let saved = playlists::load(&database).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].name, "Survives shutdown");
        assert_eq!(saved[0].tracks, [tracks[1].path.clone()]);
    }

    #[cfg(unix)]
    #[test]
    fn candidate_toggle_preserves_lossy_ui_path_identity() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let directory = tempfile::tempdir().unwrap();
        let mut tracks = tracks(directory.path());
        tracks[0].path = directory
            .path()
            .join(OsString::from_vec(b"song-\xff.wav".to_vec()));
        tracks[1].path = directory
            .path()
            .join(OsString::from_vec(b"song-\xfe.wav".to_vec()));
        let mut service = service();
        service.refresh_candidates(&tracks);
        let displayed_path = PathBuf::from(tracks[0].path.to_string_lossy().as_ref());
        assert_eq!(
            displayed_path,
            PathBuf::from(tracks[1].path.to_string_lossy().as_ref())
        );
        assert!(service.toggle(&displayed_path));
        assert_eq!(service.paths(), std::slice::from_ref(&displayed_path));
        assert!(service.toggle(&displayed_path));
        assert!(service.paths().is_empty());
    }
}
