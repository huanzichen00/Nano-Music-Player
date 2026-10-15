use crate::{
    genius::{Cancellation, Candidate, Error, Request, RequestGate, RequestPermit, ResultSet},
    library::Track,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc::Receiver,
};

// A cancelled blocking provider still owns its permit until it actually exits.
static WORKERS: RequestGate = RequestGate::new(1);

type Completion = (Request, Result<ResultSet, Error>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PreviewEntry {
    pub title: String,
    pub artist: String,
    pub path: PathBuf,
}

/// Owns recommendation requests and their frozen preview, independently of the UI.
#[derive(Default)]
pub(super) struct GeniusService {
    seed: Option<PathBuf>,
    request: Option<Receiver<Completion>>,
    entries: Vec<PreviewEntry>,
    variation: u64,
    cancellation: Option<Cancellation>,
}

impl GeniusService {
    pub(super) fn is_busy(&self) -> bool {
        self.request.is_some()
    }

    pub(super) fn set_seed(&mut self, seed: PathBuf) {
        self.seed = Some(seed);
        self.entries.clear();
    }

    pub(super) fn prepare_seed(
        &mut self,
        current: Option<&Track>,
        refresh: bool,
    ) -> Option<PathBuf> {
        if self.seed.is_none() {
            self.seed = current.map(|track| track.path.clone());
        }
        if refresh {
            self.variation = self.variation.wrapping_add(1);
        } else {
            self.variation = 0;
            self.entries.clear();
        }
        self.seed.clone()
    }

    /// Returns false for an already active request without changing its snapshot.
    pub(super) fn start(
        &mut self,
        directory: &Path,
        tracks: &[Track],
        current: Option<&Track>,
        refresh: bool,
    ) -> Result<bool, &'static str> {
        if self.is_busy() {
            return Ok(false);
        }
        let seed = self
            .prepare_seed(current, refresh)
            .ok_or("Choose a song first")?;
        let request = self.snapshot(seed, tracks);
        request
            .validate()
            .map_err(|_| "Not enough available local songs")?;
        let permit = WORKERS
            .try_acquire()
            .map_err(|_| "Genius failed; retry is available")?;
        let settings = crate::ai::DeepSeekSettings::load(&directory.join("deepseek.json"));
        self.spawn(request, permit, move |request, cancellation| {
            let provider = settings.map_err(|_| Error::Provider).and_then(|settings| {
                crate::ai::DeepSeek::new(&settings).map_err(|_| Error::Provider)
            })?;
            crate::genius::generate(request, &provider, cancellation)
        });
        Ok(true)
    }

    fn snapshot(&self, seed: PathBuf, tracks: &[Track]) -> Request {
        let seed_signature = tracks
            .iter()
            .find(|track| track.path == seed)
            .map(signature);
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        for (index, track) in tracks.iter().enumerate() {
            if !track.path.is_file() {
                continue;
            }
            let signature = signature(track);
            if track.path != seed
                && (Some(&signature) == seed_signature.as_ref() || !seen.insert(signature.clone()))
            {
                continue;
            }
            seen.insert(signature);
            candidates.push(Candidate {
                id: format!("candidate-{index}"),
                path: track.path.clone(),
                title: track.title.chars().take(160).collect(),
                artist: track.artist.chars().take(160).collect(),
                album: track.album.chars().take(160).collect(),
                genre: track.genre.chars().take(80).collect(),
                duration_seconds: track.duration.max(0.0),
            });
        }
        Request {
            request_id: self.variation + 1,
            seed_path: seed,
            library_revision: Self::library_revision_for(tracks),
            variation: self.variation,
            limit: 24,
            candidates,
        }
    }

    fn spawn(
        &mut self,
        request: Request,
        permit: RequestPermit<'static>,
        worker: impl FnOnce(&Request, &Cancellation) -> Result<ResultSet, Error> + Send + 'static,
    ) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let cancellation = Cancellation::default();
        let worker_cancellation = cancellation.clone();
        std::thread::spawn(move || {
            let _permit = permit;
            let result = worker(&request, &worker_cancellation);
            let _ = sender.send((request, result));
        });
        self.request = Some(receiver);
        self.cancellation = Some(cancellation);
    }

    pub(super) fn poll(&mut self, tracks: &[Track]) -> Option<Result<bool, &'static str>> {
        let (request, result) = self.request.as_ref()?.try_recv().ok()?;
        self.request = None;
        self.cancellation = None;
        Some(self.apply_result(&request, result, tracks))
    }

    pub(super) fn apply_result(
        &mut self,
        request: &Request,
        result: Result<ResultSet, Error>,
        tracks: &[Track],
    ) -> Result<bool, &'static str> {
        if !self.context_matches(request, tracks) {
            return Err("Library or seed changed; result discarded");
        }
        let result = result.map_err(|_| "Genius failed; retry is available")?;
        self.entries.clear();
        if let Some(seed) = tracks.iter().find(|track| track.path == request.seed_path) {
            self.entries.push(PreviewEntry::from(seed));
        }
        for path in result.ordered_paths {
            if let Some(track) = tracks
                .iter()
                .find(|track| track.path == path && track.path.is_file())
            {
                self.entries.push(PreviewEntry::from(track));
            }
        }
        Ok(result.sparse)
    }

    pub(super) fn context_matches(&self, request: &Request, tracks: &[Track]) -> bool {
        self.seed.as_deref() == Some(request.seed_path.as_path())
            && Self::library_revision_for(tracks) == request.library_revision
            && request.candidates.iter().all(|candidate| {
                tracks.iter().any(|track| {
                    track.path == candidate.path
                        && track.title.chars().take(160).collect::<String>() == candidate.title
                        && track.artist.chars().take(160).collect::<String>() == candidate.artist
                        && track.album.chars().take(160).collect::<String>() == candidate.album
                        && track.genre.chars().take(80).collect::<String>() == candidate.genre
                        && track.duration.max(0.0) == candidate.duration_seconds
                })
            })
    }

    pub(super) fn library_revision_for(tracks: &[Track]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for track in tracks {
            track.path.hash(&mut hasher);
            track.title.hash(&mut hasher);
            track.artist.hash(&mut hasher);
            track.album.hash(&mut hasher);
            track.genre.hash(&mut hasher);
            track.duration.to_bits().hash(&mut hasher);
            track.path.is_file().hash(&mut hasher);
        }
        hasher.finish()
    }

    pub(super) fn entries(&self) -> &[PreviewEntry] {
        &self.entries
    }

    pub(super) fn close(&mut self) {
        if let Some(cancellation) = self.cancellation.take() {
            cancellation.cancel();
        }
        self.request = None;
        self.seed = None;
        self.entries.clear();
        self.variation = 0;
    }

    pub(super) fn playable_indices(&self, tracks: &[Track]) -> Vec<usize> {
        // Preview actions historically resolve the lossy path strings shown by the UI.
        self.entries
            .iter()
            .filter_map(|entry| {
                tracks.iter().position(|track| {
                    track.path.to_string_lossy() == entry.path.to_string_lossy()
                        && track.path.is_file()
                })
            })
            .collect()
    }

    pub(super) fn playlist_snapshot(
        &self,
        tracks: &[Track],
    ) -> Result<(String, Vec<PathBuf>), String> {
        let paths: Vec<_> = self
            .playable_indices(tracks)
            .into_iter()
            .filter_map(|id| tracks.get(id).map(|track| track.path.clone()))
            .collect();
        if paths.is_empty() || paths.len() != self.entries.len() {
            return Err("Some selected songs are unavailable".into());
        }
        let seed = self
            .seed
            .as_ref()
            .and_then(|path| tracks.iter().find(|track| &track.path == path));
        let name = seed
            .map(|track| format!("{} - {}", track.artist, track.title))
            .unwrap_or_else(|| "Genius Mix".into());
        Ok((name, paths))
    }

    #[cfg(test)]
    pub(super) fn variation(&self) -> u64 {
        self.variation
    }

    #[cfg(test)]
    pub(super) fn set_preview(&mut self, seed: PathBuf, entries: Vec<PreviewEntry>) {
        self.seed = Some(seed);
        self.entries = entries;
    }
}

impl Drop for GeniusService {
    fn drop(&mut self) {
        self.close();
    }
}

impl From<&Track> for PreviewEntry {
    fn from(track: &Track) -> Self {
        Self {
            title: track.title.clone(),
            artist: track.artist.clone(),
            path: track.path.clone(),
        }
    }
}

fn signature(track: &Track) -> String {
    format!(
        "{}\0{}\0{}",
        track.title.trim().to_lowercase(),
        track.artist.trim().to_lowercase(),
        track.album.trim().to_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn tracks(directory: &Path) -> Vec<Track> {
        ["Seed", "First", "Second"]
            .into_iter()
            .map(|title| {
                let path = directory.join(format!("{title}.wav"));
                std::fs::write(&path, b"offline fixture").unwrap();
                Track {
                    path,
                    title: title.into(),
                    artist: "Artist".into(),
                    album: "Album".into(),
                    genre: "Genre".into(),
                    number: 1,
                    duration: 60.0,
                    artwork: Vec::new(),
                    lyrics: Default::default(),
                }
            })
            .collect()
    }

    fn poll(service: &mut GeniusService, tracks: &[Track]) -> Result<bool, &'static str> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = service.poll(tracks) {
                return result;
            }
            assert!(Instant::now() < deadline, "Genius fixture timed out");
            std::thread::yield_now();
        }
    }

    #[test]
    fn snapshot_deduplicates_metadata_and_preserves_seed_and_preview_order() {
        let directory = tempfile::tempdir().unwrap();
        let mut tracks = tracks(directory.path());
        let mut duplicate = tracks[1].clone();
        duplicate.path = directory.path().join("duplicate.wav");
        std::fs::write(&duplicate.path, b"duplicate fixture").unwrap();
        tracks.push(duplicate);
        let mut service = GeniusService::default();
        service.set_seed(tracks[0].path.clone());
        service.prepare_seed(Some(&tracks[1]), false);
        assert_eq!(
            service.prepare_seed(Some(&tracks[1]), true),
            Some(tracks[0].path.clone())
        );
        assert_eq!(service.variation(), 1);
        let request = service.snapshot(tracks[0].path.clone(), &tracks);
        assert_eq!(request.candidates.len(), 3);
        let result = request
            .validate_response(r#"{"ids":["candidate-2","candidate-1"]}"#)
            .unwrap();
        assert!(service.apply_result(&request, Ok(result), &tracks).unwrap());
        assert_eq!(service.playable_indices(&tracks), [0, 2, 1]);
        assert_eq!(
            service.playlist_snapshot(&tracks).unwrap().1,
            [
                tracks[0].path.clone(),
                tracks[2].path.clone(),
                tracks[1].path.clone()
            ]
        );
    }

    #[test]
    fn metadata_and_availability_changes_reject_results_without_replacing_preview() {
        let directory = tempfile::tempdir().unwrap();
        let mut tracks = tracks(directory.path());
        let mut service = GeniusService::default();
        service.set_preview(tracks[0].path.clone(), vec![PreviewEntry::from(&tracks[0])]);
        let request = service.snapshot(tracks[0].path.clone(), &tracks);
        let result = request
            .validate_response(r#"{"ids":["candidate-1"]}"#)
            .unwrap();
        let original = tracks.clone();
        for field in 0..5 {
            tracks = original.clone();
            match field {
                0 => tracks[1].title.push('!'),
                1 => tracks[1].artist.push('!'),
                2 => tracks[1].album.push('!'),
                3 => tracks[1].genre.push('!'),
                _ => tracks[1].duration += 1.0,
            }
            assert!(
                service
                    .apply_result(&request, Ok(result.clone()), &tracks)
                    .is_err()
            );
            assert_eq!(service.entries().len(), 1);
        }
        tracks = original;
        std::fs::remove_file(&tracks[1].path).unwrap();
        assert!(service.apply_result(&request, Ok(result), &tracks).is_err());
        assert_eq!(service.entries().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn playable_indices_preserve_lossy_ui_path_identity() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let directory = tempfile::tempdir().unwrap();
        let mut tracks = tracks(directory.path());
        let first_invalid = directory
            .path()
            .join(OsString::from_vec(b"song-\xff.wav".to_vec()));
        let second_invalid = directory
            .path()
            .join(OsString::from_vec(b"song-\xfe.wav".to_vec()));
        tracks[0].path = PathBuf::from(first_invalid.to_string_lossy().as_ref());
        std::fs::write(&tracks[0].path, b"offline fixture").unwrap();
        let mut service = GeniusService::default();
        service.set_preview(
            second_invalid.clone(),
            vec![PreviewEntry {
                title: tracks[0].title.clone(),
                artist: tracks[0].artist.clone(),
                path: second_invalid.clone(),
            }],
        );
        assert_ne!(first_invalid, second_invalid);
        assert_ne!(tracks[0].path, second_invalid);
        assert_eq!(
            first_invalid.to_string_lossy(),
            second_invalid.to_string_lossy()
        );
        assert_eq!(service.playable_indices(&tracks), [0]);
        assert_eq!(
            service.playlist_snapshot(&tracks).unwrap().1,
            [tracks[0].path.clone()]
        );
    }

    #[test]
    fn close_drops_old_receiver_while_blocked_worker_retains_its_gate_permit() {
        static GATE: RequestGate = RequestGate::new(1);
        let directory = tempfile::tempdir().unwrap();
        let tracks = tracks(directory.path());
        let mut service = GeniusService::default();
        service.set_seed(tracks[0].path.clone());
        let request = service.snapshot(tracks[0].path.clone(), &tracks);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (cancelled_tx, cancelled_rx) = std::sync::mpsc::channel();
        service.spawn(request, GATE.try_acquire().unwrap(), move |_, token| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            cancelled_tx.send(token.is_cancelled()).unwrap();
            Err(Error::Cancelled)
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        service.set_seed(tracks[1].path.clone());
        assert!(
            !service
                .start(directory.path(), &tracks, Some(&tracks[1]), true)
                .unwrap()
        );
        assert_eq!(service.variation(), 0);
        service.close();
        assert!(!service.is_busy());
        assert!(matches!(GATE.try_acquire(), Err(Error::Busy)));
        service.set_preview(tracks[1].path.clone(), vec![PreviewEntry::from(&tracks[1])]);
        release_tx.send(()).unwrap();
        assert!(cancelled_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        let permit = loop {
            if let Ok(permit) = GATE.try_acquire() {
                break permit;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert!(service.poll(&tracks).is_none());
        assert_eq!(service.entries(), [PreviewEntry::from(&tracks[1])]);
        let request = service.snapshot(tracks[1].path.clone(), &tracks);
        service.spawn(request, permit, |request, _| {
            request.validate_response(r#"{"ids":["candidate-2"]}"#)
        });
        assert!(poll(&mut service, &tracks).unwrap());
        assert_eq!(service.playable_indices(&tracks), [1, 2]);
    }
}
