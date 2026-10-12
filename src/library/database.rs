use super::Track;
use rusqlite::{Connection, params};
use std::path::Path;

pub struct Database(Connection);

impl Database {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let connection = Connection::open(path).map_err(|e| e.to_string())?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS tracks (path TEXT PRIMARY KEY, modified INTEGER NOT NULL, metadata BLOB NOT NULL);")
            .map_err(|e| e.to_string())?;
        Ok(Self(connection))
    }

    pub fn load(&self) -> Result<Vec<Track>, String> {
        let mut statement = self
            .0
            .prepare("SELECT metadata FROM tracks ORDER BY path")
            .map_err(|e| e.to_string())?;
        let records = statement
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(|e| e.to_string())?;
        records
            .map(|record| {
                serde_json::from_slice(&record.map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())
            })
            .collect()
    }

    pub fn cached(&self, path: &Path, modified: u64) -> Option<Track> {
        let data: Vec<u8> = self
            .0
            .query_row(
                "SELECT metadata FROM tracks WHERE path = ? AND modified = ?",
                params![path.to_string_lossy(), modified],
                |row| row.get(0),
            )
            .ok()?;
        serde_json::from_slice(&data).ok()
    }

    pub fn save(&self, track: &Track, modified: u64) -> Result<(), String> {
        self.0
            .execute(
                "INSERT OR REPLACE INTO tracks VALUES (?, ?, ?)",
                params![
                    track.path.to_string_lossy(),
                    modified,
                    serde_json::to_vec(track).map_err(|e| e.to_string())?
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
