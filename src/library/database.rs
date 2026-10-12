use super::{Track, lyrics::Lyrics};
use rusqlite::{Connection, params};
use std::path::Path;

pub struct Database(Connection);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LyricsAssociation {
    Automatic,
    Manual { source: String, lyrics: Lyrics },
    Removed,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let connection = Connection::open(path).map_err(|e| e.to_string())?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS tracks (path TEXT PRIMARY KEY, modified INTEGER NOT NULL, metadata BLOB NOT NULL);")
            .map_err(|e| e.to_string())?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS lyrics_override (audio_path TEXT PRIMARY KEY, source_path TEXT, lyrics BLOB, removed INTEGER NOT NULL DEFAULT 0);")
            .map_err(|e| e.to_string())?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS lyric_romaji (audio_path TEXT PRIMARY KEY, annotation BLOB NOT NULL);")
            .map_err(|e| e.to_string())?;
        Ok(Self(connection))
    }

    pub fn load_romaji(
        &self,
    ) -> Result<std::collections::HashMap<std::path::PathBuf, super::romaji::Romaji>, String> {
        let mut statement = self
            .0
            .prepare("SELECT audio_path, annotation FROM lyric_romaji")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|e| e.to_string())?;
        let mut result = std::collections::HashMap::new();
        for row in rows {
            let (path, bytes) = row.map_err(|e| e.to_string())?;
            // A damaged or obsolete annotation must never prevent the music library from opening.
            if let Ok(romaji) = serde_json::from_slice::<super::romaji::Romaji>(&bytes)
                && romaji.validate().is_ok()
            {
                result.insert(path.into(), romaji);
            }
        }
        Ok(result)
    }

    pub fn save_romaji(&self, audio: &Path, romaji: &super::romaji::Romaji) -> Result<(), String> {
        romaji.validate()?;
        self.0
            .execute(
                "INSERT OR REPLACE INTO lyric_romaji(audio_path, annotation) VALUES (?, ?)",
                params![
                    audio.to_string_lossy(),
                    serde_json::to_vec(romaji).map_err(|e| e.to_string())?
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn remove_romaji(&self, audio: &Path) -> Result<(), String> {
        self.0
            .execute(
                "DELETE FROM lyric_romaji WHERE audio_path = ?",
                [audio.to_string_lossy()],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
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

    pub fn set_manual_lyrics(
        &self,
        audio: &Path,
        source: &Path,
        lyrics: &Lyrics,
    ) -> Result<(), String> {
        if lyrics.is_empty() {
            return Err("lyrics file is empty or invalid".into());
        }
        let bytes = serde_json::to_vec(lyrics).map_err(|e| e.to_string())?;
        self.0.execute("INSERT OR REPLACE INTO lyrics_override(audio_path, source_path, lyrics, removed) VALUES (?, ?, ?, 0)",
            params![audio.to_string_lossy(), source.to_string_lossy(), bytes]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn remove_lyrics(&self, audio: &Path) -> Result<(), String> {
        self.0.execute("INSERT OR REPLACE INTO lyrics_override(audio_path, source_path, lyrics, removed) VALUES (?, NULL, NULL, 1)",
            [audio.to_string_lossy()]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn use_automatic_lyrics(&self, audio: &Path) -> Result<(), String> {
        self.0
            .execute(
                "DELETE FROM lyrics_override WHERE audio_path = ?",
                [audio.to_string_lossy()],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn lyrics_association(&self, audio: &Path) -> Result<LyricsAssociation, String> {
        let row: Option<(Option<String>, Option<Vec<u8>>, bool)> = self
            .0
            .query_row(
                "SELECT source_path, lyrics, removed FROM lyrics_override WHERE audio_path = ?",
                [audio.to_string_lossy()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        match row {
            None => Ok(LyricsAssociation::Automatic),
            Some((_, _, true)) => Ok(LyricsAssociation::Removed),
            Some((source, Some(data), false)) => Ok(LyricsAssociation::Manual {
                source: source.unwrap_or_default(),
                lyrics: serde_json::from_slice(&data).map_err(|e| e.to_string())?,
            }),
            Some(_) => Ok(LyricsAssociation::Automatic),
        }
    }

    pub fn apply_lyrics(&self, track: &mut Track) -> Result<(), String> {
        match self.lyrics_association(&track.path)? {
            LyricsAssociation::Automatic => {}
            LyricsAssociation::Removed => track.lyrics = Lyrics::default(),
            LyricsAssociation::Manual { lyrics, .. } => track.lyrics = lyrics,
        }
        Ok(())
    }
}

use rusqlite::OptionalExtension;
