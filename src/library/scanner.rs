use super::{Track, database::Database};
use lofty::{
    file::{AudioFile, TaggedFileExt},
    prelude::Accessor,
    probe::Probe,
};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
    time::UNIX_EPOCH,
};

pub struct ScanResult {
    pub tracks: Vec<Track>,
    pub warnings: Vec<String>,
}

pub enum ImportEvent {
    SelectionClosed,
    Completed(Result<Option<ScanResult>, String>),
}

/// Folder selection, filesystem traversal and metadata reads all happen off the UI thread.
pub fn import(database_path: PathBuf) -> Receiver<ImportEvent> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = (|| {
            let folders = rfd::FileDialog::new()
                .set_title("Import music folders")
                .pick_folders();
            let _ = sender.send(ImportEvent::SelectionClosed);
            let Some(folders) = folders else {
                return Ok(None);
            };
            let database = Database::open(&database_path)?;
            let mut tracks = database.load()?;
            let mut warnings = Vec::new();
            for folder in folders {
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
                    use std::hash::{Hash, Hasher};
                    let mut stamp = std::collections::hash_map::DefaultHasher::new();
                    for file in std::iter::once(path.to_path_buf())
                        .chain(super::lyrics::sidecar_candidates(path))
                    {
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
                    let modified = stamp.finish() & i64::MAX as u64;
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
            Ok(Some(ScanResult { tracks, warnings }))
        })();
        let _ = sender.send(ImportEvent::Completed(result));
    });
    receiver
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
