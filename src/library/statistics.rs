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

/// A qualified play emitted at most once by a [`ListeningSession`]. The caller
/// supplies the wall-clock `played_at` when persisting this with `record_play`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedPlay {
    pub path: PathBuf,
    pub listened: Duration,
}

#[derive(Debug, Clone, Copy)]
struct ListeningSample {
    timestamp: Duration,
    position: Duration,
    playing: bool,
}

/// Pure playback accounting, independent of the controller, audio backend, and
/// wall clock. Sample timestamps must share one monotonic time origin and be
/// captured when the audio position is sampled, rather than when the UI drains
/// a batch of queued events. The caller rejects samples from other tracks/load
/// generations and creates a fresh session for a new track or explicit restart.
///
/// Credit is the smaller of elapsed sample time and advancing media position,
/// only between two playing samples. Equal/stale timestamps, stalled positions,
/// and backward or implausibly fast position jumps earn nothing. Seek and pause
/// commands must call `seek_reset`/`set_paused` immediately: position alone cannot
/// distinguish a small seek from real playback. The first subsequent sample
/// establishes a new baseline and queued samples predating the boundary are ignored.
///
/// A known duration normally requires `max(3s, min(30s, duration / 2))` of credit;
/// unknown/zero duration requires 30s. For tracks shorter than 3s, only natural
/// completion after at least half the duration in observed credit qualifies.
/// Completion adds no inferred tail or wall time, so a track with no observed
/// advance cannot qualify. If the backend provides a final position sample,
/// observe it before `complete`. An already-qualified session never emits again.
#[derive(Debug, Clone)]
pub struct ListeningSession {
    path: PathBuf,
    duration: Option<Duration>,
    listened: Duration,
    last_sample: Option<ListeningSample>,
    latest_timestamp: Option<Duration>,
    paused: bool,
    counted: bool,
    completed: bool,
}

impl ListeningSession {
    pub fn new(path: PathBuf, duration: Option<Duration>) -> Self {
        Self {
            path,
            duration: duration.filter(|duration| !duration.is_zero()),
            listened: Duration::ZERO,
            last_sample: None,
            latest_timestamp: None,
            paused: false,
            counted: false,
            completed: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Follow a confirmed rename of the currently playing file without creating
    /// another session. Earned credit, pause state, and the count latch survive.
    /// The statistics worker must also migrate the persisted path with
    /// [`StatisticsStore::rename_path`] before recording subsequent plays.
    pub fn rename_path(&mut self, from: &Path, to: &Path) -> bool {
        if self.path != from {
            return false;
        }
        self.path = to.to_path_buf();
        true
    }

    pub fn listened(&self) -> Duration {
        self.listened
    }

    pub fn counted(&self) -> bool {
        self.counted
    }

    /// Observe an authoritative position sample. A false `playing` also breaks
    /// the interval, covering paused/loading/stopped/error status samples.
    pub fn observe(
        &mut self,
        timestamp: Duration,
        position: Duration,
        playing: bool,
    ) -> Option<QualifiedPlay> {
        if self.completed || self.latest_timestamp.is_some_and(|last| timestamp <= last) {
            return None;
        }
        let sample = ListeningSample {
            timestamp,
            position,
            playing: playing && !self.paused,
        };
        if let Some(previous) = self.last_sample
            && previous.playing
            && sample.playing
            && let Some(advance) = position.checked_sub(previous.position)
        {
            let elapsed = timestamp - previous.timestamp;
            // Allow minor backend position jitter, but never credit more than
            // actual elapsed time or the observed advancement. Explicit seeks
            // use seek_reset even if their jump falls inside this tolerance.
            let plausible = elapsed
                .saturating_add(elapsed / 2)
                .saturating_add(Duration::from_millis(250));
            if advance <= plausible {
                self.listened = self.listened.saturating_add(advance.min(elapsed));
            }
        }
        self.latest_timestamp = Some(timestamp);
        self.last_sample = Some(sample);
        let threshold = self.duration.map_or(Duration::from_secs(30), |duration| {
            (duration / 2)
                .min(Duration::from_secs(30))
                .max(Duration::from_secs(3))
        });
        self.qualify(
            self.duration
                .is_none_or(|duration| duration >= Duration::from_secs(3))
                && self.listened >= threshold,
        )
    }

    /// Drop the position baseline at a seek command, before queued progress is
    /// consumed. Does not restart the session or discard already-earned credit.
    pub fn seek_reset(&mut self, timestamp: Duration) {
        self.reset_baseline(timestamp);
    }

    /// Apply a requested pause/resume before the audio worker acknowledges it.
    /// Samples reporting Playing while this gate is paused cannot earn credit.
    pub fn set_paused(&mut self, timestamp: Duration, paused: bool) {
        if paused != self.paused && self.reset_baseline(timestamp) {
            self.paused = paused;
        }
    }

    /// Close a naturally finished session, including a gapless transition.
    /// Never call this for a stop, skip, failed load, or explicit restart. Stale
    /// completions are ignored; a valid completion closes the session even when
    /// it does not qualify. This method never manufactures listening time.
    pub fn complete(&mut self, timestamp: Duration) -> Option<QualifiedPlay> {
        if self.completed || self.latest_timestamp.is_some_and(|last| timestamp < last) {
            return None;
        }
        self.completed = true;
        let short_completed = self.duration.is_some_and(|duration| {
            duration < Duration::from_secs(3)
                && self.listened > Duration::ZERO
                && self.listened >= duration / 2
        }) && !self.paused
            && self.last_sample.is_some_and(|sample| sample.playing);
        self.qualify(short_completed)
    }

    fn reset_baseline(&mut self, timestamp: Duration) -> bool {
        if self.completed || self.latest_timestamp.is_some_and(|last| timestamp < last) {
            return false;
        }
        self.latest_timestamp = Some(timestamp);
        self.last_sample = None;
        true
    }

    fn qualify(&mut self, qualified: bool) -> Option<QualifiedPlay> {
        if self.counted || !qualified {
            return None;
        }
        self.counted = true;
        Some(QualifiedPlay {
            path: self.path.clone(),
            listened: self.listened,
        })
    }
}

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

    /// Move statistics and all retained history together after a confirmed
    /// filesystem rename. Existing destination records (for example, a scan that
    /// registered the new path first) are merged: earliest first-added timestamp,
    /// sum of both counts, and latest non-null last-played timestamp. Neither
    /// history order/IDs nor the history bound changes. Unknown and same-path
    /// renames are harmless, and repeating a successful rename cannot double the
    /// count. Invalid counts or overflow reject the entire transaction.
    pub fn rename_path(&self, from: &Path, to: &Path) -> Result<(), String> {
        if from == to {
            return Ok(());
        }
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let from_text = from.to_string_lossy();
        let to_text = to.to_string_lossy();
        if from_text == to_text {
            return Err("rename paths share a lossy database identity".into());
        }
        let count = |path: &str| {
            transaction
                .query_row(
                    "SELECT play_count FROM playback_statistics WHERE path = ?",
                    [path],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|error| error.to_string())
        };
        if let Some(source_count) = count(&from_text)? {
            let destination_count = count(&to_text)?.unwrap_or(0);
            if source_count < 0 || destination_count < 0 {
                return Err("cannot merge negative playback counts".into());
            }
            source_count
                .checked_add(destination_count)
                .ok_or("merged playback count exceeds SQLite integer range")?;
            transaction
                .execute(
                    "INSERT INTO playback_statistics(path, added_at, play_count, last_played)
                     SELECT ?, added_at, play_count, last_played
                     FROM playback_statistics WHERE path = ?
                     ON CONFLICT(path) DO UPDATE SET
                         added_at = MIN(playback_statistics.added_at, excluded.added_at),
                         play_count = playback_statistics.play_count + excluded.play_count,
                         last_played = COALESCE(
                             MAX(playback_statistics.last_played, excluded.last_played),
                             playback_statistics.last_played, excluded.last_played)",
                    params![to_text, from_text],
                )
                .map_err(|error| error.to_string())?;
        }
        transaction
            .execute(
                "UPDATE listening_history SET path = ? WHERE path = ?",
                params![to_text, from_text],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "DELETE FROM playback_statistics WHERE path = ?",
                [from_text],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())
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
        let updated = transaction
            .execute(
                "UPDATE playback_statistics SET play_count = play_count + 1,
                     last_played = COALESCE(MAX(last_played, ?), ?)
                 WHERE path = ? AND play_count BETWEEN 0 AND 9223372036854775806",
                params![played_at, played_at, path_text],
            )
            .map_err(|e| e.to_string())?;
        if updated != 1 {
            return Err("playback count is invalid or exceeds SQLite integer range".into());
        }
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
            .query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
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
            .map(|row| row.map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        tracks.retain(|track| present.contains(&track.0));

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

    fn session(duration: Option<f64>) -> ListeningSession {
        ListeningSession::new(path("one"), duration.map(Duration::from_secs_f64))
    }

    fn observe(
        session: &mut ListeningSession,
        timestamp: f64,
        position: f64,
        playing: bool,
    ) -> Option<QualifiedPlay> {
        session.observe(
            Duration::from_secs_f64(timestamp),
            Duration::from_secs_f64(position),
            playing,
        )
    }

    #[test]
    fn session_qualifies_once_at_half_known_duration() {
        let mut session = session(Some(20.0));
        assert_eq!(session.path(), path("one"));
        assert!(observe(&mut session, 0.0, 0.0, true).is_none());
        assert!(observe(&mut session, 9.0, 9.0, true).is_none());
        assert_eq!(
            observe(&mut session, 10.0, 10.0, true),
            Some(QualifiedPlay {
                path: path("one"),
                listened: Duration::from_secs(10),
            })
        );
        assert!(session.counted());
        assert!(observe(&mut session, 19.0, 19.0, true).is_none());
        assert!(session.complete(Duration::from_secs(20)).is_none());
        assert!(session.complete(Duration::from_secs(20)).is_none());
        assert!(observe(&mut session, 21.0, 20.0, true).is_none());
    }

    #[test]
    fn unknown_zero_and_long_durations_require_thirty_seconds() {
        for duration in [None, Some(0.0), Some(60.0), Some(600.0)] {
            let mut session = session(duration);
            observe(&mut session, 0.0, 0.0, true);
            assert!(observe(&mut session, 29.0, 29.0, true).is_none());
            assert!(observe(&mut session, 30.0, 30.0, true).is_some());
        }
    }

    #[test]
    fn three_second_floor_applies_to_tracks_at_least_three_seconds_long() {
        for duration in [3.0, 4.0, 5.0] {
            let mut session = session(Some(duration));
            observe(&mut session, 0.0, 0.0, true);
            assert!(observe(&mut session, 2.9, 2.9, true).is_none());
            assert!(observe(&mut session, 3.0, 3.0, true).is_some());
        }
    }

    #[test]
    fn short_track_requires_natural_completion_and_half_duration_of_credit() {
        for duration in [0.002, 0.2, 2.0] {
            let mut session = session(Some(duration));
            observe(&mut session, 0.0, 0.0, true);
            assert!(observe(&mut session, duration / 2.0, duration / 2.0, true).is_none());
            assert!(!session.counted());
            let play = session.complete(Duration::from_secs_f64(duration)).unwrap();
            assert_eq!(play.listened, Duration::from_secs_f64(duration / 2.0));
            assert!(
                session
                    .complete(Duration::from_secs_f64(duration))
                    .is_none()
            );
        }

        let mut skipped = session(Some(2.0));
        observe(&mut skipped, 0.0, 0.0, true);
        observe(&mut skipped, 0.9, 0.9, true);
        assert!(skipped.complete(Duration::from_secs(2)).is_none());
        assert_eq!(skipped.listened(), Duration::from_millis(900));
    }

    #[test]
    fn completion_never_infers_progress_from_elapsed_time() {
        let mut short = session(Some(2.0));
        observe(&mut short, 0.0, 0.0, true);
        for time in 1..=10 {
            observe(&mut short, time as f64, 0.0, true);
        }
        assert!(short.complete(Duration::from_secs(11)).is_none());
        assert_eq!(short.listened(), Duration::ZERO);

        let mut long = session(Some(10.0));
        observe(&mut long, 0.0, 0.0, true);
        observe(&mut long, 4.9, 4.9, true);
        assert!(long.complete(Duration::from_secs(10)).is_none());
        assert_eq!(long.listened(), Duration::from_millis(4900));
    }

    #[test]
    fn credit_is_limited_by_both_elapsed_time_and_position_advance() {
        let mut session = session(None);
        observe(&mut session, 0.0, 0.0, true);
        observe(&mut session, 2.0, 1.0, true);
        assert_eq!(session.listened(), Duration::from_secs(1));
        observe(&mut session, 3.0, 2.2, true);
        assert_eq!(session.listened(), Duration::from_secs(2));
        observe(&mut session, 13.0, 2.2, true);
        assert_eq!(session.listened(), Duration::from_secs(2));
    }

    #[test]
    fn position_discontinuities_rebase_without_earning_credit() {
        let mut session = session(None);
        observe(&mut session, 0.0, 0.0, true);
        observe(&mut session, 1.0, 1.0, true);
        observe(&mut session, 2.0, 200.0, true);
        observe(&mut session, 3.0, 201.0, true);
        observe(&mut session, 4.0, 20.0, true);
        observe(&mut session, 5.0, 21.0, true);
        assert_eq!(session.listened(), Duration::from_secs(3));
    }

    #[test]
    fn seek_reset_excludes_even_small_jumps_and_rejects_queued_samples() {
        let mut session = session(None);
        observe(&mut session, 0.0, 0.0, true);
        observe(&mut session, 1.0, 1.0, true);
        session.seek_reset(Duration::from_millis(1200));
        // An old sample must not rebase to this bogus position.
        observe(&mut session, 1.1, 500.0, true);
        observe(&mut session, 1.2, 500.0, true);
        observe(&mut session, 1.3, 1.6, true);
        assert_eq!(session.listened(), Duration::from_secs(1));
        observe(&mut session, 1.5, 1.8, true);
        assert_eq!(session.listened(), Duration::from_millis(1200));
        session.seek_reset(Duration::from_secs(2));
        observe(&mut session, 2.1, 0.0, true);
        observe(&mut session, 2.3, 0.2, true);
        assert_eq!(session.listened(), Duration::from_millis(1400));
    }

    #[test]
    fn requested_pause_and_resume_ignore_stale_playing_status_and_pause_interval() {
        let mut session = session(None);
        observe(&mut session, 0.0, 0.0, true);
        observe(&mut session, 1.0, 1.0, true);
        session.set_paused(Duration::from_millis(1100), true);
        observe(&mut session, 1.0, 400.0, true);
        observe(&mut session, 2.0, 2.0, true);
        observe(&mut session, 10.0, 10.0, true);
        assert_eq!(session.listened(), Duration::from_secs(1));
        session.set_paused(Duration::from_secs(11), false);
        observe(&mut session, 10.5, 400.0, true);
        observe(&mut session, 11.1, 1.1, true);
        observe(&mut session, 11.3, 1.3, true);
        assert_eq!(session.listened(), Duration::from_millis(1200));
    }

    #[test]
    fn nonplaying_status_samples_break_the_counted_interval() {
        let mut session = session(None);
        observe(&mut session, 0.0, 0.0, true);
        observe(&mut session, 1.0, 1.0, true);
        observe(&mut session, 2.0, 2.0, false);
        observe(&mut session, 50.0, 2.0, false);
        observe(&mut session, 51.0, 3.0, true);
        observe(&mut session, 52.0, 4.0, true);
        assert_eq!(session.listened(), Duration::from_secs(2));
    }

    #[test]
    fn repeated_or_out_of_order_samples_and_stale_boundaries_do_not_rebase() {
        let mut session = session(None);
        observe(&mut session, 0.0, 0.0, true);
        observe(&mut session, 2.0, 2.0, true);
        observe(&mut session, 2.0, 200.0, true);
        observe(&mut session, 1.0, 200.0, false);
        session.set_paused(Duration::from_secs(1), true);
        session.seek_reset(Duration::from_secs(1));
        assert!(session.complete(Duration::from_secs(1)).is_none());
        observe(&mut session, 3.0, 3.0, true);
        assert_eq!(session.listened(), Duration::from_secs(3));
    }

    #[test]
    fn paused_or_unconfirmed_short_completion_cannot_qualify() {
        let mut session = session(Some(2.0));
        observe(&mut session, 0.0, 0.0, true);
        observe(&mut session, 1.0, 1.0, true);
        session.set_paused(Duration::from_millis(1100), true);
        assert!(session.complete(Duration::from_secs(2)).is_none());

        let mut reset = ListeningSession::new(path("one"), Some(Duration::from_secs(2)));
        observe(&mut reset, 0.0, 0.0, true);
        observe(&mut reset, 1.0, 1.0, true);
        reset.seek_reset(Duration::from_millis(1100));
        assert!(reset.complete(Duration::from_secs(2)).is_none());
    }

    #[test]
    fn a_new_session_for_restart_or_another_track_has_a_fresh_latch_and_credit() {
        for track in [path("one"), path("one"), path("two")] {
            let mut session = ListeningSession::new(track.clone(), Some(Duration::from_secs(6)));
            observe(&mut session, 10.0, 0.0, true);
            assert_eq!(session.listened(), Duration::ZERO);
            let play = observe(&mut session, 13.0, 3.0, true).unwrap();
            assert_eq!(play.path, track);
            assert_eq!(play.listened, Duration::from_secs(3));
        }
    }

    #[test]
    fn active_session_rename_preserves_credit_and_count_latch() {
        let mut session = session(Some(6.0));
        observe(&mut session, 0.0, 0.0, true);
        observe(&mut session, 2.0, 2.0, true);
        assert!(!session.rename_path(&path("unrelated"), &path("ignored")));
        assert!(session.rename_path(&path("one"), &path("renamed")));
        assert_eq!(session.listened(), Duration::from_secs(2));
        assert_eq!(
            observe(&mut session, 3.0, 3.0, true).unwrap().path,
            path("renamed")
        );
        assert!(session.rename_path(&path("renamed"), &path("again")));
        assert!(observe(&mut session, 4.0, 4.0, true).is_none());
        assert!(session.counted());
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

    #[test]
    fn rename_preserves_statistics_and_history_across_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("library.sqlite");
        let from = path("before");
        let to = path("after");
        {
            let store = StatisticsStore::open(&database).unwrap();
            store
                .register_library_paths(std::slice::from_ref(&from), 10)
                .unwrap();
            store.record_play(&from, 20, 30).unwrap();
            store.record_play(&from, 30, 40).unwrap();
            store.rename_path(&from, &to).unwrap();
        }
        let store = StatisticsStore::open(&database).unwrap();
        assert!(store.track_statistics(&from).unwrap().is_none());
        assert_eq!(
            store.track_statistics(&to).unwrap(),
            Some(TrackStatistics {
                added_at: 10,
                play_count: 2,
                last_played: Some(30),
            })
        );
        assert_eq!(
            store.listening_history(10).unwrap(),
            vec![
                ListeningHistoryEntry {
                    path: to.clone(),
                    played_at: 30,
                    listened_seconds: 40,
                },
                ListeningHistoryEntry {
                    path: to.clone(),
                    played_at: 20,
                    listened_seconds: 30,
                }
            ]
        );
        let current = [from, to.clone()];
        for kind in [
            SmartPlaylist::RecentlyAdded,
            SmartPlaylist::MostPlayed,
            SmartPlaylist::RecentlyPlayed,
        ] {
            assert_eq!(
                store.smart_playlist(kind, &current).unwrap(),
                vec![to.clone()]
            );
        }
        assert!(
            store
                .smart_playlist(SmartPlaylist::NeverPlayed, &current)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rename_collision_merges_counts_timestamps_and_bounded_history_without_duplicates() {
        let (_directory, mut store) = store();
        store.set_history_limit(3);
        let from = path("before");
        let to = path("after");
        store
            .register_library_paths(std::slice::from_ref(&from), 10)
            .unwrap();
        store
            .register_library_paths(std::slice::from_ref(&to), 5)
            .unwrap();
        store.record_play(&from, 20, 30).unwrap();
        store.record_play(&to, 40, 35).unwrap();
        store.record_play(&from, 30, 40).unwrap();
        store.record_play(&from, 50, 45).unwrap();
        store.rename_path(&from, &to).unwrap();
        store.rename_path(&from, &to).unwrap();
        store.rename_path(&to, &to).unwrap();
        assert_eq!(
            store.track_statistics(&to).unwrap(),
            Some(TrackStatistics {
                added_at: 5,
                play_count: 4,
                last_played: Some(50),
            })
        );
        let history = store.listening_history(100).unwrap();
        assert_eq!(history.len(), 3);
        assert!(history.iter().all(|entry| entry.path == to));
        assert_eq!(
            history
                .iter()
                .map(|entry| entry.played_at)
                .collect::<Vec<_>>(),
            vec![50, 40, 30]
        );
        store.record_play(&to, 60, 30).unwrap();
        assert_eq!(store.listening_history(100).unwrap().len(), 3);
        assert_eq!(store.track_statistics(&to).unwrap().unwrap().play_count, 5);
    }

    #[test]
    fn rename_into_unplayed_record_and_from_unplayed_record_preserves_nonnull_last_played() {
        for source_played in [true, false] {
            let (_directory, store) = store();
            let from = path("before");
            let to = path("after");
            store
                .register_library_paths(std::slice::from_ref(&from), 5)
                .unwrap();
            store
                .register_library_paths(std::slice::from_ref(&to), 10)
                .unwrap();
            store
                .record_play(if source_played { &from } else { &to }, 30, 40)
                .unwrap();
            store.rename_path(&from, &to).unwrap();
            assert_eq!(
                store.track_statistics(&to).unwrap(),
                Some(TrackStatistics {
                    added_at: 5,
                    play_count: 1,
                    last_played: Some(30),
                })
            );
        }
        let (_directory, store) = store();
        store
            .register_library_paths(&[path("before"), path("after")], 10)
            .unwrap();
        store.rename_path(&path("before"), &path("after")).unwrap();
        assert_eq!(
            store
                .track_statistics(&path("after"))
                .unwrap()
                .unwrap()
                .last_played,
            None
        );
        store.rename_path(&path("unknown"), &path("after")).unwrap();
        assert!(store.track_statistics(&path("unknown")).unwrap().is_none());
    }

    #[test]
    fn rename_rolls_back_statistics_if_history_update_fails() {
        let (_directory, store) = store();
        let from = path("before");
        let to = path("after");
        store.record_play(&from, 10, 30).unwrap();
        store.record_play(&to, 20, 30).unwrap();
        let source = store.track_statistics(&from).unwrap();
        let destination = store.track_statistics(&to).unwrap();
        let history = store.listening_history(10).unwrap();
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER reject_history_rename BEFORE UPDATE OF path ON listening_history
             BEGIN SELECT RAISE(ABORT, 'history update failed'); END;",
            )
            .unwrap();
        assert!(store.rename_path(&from, &to).is_err());
        assert_eq!(store.track_statistics(&from).unwrap(), source);
        assert_eq!(store.track_statistics(&to).unwrap(), destination);
        assert_eq!(store.listening_history(10).unwrap(), history);
    }

    #[test]
    fn rename_rejects_count_overflow_without_mutating_either_path() {
        let (_directory, store) = store();
        let from = path("before");
        let to = path("after");
        store.record_play(&from, 10, 30).unwrap();
        store.record_play(&to, 20, 30).unwrap();
        store
            .connection
            .execute(
                "UPDATE playback_statistics SET play_count = ? WHERE path = ?",
                params![i64::MAX, to.to_string_lossy()],
            )
            .unwrap();
        let history = store.listening_history(10).unwrap();
        assert!(store.rename_path(&from, &to).is_err());
        assert_eq!(
            store.track_statistics(&from).unwrap().unwrap().play_count,
            1
        );
        assert_eq!(
            store.track_statistics(&to).unwrap().unwrap().play_count,
            i64::MAX as u64
        );
        assert_eq!(store.listening_history(10).unwrap(), history);
        assert!(store.record_play(&to, 30, 30).is_err());
        assert_eq!(store.listening_history(10).unwrap(), history);
    }

    #[test]
    fn smart_queries_propagate_row_errors_even_for_paths_outside_current_library() {
        let (_directory, store) = store();
        let valid = path("valid");
        store.record_play(&valid, 10, 30).unwrap();
        store.connection.execute(
            "INSERT INTO playback_statistics(path, added_at, play_count) VALUES (?, 'corrupt', 0)",
            [path("removed").to_string_lossy()],
        ).unwrap();
        for kind in SmartPlaylist::ALL {
            assert!(
                store
                    .smart_playlist(kind, std::slice::from_ref(&valid))
                    .is_err()
            );
        }
    }

    #[test]
    fn smart_lists_resolve_all_ranking_ties_by_path_and_exclude_absent_tracks() {
        let (_directory, store) = store();
        let a = path("a");
        let b = path("b");
        let c = path("c");
        let d = path("d");
        let e = path("e");
        let removed = path("removed");
        store
            .register_library_paths(
                &[
                    a.clone(),
                    b.clone(),
                    c.clone(),
                    d.clone(),
                    e.clone(),
                    removed.clone(),
                ],
                10,
            )
            .unwrap();
        store.register_library_paths(&[path("newer")], 20).unwrap();
        for (track, time) in [(&b, 50), (&a, 60), (&c, 50), (&removed, 100)] {
            store.record_play(track, time, 30).unwrap();
        }
        let current = [
            e.clone(),
            c.clone(),
            a.clone(),
            d.clone(),
            b.clone(),
            path("newer"),
            a.clone(),
        ];
        assert_eq!(
            store
                .smart_playlist(SmartPlaylist::RecentlyAdded, &current)
                .unwrap(),
            vec![
                path("newer"),
                a.clone(),
                b.clone(),
                c.clone(),
                d.clone(),
                e.clone()
            ]
        );
        assert_eq!(
            store
                .smart_playlist(SmartPlaylist::MostPlayed, &current)
                .unwrap(),
            vec![a.clone(), b.clone(), c.clone()]
        );
        assert_eq!(
            store
                .smart_playlist(SmartPlaylist::RecentlyPlayed, &current)
                .unwrap(),
            vec![a, b, c]
        );
        assert_eq!(
            store
                .smart_playlist(SmartPlaylist::NeverPlayed, &current)
                .unwrap(),
            vec![path("newer"), d, e]
        );
    }

    #[test]
    fn delayed_play_timestamps_do_not_regress_last_played() {
        let (_directory, store) = store();
        let track = path("one");
        store.record_play(&track, 20, 30).unwrap();
        store.record_play(&track, 10, 30).unwrap();
        assert_eq!(
            store.track_statistics(&track).unwrap().unwrap().last_played,
            Some(20)
        );
        assert_eq!(
            store.track_statistics(&track).unwrap().unwrap().play_count,
            2
        );
    }
}
