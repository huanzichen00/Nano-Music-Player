//! An application-owned library; external document URLs are only used during import.
//!
//! All operations are synchronous and belong on a worker thread. Metadata is
//! cached with the shared scanner, while Music is the source of truth. On each
//! load the cache is reconciled against that directory, so a restored iOS
//! container does not keep unusable absolute paths from its previous location.

use crate::library::{
    Track,
    database::Database,
    paths::{is_audio, normalize},
    scanner::{self, ScanResult},
};
use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone)]
pub struct Storage {
    documents: PathBuf,
    music: PathBuf,
    database: PathBuf,
    staging: PathBuf,
    worker: Arc<Mutex<()>>,
}

impl Storage {
    pub fn new(documents: PathBuf) -> Result<Self, String> {
        let documents = normalize(&documents);
        let music = documents.join("Music");
        let support = documents.join(".nano");
        let staging = support.join("imports");
        for directory in [&music, &support, &staging] {
            fs::create_dir_all(directory)
                .map_err(|error| format!("Cannot create {}: {error}", directory.display()))?;
        }
        let database = support.join("library.sqlite3");
        Database::open(&database)
            .map_err(|error| format!("Cannot open the music library: {error}"))?;
        let storage = Self {
            documents,
            music,
            database,
            staging,
            worker: Arc::new(Mutex::new(())),
        };
        storage.migrate_container()?;
        crate::library::desktop_sync::apply_pending(&storage.documents);
        crate::library::desktop_sync::repair_applied_artwork(&storage.documents);
        Ok(storage)
    }

    pub fn music_dir(&self) -> &Path {
        &self.music
    }

    pub fn settings_path(&self) -> PathBuf {
        self.documents.join(".nano/settings.json")
    }

    pub fn support_dir(&self) -> &Path {
        self.database
            .parent()
            .expect("library has a support directory")
    }

    pub fn database_path(&self) -> &Path {
        &self.database
    }

    /// Run before controllers open path-bound caches or playlist services.
    /// A recorded Music prefix is relocated by its exact relative suffix; a
    /// same-named song in another folder is never used as a substitute.
    fn migrate_container(&self) -> Result<(), String> {
        use crate::library::{playlists, statistics::StatisticsStore};
        let marker = self.support_dir().join("managed-music-roots.json");
        let mut roots: BTreeSet<PathBuf> = fs::read(&marker)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<PathBuf>>(&bytes).ok())
            .unwrap_or_default()
            .into_iter()
            .collect();
        let legacy = roots.is_empty();
        let connection =
            rusqlite::Connection::open(&self.database).map_err(|error| error.to_string())?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        let tables: BTreeSet<String> = connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .map_err(|error| error.to_string())?
            .query_map([], |row| row.get(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|error| error.to_string())?;
        let mut paths = BTreeSet::new();
        for (table, column) in [
            ("tracks", "path"),
            ("playlist_tracks", "path"),
            ("lyrics_override", "audio_path"),
            ("lyric_romaji", "audio_path"),
            ("playback_statistics", "path"),
            ("listening_history", "path"),
            ("track_ratings", "path"),
            ("desktop_sync_tracks", "path"),
        ] {
            if tables.contains(table) {
                let mut statement = connection
                    .prepare(&format!("SELECT DISTINCT {column} FROM {table}"))
                    .map_err(|error| error.to_string())?;
                let entries = statement
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|error| error.to_string())?;
                for entry in entries {
                    paths.insert(PathBuf::from(entry.map_err(|error| error.to_string())?));
                }
            }
        }
        drop(connection);
        let translation_path = self.support_dir().join("translations.json");
        let mut translations = crate::translation::Cache::load(&translation_path).ok();
        let entries: Vec<crate::translation::Translation> = translations
            .as_ref()
            .and_then(|cache| serde_json::to_value(cache).ok())
            .and_then(|value| {
                value
                    .get("entries")
                    .and_then(|entries| entries.as_object())
                    .cloned()
            })
            .map(|entries| {
                entries
                    .into_values()
                    .filter_map(|value| serde_json::from_value(value).ok())
                    .collect()
            })
            .unwrap_or_default();
        paths.extend(entries.iter().map(|entry| entry.key.track_id.clone()));
        if legacy {
            // Older mobile versions did not record the managed root. Recover
            // only unambiguous, complete Music-relative matches, never basename
            // matches, and record the prefix for future launches.
            for path in &paths {
                if path.starts_with(&self.music) {
                    continue;
                }
                let candidates: Vec<_> = path
                    .ancestors()
                    .skip(1)
                    .filter(|root| root.file_name() == Some(OsStr::new("Music")))
                    .filter(|root| {
                        path.strip_prefix(root)
                            .is_ok_and(|relative| self.music.join(relative).is_file())
                    })
                    .collect();
                if candidates.len() == 1 {
                    roots.insert(candidates[0].to_path_buf());
                }
            }
        }
        roots.insert(self.music.clone());
        let mapping: std::collections::BTreeMap<_, _> = paths
            .into_iter()
            .filter_map(|old| relocated_path(&old, &roots, &self.music).map(|new| (old, new)))
            .collect();
        if !mapping.is_empty() {
            if tables.contains("playlists") {
                for playlist in playlists::load(&self.database)? {
                    let mut seen = BTreeSet::new();
                    let updated: Vec<_> = playlist
                        .tracks
                        .iter()
                        .map(|path| mapping.get(path).unwrap_or(path).clone())
                        .filter(|path| seen.insert(path.clone()))
                        .collect();
                    if updated != playlist.tracks {
                        playlists::update(&self.database, playlist.id, &playlist.name, &updated)?;
                    }
                }
            }
            if tables.contains("playback_statistics")
                || tables.contains("listening_history")
                || tables.contains("track_ratings")
            {
                let statistics = StatisticsStore::open(&self.database)?;
                for (old, new) in &mapping {
                    statistics.rename_path(old, new)?;
                }
            }
            let database = Database::open(&self.database)?;
            let romaji = database.load_romaji()?;
            for (old, new) in &mapping {
                use crate::library::database::LyricsAssociation;
                if matches!(
                    database.lyrics_association(new)?,
                    LyricsAssociation::Automatic
                ) {
                    match database.lyrics_association(old)? {
                        LyricsAssociation::Manual { source, lyrics } => {
                            let source = PathBuf::from(source);
                            let source =
                                relocated_path(&source, &roots, &self.music).unwrap_or(source);
                            database.set_manual_lyrics(new, &source, &lyrics)?;
                            database.use_automatic_lyrics(old)?;
                        }
                        LyricsAssociation::Removed => {
                            database.remove_lyrics(new)?;
                            database.use_automatic_lyrics(old)?;
                        }
                        LyricsAssociation::Automatic => {}
                    }
                }
                if let Some(annotation) = romaji.get(old)
                    && !romaji.contains_key(new)
                {
                    database.save_romaji(new, annotation)?;
                    database.remove_romaji(old)?;
                }
            }
            if let Some(cache) = translations.as_mut() {
                let mut changed = false;
                for entry in entries {
                    if let Some(new) = mapping.get(&entry.key.track_id) {
                        let mut relocated = entry.clone();
                        relocated.key.track_id = new.clone();
                        if cache.get(&relocated.key).is_none() {
                            cache.insert(relocated);
                        }
                        cache.remove(&entry.key);
                        changed = true;
                    }
                }
                if changed {
                    cache.save(&translation_path).map_err(|error| {
                        format!("Cannot relocate lyric translations: {error:?}")
                    })?;
                }
            }
            // Import ownership and playlist baselines follow the same precise
            // relocation. Otherwise a container move would look like a phone
            // playlist edit on the next desktop synchronization.
            if tables.contains("desktop_sync_tracks") || tables.contains("desktop_sync_playlists") {
                let mut connection = rusqlite::Connection::open(&self.database)
                    .map_err(|error| error.to_string())?;
                connection
                    .busy_timeout(std::time::Duration::from_secs(5))
                    .map_err(|error| error.to_string())?;
                let transaction = connection
                    .transaction()
                    .map_err(|error| error.to_string())?;
                if tables.contains("desktop_sync_tracks") {
                    for (old, new) in &mapping {
                        transaction
                            .execute(
                                "UPDATE desktop_sync_tracks SET path=? WHERE path=?",
                                rusqlite::params![new.to_string_lossy(), old.to_string_lossy()],
                            )
                            .map_err(|error| error.to_string())?;
                    }
                }
                if tables.contains("desktop_sync_playlists") {
                    let snapshots: Vec<(String, i64, Vec<u8>)> = transaction.prepare("SELECT source_id,source_playlist_id,last_paths FROM desktop_sync_playlists").map_err(|error| error.to_string())?
                        .query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(|error| error.to_string())?
                        .collect::<Result<_,_>>().map_err(|error| error.to_string())?;
                    for (source, id, bytes) in snapshots {
                        let previous: Vec<PathBuf> =
                            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
                        let current: Vec<_> = previous
                            .iter()
                            .map(|path| mapping.get(path).unwrap_or(path).clone())
                            .collect();
                        if current != previous {
                            transaction.execute("UPDATE desktop_sync_playlists SET last_paths=? WHERE source_id=? AND source_playlist_id=?", rusqlite::params![serde_json::to_vec(&current).map_err(|error| error.to_string())?,source,id]).map_err(|error| error.to_string())?;
                        }
                    }
                }
                transaction.commit().map_err(|error| error.to_string())?;
            }
        }
        let temporary = marker.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec(&roots).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fs::rename(temporary, marker).map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn load(&self) -> Result<Vec<Track>, String> {
        self.refresh().map(|result| result.tracks)
    }

    pub fn refresh(&self) -> Result<ScanResult, String> {
        let _worker = self
            .worker
            .lock()
            .map_err(|_| "The music library worker stopped unexpectedly".to_owned())?;
        self.refresh_unlocked()
    }

    /// Copy readable, supported audio into Music without replacing existing files.
    /// Each failed source becomes a warning; successfully imported songs remain
    /// available even when another item in the same selection cannot be read.
    pub fn import_files(&self, paths: &[PathBuf]) -> Result<ScanResult, String> {
        self.import_files_if(paths, || true)
    }

    /// A transfer may be canceled while waiting for the library lock or copying.
    /// Check admission again before publishing, rather than only at HTTP EOF.
    pub(crate) fn import_files_if(
        &self,
        paths: &[PathBuf],
        should_import: impl Fn() -> bool,
    ) -> Result<ScanResult, String> {
        let _worker = self
            .worker
            .lock()
            .map_err(|_| "The music library worker stopped unexpectedly".to_owned())?;
        if !should_import() {
            return Err("Music import was canceled".into());
        }
        let mut warnings = Vec::new();
        let mut selected = BTreeSet::new();
        for source in paths {
            let source = normalize(source);
            if !selected.insert(source.clone()) {
                continue;
            }
            if let Err(error) = self.import_one(&source, &should_import) {
                warnings.push(format!("{}: {error}", source.display()));
            }
        }
        let mut result = self.refresh_unlocked()?;
        result.warnings.splice(0..0, warnings);
        Ok(result)
    }

    fn refresh_unlocked(&self) -> Result<ScanResult, String> {
        let warnings = self.receive_shared_files();
        let mut result = scanner::scan_folders(&self.database, std::slice::from_ref(&self.music))?;
        result.warnings.splice(0..0, warnings);
        let database = Database::open(&self.database)?;
        let mut retained = Vec::with_capacity(result.tracks.len());
        for track in result.tracks {
            // A new container path is indexed by scan_folders above. Retire
            // metadata carrying the old prefix rather than guessing a match by
            // filename, which could associate two different songs incorrectly.
            let mut keep = track.path.starts_with(&self.music);
            if keep {
                match track.path.metadata() {
                    Ok(metadata) => keep = metadata.is_file(),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => keep = false,
                    Err(error) => result
                        .warnings
                        .push(format!("{}: {error}", track.path.display())),
                }
            }
            if keep {
                retained.push(track);
            } else {
                // Only scanner metadata is evicted; database-owned user data
                // remains intact for future migration support.
                database.evict_track(&track.path)?;
            }
        }
        result.tracks = retained;
        result
            .warnings
            .extend(crate::library::desktop_sync::apply_artwork(
                &self.database,
                &mut result.tracks,
            ));
        Ok(result)
    }

    /// Finder and Apple Devices initially copy files into Documents itself.
    /// Relocate valid audio into Music using same-volume links. This preserves
    /// the inode even if the computer still has the file open for writing.
    fn receive_shared_files(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let entries = match fs::read_dir(&self.documents) {
            Ok(entries) => entries,
            Err(error) => return vec![format!("Cannot read shared files: {error}")],
        };
        for entry in entries {
            let result = (|| {
                let entry = entry.map_err(|error| error.to_string())?;
                let source = entry.path();
                if !entry
                    .file_type()
                    .map_err(|error| error.to_string())?
                    .is_file()
                    || !is_audio(&source)
                {
                    return Ok(());
                }
                scanner::read_track(source.clone())
                    .map_err(|error| format!("{}: {error}", source.display()))?;
                let destination = self.publish_file(&source, &source)?;
                if let Err(error) = fs::remove_file(&source) {
                    // Roll back our new link if the original cannot be moved.
                    let _ = fs::remove_file(destination);
                    return Err(format!("Cannot organize {}: {error}", source.display()));
                }
                Ok(())
            })();
            if let Err(error) = result {
                warnings.push(error);
            }
        }
        warnings
    }

    fn import_one(&self, source: &Path, should_import: &impl Fn() -> bool) -> Result<(), String> {
        if !should_import() {
            return Err("Music import was canceled".into());
        }
        if !is_audio(source) {
            return Err("Unsupported audio file type".into());
        }
        let mut input = File::open(source).map_err(|error| format!("Cannot open file: {error}"))?;
        if !input
            .metadata()
            .map_err(|error| format!("Cannot inspect file: {error}"))?
            .is_file()
        {
            return Err("The selected path is not a regular file".into());
        }
        // Documents already managed by this library do not need another copy.
        if source.starts_with(&self.music) {
            return Ok(());
        }
        let (staged, mut output) =
            StagedFile::create(&self.staging, source.extension().unwrap_or_default())?;
        io::copy(&mut input, &mut output)
            .map_err(|error| format!("Cannot copy audio into the app: {error}"))?;
        output
            .sync_all()
            .map_err(|error| format!("Cannot finish saving audio: {error}"))?;
        drop(output);
        // Verify the complete copy before publishing it in Music. Broken or
        // interrupted downloads never become visible as library entries.
        scanner::read_track(staged.path.clone())
            .map_err(|error| format!("Cannot read audio metadata: {error}"))?;
        if !should_import() {
            return Err("Music import was canceled".into());
        }
        self.publish_file(&staged.path, source).map(|_| ())
    }

    fn publish_file(&self, complete_file: &Path, original_name: &Path) -> Result<PathBuf, String> {
        let filename = original_name
            .file_name()
            .ok_or_else(|| "The selected audio has no filename".to_owned())?;
        let mut number = 1usize;
        loop {
            let destination = self.music.join(if number == 1 {
                filename.to_os_string()
            } else {
                numbered_filename(original_name, number)
            });
            // A same-volume link publishes a fully written file atomically and
            // fails if its name is occupied, including concurrent importers.
            match fs::hard_link(complete_file, &destination) {
                Ok(()) => return Ok(destination),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    number = number
                        .checked_add(1)
                        .ok_or_else(|| "Too many audio files with the same name".to_owned())?;
                }
                Err(error) => {
                    return Err(format!(
                        "Cannot save audio as {}: {error}",
                        destination.display()
                    ));
                }
            }
        }
    }
}

fn relocated_path(path: &Path, roots: &BTreeSet<PathBuf>, music: &Path) -> Option<PathBuf> {
    if path.starts_with(music) {
        return None;
    }
    let candidates: BTreeSet<_> = roots
        .iter()
        .filter(|root| root.as_path() != music)
        .filter_map(|root| {
            path.strip_prefix(root)
                .ok()
                .map(|relative| music.join(relative))
        })
        .filter(|candidate| candidate.is_file())
        .collect();
    (candidates.len() == 1).then(|| candidates.into_iter().next().unwrap())
}

fn numbered_filename(source: &Path, number: usize) -> OsString {
    let mut filename = source.file_stem().unwrap_or_default().to_os_string();
    filename.push(format!(" ({number})"));
    if let Some(extension) = source.extension() {
        filename.push(".");
        filename.push(extension);
    }
    filename
}

struct StagedFile {
    path: PathBuf,
}

impl StagedFile {
    fn create(directory: &Path, extension: &OsStr) -> Result<(Self, File), String> {
        static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
        loop {
            let mut filename = OsString::from(format!(
                "import-{}-{}.",
                std::process::id(),
                NEXT_FILE.fetch_add(1, Ordering::Relaxed)
            ));
            filename.push(extension);
            let path = directory.join(filename);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok((Self { path }, file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("Cannot prepare an audio import: {error}")),
            }
        }
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(directory: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(directory).unwrap();
        let path = directory.join(name);
        fs::write(&path, include_bytes!("../../tests/fixtures/quiet.wav")).unwrap();
        path
    }

    #[test]
    fn duplicate_filenames_are_preserved_and_the_library_survives_restart() {
        let directory = tempfile::tempdir().unwrap();
        let documents = directory.path().join("Documents");
        let first = audio(&directory.path().join("first"), "song.wav");
        let second = audio(&directory.path().join("second"), "song.wav");
        let storage = Storage::new(documents.clone()).unwrap();
        let result = storage.import_files(&[first, second]).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert_eq!(result.tracks.len(), 2);
        assert!(storage.music_dir().join("song.wav").is_file());
        assert!(storage.music_dir().join("song (2).wav").is_file());
        assert_eq!(Storage::new(documents).unwrap().load().unwrap().len(), 2);
    }

    #[test]
    fn failed_selection_items_do_not_replace_or_remove_existing_audio() {
        let directory = tempfile::tempdir().unwrap();
        let storage = Storage::new(directory.path().join("Documents")).unwrap();
        let existing = audio(&directory.path().join("first"), "song.wav");
        storage.import_files(&[existing]).unwrap();
        let original = fs::read(storage.music_dir().join("song.wav")).unwrap();
        let good = audio(&directory.path().join("second"), "song.wav");
        let broken = directory.path().join("broken.mp3");
        fs::write(&broken, b"not audio").unwrap();
        let unsupported = directory.path().join("cover.png");
        fs::write(&unsupported, b"not audio either").unwrap();
        let not_file = directory.path().join("folder.wav");
        fs::create_dir(&not_file).unwrap();
        let missing = directory.path().join("missing.wav");
        let result = storage
            .import_files(&[good, broken, unsupported, not_file, missing])
            .unwrap();
        assert_eq!(result.tracks.len(), 2);
        assert_eq!(result.warnings.len(), 4, "{:?}", result.warnings);
        assert_eq!(
            fs::read(storage.music_dir().join("song.wav")).unwrap(),
            original
        );
        assert!(!storage.music_dir().join("broken.mp3").exists());
        assert_eq!(fs::read_dir(&storage.staging).unwrap().count(), 0);
    }

    #[test]
    fn moving_the_container_reindexes_audio_at_its_current_path() {
        let directory = tempfile::tempdir().unwrap();
        let old_documents = directory.path().join("old-container");
        let source = audio(&directory.path().join("source"), "song.wav");
        let old_storage = Storage::new(old_documents.clone()).unwrap();
        old_storage.import_files(&[source]).unwrap();
        let new_documents = directory.path().join("new-container");
        fs::rename(&old_documents, &new_documents).unwrap();
        let restored = Storage::new(new_documents.clone()).unwrap();
        let tracks = restored.load().unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].path, new_documents.join("Music/song.wav"));
        assert!(tracks[0].path.is_file());
        let cached = Database::open(&restored.database).unwrap().load().unwrap();
        assert_eq!(cached, tracks);
        assert!(
            cached
                .iter()
                .all(|track| !track.path.starts_with(&old_documents))
        );
    }

    #[test]
    fn container_move_restores_playlists_lyrics_romaji_statistics_and_translation_keys() {
        use crate::{
            library::{
                database::LyricsAssociation, lyrics::Lyrics, playlists, romaji::Romaji,
                statistics::StatisticsStore,
            },
            translation::{self, Request, SourceLine, TargetLanguage},
        };
        let directory = tempfile::tempdir().unwrap();
        let old_documents = directory.path().join("old");
        let storage = Storage::new(old_documents.clone()).unwrap();
        let first = audio(&storage.music_dir().join("one"), "same.wav");
        let second = audio(&storage.music_dir().join("two"), "same.wav");
        storage.refresh().unwrap();
        let absent = storage.music_dir().join("missing/same.wav");
        let playlist = playlists::create(
            storage.database_path(),
            "Trip",
            &[second.clone(), first.clone(), absent.clone()],
        )
        .unwrap();
        let lyrics = Lyrics::plain_text("音楽");
        let source = first.with_extension("lrc");
        fs::write(&source, "音楽").unwrap();
        let romaji =
            Romaji::new(lyrics.clone(), vec!["ongaku".into()], "offline-test".into()).unwrap();
        {
            let database = Database::open(storage.database_path()).unwrap();
            database
                .set_manual_lyrics(&first, &source, &lyrics)
                .unwrap();
            database.save_romaji(&first, &romaji).unwrap();
            let stats = StatisticsStore::open(storage.database_path()).unwrap();
            stats
                .register_library_paths(std::slice::from_ref(&first), 100)
                .unwrap();
            stats.set_rating(&first, 4).unwrap();
            stats.record_play(&first, 200, 30).unwrap();
        }
        let mut request = Request {
            request_id: 1,
            track_id: first.clone(),
            target: TargetLanguage::English,
            provider_version: "offline-test".into(),
            source: vec![SourceLine {
                at_ms: None,
                text: "音楽".into(),
            }],
        };
        let translated = request
            .validate_response(r#"{"translations":["Music"]}"#)
            .unwrap();
        let mut cache = translation::Cache::default();
        cache.insert(translated);
        cache
            .save(&storage.support_dir().join("translations.json"))
            .unwrap();
        let new_documents = directory.path().join("new");
        fs::rename(&old_documents, &new_documents).unwrap();
        let restored = Storage::new(new_documents.clone()).unwrap();
        let new_first = restored.music_dir().join("one/same.wav");
        let new_second = restored.music_dir().join("two/same.wav");
        let restored_playlists = playlists::load(restored.database_path()).unwrap();
        assert_eq!(restored_playlists[0].id, playlist.id);
        assert_eq!(
            restored_playlists[0].tracks,
            [new_second, new_first.clone(), absent.clone()],
            "missing targets stay recorded; same basenames are not guessed"
        );
        let database = Database::open(restored.database_path()).unwrap();
        assert_eq!(
            database.lyrics_association(&new_first).unwrap(),
            LyricsAssociation::Manual {
                source: new_first
                    .with_extension("lrc")
                    .to_string_lossy()
                    .into_owned(),
                lyrics
            }
        );
        assert_eq!(
            database.load_romaji().unwrap().get(&new_first),
            Some(&romaji)
        );
        let statistics = StatisticsStore::open(restored.database_path()).unwrap();
        assert_eq!(statistics.ratings().unwrap().get(&new_first), Some(&4));
        assert_eq!(
            statistics
                .track_statistics(&new_first)
                .unwrap()
                .unwrap()
                .play_count,
            1
        );
        assert!(statistics.track_statistics(&first).unwrap().is_none());
        assert_eq!(statistics.listening_history(10).unwrap()[0].path, new_first);
        request.track_id = restored.music_dir().join("one/same.wav");
        let cache =
            translation::Cache::load(&restored.support_dir().join("translations.json")).unwrap();
        assert_eq!(cache.get(&request.cache_key()).unwrap().lines, ["Music"]);
        // The old prefix remains in the journal so a temporarily unavailable
        // song can be restored and relocated on a later launch.
        audio(&restored.music_dir().join("missing"), "same.wav");
        drop(database);
        drop(statistics);
        let reopened = Storage::new(new_documents).unwrap();
        assert_eq!(
            playlists::load(reopened.database_path()).unwrap()[0].tracks[2],
            reopened.music_dir().join("missing/same.wav")
        );
    }

    #[test]
    fn legacy_container_without_root_journal_recovers_only_exact_music_relative_paths() {
        use crate::library::playlists;
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old");
        let storage = Storage::new(old.clone()).unwrap();
        let source = audio(&storage.music_dir().join("album"), "same.wav");
        storage.refresh().unwrap();
        let missing = storage.music_dir().join("other/same.wav");
        playlists::create(
            storage.database_path(),
            "Legacy",
            &[source, missing.clone()],
        )
        .unwrap();
        fs::remove_file(storage.support_dir().join("managed-music-roots.json")).unwrap();
        let new = directory.path().join("new");
        fs::rename(old, &new).unwrap();
        let restored = Storage::new(new).unwrap();
        let tracks = playlists::load(restored.database_path()).unwrap()[0]
            .tracks
            .clone();
        assert_eq!(
            tracks,
            [restored.music_dir().join("album/same.wav"), missing]
        );
    }

    #[test]
    fn canceled_import_keeps_the_library_and_refresh_removes_deleted_files() {
        let directory = tempfile::tempdir().unwrap();
        let storage = Storage::new(directory.path().join("Documents")).unwrap();
        let source = audio(&directory.path().join("source"), "song.wav");
        storage.import_files(&[source]).unwrap();
        assert_eq!(storage.import_files(&[]).unwrap().tracks.len(), 1);
        fs::remove_file(storage.music_dir().join("song.wav")).unwrap();
        assert!(storage.refresh().unwrap().tracks.is_empty());
        assert!(storage.load().unwrap().is_empty());
    }

    #[test]
    fn usb_shared_files_are_organized_once_and_invalid_files_remain_available() {
        let directory = tempfile::tempdir().unwrap();
        let documents = directory.path().join("Documents");
        let storage = Storage::new(documents.clone()).unwrap();
        let existing = audio(&directory.path().join("source"), "song.wav");
        storage.import_files(&[existing]).unwrap();
        let shared = audio(&documents, "song.wav");
        let broken = documents.join("broken.mp3");
        fs::write(&broken, b"not audio").unwrap();
        let result = storage.refresh().unwrap();
        assert_eq!(result.tracks.len(), 2);
        assert_eq!(result.warnings.len(), 1);
        assert!(!shared.exists());
        assert!(storage.music_dir().join("song (2).wav").is_file());
        assert_eq!(fs::read(&broken).unwrap(), b"not audio");
        assert_eq!(storage.refresh().unwrap().tracks.len(), 2);
    }

    #[test]
    fn canceled_transfer_cannot_publish_any_audio() {
        let directory = tempfile::tempdir().unwrap();
        let storage = Storage::new(directory.path().join("Documents")).unwrap();
        let source = audio(&directory.path().join("source"), "song.wav");
        assert!(storage.import_files_if(&[source], || false).is_err());
        assert!(storage.load().unwrap().is_empty());
        assert_eq!(fs::read_dir(&storage.staging).unwrap().count(), 0);
    }
}
