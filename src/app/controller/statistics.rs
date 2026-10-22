//! Application statistics persistence and optimistic rating coordination.
//!
//! The worker owns its SQLite connection. The controller keeps the UI projection
//! and listening samples, and consumes these events without sharing either with
//! the worker.

use crate::library::statistics::{QualifiedPlay, SmartPlaylist, StatisticsStore};
#[cfg(not(any(target_os = "ios", feature = "mobile")))]
use crate::library::statistics::{StatisticsRange, StatisticsSnapshot, StatisticsTrack};
#[cfg(not(any(target_os = "ios", feature = "mobile")))]
use chrono::Offset;

#[cfg(not(any(target_os = "ios", feature = "mobile")))]
#[derive(Clone, Copy, PartialEq, Eq)]
struct QueryKey {
    range: StatisticsRange,
    revision: u64,
    date: chrono::NaiveDate,
    offset: i32,
}

#[cfg(not(any(target_os = "ios", feature = "mobile")))]
#[derive(Default)]
struct Queries {
    revision: std::cell::Cell<u64>,
    next_request: u64,
    attempted: Option<QueryKey>,
    pending: Option<(u64, QueryKey)>,
    cache: HashMap<StatisticsRange, (QueryKey, StatisticsSnapshot)>,
}

#[cfg(not(any(target_os = "ios", feature = "mobile")))]
impl Queries {
    fn key(&self, range: StatisticsRange) -> QueryKey {
        let now = chrono::Local::now();
        QueryKey {
            range,
            revision: self.revision.get(),
            date: now.date_naive(),
            offset: now.offset().fix().local_minus_utc(),
        }
    }
}

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

enum Command {
    #[cfg(not(any(target_os = "ios", feature = "mobile")))]
    Query {
        request: u64,
        key: QueryKey,
        tracks: Vec<StatisticsTrack>,
        now: chrono::DateTime<chrono::Local>,
    },
    Library(Vec<PathBuf>),
    Rate {
        path: PathBuf,
        stars: u8,
        request: u64,
    },
    Shutdown,
    Played {
        path: PathBuf,
        played_at: i64,
        listened_seconds: u32,
    },
}

enum WorkerEvent {
    #[cfg(not(any(target_os = "ios", feature = "mobile")))]
    Statistics {
        request: u64,
        key: QueryKey,
        result: Result<StatisticsSnapshot, String>,
    },
    RatingsLoaded(HashMap<PathBuf, u8>),
    RatingSaved {
        path: PathBuf,
        stars: u8,
        request: u64,
        result: Result<(), String>,
    },
    SmartPlaylists(HashMap<SmartPlaylist, Vec<PathBuf>>),
    Error(String),
}

pub(super) enum Event {
    #[cfg(not(any(target_os = "ios", feature = "mobile")))]
    Statistics {
        range: StatisticsRange,
        snapshot: StatisticsSnapshot,
    },
    #[cfg(not(any(target_os = "ios", feature = "mobile")))]
    StatisticsFailed {
        range: StatisticsRange,
        error: String,
    },
    RatingsLoaded(HashMap<PathBuf, u8>),
    RatingReverted {
        path: PathBuf,
        stars: u8,
    },
    SmartPlaylists(HashMap<SmartPlaylist, Vec<PathBuf>>),
    Unavailable(String),
}

pub(super) struct StatisticsService {
    #[cfg(not(any(target_os = "ios", feature = "mobile")))]
    queries: Queries,
    commands: Sender<Command>,
    events: Receiver<WorkerEvent>,
    worker: Option<JoinHandle<()>>,
    pending_ratings: HashMap<PathBuf, (u64, u8)>,
    rating_request: u64,
}

impl StatisticsService {
    pub(super) fn new(database: PathBuf) -> Self {
        let (commands, receiver) = mpsc::channel();
        let (events, event_receiver) = mpsc::channel();
        let worker = thread::spawn(move || run_worker(database, receiver, events));
        Self {
            #[cfg(not(any(target_os = "ios", feature = "mobile")))]
            queries: Queries::default(),
            commands,
            events: event_receiver,
            worker: Some(worker),
            pending_ratings: HashMap::new(),
            rating_request: 0,
        }
    }

    #[cfg(not(any(target_os = "ios", feature = "mobile")))]
    pub(super) fn needs_query(&self, range: StatisticsRange) -> bool {
        self.queries.attempted != Some(self.queries.key(range))
    }

    /// Cache by library/play revision and local date/offset. An attempted failure
    /// stays idle until navigation or explicit Refresh, avoiding poll-loop retries.
    #[cfg(not(any(target_os = "ios", feature = "mobile")))]
    pub(super) fn query(
        &mut self,
        range: StatisticsRange,
        tracks: Vec<StatisticsTrack>,
        force: bool,
    ) -> Result<Option<StatisticsSnapshot>, String> {
        let key = self.queries.key(range);
        self.queries.attempted = Some(key);
        if !force {
            if self
                .queries
                .pending
                .is_some_and(|(_, pending)| pending == key)
            {
                return Ok(None);
            }
            if let Some((cached_key, snapshot)) = self.queries.cache.get(&range)
                && *cached_key == key
            {
                self.queries.pending = None;
                return Ok(Some(snapshot.clone()));
            }
        } else {
            self.queries.cache.remove(&range);
        }
        self.queries.next_request = self.queries.next_request.wrapping_add(1);
        let request = self.queries.next_request;
        self.queries.pending = Some((request, key));
        if self
            .commands
            .send(Command::Query {
                request,
                key,
                tracks,
                now: chrono::Local::now(),
            })
            .is_err()
        {
            self.queries.pending = None;
            return Err("Statistics worker disconnected".into());
        }
        Ok(None)
    }

    #[cfg(not(any(target_os = "ios", feature = "mobile")))]
    pub(super) fn invalidate(&self) {
        self.queries
            .revision
            .set(self.queries.revision.get().wrapping_add(1));
    }

    pub(super) fn set_library(&self, paths: Vec<PathBuf>) {
        #[cfg(not(any(target_os = "ios", feature = "mobile")))]
        self.invalidate();
        let _ = self.commands.send(Command::Library(paths));
    }

    pub(super) fn record_play(&self, play: QualifiedPlay) {
        #[cfg(not(any(target_os = "ios", feature = "mobile")))]
        self.invalidate();
        let _ = self.commands.send(Command::Played {
            path: play.path,
            played_at: chrono::Utc::now().timestamp(),
            listened_seconds: play.listened.as_secs().min(u32::MAX as u64) as u32,
        });
    }

    /// Queue an optimistic edit, returning the rollback value if the worker is
    /// unavailable. During a burst, the fallback remains the last confirmed edit.
    pub(super) fn rate(&mut self, path: &Path, stars: u8, old: u8) -> Result<(), u8> {
        self.rating_request = self.rating_request.wrapping_add(1);
        let previous = self
            .pending_ratings
            .get(path)
            .map_or(old, |(_, previous)| *previous);
        let request = self.rating_request;
        self.pending_ratings
            .insert(path.to_path_buf(), (request, previous));
        if self
            .commands
            .send(Command::Rate {
                path: path.to_path_buf(),
                stars,
                request,
            })
            .is_err()
        {
            self.pending_ratings.remove(path);
            Err(previous)
        } else {
            Ok(())
        }
    }

    /// Consume acknowledgements internally and expose only UI-relevant changes.
    /// Pending paths overlay snapshots only while they remain in the controller's
    /// projection; a refresh may already have removed a path there.
    pub(super) fn poll(&mut self, current_ratings: &HashMap<PathBuf, u8>) -> Option<Event> {
        while let Ok(event) = self.events.try_recv() {
            match event {
                #[cfg(not(any(target_os = "ios", feature = "mobile")))]
                WorkerEvent::Statistics {
                    request,
                    key,
                    result,
                } => {
                    if self.queries.pending != Some((request, key)) {
                        continue;
                    }
                    self.queries.pending = None;
                    if key != self.queries.key(key.range) {
                        self.queries.attempted = None;
                        continue;
                    }
                    return Some(match result {
                        Ok(snapshot) => {
                            self.queries
                                .cache
                                .insert(key.range, (key, snapshot.clone()));
                            Event::Statistics {
                                range: key.range,
                                snapshot,
                            }
                        }
                        Err(error) => Event::StatisticsFailed {
                            range: key.range,
                            error,
                        },
                    });
                }
                WorkerEvent::RatingsLoaded(mut ratings) => {
                    for path in self.pending_ratings.keys() {
                        if let Some(stars) = current_ratings.get(path) {
                            ratings.insert(path.clone(), *stars);
                        }
                    }
                    return Some(Event::RatingsLoaded(ratings));
                }
                WorkerEvent::RatingSaved {
                    path,
                    stars,
                    request,
                    result,
                } => {
                    if let Some((expected, previous)) = self.pending_ratings.get(&path).copied() {
                        if request == expected {
                            self.pending_ratings.remove(&path);
                            if result.is_err() {
                                return Some(Event::RatingReverted {
                                    path,
                                    stars: previous,
                                });
                            }
                        } else if request < expected
                            && result.is_ok()
                            && let Some(pending) = self.pending_ratings.get_mut(&path)
                        {
                            pending.1 = stars;
                        }
                    }
                }
                WorkerEvent::SmartPlaylists(playlists) => {
                    return Some(Event::SmartPlaylists(playlists));
                }
                WorkerEvent::Error(error) => return Some(Event::Unavailable(error)),
            }
        }
        None
    }

    /// FIFO shutdown drains queued ratings and listening records before joining.
    /// The controller calls this before joining its playlist persistence worker.
    pub(super) fn shutdown(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = self.commands.send(Command::Shutdown);
            let _ = worker.join();
        }
    }
}

impl Drop for StatisticsService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(not(any(target_os = "ios", feature = "mobile")))]
fn run_worker(database: PathBuf, receiver: Receiver<Command>, events: Sender<WorkerEvent>) {
    // Keep the worker alive after an open failure. A later explicit request can
    // reopen a repaired/unlocked database without restarting music playback.
    let mut store = match StatisticsStore::open(&database) {
        Ok(store) => Some(store),
        Err(error) => {
            let _ = events.send(WorkerEvent::Error(error));
            None
        }
    };
    if let Some(store) = &store {
        match store.ratings() {
            Ok(ratings) => {
                let _ = events.send(WorkerEvent::RatingsLoaded(ratings));
            }
            Err(error) => {
                let _ = events.send(WorkerEvent::Error(error));
            }
        }
    }
    let mut current_paths = Vec::new();
    while let Ok(command) = receiver.recv() {
        if matches!(command, Command::Shutdown) {
            break;
        }
        if let Command::Library(paths) = &command {
            current_paths.clone_from(paths);
        }
        if store.is_none() {
            match StatisticsStore::open(&database) {
                Ok(opened) => {
                    if let Ok(ratings) = opened.ratings() {
                        let _ = events.send(WorkerEvent::RatingsLoaded(ratings));
                    }
                    store = Some(opened);
                }
                Err(error) => {
                    let event = match command {
                        Command::Query { request, key, .. } => WorkerEvent::Statistics {
                            request,
                            key,
                            result: Err(error),
                        },
                        Command::Rate {
                            path,
                            stars,
                            request,
                        } => WorkerEvent::RatingSaved {
                            path,
                            stars,
                            request,
                            result: Err(error),
                        },
                        _ => WorkerEvent::Error(error),
                    };
                    let _ = events.send(event);
                    continue;
                }
            }
        }
        let store = store.as_ref().expect("opened store");
        let result = match command {
            Command::Shutdown => break,
            Command::Query {
                request,
                key,
                tracks,
                now,
            } => {
                let result = store.listening_statistics(&tracks, key.range, now);
                let _ = events.send(WorkerEvent::Statistics {
                    request,
                    key,
                    result,
                });
                continue;
            }
            Command::Rate {
                path,
                stars,
                request,
            } => {
                let result = store.set_rating(&path, stars);
                let _ = events.send(WorkerEvent::RatingSaved {
                    path,
                    stars,
                    request,
                    result,
                });
                continue;
            }
            Command::Library(_) => {
                store.register_library_paths(&current_paths, chrono::Utc::now().timestamp())
            }
            Command::Played {
                path,
                played_at,
                listened_seconds,
            } => store.record_play(&path, played_at, listened_seconds),
        };
        if let Err(error) = result {
            let _ = events.send(WorkerEvent::Error(error));
            continue;
        }
        let snapshots = SmartPlaylist::ALL
            .into_iter()
            .map(|kind| {
                store
                    .smart_playlist(kind, &current_paths)
                    .map(|paths| (kind, paths))
            })
            .collect::<Result<HashMap<_, _>, _>>();
        match snapshots {
            Ok(playlists) => {
                if events.send(WorkerEvent::SmartPlaylists(playlists)).is_err() {
                    return;
                }
            }
            Err(error) => {
                let _ = events.send(WorkerEvent::Error(error));
            }
        }
    }
}

#[cfg(any(target_os = "ios", feature = "mobile"))]
fn run_worker(database: PathBuf, receiver: Receiver<Command>, events: Sender<WorkerEvent>) {
    let store = match StatisticsStore::open(&database) {
        Ok(store) => store,
        Err(error) => {
            let _ = events.send(WorkerEvent::Error(error));
            return;
        }
    };
    match store.ratings() {
        Ok(ratings) => {
            let _ = events.send(WorkerEvent::RatingsLoaded(ratings));
        }
        Err(error) => {
            let _ = events.send(WorkerEvent::Error(error));
        }
    }
    let mut current_paths = Vec::new();
    while let Ok(command) = receiver.recv() {
        let result = match command {
            Command::Shutdown => break,
            Command::Rate {
                path,
                stars,
                request,
            } => {
                let result = store.set_rating(&path, stars);
                let _ = events.send(WorkerEvent::RatingSaved {
                    path,
                    stars,
                    request,
                    result,
                });
                continue;
            }
            Command::Library(paths) => {
                current_paths = paths;
                store.register_library_paths(&current_paths, chrono::Utc::now().timestamp())
            }
            Command::Played {
                path,
                played_at,
                listened_seconds,
            } => store.record_play(&path, played_at, listened_seconds),
        };
        if let Err(error) = result {
            let _ = events.send(WorkerEvent::Error(error));
            continue;
        }
        let snapshots = SmartPlaylist::ALL
            .into_iter()
            .map(|kind| {
                store
                    .smart_playlist(kind, &current_paths)
                    .map(|paths| (kind, paths))
            })
            .collect::<Result<HashMap<_, _>, _>>();
        match snapshots {
            Ok(playlists) => {
                if events.send(WorkerEvent::SmartPlaylists(playlists)).is_err() {
                    return;
                }
            }
            Err(error) => {
                let _ = events.send(WorkerEvent::Error(error));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn channels() -> (StatisticsService, Receiver<Command>, Sender<WorkerEvent>) {
        let (commands, receiver) = mpsc::channel();
        let (events, event_receiver) = mpsc::channel();
        (
            StatisticsService {
                #[cfg(not(any(target_os = "ios", feature = "mobile")))]
                queries: Queries::default(),
                commands,
                events: event_receiver,
                worker: None,
                pending_ratings: HashMap::new(),
                rating_request: 0,
            },
            receiver,
            events,
        )
    }

    fn receive(service: &mut StatisticsService, current_ratings: &HashMap<PathBuf, u8>) -> Event {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(event) = service.poll(current_ratings) {
                return event;
            }
            assert!(Instant::now() < deadline, "statistics worker timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn acknowledge(
        commands: &Receiver<Command>,
        events: &Sender<WorkerEvent>,
        result: Result<(), String>,
    ) {
        let Command::Rate {
            path,
            stars,
            request,
        } = commands.try_recv().unwrap()
        else {
            panic!("expected a rating command");
        };
        events
            .send(WorkerEvent::RatingSaved {
                path,
                stars,
                request,
                result,
            })
            .unwrap();
    }

    #[test]
    fn latest_failed_edit_reverts_to_an_earlier_confirmed_edit() {
        let (mut service, commands, events) = channels();
        let path = PathBuf::from("/music/song.wav");
        service.rate(&path, 2, 1).unwrap();
        service.rate(&path, 5, 2).unwrap();
        acknowledge(&commands, &events, Ok(()));
        acknowledge(&commands, &events, Err("failed write".into()));
        let current = HashMap::from([(path.clone(), 5)]);
        assert!(matches!(
            service.poll(&current),
            Some(Event::RatingReverted { path: actual, stars: 2 }) if actual == path
        ));
        assert!(service.poll(&current).is_none());
        // A later edit uses the projected confirmed value, not the old burst.
        service.rate(&path, 4, 2).unwrap();
        acknowledge(&commands, &events, Err("failed again".into()));
        assert!(matches!(
            service.poll(&current),
            Some(Event::RatingReverted { stars: 2, .. })
        ));
    }

    #[test]
    fn older_failures_do_not_revert_the_latest_optimistic_edit() {
        let (mut service, commands, events) = channels();
        let path = PathBuf::from("/music/song.wav");
        service.rate(&path, 2, 1).unwrap();
        service.rate(&path, 4, 2).unwrap();
        acknowledge(&commands, &events, Err("old failure".into()));
        acknowledge(&commands, &events, Ok(()));
        let current = HashMap::from([(path.clone(), 4)]);
        assert!(service.poll(&current).is_none());
        events
            .send(WorkerEvent::RatingsLoaded(HashMap::from([(
                path.clone(),
                3,
            )])))
            .unwrap();
        let Some(Event::RatingsLoaded(loaded)) = service.poll(&current) else {
            panic!("expected the confirmed snapshot");
        };
        assert_eq!(
            loaded[&path], 3,
            "completed edits no longer overlay snapshots"
        );
    }

    #[test]
    fn snapshots_overlay_pending_edits_only_for_paths_still_in_the_ui_projection() {
        let (mut service, _commands, events) = channels();
        let old = PathBuf::from("/music/old.wav");
        let renamed = PathBuf::from("/music/renamed.wav");
        let untouched = PathBuf::from("/music/untouched.wav");
        service.rate(&old, 5, 1).unwrap();
        let persisted = HashMap::from([(old.clone(), 1), (untouched.clone(), 2)]);
        events.send(WorkerEvent::RatingsLoaded(persisted)).unwrap();
        let current = HashMap::from([(old.clone(), 5)]);
        let Some(Event::RatingsLoaded(loaded)) = service.poll(&current) else {
            panic!("expected initial ratings");
        };
        assert_eq!(loaded[&old], 5);
        assert_eq!(loaded[&untouched], 2);

        // Library rename handling removes the old key before its worker snapshot
        // arrives. A pending request must not resurrect that old key.
        events
            .send(WorkerEvent::RatingsLoaded(HashMap::from([(
                renamed.clone(),
                5,
            )])))
            .unwrap();
        let current = HashMap::from([(renamed.clone(), 5)]);
        let Some(Event::RatingsLoaded(loaded)) = service.poll(&current) else {
            panic!("expected renamed ratings");
        };
        assert!(!loaded.contains_key(&old));
        assert_eq!(loaded[&renamed], 5);
    }

    #[test]
    fn disconnected_rating_write_returns_the_last_confirmed_value() {
        let (mut service, commands, _events) = channels();
        let path = PathBuf::from("/music/song.wav");
        service.rate(&path, 2, 1).unwrap();
        drop(commands);
        assert_eq!(service.rate(&path, 5, 2), Err(1));
        // A failed send ends the pending burst.
        assert_eq!(service.rate(&path, 3, 4), Err(4));
    }

    #[test]
    fn shutdown_and_drop_drain_ratings_and_listening_records() {
        let tmp = tempfile::tempdir().unwrap();
        let database = tmp.path().join("library.sqlite3");
        let old = tmp.path().join("old.wav");
        {
            let mut service = StatisticsService::new(database.clone());
            service.set_library(vec![old.clone()]);
            service.rate(&old, 4, 0).unwrap();
            service.record_play(QualifiedPlay {
                path: old.clone(),
                listened: Duration::from_secs(42),
            });
            service.shutdown();
            service.shutdown();
        }
        let store = StatisticsStore::open(&database).unwrap();
        assert_eq!(store.ratings().unwrap()[&old], 4);
        assert_eq!(store.track_statistics(&old).unwrap().unwrap().play_count, 1);
        let history = store.listening_history(10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].path, old);
        assert_eq!(history[0].listened_seconds, 42);
        drop(store);

        // RAII must also drain writes when no explicit controller shutdown runs.
        {
            let mut service = StatisticsService::new(database.clone());
            service.rate(&old, 3, 4).unwrap();
        }
        assert_eq!(
            StatisticsStore::open(&database).unwrap().ratings().unwrap()[&old],
            3
        );
    }

    #[test]
    fn initial_ratings_and_library_snapshots_arrive_without_ui_or_audio() {
        let tmp = tempfile::tempdir().unwrap();
        let database = tmp.path().join("library.sqlite3");
        let path = tmp.path().join("song.wav");
        StatisticsStore::open(&database)
            .unwrap()
            .set_rating(&path, 4)
            .unwrap();
        let mut service = StatisticsService::new(database);
        let current = HashMap::new();
        let Event::RatingsLoaded(ratings) = receive(&mut service, &current) else {
            panic!("expected startup ratings");
        };
        assert_eq!(ratings[&path], 4);
        service.set_library(vec![path.clone()]);
        let Event::SmartPlaylists(playlists) = receive(&mut service, &current) else {
            panic!("expected library snapshots");
        };
        assert_eq!(playlists.len(), SmartPlaylist::ALL.len());
        assert_eq!(playlists[&SmartPlaylist::NeverPlayed], vec![path]);
    }

    #[test]
    fn database_open_error_is_reported_and_shutdown_remains_safe() {
        let tmp = tempfile::tempdir().unwrap();
        // A directory cannot be opened as a SQLite file.
        let mut service = StatisticsService::new(tmp.path().to_path_buf());
        assert!(matches!(
            receive(&mut service, &HashMap::new()),
            Event::Unavailable(error) if !error.is_empty()
        ));
        let path = PathBuf::from("/music/song.wav");
        service.shutdown();
        assert_eq!(service.rate(&path, 5, 2), Err(2));
        service.shutdown();
    }
    #[cfg(not(any(target_os = "ios", feature = "mobile")))]
    mod desktop_tests {
        use super::*;
        fn empty_snapshot() -> StatisticsSnapshot {
            let tmp = tempfile::tempdir().unwrap();
            StatisticsStore::open(&tmp.path().join("db"))
                .unwrap()
                .listening_statistics(&[], StatisticsRange::AllTime, chrono::Local::now())
                .unwrap()
        }

        #[test]
        fn statistics_cache_reuses_results_and_rejects_old_requests_and_revisions() {
            let (mut service, commands, events) = channels();
            let all = StatisticsRange::AllTime;
            assert!(service.needs_query(all));
            assert!(service.query(all, vec![], false).unwrap().is_none());
            let Command::Query { request, key, .. } = commands.try_recv().unwrap() else {
                panic!("query");
            };
            assert!(!service.needs_query(all));
            // A second poll/navigation while in flight must not enqueue a duplicate.
            service.query(all, vec![], false).unwrap();
            assert!(commands.try_recv().is_err());
            let snapshot = empty_snapshot();
            events
                .send(WorkerEvent::Statistics {
                    request,
                    key,
                    result: Ok(snapshot.clone()),
                })
                .unwrap();
            assert!(matches!(
                service.poll(&HashMap::new()),
                Some(Event::Statistics { .. })
            ));
            assert_eq!(
                service.query(all, vec![], false).unwrap(),
                Some(snapshot.clone())
            );
            assert!(commands.try_recv().is_err());

            service
                .query(StatisticsRange::Last7Days, vec![], false)
                .unwrap();
            let Command::Query { request, key, .. } = commands.try_recv().unwrap() else {
                panic!("query");
            };
            // Returning to a cached range cancels publication of the outstanding range.
            service.query(all, vec![], false).unwrap();
            events
                .send(WorkerEvent::Statistics {
                    request,
                    key,
                    result: Ok(snapshot.clone()),
                })
                .unwrap();
            assert!(service.poll(&HashMap::new()).is_none());

            service.query(all, vec![], true).unwrap();
            let Command::Query { request, key, .. } = commands.try_recv().unwrap() else {
                panic!("query");
            };
            service.record_play(QualifiedPlay {
                path: PathBuf::from("/new.wav"),
                listened: Duration::from_secs(30),
            });
            events
                .send(WorkerEvent::Statistics {
                    request,
                    key,
                    result: Ok(snapshot),
                })
                .unwrap();
            assert!(service.poll(&HashMap::new()).is_none());
            assert!(
                service.needs_query(all),
                "a play arriving during a query requires a new snapshot"
            );
        }

        #[test]
        fn explicit_refresh_survives_navigation_to_another_statistics_subpage() {
            let (mut service, commands, events) = channels();
            let all = StatisticsRange::AllTime;
            let old = empty_snapshot();
            let key = service.queries.key(all);
            service.queries.cache.insert(all, (key, old.clone()));
            service.query(all, vec![], true).unwrap();
            let Command::Query { request, key, .. } = commands.try_recv().unwrap() else {
                panic!("query");
            };
            assert!(
                service.query(all, vec![], false).unwrap().is_none(),
                "subpage waits for explicit refresh"
            );
            assert!(commands.try_recv().is_err());
            let mut fresh = old;
            fresh.total_plays = 17;
            events
                .send(WorkerEvent::Statistics {
                    request,
                    key,
                    result: Ok(fresh.clone()),
                })
                .unwrap();
            assert!(
                matches!(service.poll(&HashMap::new()), Some(Event::Statistics { snapshot, .. }) if snapshot.total_plays == 17)
            );
            assert_eq!(service.query(all, vec![], false).unwrap(), Some(fresh));
        }

        fn receive_statistics(service: &mut StatisticsService) -> Event {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(event @ (Event::Statistics { .. } | Event::StatisticsFailed { .. })) =
                    service.poll(&HashMap::new())
                {
                    return event;
                }
                assert!(Instant::now() < deadline, "query timed out");
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        #[test]
        fn statistics_sql_failure_does_not_kill_worker_and_refresh_recovers() {
            let tmp = tempfile::tempdir().unwrap();
            let db = tmp.path().join("library.sqlite3");
            let mut service = StatisticsService::new(db.clone());
            receive(&mut service, &HashMap::new()); // Ready.
            rusqlite::Connection::open(&db)
                .unwrap()
                .execute_batch("DROP TABLE listening_history")
                .unwrap();
            service
                .query(StatisticsRange::AllTime, vec![], false)
                .unwrap();
            assert!(matches!(
                receive_statistics(&mut service),
                Event::StatisticsFailed { .. }
            ));
            assert!(
                !service.needs_query(StatisticsRange::AllTime),
                "failure must not retry every frame"
            );
            StatisticsStore::open(&db).unwrap(); // Repair schema, without restarting service.
            service
                .query(StatisticsRange::AllTime, vec![], true)
                .unwrap();
            assert!(matches!(
                receive_statistics(&mut service),
                Event::Statistics { .. }
            ));
        }

        #[test]
        fn statistics_retry_reopens_database_after_startup_failure() {
            let tmp = tempfile::tempdir().unwrap();
            let db = tmp.path().join("blocked");
            std::fs::create_dir(&db).unwrap();
            let mut service = StatisticsService::new(db.clone());
            assert!(matches!(
                receive(&mut service, &HashMap::new()),
                Event::Unavailable(_)
            ));
            service
                .query(StatisticsRange::AllTime, vec![], false)
                .unwrap();
            assert!(matches!(
                receive_statistics(&mut service),
                Event::StatisticsFailed { .. }
            ));
            std::fs::remove_dir(&db).unwrap();
            service
                .query(StatisticsRange::AllTime, vec![], true)
                .unwrap();
            assert!(matches!(
                receive_statistics(&mut service),
                Event::Statistics { .. }
            ));
        }

        #[test]
        fn sqlite_lock_never_blocks_query_submission() {
            let tmp = tempfile::tempdir().unwrap();
            let db = tmp.path().join("library.sqlite3");
            let mut service = StatisticsService::new(db.clone());
            receive(&mut service, &HashMap::new());
            let lock = rusqlite::Connection::open(&db).unwrap();
            lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
            let submitted = Instant::now();
            service
                .query(StatisticsRange::Last30Days, vec![], false)
                .unwrap();
            assert!(
                submitted.elapsed() < Duration::from_secs(1),
                "UI only sends a channel command"
            );
            lock.execute_batch("ROLLBACK").unwrap();
            assert!(matches!(
                receive_statistics(&mut service),
                Event::Statistics { .. }
            ));
        }
    }
}
