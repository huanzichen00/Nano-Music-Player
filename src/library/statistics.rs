//! Path-based playback statistics and deterministic smart-playlist queries.
//!
//! This store intentionally owns its tables inside the app's SQLite database, but
//! uses its own connection and schema so it does not couple statistics to scanner,
//! manual playlist, or Genius persistence.

use rusqlite::{Connection, OptionalExtension, params};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

/// The listening history is deliberately bounded so long-running libraries do not
/// grow the database without limit.
pub const DEFAULT_HISTORY_LIMIT: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmartPlaylist {
    RecentlyAdded,
    MostPlayed,
    RecentlyPlayed,
    NeverPlayed,
}

impl SmartPlaylist {
    pub const ALL: [Self; 4] = [
        Self::RecentlyAdded,
        Self::MostPlayed,
        Self::RecentlyPlayed,
        Self::NeverPlayed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::RecentlyAdded => "Recently Added",
            Self::MostPlayed => "Most Played",
            Self::RecentlyPlayed => "Recently Played",
            Self::NeverPlayed => "Never Played",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackStatistics {
    pub added_at: i64,
    pub play_count: u64,
    pub last_played: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListeningHistoryEntry {
    pub path: PathBuf,
    pub played_at: i64,
    pub listened_seconds: u32,
}

pub struct StatisticsStore {
    connection: Connection,
    history_limit: usize,
}

impl StatisticsStore {
    /// Open the statistics tables in the existing app database file.
    pub fn open(database_path: &Path) -> Result<Self, String> {
        if let Some(parent) = database_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let connection = Connection::open(database_path).map_err(|error| error.to_string())?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS playback_statistics (
                    path TEXT PRIMARY KEY,
                    added_at INTEGER NOT NULL,
                    play_count INTEGER NOT NULL DEFAULT 0,
                    last_played INTEGER
                );
                CREATE TABLE IF NOT EXISTS listening_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    path TEXT NOT NULL,
                    played_at INTEGER NOT NULL,
                    listened_seconds INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS listening_history_played_at
                    ON listening_history(played_at DESC, id DESC);",
            )
            .map_err(|error| error.to_string())?;
        Ok(Self {
            connection,
            history_limit: DEFAULT_HISTORY_LIMIT,
        })
    }

    /// Set the first-seen time for previously unknown paths. Re-scans and metadata
    /// updates leave existing timestamps untouched; entries are retained if a file
    /// disappears so a restored file keeps its history.
    pub fn register_library_paths(&self, paths: &[PathBuf], added_at: i64) -> Result<(), String> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        {
            let mut statement = transaction
                .prepare("INSERT OR IGNORE INTO playback_statistics(path, added_at) VALUES (?, ?)")
                .map_err(|e| e.to_string())?;
            for path in paths {
                statement
                    .execute(params![path.to_string_lossy(), added_at])
                    .map_err(|e| e.to_string())?;
            }
        }
        transaction.commit().map_err(|e| e.to_string())
    }

    /// Persist one completed listening session. The caller owns the threshold and
    /// per-session latch, so this records exactly one already-qualified play.
    pub fn record_play(
        &self,
        path: &Path,
        played_at: i64,
        listened_seconds: u32,
    ) -> Result<(), String> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let path_text = path.to_string_lossy();
        transaction
            .execute(
                "INSERT OR IGNORE INTO playback_statistics(path, added_at) VALUES (?, ?)",
                params![path_text, played_at],
            )
            .map_err(|e| e.to_string())?;
        transaction
            .execute(
                "UPDATE playback_statistics SET play_count = play_count + 1, last_played = ? WHERE path = ?",
                params![played_at, path_text],
            )
            .map_err(|e| e.to_string())?;
        transaction
            .execute(
                "INSERT INTO listening_history(path, played_at, listened_seconds) VALUES (?, ?, ?)",
                params![path_text, played_at, listened_seconds],
            )
            .map_err(|e| e.to_string())?;
        transaction
            .execute(
                "DELETE FROM listening_history WHERE id NOT IN
                    (SELECT id FROM listening_history ORDER BY id DESC LIMIT ?)",
                [self.history_limit as i64],
            )
            .map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| e.to_string())
    }

    pub fn track_statistics(&self, path: &Path) -> Result<Option<TrackStatistics>, String> {
        self.connection
            .query_row(
                "SELECT added_at, play_count, last_played FROM playback_statistics WHERE path = ?",
                [path.to_string_lossy()],
                |row| {
                    Ok(TrackStatistics {
                        added_at: row.get(0)?,
                        play_count: row.get::<_, i64>(1)?.max(0) as u64,
                        last_played: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(|error| error.to_string())
    }

    pub fn listening_history(&self, limit: usize) -> Result<Vec<ListeningHistoryEntry>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT path, played_at, listened_seconds FROM listening_history
                 ORDER BY played_at DESC, id DESC LIMIT ?",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([limit as i64], |row| {
                Ok(ListeningHistoryEntry {
                    path: PathBuf::from(row.get::<_, String>(0)?),
                    played_at: row.get(1)?,
                    listened_seconds: row.get::<_, i64>(2)?.max(0) as u32,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.map(|row| row.map_err(|error| error.to_string()))
            .collect()
    }

    /// Return a sorted path list for a smart playlist, restricted to paths in the
    /// current library snapshot. Ties are resolved by path for repeatable output.
    pub fn smart_playlist(
        &self,
        playlist: SmartPlaylist,
        current_paths: &[PathBuf],
    ) -> Result<Vec<PathBuf>, String> {
        let present: HashSet<String> = current_paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        let mut statement = self
            .connection
            .prepare("SELECT path, added_at, play_count, last_played FROM playback_statistics")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        let mut tracks = rows
            .filter_map(|row| match row {
                Ok((path, added_at, play_count, last_played)) if present.contains(&path) => {
                    Some((path, added_at, play_count, last_played))
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        match playlist {
            SmartPlaylist::RecentlyAdded => {
                tracks.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)))
            }
            SmartPlaylist::MostPlayed => tracks.sort_by(|a, b| {
                // Zero-play tracks belong in Never Played rather than this ranking.
                b.2.cmp(&a.2)
                    .then_with(|| b.3.cmp(&a.3))
                    .then_with(|| a.0.cmp(&b.0))
            }),
            SmartPlaylist::RecentlyPlayed => {
                tracks.retain(|track| track.3.is_some());
                tracks.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.cmp(&b.0)))
            }
            SmartPlaylist::NeverPlayed => {
                tracks.retain(|track| track.2 == 0);
                tracks.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            }
        }
        if playlist == SmartPlaylist::MostPlayed {
            tracks.retain(|track| track.2 > 0);
        }
        Ok(tracks
            .into_iter()
            .map(|(path, _, _, _)| PathBuf::from(path))
            .collect())
    }

    #[cfg(test)]
    fn set_history_limit(&mut self, limit: usize) {
        self.history_limit = limit;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> PathBuf {
        PathBuf::from(format!("/music/{name}.flac"))
    }

    fn store() -> (tempfile::TempDir, StatisticsStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = StatisticsStore::open(&directory.path().join("library.sqlite")).unwrap();
        (directory, store)
    }

    #[test]
    fn statistics_and_history_survive_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("library.sqlite");
        let track = path("one");
        {
            let store = StatisticsStore::open(&database).unwrap();
            store
                .register_library_paths(std::slice::from_ref(&track), 100)
                .unwrap();
            store.record_play(&track, 250, 31).unwrap();
        }
        let store = StatisticsStore::open(&database).unwrap();
        assert_eq!(
            store.track_statistics(&track).unwrap(),
            Some(TrackStatistics {
                added_at: 100,
                play_count: 1,
                last_played: Some(250)
            })
        );
        assert_eq!(
            store.listening_history(10).unwrap(),
            vec![ListeningHistoryEntry {
                path: track,
                played_at: 250,
                listened_seconds: 31,
            }]
        );
    }

    #[test]
    fn re_registration_preserves_added_time_and_smart_lists_are_ordered_and_filtered() {
        let (_directory, store) = store();
        let a = path("a");
        let b = path("b");
        let c = path("c");
        store
            .register_library_paths(&[a.clone(), b.clone(), c.clone()], 100)
            .unwrap();
        store
            .register_library_paths(std::slice::from_ref(&a), 500)
            .unwrap();
        store.record_play(&a, 300, 40).unwrap();
        store.record_play(&a, 400, 45).unwrap();
        store.record_play(&b, 350, 35).unwrap();
        let current = vec![a.clone(), b.clone(), c.clone(), path("removed")];

        assert_eq!(store.track_statistics(&a).unwrap().unwrap().added_at, 100);
        assert_eq!(
            store
                .smart_playlist(SmartPlaylist::RecentlyAdded, &current)
                .unwrap(),
            vec![a.clone(), b.clone(), c.clone()]
        );
        assert_eq!(
            store
                .smart_playlist(SmartPlaylist::MostPlayed, &current)
                .unwrap(),
            vec![a.clone(), b.clone()]
        );
        assert_eq!(
            store
                .smart_playlist(SmartPlaylist::RecentlyPlayed, &current)
                .unwrap(),
            vec![a.clone(), b.clone()]
        );
        assert_eq!(
            store
                .smart_playlist(SmartPlaylist::NeverPlayed, &current)
                .unwrap(),
            vec![c]
        );
    }

    #[test]
    fn history_is_bounded_to_most_recent_entries() {
        let (_directory, mut store) = store();
        store.set_history_limit(2);
        let track = path("one");
        for time in 1..=3 {
            store.record_play(&track, time, 30).unwrap();
        }
        assert_eq!(
            store
                .listening_history(10)
                .unwrap()
                .iter()
                .map(|entry| entry.played_at)
                .collect::<Vec<_>>(),
            vec![3, 2]
        );
        assert_eq!(
            store.track_statistics(&track).unwrap().unwrap().play_count,
            3
        );
    }
}
