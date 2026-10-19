//! Verified, idempotent desktop-library imports. Packages contain portable IDs;
//! all destination paths and translation keys are rebuilt inside this container.
use super::{
    Track, database::Database, lyrics::Lyrics, paths::is_audio, playlists, romaji::Romaji,
    statistics::StatisticsStore,
};
use crate::translation::{self, SourceLine, TargetLanguage};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub source_id: String,
    pub export_id: String,
    pub tracks: Vec<Song>,
    #[serde(default)]
    pub playlists: Vec<SavedPlaylist>,
    #[serde(default)]
    pub lyrics: Vec<LyricOverride>,
    #[serde(default)]
    pub romaji: Vec<Reading>,
    #[serde(default)]
    pub ratings: Vec<Rating>,
    #[serde(default)]
    pub statistics: Vec<Statistics>,
    #[serde(default)]
    pub history: Vec<History>,
    #[serde(default)]
    pub translations: Vec<Translated>,
}
#[derive(Debug, Deserialize)]
pub struct Song {
    pub id: String,
    pub file: String,
    pub size: u64,
    pub sha256: String,
    pub artwork_file: Option<String>,
    pub artwork_size: Option<u64>,
    pub artwork_sha256: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub number: u32,
    pub duration: f64,
    #[serde(default)]
    pub lyrics: Lyrics,
}
#[derive(Debug, Deserialize)]
pub struct SavedPlaylist {
    pub id: i64,
    pub name: String,
    pub tracks: Vec<String>,
}
#[derive(Debug, Deserialize)]
pub struct LyricOverride {
    pub track: String,
    pub removed: bool,
    pub lyrics: Option<Lyrics>,
}
#[derive(Debug, Deserialize)]
pub struct Reading {
    pub track: String,
    pub annotation: Romaji,
}
#[derive(Debug, Deserialize)]
pub struct Rating {
    pub track: String,
    pub stars: u8,
}
#[derive(Debug, Deserialize)]
pub struct Statistics {
    pub track: String,
    pub added_at: i64,
    pub play_count: u64,
    pub last_played: Option<i64>,
}
#[derive(Debug, Deserialize)]
pub struct History {
    pub id: i64,
    pub track: String,
    pub played_at: i64,
    pub listened_seconds: u32,
}
#[derive(Debug, Deserialize)]
pub struct Translated {
    pub track: String,
    pub target: TargetLanguage,
    pub provider_version: String,
    pub source: Vec<SourceLine>,
    pub lines: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncReceipt {
    pub version: u32,
    pub source_id: String,
    pub export_id: String,
    pub complete: bool,
    pub tracks: usize,
    pub playlists: usize,
    pub history: usize,
    pub files_copied: usize,
    pub history_added: usize,
    pub conflicts: usize,
    pub warnings: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct SyncReport {
    pub source_id: String,
    pub export_id: String,
    pub complete: bool,
    pub receipt: Option<SyncReceipt>,
    pub error: Option<String>,
}

/// A malformed or unfinished package never prevents the application from opening.
pub fn apply_pending(documents: &Path) -> Vec<SyncReport> {
    let pending = documents.join("DesktopSync");
    let Ok(entries) = fs::read_dir(&pending) else {
        return Vec::new();
    };
    if fs::symlink_metadata(&pending).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Vec::new();
    }
    let mut packages: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    packages.sort();
    let mut reports = Vec::new();
    for package in packages {
        if !package.join("READY").exists() {
            continue;
        }
        let source_id = package
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let export_id = checked_blob(&package, "manifest.json", None, None)
            .ok()
            .filter(|path| {
                fs::metadata(path).is_ok_and(|metadata| metadata.len() <= 16 * 1024 * 1024)
            })
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| {
                value
                    .get("export_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        let report = match import_package(documents, &package) {
            Ok(receipt) => SyncReport {
                source_id,
                export_id,
                complete: true,
                receipt: Some(receipt),
                error: None,
            },
            Err(error) => SyncReport {
                source_id,
                export_id,
                complete: false,
                receipt: None,
                error: Some(error),
            },
        };
        if plain_directory(&package).is_ok() {
            let _ = write_json(&package.join("report.json"), &report);
        }
        reports.push(report);
    }
    reports
}

pub fn import_package(documents: &Path, package: &Path) -> Result<SyncReceipt, String> {
    plain_directory(documents)?;
    let relative = package
        .strip_prefix(documents)
        .map_err(|_| "Sync package is outside Documents")?;
    let components: Vec<_> = relative.components().collect();
    if components.len() != 2 || components[0].as_os_str() != "DesktopSync" {
        return Err("Invalid sync package location".into());
    }
    safe_join(documents, relative, false)?;
    checked_blob(package, "READY", None, None)?;
    let manifest_path = checked_blob(package, "manifest.json", None, None)?;
    if fs::metadata(&manifest_path).map_err(error)?.len() > 16 * 1024 * 1024 {
        return Err("Sync manifest exceeds 16 MiB".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(manifest_path).map_err(error)?).map_err(error)?;
    if manifest.version != 1
        || !identifier(&manifest.source_id)
        || !identifier(&manifest.export_id)
        || package.file_name().and_then(|name| name.to_str()) != Some(&manifest.source_id)
        || manifest.tracks.len() > 100_000
    {
        return Err("Unsupported or invalid sync manifest".into());
    }
    let ready = checked_blob(package, "READY", None, None)?;
    if fs::metadata(&ready).map_err(error)?.len() > 128
        || fs::read_to_string(ready).map_err(error)?.trim() != manifest.export_id
    {
        return Err("READY does not match this export_id".into());
    }
    let mut paths = BTreeMap::new();
    let mut payloads = BTreeMap::new();
    let mut metadata = BTreeMap::new();
    // Validate every byte and every association before creating a DB or target.
    for song in &manifest.tracks {
        if !identifier(&song.id)
            || !identifier(&song.sha256)
            || !song.duration.is_finite()
            || song.duration < 0.0
            || paths.contains_key(&song.id)
        {
            return Err("Invalid or duplicate track ID/metadata".into());
        }
        let relative = Path::new(&song.file);
        let components: Vec<_> = relative.components().collect();
        if components.len() != 3
            || components[0].as_os_str() != "Music"
            || components[1].as_os_str() != song.id.as_str()
            || !is_audio(relative)
        {
            return Err("Track file must be Music/<track_id>/<audio filename>".into());
        }
        let payload = checked_blob(package, &song.file, Some(song.size), Some(&song.sha256))?;
        super::scanner::read_track(payload.clone())
            .map_err(|error| format!("Invalid audio file {}: {error}", song.file))?;
        let artwork = match (&song.artwork_file, song.artwork_size, &song.artwork_sha256) {
            (None, None, None) => Vec::new(),
            (Some(file), Some(size), Some(hash))
                if file == &format!("Artwork/{}.bin", song.id)
                    && size <= 32 * 1024 * 1024
                    && identifier(hash) =>
            {
                fs::read(checked_blob(package, file, Some(size), Some(hash))?).map_err(error)?
            }
            _ => return Err("Invalid artwork descriptor".into()),
        };
        let destination = documents
            .join("Music/Desktop")
            .join(&manifest.source_id)
            .join(&song.id)
            .join(components[2].as_os_str());
        paths.insert(song.id.clone(), destination.clone());
        payloads.insert(song.id.clone(), payload);
        metadata.insert(
            song.id.clone(),
            Track {
                path: destination,
                title: song.title.clone(),
                artist: song.artist.clone(),
                album: song.album.clone(),
                genre: song.genre.clone(),
                number: song.number,
                duration: song.duration,
                artwork,
                lyrics: song.lyrics.clone(),
            },
        );
    }
    let known = |id: &str| {
        if paths.contains_key(id) {
            Ok(())
        } else {
            Err(format!("Unknown sync track: {id}"))
        }
    };
    let mut ids = BTreeSet::new();
    for playlist in &manifest.playlists {
        if !ids.insert(playlist.id)
            || playlist.id <= 0
            || playlist.name.trim().is_empty()
            || playlist.name.chars().count() > 100
            || playlist.name.chars().any(char::is_control)
        {
            return Err("Invalid playlist descriptor".into());
        }
        let mut unique = BTreeSet::new();
        for id in &playlist.tracks {
            known(id)?;
            if !unique.insert(id) {
                return Err("Duplicate playlist track".into());
            }
        }
    }
    let mut seen = BTreeSet::new();
    for item in &manifest.lyrics {
        known(&item.track)?;
        if !seen.insert(&item.track)
            || (!item.removed && item.lyrics.as_ref().is_none_or(Lyrics::is_empty))
        {
            return Err("Invalid lyrics association".into());
        }
    }
    seen.clear();
    for item in &manifest.romaji {
        known(&item.track)?;
        if !seen.insert(&item.track) {
            return Err("Duplicate romanization".into());
        }
        item.annotation.validate()?;
    }
    seen.clear();
    for item in &manifest.ratings {
        known(&item.track)?;
        if !seen.insert(&item.track) || item.stars > 5 {
            return Err("Invalid rating".into());
        }
    }
    seen.clear();
    for item in &manifest.statistics {
        known(&item.track)?;
        if !seen.insert(&item.track) || item.play_count > i64::MAX as u64 {
            return Err("Invalid statistics".into());
        }
    }
    let mut history_ids = BTreeSet::new();
    for item in &manifest.history {
        known(&item.track)?;
        if item.id <= 0 || !history_ids.insert(item.id) {
            return Err("Invalid history ID".into());
        }
    }
    let mut translated = Vec::new();
    for item in &manifest.translations {
        known(&item.track)?;
        let request = translation::Request {
            request_id: 0,
            track_id: paths[&item.track].clone(),
            target: item.target,
            provider_version: item.provider_version.clone(),
            source: item.source.clone(),
        };
        translated.push(
            request
                .validate_response(&serde_json::json!({"translations": item.lines}).to_string())
                .map_err(|error| format!("Invalid translation: {error:?}"))?,
        );
    }
    let support = documents.join(".nano");
    if support.exists() {
        plain_directory(&support)?;
    }
    let database_path = support.join("library.sqlite3");
    if fs::symlink_metadata(&database_path).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err("Database cannot be a symbolic link".into());
    }
    let translation_path = support.join("translations.json");
    if fs::symlink_metadata(&translation_path)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err("Translation cache cannot be a symbolic link".into());
    }
    let mut cache = translation::Cache::load(&translation_path)
        .map_err(|error| format!("Cannot preserve phone translations: {error:?}"))?;
    Database::open(&database_path)?;
    playlists::load(&database_path)?;
    StatisticsStore::open(&database_path)?;
    let mut connection = Connection::open(&database_path).map_err(error)?;
    connection
        .busy_timeout(std::time::Duration::from_secs(10))
        .map_err(error)?;
    connection.execute_batch("CREATE TABLE IF NOT EXISTS desktop_sync_tracks(source_id TEXT NOT NULL, track_id TEXT NOT NULL, path TEXT NOT NULL, sha256 TEXT NOT NULL, PRIMARY KEY(source_id,track_id));
        CREATE TABLE IF NOT EXISTS desktop_sync_playlists(source_id TEXT NOT NULL, source_playlist_id INTEGER NOT NULL, local_playlist_id INTEGER NOT NULL, last_name TEXT NOT NULL, last_paths BLOB NOT NULL, PRIMARY KEY(source_id,source_playlist_id));
        CREATE TABLE IF NOT EXISTS desktop_sync_statistics(source_id TEXT NOT NULL, track_id TEXT NOT NULL, play_count INTEGER NOT NULL, PRIMARY KEY(source_id,track_id));
        CREATE TABLE IF NOT EXISTS desktop_sync_history(source_id TEXT NOT NULL, source_history_id INTEGER NOT NULL, local_history_id INTEGER NOT NULL, PRIMARY KEY(source_id,source_history_id));").map_err(error)?;
    artwork_schema(&connection)?;
    let mut receipt = SyncReceipt {
        version: 1,
        source_id: manifest.source_id.clone(),
        export_id: manifest.export_id.clone(),
        complete: true,
        tracks: manifest.tracks.len(),
        playlists: manifest.playlists.len(),
        history: manifest.history.len(),
        files_copied: 0,
        history_added: 0,
        conflicts: 0,
        warnings: Vec::new(),
    };
    let mut publication = Publication::new(&support)?;
    let transaction = connection.transaction().map_err(error)?;
    let mut staged_artwork = BTreeSet::new();
    for song in &manifest.tracks {
        let destination = &paths[&song.id];
        let old_hash: Option<String> = transaction
            .query_row(
                "SELECT sha256 FROM desktop_sync_tracks WHERE source_id=? AND track_id=?",
                params![manifest.source_id, song.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(error)?;
        let mut retain_phone = false;
        if destination.exists() {
            safe_join(
                documents,
                destination.strip_prefix(documents).map_err(error)?,
                true,
            )?;
            let (_, current) = digest_file(destination)?;
            if current != song.sha256 {
                if old_hash.as_ref() == Some(&current) {
                    publication.stage(&payloads[&song.id], destination, song.size, &song.sha256)?;
                    receipt.files_copied += 1;
                } else {
                    retain_phone = true;
                    conflict(&mut receipt, format!("Kept phone audio for {}", song.title));
                }
            }
        } else {
            publication.stage(&payloads[&song.id], destination, song.size, &song.sha256)?;
            receipt.files_copied += 1;
        }
        if !retain_phone {
            transaction.execute("INSERT INTO tracks(path,modified,metadata) VALUES (?,0,?) ON CONFLICT(path) DO UPDATE SET modified=0,metadata=excluded.metadata", params![destination.to_string_lossy(), serde_json::to_vec(&metadata[&song.id]).map_err(error)?]).map_err(error)?;
            transaction.execute("INSERT INTO desktop_sync_tracks VALUES (?,?,?,?) ON CONFLICT(source_id,track_id) DO UPDATE SET path=excluded.path,sha256=excluded.sha256", params![manifest.source_id,song.id,destination.to_string_lossy(),song.sha256]).map_err(error)?;
            if let Some((file, hash, size)) = stage_artwork(
                &mut publication,
                documents,
                package,
                song,
                &mut staged_artwork,
            )? {
                transaction.execute("INSERT INTO desktop_sync_artwork VALUES (?,?,?,?,?) ON CONFLICT(source_id,track_id) DO UPDATE SET file=excluded.file,sha256=excluded.sha256,size=excluded.size", params![manifest.source_id,song.id,file,hash,size]).map_err(error)?;
            }
        }
    }
    for item in &manifest.playlists {
        let desired: Vec<String> = item
            .tracks
            .iter()
            .map(|id| paths[id].to_string_lossy().into_owned())
            .collect();
        let mapped: Option<(i64,String,Vec<u8>)> = transaction.query_row("SELECT local_playlist_id,last_name,last_paths FROM desktop_sync_playlists WHERE source_id=? AND source_playlist_id=?", params![manifest.source_id,item.id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(error)?;
        let mut local_id = None;
        if let Some((id, last_name, last_paths)) = mapped {
            let current: Option<String> = transaction
                .query_row("SELECT name FROM playlists WHERE id=?", [id], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(error)?;
            if let Some(current) = current {
                let current_paths: Vec<String> = transaction
                    .prepare(
                        "SELECT path FROM playlist_tracks WHERE playlist_id=? ORDER BY position",
                    )
                    .map_err(error)?
                    .query_map([id], |row| row.get(0))
                    .map_err(error)?
                    .collect::<Result<_, _>>()
                    .map_err(error)?;
                let previous: Vec<String> = serde_json::from_slice(&last_paths).map_err(error)?;
                if current != last_name || current_paths != previous {
                    conflict(
                        &mut receipt,
                        format!("Kept phone edits to playlist {current}"),
                    );
                    continue;
                }
                local_id = Some(id);
            }
        }
        let name = playlist_name(&transaction, item.name.trim(), local_id)?;
        let id = if let Some(id) = local_id {
            transaction
                .execute(
                    "UPDATE playlists SET name=?,name_key=? WHERE id=?",
                    params![name, name.to_lowercase(), id],
                )
                .map_err(error)?;
            transaction
                .execute("DELETE FROM playlist_tracks WHERE playlist_id=?", [id])
                .map_err(error)?;
            id
        } else {
            transaction
                .execute(
                    "INSERT INTO playlists(name,name_key) VALUES (?,?)",
                    params![name, name.to_lowercase()],
                )
                .map_err(error)?;
            transaction.last_insert_rowid()
        };
        for (position, path) in desired.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO playlist_tracks VALUES (?,?,?)",
                    params![id, position as i64, path],
                )
                .map_err(error)?;
        }
        transaction.execute("INSERT INTO desktop_sync_playlists VALUES (?,?,?,?,?) ON CONFLICT(source_id,source_playlist_id) DO UPDATE SET local_playlist_id=excluded.local_playlist_id,last_name=excluded.last_name,last_paths=excluded.last_paths", params![manifest.source_id,item.id,id,name,serde_json::to_vec(&desired).map_err(error)?]).map_err(error)?;
    }
    for item in &manifest.lyrics {
        let path = paths[&item.track].to_string_lossy();
        let current: Option<(bool, Option<Vec<u8>>)> = transaction
            .query_row(
                "SELECT removed,lyrics FROM lyrics_override WHERE audio_path=?",
                [path.as_ref()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(error)?;
        if let Some((removed, bytes)) = current {
            let existing: Option<Lyrics> = bytes
                .as_deref()
                .map(serde_json::from_slice)
                .transpose()
                .map_err(error)?;
            if removed != item.removed || (!removed && existing != item.lyrics) {
                conflict(
                    &mut receipt,
                    format!("Kept phone lyrics for {}", metadata[&item.track].title),
                );
            }
        } else {
            let bytes = item
                .lyrics
                .as_ref()
                .map(serde_json::to_vec)
                .transpose()
                .map_err(error)?;
            transaction.execute("INSERT INTO lyrics_override(audio_path,source_path,lyrics,removed) VALUES (?,'Desktop sync',?,?)", params![path,bytes,item.removed]).map_err(error)?;
        }
    }
    for item in &manifest.romaji {
        let path = paths[&item.track].to_string_lossy();
        let existing: Option<Vec<u8>> = transaction
            .query_row(
                "SELECT annotation FROM lyric_romaji WHERE audio_path=?",
                [path.as_ref()],
                |row| row.get(0),
            )
            .optional()
            .map_err(error)?;
        if let Some(bytes) = existing {
            if serde_json::from_slice::<Romaji>(&bytes).map_err(error)? != item.annotation {
                conflict(
                    &mut receipt,
                    format!("Kept phone readings for {}", metadata[&item.track].title),
                );
            }
        } else {
            transaction
                .execute(
                    "INSERT INTO lyric_romaji VALUES (?,?)",
                    params![path, serde_json::to_vec(&item.annotation).map_err(error)?],
                )
                .map_err(error)?;
        }
    }
    for item in &manifest.ratings {
        let path = paths[&item.track].to_string_lossy();
        let existing: Option<u8> = transaction
            .query_row(
                "SELECT stars FROM track_ratings WHERE path=?",
                [path.as_ref()],
                |row| row.get(0),
            )
            .optional()
            .map_err(error)?;
        if existing.is_some_and(|stars| stars != item.stars) {
            conflict(
                &mut receipt,
                format!("Kept phone rating for {}", metadata[&item.track].title),
            );
        }
        transaction
            .execute(
                "INSERT OR IGNORE INTO track_ratings VALUES (?,?)",
                params![path, item.stars],
            )
            .map_err(error)?;
    }
    for item in &manifest.statistics {
        let path = paths[&item.track].to_string_lossy();
        let previous: u64 = transaction
            .query_row(
                "SELECT play_count FROM desktop_sync_statistics WHERE source_id=? AND track_id=?",
                params![manifest.source_id, item.track],
                |row| row.get(0),
            )
            .optional()
            .map_err(error)?
            .unwrap_or(0);
        let delta = item.play_count.saturating_sub(previous);
        let current: u64 = transaction
            .query_row(
                "SELECT play_count FROM playback_statistics WHERE path=?",
                [path.as_ref()],
                |row| row.get(0),
            )
            .optional()
            .map_err(error)?
            .unwrap_or(0);
        if current
            .checked_add(delta)
            .is_none_or(|count| count > i64::MAX as u64)
        {
            return Err("Playback count overflow".into());
        }
        transaction.execute("INSERT INTO playback_statistics VALUES (?,?,?,?) ON CONFLICT(path) DO UPDATE SET added_at=MIN(playback_statistics.added_at,excluded.added_at),play_count=playback_statistics.play_count+excluded.play_count,last_played=COALESCE(MAX(playback_statistics.last_played,excluded.last_played),playback_statistics.last_played,excluded.last_played)", params![path,item.added_at,delta,item.last_played]).map_err(error)?;
        transaction.execute("INSERT INTO desktop_sync_statistics VALUES (?,?,?) ON CONFLICT(source_id,track_id) DO UPDATE SET play_count=MAX(desktop_sync_statistics.play_count,excluded.play_count)", params![manifest.source_id,item.track,item.play_count]).map_err(error)?;
    }
    for item in &manifest.history {
        let path = paths[&item.track].to_string_lossy();
        let existing: Option<i64> = transaction.query_row("SELECT local_history_id FROM desktop_sync_history WHERE source_id=? AND source_history_id=?", params![manifest.source_id,item.id], |row| row.get(0)).optional().map_err(error)?;
        if let Some(id) = existing {
            transaction
                .execute(
                    "UPDATE listening_history SET path=?,played_at=?,listened_seconds=? WHERE id=?",
                    params![path, item.played_at, item.listened_seconds, id],
                )
                .map_err(error)?;
        } else {
            transaction
                .execute(
                    "INSERT INTO listening_history(path,played_at,listened_seconds) VALUES (?,?,?)",
                    params![path, item.played_at, item.listened_seconds],
                )
                .map_err(error)?;
            let local = transaction.last_insert_rowid();
            transaction
                .execute(
                    "INSERT INTO desktop_sync_history VALUES (?,?,?)",
                    params![manifest.source_id, item.id, local],
                )
                .map_err(error)?;
            receipt.history_added += 1;
        }
    }
    let mut cache_changed = false;
    for value in translated {
        if let Some(existing) = cache.get(&value.key) {
            if existing != &value {
                conflict(&mut receipt, "Kept phone lyric translation".into());
            }
        } else {
            cache.insert(value);
            cache_changed = true;
        }
    }
    publication.publish(documents)?;
    if cache_changed {
        cache
            .save(&translation_path)
            .map_err(|error| format!("Cannot save imported translations: {error:?}"))?;
    }
    transaction.commit().map_err(error)?;
    publication.committed = true;
    write_json(&package.join("receipt.json"), &receipt)?;
    fs::rename(package.join("READY"), package.join("APPLIED")).map_err(error)?;
    Ok(receipt)
}

fn artwork_schema(connection: &Connection) -> Result<(), String> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS desktop_sync_artwork(source_id TEXT NOT NULL,track_id TEXT NOT NULL,file TEXT NOT NULL,sha256 TEXT NOT NULL,size INTEGER NOT NULL,PRIMARY KEY(source_id,track_id));").map_err(error)
}

fn stage_artwork(
    publication: &mut Publication,
    documents: &Path,
    package: &Path,
    song: &Song,
    staged: &mut BTreeSet<String>,
) -> Result<Option<(String, String, u64)>, String> {
    let (Some(source), Some(hash), Some(size)) =
        (&song.artwork_file, &song.artwork_sha256, song.artwork_size)
    else {
        return Ok(None);
    };
    if !identifier(&song.id)
        || !identifier(hash)
        || source != &format!("Artwork/{}.bin", song.id)
        || size > 32 * 1024 * 1024
    {
        return Err("Invalid artwork descriptor".into());
    }
    let relative = format!("desktop-artwork/{hash}.bin");
    if staged.insert(hash.clone()) {
        let source = checked_blob(package, source, Some(size), Some(hash))?;
        let destination = documents.join(".nano").join(&relative);
        let valid = destination.exists()
            && safe_join(
                documents,
                destination.strip_prefix(documents).map_err(error)?,
                true,
            )
            .is_ok()
            && digest_file(&destination).is_ok_and(|value| value == (size, hash.clone()));
        if !valid {
            publication.stage(&source, &destination, size, hash)?;
        }
    }
    Ok(Some((relative, hash.clone(), size)))
}

/// One-time recovery for already successful v1 imports, including the version
/// that kept artwork only in the scanner's replaceable metadata cache. This
/// reads cover payloads only; audio, playback data and source receipts are left
/// unchanged. A partially uploaded newer export is never used for recovery.
pub fn repair_applied_artwork(documents: &Path) -> Vec<String> {
    let pending = documents.join("DesktopSync");
    if plain_directory(&pending).is_err() {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(&pending) else {
        return Vec::new();
    };
    let mut warnings = Vec::new();
    for package in entries.flatten().map(|entry| entry.path()) {
        if !package.join("APPLIED").is_file() {
            continue;
        }
        if let Err(error) = repair_package_artwork(documents, &package) {
            if plain_directory(&package).is_ok() {
                let _ = write_json(
                    &package.join("artwork-repair-report.json"),
                    &serde_json::json!({"version":1,"complete":false,"error":error}),
                );
            }
            warnings.push(format!("Cover recovery: {error}"));
        }
    }
    warnings
}

fn repair_package_artwork(documents: &Path, package: &Path) -> Result<(), String> {
    let receipt = checked_blob(package, "receipt.json", None, None)?;
    let manifest = checked_blob(package, "manifest.json", None, None)?;
    if fs::metadata(&receipt).map_err(error)?.len() > 1024 * 1024
        || fs::metadata(&manifest).map_err(error)?.len() > 16 * 1024 * 1024
    {
        return Err("Artwork recovery manifest is too large".into());
    }
    let receipt: SyncReceipt =
        serde_json::from_slice(&fs::read(receipt).map_err(error)?).map_err(error)?;
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(manifest).map_err(error)?).map_err(error)?;
    let marker = checked_blob(package, "APPLIED", None, None)?;
    if fs::metadata(&marker).map_err(error)?.len() > 128 {
        return Err("Invalid artwork recovery marker".into());
    }
    if manifest.version != 1
        || !receipt.complete
        || !identifier(&manifest.source_id)
        || !identifier(&manifest.export_id)
        || receipt.source_id != manifest.source_id
        || receipt.export_id != manifest.export_id
        || fs::read_to_string(marker).map_err(error)?.trim() != manifest.export_id
        || package.file_name().and_then(|name| name.to_str()) != Some(&manifest.source_id)
    {
        return Ok(());
    }
    safe_join(
        documents,
        package.strip_prefix(documents).map_err(error)?,
        false,
    )?;
    let support = documents.join(".nano");
    plain_directory(&support)?;
    let database = support.join("library.sqlite3");
    if fs::symlink_metadata(&database)
        .map_err(error)?
        .file_type()
        .is_symlink()
    {
        return Err("Artwork database cannot be a symbolic link".into());
    }
    let mut connection = Connection::open(database).map_err(error)?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(error)?;
    let owned:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='desktop_sync_tracks')",[],|row|row.get(0)).map_err(error)?;
    if !owned {
        return Ok(());
    }
    artwork_schema(&connection)?;
    let mut publication = Publication::new(&support)?;
    let transaction = connection.transaction().map_err(error)?;
    let mut staged = BTreeSet::new();
    let mut restored = 0;
    for song in &manifest.tracks {
        let existing: Option<(String, u64)> = transaction
            .query_row(
                "SELECT file,size FROM desktop_sync_artwork WHERE source_id=? AND track_id=?",
                params![manifest.source_id, song.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(error)?;
        if let Some((file, size)) = existing {
            if safe_join(&support, Path::new(&file), true)
                .is_ok_and(|path| fs::metadata(path).is_ok_and(|metadata| metadata.len() == size))
            {
                continue;
            }
            transaction
                .execute(
                    "DELETE FROM desktop_sync_artwork WHERE source_id=? AND track_id=?",
                    params![manifest.source_id, song.id],
                )
                .map_err(error)?;
        }
        let owned: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM desktop_sync_tracks WHERE source_id=? AND track_id=?)",
                params![manifest.source_id, song.id],
                |row| row.get(0),
            )
            .map_err(error)?;
        if !owned {
            continue;
        }
        if let Some((file, hash, size)) =
            stage_artwork(&mut publication, documents, package, song, &mut staged)?
        {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO desktop_sync_artwork VALUES (?,?,?,?,?)",
                    params![manifest.source_id, song.id, file, hash, size],
                )
                .map_err(error)?;
            restored += 1;
        }
    }
    publication.publish(documents)?;
    transaction.commit().map_err(error)?;
    publication.committed = true;
    if restored > 0 {
        write_json(
            &package.join("artwork-repair-report.json"),
            &serde_json::json!({"version":1,"source_id":manifest.source_id,"export_id":manifest.export_id,"complete":true,"restored":restored}),
        )?;
    }
    Ok(())
}

/// Overlay a durable imported cover only when a scan produced no artwork.
/// Save just that field back into the existing scanner cache, retaining its
/// stamp and automatic lyrics so manual/automatic lyric choices keep working.
pub fn apply_artwork(database_path: &Path, tracks: &mut [Track]) -> Vec<String> {
    let result = (|| -> Result<Vec<String>, String> {
        let connection = Connection::open(database_path).map_err(error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(error)?;
        let present:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='desktop_sync_artwork')",[],|row|row.get(0)).map_err(error)?;
        if !present {
            return Ok(Vec::new());
        }
        let covers:BTreeMap<PathBuf,(String,String,u64)>=connection.prepare("SELECT t.path,a.file,a.sha256,a.size FROM desktop_sync_artwork a JOIN desktop_sync_tracks t ON t.source_id=a.source_id AND t.track_id=a.track_id").map_err(error)?
            .query_map([],|row|Ok((PathBuf::from(row.get::<_,String>(0)?),(row.get(1)?,row.get(2)?,row.get(3)?)))).map_err(error)?
            .collect::<Result<_,_>>().map_err(error)?;
        let support = database_path
            .parent()
            .ok_or("Missing artwork support directory")?;
        let database = Database::open(database_path)?;
        let mut loaded: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut warnings = Vec::new();
        let mut failed = BTreeSet::new();
        for track in tracks.iter_mut().filter(|track| track.artwork.is_empty()) {
            let Some((file, hash, size)) = covers.get(&track.path) else {
                continue;
            };
            if *size > 32 * 1024 * 1024
                || !identifier(hash)
                || file != &format!("desktop-artwork/{hash}.bin")
            {
                warnings.push("Cannot restore cover: invalid persistent artwork descriptor".into());
                continue;
            }
            if failed.contains(file) {
                continue;
            }
            if !loaded.contains_key(file) {
                match checked_blob(support, file, Some(*size), Some(hash))
                    .and_then(|path| fs::read(path).map_err(error))
                {
                    Ok(bytes) => {
                        loaded.insert(file.clone(), bytes);
                    }
                    Err(error) => {
                        warnings.push(format!("Cannot restore cover: {error}"));
                        failed.insert(file.clone());
                        continue;
                    }
                }
            }
            let bytes = &loaded[file];
            let stamp: Option<u64> = connection
                .query_row(
                    "SELECT modified FROM tracks WHERE path=?",
                    [track.path.to_string_lossy().as_ref()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(error)?;
            if let Some(stamp) = stamp
                && let Some(mut cached) = database.cached(&track.path, stamp)
            {
                cached.artwork = bytes.clone();
                database.save(&cached, stamp)?;
            }
            track.artwork = bytes.clone();
        }
        Ok(warnings)
    })();
    result.unwrap_or_else(|error| vec![format!("Cannot restore covers: {error}")])
}

fn identifier(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn conflict(receipt: &mut SyncReceipt, message: String) {
    receipt.conflicts += 1;
    receipt.warnings.push(message);
}
fn plain_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(error)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(format!("Not a plain directory: {}", path.display()))
    }
}
fn safe_join(base: &Path, relative: &Path, file: bool) -> Result<PathBuf, String> {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("Unsafe relative path in sync package".into());
    }
    let mut path = base.to_owned();
    let count = relative.components().count();
    for (index, component) in relative.components().enumerate() {
        path.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&path).map_err(error)?;
        if metadata.file_type().is_symlink()
            || (index + 1 < count || !file) && !metadata.is_dir()
            || index + 1 == count && file && !metadata.is_file()
        {
            return Err(format!("Unsafe file or symbolic link: {}", path.display()));
        }
    }
    Ok(path)
}
fn checked_blob(
    base: &Path,
    file: &str,
    size: Option<u64>,
    hash: Option<&str>,
) -> Result<PathBuf, String> {
    plain_directory(base)?;
    let path = safe_join(base, Path::new(file), true)?;
    if let Some(size) = size
        && fs::metadata(&path).map_err(error)?.len() != size
    {
        return Err(format!("File size mismatch: {file}"));
    }
    if let Some(hash) = hash
        && digest_file(&path)?.1 != hash
    {
        return Err(format!("File SHA256 mismatch: {file}"));
    }
    Ok(path)
}
fn digest_file(path: &Path) -> Result<(u64, String), String> {
    let mut file = File::open(path).map_err(error)?;
    let mut hasher = Sha256::new();
    let mut total = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(error)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        total += count as u64;
    }
    Ok((total, format!("{:x}", hasher.finalize())))
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let temp = path.with_extension(format!("{}.tmp", rand::random::<u64>()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(error)?;
        file.write_all(&serde_json::to_vec_pretty(value).map_err(error)?)
            .map_err(error)?;
        file.sync_all().map_err(error)?;
        fs::rename(&temp, path).map_err(error)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
fn playlist_name(
    transaction: &rusqlite::Transaction<'_>,
    base: &str,
    own: Option<i64>,
) -> Result<String, String> {
    for number in 0..100_000 {
        let suffix = match number {
            0 => String::new(),
            1 => " (Desktop)".into(),
            _ => format!(" (Desktop {number})"),
        };
        let prefix: String = base
            .chars()
            .take(100usize.saturating_sub(suffix.chars().count()))
            .collect();
        let name = format!("{prefix}{suffix}");
        let existing: Option<i64> = transaction
            .query_row(
                "SELECT id FROM playlists WHERE name_key=?",
                [name.to_lowercase()],
                |row| row.get(0),
            )
            .optional()
            .map_err(error)?;
        if existing.is_none() || existing == own {
            return Ok(name);
        }
    }
    Err("Too many playlists with this name".into())
}

struct Publication {
    stage: PathBuf,
    files: Vec<(PathBuf, PathBuf)>,
    published: Vec<(PathBuf, Option<PathBuf>)>,
    committed: bool,
}
impl Publication {
    fn new(support: &Path) -> Result<Self, String> {
        let stage = support.join(format!("desktop-sync-stage-{:016x}", rand::random::<u64>()));
        fs::create_dir(&stage).map_err(error)?;
        Ok(Self {
            stage,
            files: Vec::new(),
            published: Vec::new(),
            committed: false,
        })
    }
    fn stage(
        &mut self,
        source: &Path,
        destination: &Path,
        size: u64,
        hash: &str,
    ) -> Result<(), String> {
        let temp = self.stage.join(self.files.len().to_string());
        let mut input = File::open(source).map_err(error)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(error)?;
        let mut digest = Sha256::new();
        let mut total = 0;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = input.read(&mut buffer).map_err(error)?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count]).map_err(error)?;
            digest.update(&buffer[..count]);
            total += count as u64;
        }
        output.sync_all().map_err(error)?;
        if total != size || format!("{:x}", digest.finalize()) != hash {
            return Err("Sync source changed while copying".into());
        }
        self.files.push((temp, destination.to_owned()));
        Ok(())
    }
    fn publish(&mut self, documents: &Path) -> Result<(), String> {
        for (index, (temp, destination)) in self.files.iter().enumerate() {
            let parent = destination
                .parent()
                .ok_or("Missing sync destination directory")?;
            let relative = parent.strip_prefix(documents).map_err(error)?;
            let mut current = documents.to_owned();
            for component in relative.components() {
                if !matches!(component, Component::Normal(_)) {
                    return Err("Unsafe sync destination".into());
                }
                current.push(component.as_os_str());
                if current.exists() {
                    plain_directory(&current)?;
                } else {
                    fs::create_dir(&current).map_err(error)?;
                }
            }
            let backup = if destination.exists() {
                let backup = self.stage.join(format!("backup-{index}"));
                fs::rename(destination, &backup).map_err(error)?;
                Some(backup)
            } else {
                None
            };
            self.published.push((destination.clone(), backup));
            fs::rename(temp, destination).map_err(error)?;
        }
        Ok(())
    }
}
impl Drop for Publication {
    fn drop(&mut self) {
        if !self.committed {
            for (destination, backup) in self.published.iter().rev() {
                let _ = fs::remove_file(destination);
                if let Some(backup) = backup {
                    let _ = fs::rename(backup, destination);
                }
            }
        }
        let _ = fs::remove_dir_all(&self.stage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const EXPORT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const FIRST: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const SECOND: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, Value) {
        let directory = tempfile::tempdir().unwrap();
        let documents = directory.path().join("Documents");
        let package = documents.join("DesktopSync").join(SOURCE);
        let lyrics = Lyrics::plain_text("音楽");
        let annotation =
            Romaji::new(lyrics.clone(), vec!["ongaku".into()], "offline".into()).unwrap();
        let mut tracks = Vec::new();
        for (id, title) in [(FIRST, "One"), (SECOND, "Two")] {
            let relative = format!("Music/{id}/song.wav");
            let path = package.join(&relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, include_bytes!("../../tests/fixtures/quiet.wav")).unwrap();
            let (size, hash) = digest_file(&path).unwrap();
            tracks.push(json!({"id":id,"file":relative,"size":size,"sha256":hash,"title":title,"artist":"Artist","album":"Album","genre":"Genre","number":1,"duration":1.0,"lyrics":lyrics}));
        }
        let manifest = json!({"version":1,"source_id":SOURCE,"export_id":EXPORT,"tracks":tracks,
            "playlists":[{"id":7,"name":"Drive","tracks":[SECOND,FIRST]}],
            "lyrics":[{"track":FIRST,"removed":false,"lyrics":lyrics}],
            "romaji":[{"track":FIRST,"annotation":annotation}],
            "ratings":[{"track":FIRST,"stars":3},{"track":SECOND,"stars":0}],
            "statistics":[{"track":FIRST,"added_at":100,"play_count":5,"last_played":200}],
            "history":[{"id":42,"track":FIRST,"played_at":200,"listened_seconds":30}],
            "translations":[{"track":FIRST,"target":"english","provider_version":"offline","source":[{"at_ms":null,"text":"音楽"}],"lines":["Music"]}]
        });
        commit(&package, &manifest);
        (directory, documents, package, manifest)
    }
    fn commit(package: &Path, manifest: &Value) {
        fs::write(
            package.join("manifest.json"),
            serde_json::to_vec_pretty(manifest).unwrap(),
        )
        .unwrap();
        fs::write(
            package.join("READY"),
            manifest["export_id"].as_str().unwrap(),
        )
        .unwrap();
    }
    fn song(documents: &Path, id: &str) -> PathBuf {
        documents
            .join("Music/Desktop")
            .join(SOURCE)
            .join(id)
            .join("song.wav")
    }
    #[test]
    fn imports_two_real_tracks_and_repeat_sync_does_not_duplicate_data() {
        let (_directory, documents, package, manifest) = fixture();
        let phone = documents.join("Music/phone.wav");
        fs::create_dir_all(phone.parent().unwrap()).unwrap();
        fs::write(&phone, include_bytes!("../../tests/fixtures/quiet.wav")).unwrap();
        let database = documents.join(".nano/library.sqlite3");
        let independent =
            playlists::create(&database, "Drive", std::slice::from_ref(&phone)).unwrap();
        let first = import_package(&documents, &package).unwrap();
        assert_eq!(first.files_copied, 2);
        assert_eq!(first.history_added, 1);
        assert!(package.join("receipt.json").is_file());
        assert!(package.join("APPLIED").is_file());
        assert!(!package.join("READY").exists());
        let saved = playlists::load(&database).unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0], independent);
        assert_eq!(saved[1].name, "Drive (Desktop)");
        assert_eq!(
            saved[1].tracks,
            [song(&documents, SECOND), song(&documents, FIRST)]
        );
        let bytes = fs::read(song(&documents, FIRST)).unwrap();
        fs::write(
            package.join(format!("Music/{FIRST}/song.wav")),
            b"source may later be overwritten",
        )
        .unwrap();
        assert_eq!(
            fs::read(song(&documents, FIRST)).unwrap(),
            bytes,
            "the package and library must never share an inode"
        );
        fs::write(package.join(format!("Music/{FIRST}/song.wav")), bytes).unwrap();
        commit(&package, &manifest);
        let again = import_package(&documents, &package).unwrap();
        assert_eq!(again.files_copied, 0);
        assert_eq!(again.history_added, 0);
        assert_eq!(again.conflicts, 0);
        assert_eq!(playlists::load(&database).unwrap().len(), 2);
        let statistics = StatisticsStore::open(&database).unwrap();
        assert_eq!(
            statistics
                .track_statistics(&song(&documents, FIRST))
                .unwrap()
                .unwrap()
                .play_count,
            5
        );
        assert_eq!(statistics.listening_history(100).unwrap().len(), 1);
        assert_eq!(
            statistics.ratings().unwrap().get(&song(&documents, SECOND)),
            Some(&0)
        );
        #[cfg(any(target_os = "ios", feature = "mobile"))]
        {
            let storage = crate::mobile::storage::Storage::new(documents.clone()).unwrap();
            let tracks = storage.refresh().unwrap().tracks;
            let track = tracks
                .iter()
                .find(|track| track.path == song(&documents, FIRST))
                .unwrap();
            assert_eq!(
                track.lyrics.lines(),
                ["音楽"],
                "desktop sidecar snapshots survive mobile rescans through their explicit override"
            );
        }
    }
    #[test]
    fn later_sync_preserves_phone_edits_and_adds_only_new_source_plays_and_history() {
        let (_directory, documents, package, mut manifest) = fixture();
        import_package(&documents, &package).unwrap();
        let path = song(&documents, FIRST);
        let database_path = documents.join(".nano/library.sqlite3");
        let playlist = playlists::load(&database_path).unwrap().remove(0);
        playlists::update(
            &database_path,
            playlist.id,
            "Phone edit",
            std::slice::from_ref(&path),
        )
        .unwrap();
        let phone_lyrics = Lyrics::plain_text("手機歌詞");
        let database = Database::open(&database_path).unwrap();
        database
            .set_manual_lyrics(&path, Path::new("Phone"), &phone_lyrics)
            .unwrap();
        let phone_romaji = Romaji::new(
            Lyrics::plain_text("音楽"),
            vec!["phone reading".into()],
            "offline".into(),
        )
        .unwrap();
        database.save_romaji(&path, &phone_romaji).unwrap();
        let statistics = StatisticsStore::open(&database_path).unwrap();
        statistics.set_rating(&path, 4).unwrap();
        statistics.record_play(&path, 300, 30).unwrap();
        let cache_path = documents.join(".nano/translations.json");
        let mut cache = translation::Cache::load(&cache_path).unwrap();
        let request = translation::Request {
            request_id: 0,
            track_id: path.clone(),
            target: TargetLanguage::English,
            provider_version: "offline".into(),
            source: vec![SourceLine {
                at_ms: None,
                text: "音楽".into(),
            }],
        };
        let phone_translation = request
            .validate_response(r#"{"translations":["Phone translation"]}"#)
            .unwrap();
        cache.insert(phone_translation.clone());
        cache.save(&cache_path).unwrap();
        manifest["export_id"] =
            json!("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc");
        manifest["ratings"][0]["stars"] = json!(5);
        manifest["statistics"][0]["play_count"] = json!(8);
        manifest["playlists"][0]["name"] = json!("Desktop edit");
        manifest["lyrics"][0]["lyrics"] =
            serde_json::to_value(Lyrics::plain_text("Desktop lyrics")).unwrap();
        manifest["romaji"][0]["annotation"]["lines"] = json!(["desktop reading"]);
        manifest["translations"][0]["lines"] = json!(["Desktop translation"]);
        manifest["history"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":43,"track":FIRST,"played_at":400,"listened_seconds":30}));
        commit(&package, &manifest);
        let result = import_package(&documents, &package).unwrap();
        assert!(result.conflicts >= 5, "{:?}", result.warnings);
        assert_eq!(result.history_added, 1);
        assert_eq!(result.files_copied, 0);
        assert_eq!(
            playlists::load(&database_path).unwrap()[0].name,
            "Phone edit"
        );
        assert_eq!(
            playlists::load(&database_path).unwrap()[0].tracks,
            std::slice::from_ref(&path)
        );
        assert_eq!(statistics.ratings().unwrap().get(&path), Some(&4));
        assert_eq!(
            statistics
                .track_statistics(&path)
                .unwrap()
                .unwrap()
                .play_count,
            9
        );
        assert_eq!(statistics.listening_history(100).unwrap().len(), 3);
        // Restoring an older desktop DB must not lower the imported high-water
        // mark and count the same source plays again on the next export.
        manifest["statistics"][0]["play_count"] = json!(1);
        commit(&package, &manifest);
        import_package(&documents, &package).unwrap();
        manifest["statistics"][0]["play_count"] = json!(8);
        commit(&package, &manifest);
        import_package(&documents, &package).unwrap();
        assert_eq!(
            statistics
                .track_statistics(&path)
                .unwrap()
                .unwrap()
                .play_count,
            9
        );
        assert!(
            matches!(database.lyrics_association(&path).unwrap(),super::super::database::LyricsAssociation::Manual{lyrics,..} if lyrics==phone_lyrics)
        );
        assert_eq!(
            database.load_romaji().unwrap().get(&path),
            Some(&phone_romaji)
        );
        assert_eq!(
            translation::Cache::load(&cache_path)
                .unwrap()
                .get(&request.cache_key()),
            Some(&phone_translation)
        );
        commit(&package, &manifest);
        import_package(&documents, &package).unwrap();
        assert_eq!(
            statistics
                .track_statistics(&path)
                .unwrap()
                .unwrap()
                .play_count,
            9
        );
        assert_eq!(statistics.listening_history(100).unwrap().len(), 3);
    }
    #[test]
    fn unsafe_missing_corrupt_or_uncommitted_packages_leave_the_library_untouched() {
        let (_directory, documents, package, mut manifest) = fixture();
        manifest["tracks"][0]["file"] = json!("../escape.wav");
        commit(&package, &manifest);
        assert!(import_package(&documents, &package).is_err());
        assert!(!documents.join(".nano").exists());
        manifest["tracks"][0]["file"] = json!(format!("Music/{FIRST}/song.wav"));
        manifest["tracks"][0]["sha256"] =
            json!("0000000000000000000000000000000000000000000000000000000000000000");
        commit(&package, &manifest);
        assert!(import_package(&documents, &package).is_err());
        assert!(!documents.join("Music").exists());
        manifest["tracks"][0]["sha256"] = json!(
            digest_file(&package.join(format!("Music/{FIRST}/song.wav")))
                .unwrap()
                .1
        );
        commit(&package, &manifest);
        fs::write(
            package.join("READY"),
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        )
        .unwrap();
        let report = apply_pending(&documents);
        assert_eq!(report.len(), 1);
        assert!(!report[0].complete);
        assert_eq!(report[0].export_id, EXPORT);
        assert!(package.join("READY").exists());
        assert!(!documents.join(".nano").exists());
        commit(&package, &manifest);
        fs::remove_file(package.join(format!("Music/{SECOND}/song.wav"))).unwrap();
        assert!(import_package(&documents, &package).is_err());
        assert!(!documents.join(".nano").exists());
    }
    #[cfg(unix)]
    #[test]
    fn symbolic_link_payloads_are_rejected_before_database_or_audio_changes() {
        let (directory, documents, package, _manifest) = fixture();
        let payload = package.join(format!("Music/{FIRST}/song.wav"));
        let outside = directory.path().join("outside.wav");
        fs::rename(&payload, &outside).unwrap();
        std::os::unix::fs::symlink(&outside, &payload).unwrap();
        assert!(import_package(&documents, &package).is_err());
        assert!(!documents.join(".nano").exists());
        assert!(outside.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn publication_failure_rolls_back_files_and_the_database_preserving_phone_records() {
        let (directory, documents, package, _manifest) = fixture();
        let phone = documents.join("Music/personal.wav");
        fs::create_dir_all(phone.parent().unwrap()).unwrap();
        fs::write(&phone, include_bytes!("../../tests/fixtures/quiet.wav")).unwrap();
        let database = documents.join(".nano/library.sqlite3");
        let personal =
            playlists::create(&database, "Personal", std::slice::from_ref(&phone)).unwrap();
        let blocked = song(&documents, SECOND).parent().unwrap().to_owned();
        fs::create_dir_all(blocked.parent().unwrap()).unwrap();
        let outside = directory.path().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, &blocked).unwrap();
        assert!(import_package(&documents, &package).is_err());
        assert!(
            !song(&documents, FIRST).exists(),
            "earlier published files must be rolled back"
        );
        assert_eq!(playlists::load(&database).unwrap(), [personal]);
        assert!(
            Database::open(&database)
                .unwrap()
                .load()
                .unwrap()
                .is_empty()
        );
        assert!(
            StatisticsStore::open(&database)
                .unwrap()
                .listening_history(10)
                .unwrap()
                .is_empty()
        );
        assert!(fs::read_dir(outside).unwrap().next().is_none());
        assert!(phone.is_file());
        assert!(package.join("READY").is_file());
    }

    #[cfg(any(target_os = "ios", feature = "mobile"))]
    #[test]
    fn moved_ios_container_keeps_sync_ownership_and_playlist_baselines() {
        let (directory, documents, package, manifest) = fixture();
        // Establish the managed-root journal before the first synchronization.
        let storage = crate::mobile::storage::Storage::new(documents.clone()).unwrap();
        assert!(package.join("APPLIED").exists());
        assert_eq!(playlists::load(storage.database_path()).unwrap().len(), 1);
        let moved = directory.path().join("restored-container");
        fs::rename(&documents, &moved).unwrap();
        let restored = crate::mobile::storage::Storage::new(moved.clone()).unwrap();
        let package = moved.join("DesktopSync").join(SOURCE);
        commit(&package, &manifest);
        let receipt = import_package(&moved, &package).unwrap();
        assert_eq!(receipt.files_copied, 0);
        assert_eq!(receipt.history_added, 0);
        assert_eq!(
            receipt.conflicts, 0,
            "container relocation is not a phone edit"
        );
        let saved = playlists::load(restored.database_path()).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].tracks, [song(&moved, SECOND), song(&moved, FIRST)]);
    }

    #[cfg(any(target_os = "ios", feature = "mobile"))]
    fn add_external_covers(package: &Path, manifest: &mut Value) -> Vec<u8> {
        let bytes = include_bytes!("../../assets/generated/music.png").to_vec();
        for id in [FIRST, SECOND] {
            let relative = format!("Artwork/{id}.bin");
            let path = package.join(&relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &bytes).unwrap();
            let (size, hash) = digest_file(&path).unwrap();
            let song = manifest["tracks"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|song| song["id"] == id)
                .unwrap();
            song["artwork_file"] = json!(relative);
            song["artwork_size"] = json!(size);
            song["artwork_sha256"] = json!(hash);
        }
        commit(package, manifest);
        bytes
    }

    #[cfg(any(target_os = "ios", feature = "mobile"))]
    #[test]
    fn external_only_covers_survive_rescan_cache_invalidation_restart_and_container_move() {
        let (directory, documents, package, mut manifest) = fixture();
        let cover = add_external_covers(&package, &mut manifest);
        assert!(
            super::super::scanner::read_track(package.join(format!("Music/{FIRST}/song.wav")))
                .unwrap()
                .artwork
                .is_empty()
        );
        import_package(&documents, &package).unwrap();
        let storage = crate::mobile::storage::Storage::new(documents.clone()).unwrap();
        let tracks = storage.refresh().unwrap().tracks;
        assert!(tracks.iter().all(|track| track.artwork == cover));
        assert_eq!(
            fs::read_dir(storage.support_dir().join("desktop-artwork"))
                .unwrap()
                .count(),
            1,
            "identical album covers share one content-addressed file"
        );
        image::load_from_memory(&tracks[0].artwork).unwrap();
        let database = Database::open(storage.database_path()).unwrap();
        let mut cleared = tracks
            .iter()
            .find(|track| track.path == song(&documents, FIRST))
            .unwrap()
            .clone();
        cleared.artwork.clear();
        database.save(&cleared, 0).unwrap();
        database.use_automatic_lyrics(&cleared.path).unwrap();
        let tracks = storage.refresh().unwrap().tracks;
        let first = tracks
            .iter()
            .find(|track| track.path == song(&documents, FIRST))
            .unwrap();
        assert_eq!(first.artwork, cover);
        assert!(
            first.lyrics.is_empty(),
            "cover updates must not bake a manual lyric override into the scanner cache"
        );
        fs::remove_dir_all(&package).unwrap();
        database.evict_track(&song(&documents, FIRST)).unwrap();
        drop(database);
        let moved = directory.path().join("moved-documents");
        fs::rename(documents, &moved).unwrap();
        let restored = crate::mobile::storage::Storage::new(moved).unwrap();
        assert!(
            restored
                .refresh()
                .unwrap()
                .tracks
                .iter()
                .all(|track| track.artwork == cover),
            "persistent artwork no longer depends on the exported package or old container path"
        );
    }

    #[cfg(any(target_os = "ios", feature = "mobile"))]
    #[test]
    fn legacy_applied_package_repairs_only_covers_without_audio_payloads_or_phone_data_changes() {
        let (_directory, documents, package, mut manifest) = fixture();
        let cover = add_external_covers(&package, &mut manifest);
        import_package(&documents, &package).unwrap();
        let support = documents.join(".nano");
        let path = song(&documents, FIRST);
        let db_path = support.join("library.sqlite3");
        Connection::open(&db_path)
            .unwrap()
            .execute("DELETE FROM desktop_sync_artwork", [])
            .unwrap();
        fs::remove_dir_all(support.join("desktop-artwork")).unwrap();
        let database = Database::open(&db_path).unwrap();
        for mut track in database.load().unwrap() {
            track.artwork.clear();
            database.save(&track, 0).unwrap();
        }
        let phone_lyrics = Lyrics::plain_text("Phone lyrics");
        database
            .set_manual_lyrics(&path, Path::new("Phone"), &phone_lyrics)
            .unwrap();
        let statistics = StatisticsStore::open(&db_path).unwrap();
        statistics.set_rating(&path, 5).unwrap();
        statistics.record_play(&path, 1234, 30).unwrap();
        let playlist = playlists::load(&db_path).unwrap().remove(0);
        playlists::update(
            &db_path,
            playlist.id,
            "Phone playlist",
            std::slice::from_ref(&path),
        )
        .unwrap();
        let receipt = fs::read(package.join("receipt.json")).unwrap();
        let count = statistics
            .track_statistics(&path)
            .unwrap()
            .unwrap()
            .play_count;
        fs::remove_dir_all(package.join("Music")).unwrap();
        let storage = crate::mobile::storage::Storage::new(documents.clone()).unwrap();
        let tracks = storage.refresh().unwrap().tracks;
        assert!(tracks.iter().all(|track| track.artwork == cover));
        assert!(package.join("artwork-repair-report.json").is_file());
        assert!(!package.join("READY").exists());
        assert_eq!(fs::read(package.join("receipt.json")).unwrap(), receipt);
        assert_eq!(statistics.ratings().unwrap().get(&path), Some(&5));
        assert_eq!(
            statistics
                .track_statistics(&path)
                .unwrap()
                .unwrap()
                .play_count,
            count
        );
        assert_eq!(playlists::load(&db_path).unwrap()[0].name, "Phone playlist");
        assert!(
            matches!(database.lyrics_association(&path).unwrap(),super::super::database::LyricsAssociation::Manual{lyrics,..} if lyrics==phone_lyrics)
        );
        let current = tracks.iter().find(|track| track.path == path).unwrap();
        let connection = Connection::open(&db_path).unwrap();
        let stamp: u64 = connection
            .query_row(
                "SELECT modified FROM tracks WHERE path=?",
                [path.to_string_lossy().as_ref()],
                |row| row.get(0),
            )
            .unwrap();
        let mut phone = current.clone();
        phone.artwork = vec![7, 8, 9];
        database.save(&phone, stamp).unwrap();
        assert_eq!(
            storage
                .refresh()
                .unwrap()
                .tracks
                .iter()
                .find(|track| track.path == path)
                .unwrap()
                .artwork,
            [7, 8, 9],
            "an existing phone cover is never replaced by a desktop fallback"
        );
    }
}
