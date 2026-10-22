//! Desktop statistics projections over the existing cumulative counts and bounded
//! history. Metadata is supplied by the current library snapshot, not duplicated
//! in a new table. This module never observes or qualifies audio playback.

use super::StatisticsStore;
use chrono::{DateTime, Days, NaiveDate, TimeZone};
use std::{collections::BTreeMap, path::PathBuf};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StatisticsRange {
    #[default]
    AllTime,
    Last7Days,
    Last30Days,
}

impl StatisticsRange {
    pub const ALL: [Self; 3] = [Self::AllTime, Self::Last7Days, Self::Last30Days];

    pub fn label(self) -> &'static str {
        match self {
            Self::AllTime => "All Time",
            Self::Last7Days => "Last 7 Days",
            Self::Last30Days => "Last 30 Days",
        }
    }

    fn days(self) -> Option<u64> {
        match self {
            Self::AllTime => None,
            Self::Last7Days => Some(7),
            Self::Last30Days => Some(30),
        }
    }
}

/// Only inexpensive metadata crosses the statistics worker boundary. Paths in
/// this snapshot are available library entries; absent paths remain in totals
/// and recent history but cannot enter a playable ranking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatisticsTrack {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatisticsArtist {
    pub name: String,
    pub plays: u64,
    /// Exact current-library membership, including spelling/case variants.
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatisticsAlbum {
    pub name: String,
    /// Track artist is the available album identity metadata in this library.
    pub artist: String,
    pub plays: u64,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatisticsRankedTrack {
    pub track: StatisticsTrack,
    pub plays: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatisticsRecentPlay {
    pub track: StatisticsTrack,
    pub played_at: i64,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatisticsDailyPlay {
    pub date: NaiveDate,
    pub plays: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatisticsSnapshot {
    pub total_plays: u64,
    pub top_artists: Vec<StatisticsArtist>,
    pub top_albums: Vec<StatisticsAlbum>,
    pub top_tracks: Vec<StatisticsRankedTrack>,
    /// Retained events in the selected range, newest first, capped at 100 rows.
    /// Repeated plays of one track are deliberately separate rows.
    pub recently_played: Vec<StatisticsRecentPlay>,
    /// Always the last 30 local calendar dates, ascending, including zero days.
    pub daily_plays: Vec<StatisticsDailyPlay>,
    /// True when cumulative counts demonstrate missing retained history. Even
    /// when false, callers should explain the existing history retention cap;
    /// no timestamp watermark can prove completeness after clock changes.
    pub history_limited: bool,
}

impl StatisticsStore {
    /// Query the existing store on its owning worker thread. All Time uses the
    /// durable cumulative counters; date ranges and daily bars use retained
    /// qualified events only. Their calendar dates use `now`'s timezone for every
    /// event (the desktop passes `Local`), rather than SQLite's process-global
    /// localtime or 24-hour subtraction. This also handles DST boundaries.
    ///
    /// Artist/album identity folds canonical Unicode, whitespace, and case only.
    /// Punctuation, accents, and collaboration credits remain distinct. Since
    /// the current Track model has no album artist, album identity is album plus
    /// track artist. Exact member paths allow the existing browser to drill in.
    pub fn listening_statistics<Tz: TimeZone>(
        &self,
        tracks: &[StatisticsTrack],
        range: StatisticsRange,
        now: DateTime<Tz>,
    ) -> Result<StatisticsSnapshot, String> {
        // A deferred read transaction keeps counters and retained events from
        // different connections (for example, a scanner path rename) coherent.
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let snapshot = {
            let today = now.date_naive();
            let first_daily = today
                .checked_sub_days(Days::new(29))
                .ok_or("statistics date is outside the supported range")?;
            let first_range = range
                .days()
                .map(|days| {
                    today
                        .checked_sub_days(Days::new(days - 1))
                        .ok_or("statistics date is outside the supported range")
                })
                .transpose()?;
            let timezone = now.timezone();

            // Read cumulative counters, including paths no longer in the library.
            // Sum in u64 so several individually valid SQLite counters do not cause
            // SQLite's signed-integer SUM overflow.
            let mut statement = transaction
                .prepare("SELECT path, MAX(play_count, 0) FROM playback_statistics")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        PathBuf::from(row.get::<_, String>(0)?),
                        row.get::<_, i64>(1)? as u64,
                    ))
                })
                .map_err(|error| error.to_string())?;
            let all_counts = rows
                .map(|row| row.map_err(|error| error.to_string()))
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            let all_total = sum_counts(all_counts.values().copied())?;
            let retained_count: u64 = transaction
                .query_row("SELECT COUNT(*) FROM listening_history", [], |row| {
                    row.get::<_, i64>(0).map(|count| count as u64)
                })
                .map_err(|error| error.to_string())?;

            // SQLite aggregates duplicate timestamps without using its UTC date()
            // function. The existing 1,000-event retention bound keeps conversion to
            // actual local calendar dates cheap and avoids ambiguous/nonexistent
            // local-midnight calculations when time zones change clocks.
            let mut daily_counts = BTreeMap::new();
            let mut range_counts = BTreeMap::<PathBuf, u64>::new();
            let mut statement = transaction
                .prepare(
                    "SELECT path, played_at, COUNT(*) FROM listening_history
                 WHERE played_at <= ? GROUP BY path, played_at",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([now.timestamp()], |row| {
                    Ok((
                        PathBuf::from(row.get::<_, String>(0)?),
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)? as u64,
                    ))
                })
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (path, played_at, count) = row.map_err(|error| error.to_string())?;
                let Some(played) = timezone.timestamp_opt(played_at, 0).single() else {
                    continue;
                };
                let date = played.date_naive();
                if date >= first_daily && date <= today {
                    add_count(daily_counts.entry(date).or_default(), count)?;
                }
                if first_range.is_some_and(|first| date >= first && date <= today) {
                    add_count(range_counts.entry(path).or_default(), count)?;
                }
            }
            let counts = if range == StatisticsRange::AllTime {
                &all_counts
            } else {
                &range_counts
            };
            let total_plays = sum_counts(counts.values().copied())?;

            // A duplicate path in an input snapshot must never double-count. Pick a
            // deterministic metadata representative independent of input ordering.
            let mut metadata = BTreeMap::<PathBuf, StatisticsTrack>::new();
            for track in tracks {
                metadata
                    .entry(track.path.clone())
                    .and_modify(|existing| {
                        if (&track.title, &track.artist, &track.album)
                            < (&existing.title, &existing.artist, &existing.album)
                        {
                            *existing = track.clone();
                        }
                    })
                    .or_insert_with(|| track.clone());
            }
            let mut artists = BTreeMap::<String, StatisticsArtist>::new();
            let mut albums = BTreeMap::<(String, String), StatisticsAlbum>::new();
            let mut top_tracks = Vec::new();
            for (path, track) in &metadata {
                let artist_name = clean_name(&track.artist);
                let album_name = clean_name(&track.album);
                let artist_key = identity(&artist_name);
                let album_key = (identity(&album_name), artist_key.clone());
                let artist = artists
                    .entry(artist_key)
                    .or_insert_with(|| StatisticsArtist {
                        name: artist_name.clone(),
                        plays: 0,
                        paths: Vec::new(),
                    });
                artist.name = artist.name.clone().min(artist_name.clone());
                artist.paths.push(path.clone());
                let album = albums.entry(album_key).or_insert_with(|| StatisticsAlbum {
                    name: album_name.clone(),
                    artist: artist_name.clone(),
                    plays: 0,
                    paths: Vec::new(),
                });
                album.name = album.name.clone().min(album_name);
                album.artist = album.artist.clone().min(artist_name);
                album.paths.push(path.clone());
                let plays = counts.get(path).copied().unwrap_or_default();
                add_count(&mut artist.plays, plays)?;
                add_count(&mut album.plays, plays)?;
                if plays > 0 {
                    top_tracks.push(StatisticsRankedTrack {
                        track: track.clone(),
                        plays,
                    });
                }
            }
            let mut top_artists: Vec<_> = artists
                .into_iter()
                .filter(|(_, artist)| artist.plays > 0)
                .collect();
            top_artists.sort_by(|(key_a, a), (key_b, b)| {
                b.plays.cmp(&a.plays).then_with(|| key_a.cmp(key_b))
            });
            let mut top_albums: Vec<_> = albums
                .into_iter()
                .filter(|(_, album)| album.plays > 0)
                .collect();
            top_albums.sort_by(|(key_a, a), (key_b, b)| {
                b.plays.cmp(&a.plays).then_with(|| key_a.cmp(key_b))
            });
            top_tracks.sort_by(|a, b| {
                b.plays
                    .cmp(&a.plays)
                    .then_with(|| identity(&a.track.title).cmp(&identity(&b.track.title)))
                    .then_with(|| identity(&a.track.artist).cmp(&identity(&b.track.artist)))
                    .then_with(|| identity(&a.track.album).cmp(&identity(&b.track.album)))
                    .then_with(|| a.track.path.cmp(&b.track.path))
            });

            let mut recently_played = Vec::new();
            let mut statement = transaction
                .prepare(
                    "SELECT path, played_at FROM listening_history WHERE played_at <= ?
                 ORDER BY played_at DESC, id DESC",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([now.timestamp()], |row| {
                    Ok((
                        PathBuf::from(row.get::<_, String>(0)?),
                        row.get::<_, i64>(1)?,
                    ))
                })
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (path, played_at) = row.map_err(|error| error.to_string())?;
                if let Some(first) = first_range {
                    let Some(played) = timezone.timestamp_opt(played_at, 0).single() else {
                        continue;
                    };
                    if played.date_naive() < first || played.date_naive() > today {
                        continue;
                    }
                }
                let available = metadata.contains_key(&path);
                let track = metadata.get(&path).cloned().unwrap_or_else(|| {
                    let title = path
                        .file_stem()
                        .or_else(|| path.file_name())
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.to_string_lossy().into_owned());
                    StatisticsTrack {
                        path,
                        title,
                        artist: String::new(),
                        album: String::new(),
                    }
                });
                recently_played.push(StatisticsRecentPlay {
                    track,
                    played_at,
                    available,
                });
                if recently_played.len() == 100 {
                    break;
                }
            }
            let daily_plays = (0..30)
                .map(|day| {
                    let date = first_daily + Days::new(day);
                    StatisticsDailyPlay {
                        date,
                        plays: daily_counts.get(&date).copied().unwrap_or_default(),
                    }
                })
                .collect();
            StatisticsSnapshot {
                total_plays,
                top_artists: top_artists.into_iter().map(|(_, artist)| artist).collect(),
                top_albums: top_albums.into_iter().map(|(_, album)| album).collect(),
                top_tracks,
                recently_played,
                daily_plays,
                history_limited: all_total > retained_count,
            }
        };
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(snapshot)
    }
}

fn clean_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .nfc()
        .collect()
}

fn identity(name: &str) -> String {
    clean_name(name).to_lowercase().nfc().collect()
}

/// Reuse the ranking identity when browsing an artist's albums in the UI.
pub fn statistics_identity(name: &str) -> String {
    identity(name)
}

fn add_count(total: &mut u64, count: u64) -> Result<(), String> {
    *total = total
        .checked_add(count)
        .ok_or("statistics count overflow")?;
    Ok(())
}

fn sum_counts(counts: impl IntoIterator<Item = u64>) -> Result<u64, String> {
    let mut total = 0;
    for count in counts {
        add_count(&mut total, count)?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, NaiveDateTime, Offset, Utc};
    use rusqlite::params;

    fn store() -> (tempfile::TempDir, StatisticsStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = StatisticsStore::open(&directory.path().join("statistics.sqlite")).unwrap();
        (directory, store)
    }

    fn track(name: &str, title: &str, artist: &str, album: &str) -> StatisticsTrack {
        StatisticsTrack {
            path: PathBuf::from(format!("/music/{name}.flac")),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
        }
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 22, 12, 0, 0).unwrap()
    }

    fn play(store: &StatisticsStore, track: &StatisticsTrack, timestamp: i64, count: usize) {
        for _ in 0..count {
            store.record_play(&track.path, timestamp, 30).unwrap();
        }
    }

    fn query(store: &StatisticsStore, tracks: &[StatisticsTrack]) -> StatisticsSnapshot {
        store
            .listening_statistics(tracks, StatisticsRange::AllTime, now())
            .unwrap()
    }

    #[test]
    fn empty_and_zero_play_libraries_have_empty_rankings_and_thirty_zero_days() {
        let (_directory, store) = store();
        let empty = query(&store, &[]);
        assert_eq!(empty.total_plays, 0);
        assert!(empty.top_artists.is_empty());
        assert!(empty.top_albums.is_empty());
        assert!(empty.top_tracks.is_empty());
        assert!(empty.recently_played.is_empty());
        assert!(!empty.history_limited);
        assert_eq!(empty.daily_plays.len(), 30);
        assert_eq!(empty.daily_plays[0].date.to_string(), "2026-09-23");
        assert_eq!(empty.daily_plays[29].date, now().date_naive());
        assert!(empty.daily_plays.iter().all(|day| day.plays == 0));

        let one = track("one", "Song", "Artist", "Album");
        store
            .register_library_paths(std::slice::from_ref(&one.path), now().timestamp())
            .unwrap();
        for range in StatisticsRange::ALL {
            assert_eq!(
                store
                    .listening_statistics(std::slice::from_ref(&one), range, now())
                    .unwrap(),
                empty
            );
        }
    }

    #[test]
    fn artist_identity_normalizes_only_case_canonical_unicode_and_whitespace() {
        let (_directory, store) = store();
        let tracks = vec![
            track("one", "One", "  Bjo\u{308}rk  ", "Debut"),
            track("two", "Two", "BJÖRK", "Debut"),
            track("three", "Three", "Björk & Guest", "Debut"),
            track("four", "Four", "Bjork", "Debut"),
            track("five", "Five", "A  Band", "Album"),
            track("six", "Six", " a\tband ", "Album"),
        ];
        for track in &tracks {
            play(&store, track, now().timestamp(), 1);
        }
        let snapshot = query(&store, &tracks);
        assert_eq!(snapshot.total_plays, 6);
        assert_eq!(snapshot.top_artists.len(), 4);
        assert_eq!(snapshot.top_artists[0].name, "A Band");
        assert_eq!(snapshot.top_artists[0].plays, 2);
        assert_eq!(snapshot.top_artists[1].name, "BJÖRK");
        assert_eq!(snapshot.top_artists[1].plays, 2);
        assert_eq!(
            snapshot.top_artists[1].paths,
            vec![tracks[0].path.clone(), tracks[1].path.clone()]
        );
    }

    #[test]
    fn album_identity_keeps_different_artists_same_title_separate() {
        let (_directory, store) = store();
        let tracks = vec![
            track("a", "Song A", "Artist A", "Greatest Hits"),
            track("b", "Song B", "Artist B", "Greatest Hits"),
            track("c", "Song C", " artist a ", " greatest  hits "),
            track("unplayed", "Song D", "Artist A", "Greatest Hits"),
        ];
        play(&store, &tracks[0], now().timestamp(), 2);
        play(&store, &tracks[1], now().timestamp(), 2);
        play(&store, &tracks[2], now().timestamp(), 1);
        let snapshot = query(&store, &tracks);
        assert_eq!(snapshot.top_albums.len(), 2);
        assert_eq!(snapshot.top_albums[0].artist, "Artist A");
        assert_eq!(snapshot.top_albums[0].plays, 3);
        assert_eq!(snapshot.top_albums[0].paths.len(), 3);
        assert_eq!(snapshot.top_albums[1].artist, "Artist B");
        assert_eq!(snapshot.top_albums[1].plays, 2);
        assert_eq!(snapshot.top_artists[0].plays, 3);
    }

    #[test]
    fn ties_are_stable_independent_of_metadata_and_database_insertion_order() {
        let (_directory, store) = store();
        let mut tracks = vec![
            track("z", "Same", "B Artist", "B Album"),
            track("b", "Same", "A Artist", "A Album"),
            track("a", "Same", "A Artist", "A Album"),
            track("c", "Earlier", "C Artist", "C Album"),
        ];
        for track in &tracks {
            play(&store, track, now().timestamp(), 1);
        }
        let first = query(&store, &tracks);
        tracks.reverse();
        assert_eq!(query(&store, &tracks), first);
        assert_eq!(first.top_tracks[0].track.title, "Earlier");
        assert_eq!(
            first.top_tracks[1].track.path,
            PathBuf::from("/music/a.flac")
        );
        assert_eq!(
            first.top_tracks[2].track.path,
            PathBuf::from("/music/b.flac")
        );
        assert_eq!(first.top_artists[0].name, "A Artist");
        assert_eq!(first.top_artists[1].name, "B Artist");
        assert_eq!(first.top_artists[2].name, "C Artist");
        assert_eq!(first.top_albums[0].name, "A Album");
    }

    #[test]
    fn duplicate_metadata_paths_do_not_double_count_or_change_output() {
        let (_directory, store) = store();
        let a = track("one", "A Title", "A Artist", "A Album");
        let mut other = a.clone();
        other.title = "Z Title".into();
        play(&store, &a, now().timestamp(), 2);
        assert_eq!(
            query(&store, &[other.clone(), a.clone()]),
            query(&store, std::slice::from_ref(&a))
        );
        assert_eq!(query(&store, &[a.clone(), other]), query(&store, &[a]));
    }

    #[test]
    fn unavailable_paths_remain_in_totals_and_recent_events_but_not_rankings() {
        let (_directory, store) = store();
        let available = track("available", "Available", "Artist", "Album");
        let removed = track("removed", "Old Title", "Old Artist", "Old Album");
        play(&store, &available, now().timestamp() - 1, 1);
        play(&store, &removed, now().timestamp(), 2);
        for range in StatisticsRange::ALL {
            let snapshot = store
                .listening_statistics(std::slice::from_ref(&available), range, now())
                .unwrap();
            assert_eq!(snapshot.total_plays, 3);
            assert_eq!(snapshot.top_tracks.len(), 1);
            assert_eq!(snapshot.top_artists[0].plays, 1);
            assert_eq!(snapshot.recently_played.len(), 3);
            assert!(!snapshot.recently_played[0].available);
            assert_eq!(snapshot.recently_played[0].track.title, "removed");
            assert!(snapshot.recently_played[2].available);
        }
    }

    #[test]
    fn retention_is_reported_without_changing_cumulative_or_smart_playlist_counts() {
        let (_directory, store) = store();
        let one = track("one", "One", "Artist", "Album");
        // Populate the same qualified-event tables in one transaction, then let
        // record_play exercise the production 1,000-row cleanup boundary.
        let transaction = store.connection.unchecked_transaction().unwrap();
        transaction
            .execute(
                "INSERT INTO playback_statistics(path, added_at, play_count, last_played)
                 VALUES (?, ?, 1004, ?)",
                params![
                    one.path.to_string_lossy(),
                    now().timestamp(),
                    now().timestamp()
                ],
            )
            .unwrap();
        for timestamp in now().timestamp() - 1004..now().timestamp() {
            transaction
                .execute(
                    "INSERT INTO listening_history(path, played_at, listened_seconds) VALUES (?, ?, 30)",
                    params![one.path.to_string_lossy(), timestamp],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
        play(&store, &one, now().timestamp(), 1);
        let all = query(&store, std::slice::from_ref(&one));
        assert_eq!(all.total_plays, 1005);
        assert_eq!(all.top_tracks[0].plays, 1005);
        assert!(all.history_limited);
        assert_eq!(all.daily_plays[29].plays, 1000);
        assert_eq!(all.recently_played.len(), 100);
        assert_eq!(store.listening_history(2000).unwrap().len(), 1000);
        for range in [StatisticsRange::Last7Days, StatisticsRange::Last30Days] {
            let bounded = store
                .listening_statistics(std::slice::from_ref(&one), range, now())
                .unwrap();
            assert_eq!(bounded.total_plays, 1000);
            assert_eq!(bounded.top_tracks[0].plays, 1000);
            assert!(bounded.history_limited);
        }
        assert_eq!(
            store
                .smart_playlist(
                    super::super::SmartPlaylist::MostPlayed,
                    std::slice::from_ref(&one.path)
                )
                .unwrap(),
            vec![one.path.clone()]
        );
        assert_eq!(
            store
                .track_statistics(&one.path)
                .unwrap()
                .unwrap()
                .play_count,
            1005
        );
    }

    #[test]
    fn exact_seven_and_thirty_local_calendar_date_boundaries_exclude_future_events() {
        let (_directory, store) = store();
        let one = track("one", "One", "Artist", "Album");
        let timezone = FixedOffset::east_opt(8 * 3600).unwrap();
        let local_now = timezone.with_ymd_and_hms(2026, 10, 22, 0, 30, 0).unwrap();
        let first_seven = timezone.with_ymd_and_hms(2026, 10, 16, 0, 0, 0).unwrap();
        let first_thirty = timezone.with_ymd_and_hms(2026, 9, 23, 0, 0, 0).unwrap();
        for timestamp in [
            first_thirty.timestamp() - 1,
            first_thirty.timestamp(),
            first_seven.timestamp() - 1,
            first_seven.timestamp(),
            local_now.timestamp(),
            local_now.timestamp() + 1,
        ] {
            play(&store, &one, timestamp, 1);
        }
        let seven = store
            .listening_statistics(
                std::slice::from_ref(&one),
                StatisticsRange::Last7Days,
                local_now,
            )
            .unwrap();
        assert_eq!(seven.total_plays, 2);
        assert_eq!(seven.recently_played.len(), 2);
        assert_eq!(seven.daily_plays[0].plays, 1);
        assert_eq!(
            seven.daily_plays.iter().map(|day| day.plays).sum::<u64>(),
            4
        );
        let thirty = store
            .listening_statistics(
                std::slice::from_ref(&one),
                StatisticsRange::Last30Days,
                local_now,
            )
            .unwrap();
        assert_eq!(thirty.total_plays, 4);
        let all = store
            .listening_statistics(&[one], StatisticsRange::AllTime, local_now)
            .unwrap();
        // Durable cumulative counters preserve their existing all-time meaning,
        // while a future wall-clock timestamp cannot leak into dated views.
        assert_eq!(all.total_plays, 6);
        assert_eq!(all.recently_played.len(), 5);
    }

    #[test]
    fn event_dates_follow_the_requested_timezone_across_utc_midnight() {
        let (_directory, store) = store();
        let one = track("one", "One", "Artist", "Album");
        let event = Utc.with_ymd_and_hms(2026, 10, 21, 11, 59, 59).unwrap();
        play(&store, &one, event.timestamp(), 1);
        for (offset, expected_date) in [(14, "2026-10-22"), (-12, "2026-10-20")] {
            let timezone = FixedOffset::east_opt(offset * 3600).unwrap();
            let snapshot = store
                .listening_statistics(
                    std::slice::from_ref(&one),
                    StatisticsRange::Last7Days,
                    (event + chrono::Duration::hours(13)).with_timezone(&timezone),
                )
                .unwrap();
            let day = snapshot
                .daily_plays
                .iter()
                .find(|day| day.plays == 1)
                .unwrap();
            assert_eq!(day.date.to_string(), expected_date);
            assert_eq!(snapshot.total_plays, 1);
        }
    }

    /// Minimal real-world US spring-forward rule without a timezone dependency.
    /// Only conversion from UTC is used by the query; the local-time methods
    /// still describe the skipped 02:00 hour for the TimeZone contract.
    #[derive(Clone)]
    struct SpringForward;

    impl TimeZone for SpringForward {
        type Offset = FixedOffset;

        fn from_offset(_: &Self::Offset) -> Self {
            Self
        }

        fn offset_from_local_date(
            &self,
            local: &NaiveDate,
        ) -> chrono::MappedLocalTime<Self::Offset> {
            self.offset_from_local_datetime(&local.and_hms_opt(12, 0, 0).unwrap())
        }

        fn offset_from_local_datetime(
            &self,
            local: &NaiveDateTime,
        ) -> chrono::MappedLocalTime<Self::Offset> {
            let before = NaiveDate::from_ymd_opt(2025, 3, 9)
                .unwrap()
                .and_hms_opt(2, 0, 0)
                .unwrap();
            let after = before + chrono::Duration::hours(1);
            if local >= &before && local < &after {
                chrono::MappedLocalTime::None
            } else {
                chrono::MappedLocalTime::Single(
                    FixedOffset::west_opt(if local < &before { 5 * 3600 } else { 4 * 3600 })
                        .unwrap(),
                )
            }
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> Self::Offset {
            self.offset_from_utc_datetime(&utc.and_hms_opt(12, 0, 0).unwrap())
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
            let transition = NaiveDate::from_ymd_opt(2025, 3, 9)
                .unwrap()
                .and_hms_opt(7, 0, 0)
                .unwrap();
            FixedOffset::west_opt(if utc < &transition {
                5 * 3600
            } else {
                4 * 3600
            })
            .unwrap()
        }
    }

    #[test]
    fn calendar_range_preserves_the_first_hour_when_dst_changes_day_length() {
        let (_directory, store) = store();
        let one = track("one", "One", "Artist", "Album");
        // Seven local dates include March 8, whose offset predates the switch.
        let local_now = SpringForward
            .with_ymd_and_hms(2025, 3, 14, 0, 30, 0)
            .unwrap();
        assert_eq!(local_now.offset().fix().local_minus_utc(), -4 * 3600);
        let first = SpringForward.with_ymd_and_hms(2025, 3, 8, 0, 0, 0).unwrap();
        play(&store, &one, first.timestamp() - 1, 1);
        play(&store, &one, first.timestamp(), 1);
        play(&store, &one, first.timestamp() + 1800, 1);
        let snapshot = store
            .listening_statistics(&[one], StatisticsRange::Last7Days, local_now)
            .unwrap();
        assert_eq!(snapshot.total_plays, 2);
        assert_eq!(snapshot.recently_played.len(), 2);
    }

    #[test]
    fn recent_history_orders_equal_timestamps_by_newest_insert_and_retains_repeats() {
        let (_directory, store) = store();
        let a = track("a", "A", "Artist", "Album");
        let b = track("b", "B", "Artist", "Album");
        play(&store, &a, now().timestamp(), 2);
        play(&store, &b, now().timestamp(), 1);
        let snapshot = query(&store, &[a.clone(), b.clone()]);
        let paths: Vec<_> = snapshot
            .recently_played
            .iter()
            .map(|entry| entry.track.path.clone())
            .collect();
        assert_eq!(paths, vec![b.path, a.path.clone(), a.path]);
    }

    #[test]
    fn clock_rollback_retention_is_not_mistaken_for_complete_recent_history() {
        let (_directory, mut store) = store();
        store.set_history_limit(2);
        let one = track("one", "One", "Artist", "Album");
        play(&store, &one, now().timestamp(), 1);
        play(&store, &one, now().timestamp() - 86400 * 90, 2);
        let snapshot = store
            .listening_statistics(&[one], StatisticsRange::Last7Days, now())
            .unwrap();
        assert_eq!(snapshot.total_plays, 0);
        assert!(snapshot.history_limited);
        assert!(snapshot.recently_played.is_empty());
    }

    #[test]
    fn query_failure_can_recover_without_changing_playback_statistics() {
        let (_directory, store) = store();
        let one = track("one", "One", "Artist", "Album");
        play(&store, &one, now().timestamp(), 1);
        store
            .connection
            .execute_batch("ALTER TABLE listening_history RENAME TO saved_history;")
            .unwrap();
        assert!(
            store
                .listening_statistics(std::slice::from_ref(&one), StatisticsRange::AllTime, now())
                .is_err()
        );
        store
            .connection
            .execute_batch("ALTER TABLE saved_history RENAME TO listening_history;")
            .unwrap();
        assert_eq!(query(&store, std::slice::from_ref(&one)).total_plays, 1);
        play(&store, &one, now().timestamp(), 1);
        assert_eq!(query(&store, &[one]).total_plays, 2);
    }
}
