use rusqlite::{Connection, Transaction, params};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub tracks: Vec<PathBuf>,
}

fn open(database: &Path) -> Result<Connection, String> {
    if let Some(parent) = database.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let connection = Connection::open(database).map_err(|error| error.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| error.to_string())?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS playlists (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                name_key TEXT NOT NULL UNIQUE
            );
            CREATE TABLE IF NOT EXISTS playlist_tracks (
                playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                position INTEGER NOT NULL,
                path TEXT NOT NULL,
                PRIMARY KEY (playlist_id, position),
                UNIQUE (playlist_id, path)
            );",
        )
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

fn path_text(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "playlist track path is not valid Unicode".to_owned())
}

pub fn load(database: &Path) -> Result<Vec<Playlist>, String> {
    let connection = open(database)?;
    let mut statement = connection
        .prepare("SELECT id, name FROM playlists ORDER BY id")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let mut playlists = Vec::new();
    for row in rows {
        let (id, name) = row.map_err(|error| error.to_string())?;
        let mut tracks_statement = connection
            .prepare("SELECT path FROM playlist_tracks WHERE playlist_id = ? ORDER BY position")
            .map_err(|error| error.to_string())?;
        let tracks = tracks_statement
            .query_map([id], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .map(|path| path.map(PathBuf::from).map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        playlists.push(Playlist { id, name, tracks });
    }
    Ok(playlists)
}

pub fn create(database: &Path, name: &str, tracks: &[PathBuf]) -> Result<Playlist, String> {
    let (name, track_paths) = validate(name, tracks, false)?;

    let connection = open(database)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let id = insert_in_transaction(&transaction, name, &name.to_lowercase(), &track_paths)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(Playlist {
        id,
        name: name.to_owned(),
        tracks: tracks.to_vec(),
    })
}

fn validate<'a>(
    name: &'a str,
    tracks: &[PathBuf],
    allow_empty: bool,
) -> Result<(&'a str, Vec<String>), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("playlist name cannot be empty".into());
    }
    if name.chars().any(char::is_control) {
        return Err("playlist name contains control characters".into());
    }
    if name.chars().count() > 100 {
        return Err("playlist name cannot exceed 100 characters".into());
    }
    if tracks.is_empty() && !allow_empty {
        return Err("playlist must contain at least one track".into());
    }
    let track_paths = tracks
        .iter()
        .map(|path| path_text(path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = std::collections::HashSet::new();
    if track_paths.iter().any(|path| !seen.insert(path)) {
        return Err("playlist track paths must be unique".into());
    }

    Ok((name, track_paths))
}

/// Replace the name and ordered paths atomically, retaining the playlist identity.
/// Missing paths stay recorded; availability is checked by the controller for new additions.
pub fn update(
    database: &Path,
    id: i64,
    name: &str,
    tracks: &[PathBuf],
) -> Result<Playlist, String> {
    let (name, track_paths) = validate(name, tracks, true)?;
    let connection = open(database)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let changed = transaction
        .execute(
            "UPDATE playlists SET name = ?, name_key = ? WHERE id = ?",
            params![name, name.to_lowercase(), id],
        )
        .map_err(name_error)?;
    if changed == 0 {
        return Err("playlist no longer exists".into());
    }
    transaction
        .execute("DELETE FROM playlist_tracks WHERE playlist_id = ?", [id])
        .map_err(|error| error.to_string())?;
    insert_tracks(&transaction, id, &track_paths)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(Playlist {
        id,
        name: name.to_owned(),
        tracks: tracks.to_vec(),
    })
}

fn name_error(error: rusqlite::Error) -> String {
    if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
        "a playlist with that name already exists".to_owned()
    } else {
        error.to_string()
    }
}

fn insert_in_transaction(
    transaction: &Transaction<'_>,
    name: &str,
    name_key: &str,
    tracks: &[String],
) -> Result<i64, String> {
    transaction
        .execute(
            "INSERT INTO playlists (name, name_key) VALUES (?, ?)",
            params![name, name_key],
        )
        .map_err(name_error)?;
    let id = transaction.last_insert_rowid();
    insert_tracks(transaction, id, tracks)?;
    Ok(id)
}

fn insert_tracks(transaction: &Transaction<'_>, id: i64, tracks: &[String]) -> Result<(), String> {
    for (position, path) in tracks.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO playlist_tracks (playlist_id, position, path) VALUES (?, ?, ?)",
                params![id, position as i64, path],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
