use super::{Track, database::Database};
use lofty::{
    file::{AudioFile, TaggedFileExt},
    prelude::Accessor,
    probe::Probe,
};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread,
    time::UNIX_EPOCH,
};

pub struct ScanResult {
    pub tracks: Vec<Track>,
    pub warnings: Vec<String>,
    /// Selected folder roots, including empty folders, retained for monitoring.
    pub roots: Vec<PathBuf>,
}

pub enum ImportEvent {
    SelectionClosed,
    Completed(Result<Option<ScanResult>, String>),
}

/// Folder selection, filesystem traversal and metadata reads all happen off the UI thread.
pub fn import(database_path: PathBuf, title: String) -> Receiver<ImportEvent> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = (|| {
            let folders = rfd::FileDialog::new().set_title(&title).pick_folders();
            let _ = sender.send(ImportEvent::SelectionClosed);
            let Some(folders) = folders else {
                return Ok(None);
            };
            scan_folders(&database_path, &folders).map(Some)
        })();
        let _ = sender.send(ImportEvent::Completed(result));
    });
    receiver
}

/// Scan selected folders on a caller-owned background thread.
pub fn scan_folders(database_path: &Path, folders: &[PathBuf]) -> Result<ScanResult, String> {
    let database = Database::open(&database_path)?;
    let mut tracks = database.load()?;
    let mut warnings = Vec::new();
    let roots: Vec<_> = folders
        .iter()
        .map(|p| super::deletion_monitor::normalize(p))
        .collect();
    for folder in &roots {
        for entry in walkdir::WalkDir::new(folder).follow_links(false) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(error.to_string());
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_lowercase();
            if !matches!(
                extension.as_str(),
                "mp3" | "flac" | "wav" | "m4a" | "aac" | "ogg" | "aiff"
            ) {
                continue;
            }
            let modified = metadata_stamp(path);
            let mut track = if let Some(track) = database.cached(path, modified) {
                track
            } else {
                match read_track(path.to_path_buf()) {
                    Ok(track) => {
                        database.save(&track, modified)?;
                        track
                    }
                    Err(error) => {
                        warnings.push(format!("{}: {error}", path.display()));
                        continue;
                    }
                }
            };
            database.apply_lyrics(&mut track)?;
            if let Some(existing) = tracks.iter_mut().find(|t| t.path == track.path) {
                *existing = track;
            } else {
                tracks.push(track);
            }
        }
    }
    tracks.sort_by_cached_key(|track| track.title.to_lowercase());
    Ok(ScanResult {
        tracks,
        warnings,
        roots,
    })
}

#[derive(Debug)]
pub struct RootReconciliation {
    pub paths: Vec<PathBuf>,
    pub missing: Vec<PathBuf>,
}

/// Reconcile a root off the UI thread. Never infer deletions from an unavailable
/// root or an incomplete traversal (e.g. permissions or a disconnected volume).
pub fn reconcile_root(root: &Path, known: &[PathBuf]) -> Result<RootReconciliation, String> {
    if !root.is_dir() {
        return Ok(RootReconciliation {
            paths: Vec::new(),
            missing: Vec::new(),
        });
    }
    let mut paths = std::collections::BTreeSet::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.file_type().is_file() && super::deletion_monitor::is_audio(entry.path()) {
            paths.insert(super::deletion_monitor::normalize(entry.path()));
        }
    }
    if !root.is_dir() {
        return Ok(RootReconciliation {
            paths: Vec::new(),
            missing: Vec::new(),
        });
    }
    let mut missing = Vec::new();
    for path in known
        .iter()
        .filter(|path| path.starts_with(root) && !paths.contains(*path))
        .cloned()
        .collect::<Vec<_>>()
    {
        // A no-symlink traversal can omit an existing playable path. Only
        // confirmed absence is a removal; permission/device errors preserve it.
        match path.metadata() {
            Ok(metadata) if metadata.is_file() => {
                paths.insert(path);
            }
            Ok(_) => missing.push(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing.push(path),
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
    }
    if !root.is_dir() {
        return Ok(RootReconciliation {
            paths: Vec::new(),
            missing: Vec::new(),
        });
    }
    Ok(RootReconciliation {
        paths: paths.into_iter().collect(),
        missing,
    })
}

/// Re-read and persist a single restored file using the same cache stamp as imports.
/// Called on the deletion worker, never on the UI thread.
pub fn refresh_track(database: &Database, path: &Path) -> Result<Track, String> {
    let stamp = metadata_stamp(path);
    let mut track = if let Some(cached) = database.cached(path, stamp) {
        cached
    } else {
        read_track(path.to_path_buf())?
    };
    database.apply_lyrics(&mut track)?;
    database.save(&track, stamp)?;
    Ok(track)
}

fn metadata_stamp(path: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut stamp = std::collections::hash_map::DefaultHasher::new();
    for file in std::iter::once(path.to_path_buf()).chain(super::lyrics::sidecar_candidates(path)) {
        file.to_string_lossy().hash(&mut stamp);
        if let Ok(metadata) = file.metadata() {
            metadata.len().hash(&mut stamp);
            metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .hash(&mut stamp);
        }
    }
    stamp.finish() & i64::MAX as u64
}

pub fn load(database_path: PathBuf) -> Receiver<Result<Vec<Track>, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = Database::open(&database_path).and_then(|db| {
            let mut tracks = db.load()?;
            for track in &mut tracks {
                // Refresh automatic sidecars on startup. read_track falls back to the original
                // audio tags when a sidecar has been removed; manual overrides remain authoritative.
                let mut refreshed =
                    read_track(track.path.clone()).unwrap_or_else(|_| track.clone());
                db.apply_lyrics(&mut refreshed)?;
                *track = refreshed;
            }
            Ok(tracks)
        });
        let _ = sender.send(result);
    });
    receiver
}

/// The cached library is available before any source file is opened. Refreshes
/// carry their original value so callers can reject obsolete background work.
pub enum CachedLoadEvent {
    Cached(Vec<Track>),
    Refreshed {
        original: Box<Track>,
        track: Box<Track>,
    },
    Complete,
    Error(String),
}

pub fn load_cached(database_path: PathBuf) -> Receiver<CachedLoadEvent> {
    let (sender, receiver) = mpsc::sync_channel(2);
    thread::spawn(move || {
        cache_then_refresh(database_path, sender, read_track);
    });
    receiver
}

fn cache_then_refresh(
    database_path: PathBuf,
    sender: mpsc::SyncSender<CachedLoadEvent>,
    read: impl Fn(PathBuf) -> Result<Track, String>,
) {
    let result = (|| {
        let db = Database::open(&database_path)?;
        let mut tracks = db.load()?;
        for track in &mut tracks {
            // Manual snapshots and removal choices require only the database.
            db.apply_lyrics(track)?;
        }
        if sender
            .send(CachedLoadEvent::Cached(tracks.clone()))
            .is_err()
        {
            return Ok(());
        }
        for original in tracks {
            let mut track = read(original.path.clone()).unwrap_or_else(|_| original.clone());
            // Re-read overrides after the source: the user may have edited them
            // while that file was being opened.
            db.apply_lyrics(&mut track)?;
            if track != original
                && sender
                    .send(CachedLoadEvent::Refreshed {
                        original: Box::new(original),
                        track: Box::new(track),
                    })
                    .is_err()
            {
                return Ok(());
            }
        }
        let _ = sender.send(CachedLoadEvent::Complete);
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        let _ = sender.send(CachedLoadEvent::Error(error));
    }
}

pub fn read_track(path: PathBuf) -> Result<Track, String> {
    let file = Probe::open(&path)
        .and_then(|probe| probe.read())
        .map_err(|e| e.to_string())?;
    let tag = file.primary_tag().or_else(|| file.first_tag());
    let lyrics = super::lyrics::sidecar(&path)
        .or_else(|| super::lyrics::embedded_sylt(&path))
        .unwrap_or_else(|| {
            tag.and_then(|t| t.get_string(&lofty::tag::ItemKey::Lyrics))
                .map(super::lyrics::Lyrics::parse)
                .unwrap_or_default()
        });
    Ok(Track {
        title: tag
            .and_then(|t| t.title())
            .map(|v| v.into_owned())
            .unwrap_or_else(|| {
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            }),
        artist: tag
            .and_then(|t| t.artist())
            .map(|v| v.into_owned())
            .unwrap_or_else(|| "Unknown Artist".to_owned()),
        album: tag
            .and_then(|t| t.album())
            .map(|v| v.into_owned())
            .unwrap_or_else(|| "Unknown Album".to_owned()),
        genre: tag
            .and_then(|t| t.genre())
            .map(|v| v.into_owned())
            .unwrap_or_else(|| "Unknown Genre".to_owned()),
        number: tag.and_then(|t| t.track()).unwrap_or_default(),
        duration: file.properties().duration().as_secs_f64(),
        artwork: tag
            .and_then(|t| t.pictures().first())
            .map_or_else(Vec::new, |picture| picture.data().to_vec()),
        path,
        lyrics,
    })
}

#[derive(Debug, Clone, Copy)]
pub enum LyricsCommand {
    Import,
    Remove,
    Automatic,
}

pub struct LyricsUpdate {
    pub path: PathBuf,
    pub lyrics: super::lyrics::Lyrics,
}

/// Dialogs, parsing and persistent association changes are performed on a worker.
pub fn edit_lyrics(
    database_path: PathBuf,
    path: PathBuf,
    command: LyricsCommand,
    title: String,
) -> Receiver<Result<Option<LyricsUpdate>, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = (|| {
            let db = Database::open(&database_path)?;
            let lyrics = match command {
                LyricsCommand::Import => {
                    let Some(source) = rfd::FileDialog::new()
                        .set_title(&title)
                        .add_filter("Lyrics", &["lrc", "txt"])
                        .pick_file()
                    else {
                        return Ok(None);
                    };
                    let lyrics = super::lyrics::read_source(&source)?;
                    db.set_manual_lyrics(&path, &source, &lyrics)?;
                    lyrics
                }
                LyricsCommand::Remove => {
                    db.remove_lyrics(&path)?;
                    super::lyrics::Lyrics::default()
                }
                LyricsCommand::Automatic => {
                    let lyrics = read_track(path.clone())?.lyrics;
                    db.use_automatic_lyrics(&path)?;
                    lyrics
                }
            };
            Ok(Some(LyricsUpdate { path, lyrics }))
        })();
        let _ = sender.send(result);
    });
    receiver
}

#[cfg(test)]
mod cached_loading_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn cached_tracks_and_manual_lyrics_publish_before_a_blocked_source_refresh() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("library.sqlite3");
        let track = Track {
            path: directory.path().join("blocked.mp3"),
            title: "Cached title".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            genre: "Genre".into(),
            number: 1,
            duration: 60.0,
            artwork: vec![],
            lyrics: super::super::lyrics::Lyrics::plain_text("Cached automatic text"),
        };
        {
            let db = Database::open(&database).unwrap();
            db.save(&track, 1).unwrap();
            db.set_manual_lyrics(
                &track.path,
                &directory.path().join("manual.txt"),
                &super::super::lyrics::Lyrics::plain_text("Persisted manual text"),
            )
            .unwrap();
        }
        let (sender, events) = mpsc::sync_channel(2);
        let (release, blocked) = mpsc::sync_channel::<()>(0);
        let (started, reading) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            cache_then_refresh(database, sender, |path| {
                started.send(()).unwrap();
                blocked.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(Track {
                    path,
                    title: "Refreshed title".into(),
                    ..track.clone()
                })
            })
        });
        let CachedLoadEvent::Cached(cached) = events.recv_timeout(Duration::from_secs(2)).unwrap()
        else {
            panic!("cache must publish first")
        };
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].title, "Cached title");
        assert_eq!(cached[0].lyrics.lines(), ["Persisted manual text"]);
        reading.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(events.try_recv(), Err(mpsc::TryRecvError::Empty)));
        release.send(()).unwrap();
        let CachedLoadEvent::Refreshed { original, track } =
            events.recv_timeout(Duration::from_secs(2)).unwrap()
        else {
            panic!("source update must follow cache")
        };
        assert_eq!(*original, cached[0]);
        assert_eq!(track.title, "Refreshed title");
        assert_eq!(track.lyrics.lines(), ["Persisted manual text"]);
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(2)).unwrap(),
            CachedLoadEvent::Complete
        ));
        worker.join().unwrap();
    }
}
