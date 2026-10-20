//! Application statistics persistence and optimistic rating coordination.
//!
//! The worker owns its SQLite connection. The controller keeps the UI projection
//! and listening samples, and consumes these events without sharing either with
//! the worker.

use crate::library::statistics::{QualifiedPlay, SmartPlaylist, StatisticsStore};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

enum Command {
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
    RatingsLoaded(HashMap<PathBuf, u8>),
    RatingReverted { path: PathBuf, stars: u8 },
    SmartPlaylists(HashMap<SmartPlaylist, Vec<PathBuf>>),
    Unavailable(String),
}

pub(super) struct StatisticsService {
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
            commands,
            events: event_receiver,
            worker: Some(worker),
            pending_ratings: HashMap::new(),
            rating_request: 0,
        }
    }

    pub(super) fn set_library(&self, paths: Vec<PathBuf>) {
        let _ = self.commands.send(Command::Library(paths));
    }

    pub(super) fn record_play(&self, play: QualifiedPlay) {
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
}
