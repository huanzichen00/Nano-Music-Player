//! Startup publication and explicit library import task ownership.
//!
//! State merging, thumbnails, and file availability remain in
//! the controller. This component owns task receivers and their readiness rules.

use crate::{
    app::thumbnails::{self, ImportEvent, Prepared},
    library::{Track, playlists},
};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

pub(super) enum LoadEvent {
    Cached(Prepared<Vec<Track>>),
    Refreshed {
        original: Box<Track>,
        prepared: Box<Prepared<Track>>,
    },
    Error(String),
    Complete {
        missing_cached: bool,
    },
}

pub(super) struct LibraryTasks {
    database: PathBuf,
    loading: Option<Receiver<thumbnails::LoadEvent>>,
    importing: Option<Receiver<ImportEvent>>,
    ready: bool,
    refreshing: Option<Receiver<Result<Prepared<crate::library::refresh::Snapshot>, String>>>,
}

impl LibraryTasks {
    pub(super) fn start(
        database: PathBuf,
    ) -> (Self, Receiver<Result<Vec<playlists::Playlist>, String>>) {
        let (playlist_sender, playlist_receiver) = mpsc::channel();
        let (library_sender, library_receiver) = mpsc::sync_channel(2);
        let startup_database = database.clone();
        thread::spawn(move || {
            // Migrate before the scanner's large metadata SELECT can hold a read
            // lock. A playlist error must not prevent music library publication.
            let _ = playlist_sender.send(playlists::load(&startup_database));
            let loading = thumbnails::load(startup_database);
            while let Ok(event) = loading.recv() {
                if library_sender.send(event).is_err() {
                    break;
                }
            }
        });
        (
            Self {
                database,
                loading: Some(library_receiver),
                importing: None,
                ready: false,
                refreshing: None,
            },
            playlist_receiver,
        )
    }

    pub(super) fn start_import(&mut self, title: String) {
        self.importing = Some(thumbnails::import(self.database.clone(), title));
    }

    pub(super) fn start_refresh(&mut self, roots: Vec<PathBuf>) {
        if self.refreshing.is_none() && self.importing.is_none() {
            self.refreshing = Some(thumbnails::refresh(self.database.clone(), roots));
        }
    }

    pub(super) fn busy(&self) -> bool {
        self.loading.is_some() || self.importing.is_some() || self.refreshing.is_some()
    }

    pub(super) fn poll_refresh(
        &mut self,
    ) -> Option<Result<Prepared<crate::library::refresh::Snapshot>, String>> {
        let result = match self.refreshing.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("Library refresh stopped unexpectedly".into()),
        };
        self.refreshing = None;
        Some(result)
    }

    pub(super) fn is_importing(&self) -> bool {
        self.importing.is_some()
    }

    pub(super) fn ready(&self) -> bool {
        self.ready
    }

    /// Consume at most one startup event, preserving bounded refresh backpressure
    /// and the controller's existing amount of library work per UI poll.
    pub(super) fn poll_load(&mut self) -> Option<LoadEvent> {
        let result = self.loading.as_ref()?.try_recv();
        match result {
            Ok(thumbnails::LoadEvent::Cached(prepared)) => {
                self.ready = true;
                Some(LoadEvent::Cached(prepared))
            }
            Ok(thumbnails::LoadEvent::Refreshed { original, prepared }) => {
                Some(LoadEvent::Refreshed { original, prepared })
            }
            Ok(thumbnails::LoadEvent::Error(message)) => {
                self.loading = None;
                self.ready = true;
                Some(LoadEvent::Error(message))
            }
            Ok(thumbnails::LoadEvent::Complete) | Err(TryRecvError::Disconnected) => {
                self.loading = None;
                let missing_cached = !self.ready;
                self.ready = true;
                Some(LoadEvent::Complete { missing_cached })
            }
            Err(TryRecvError::Empty) => None,
        }
    }

    /// SelectionClosed and Completed may both be drained in one controller poll.
    /// A disconnected import without completion retains the existing busy state.
    pub(super) fn poll_import(&mut self) -> Option<ImportEvent> {
        let event = self.importing.as_ref()?.try_recv().ok()?;
        if matches!(event, ImportEvent::Completed(_)) {
            self.importing = None;
        }
        Some(event)
    }

    #[cfg(test)]
    pub(super) fn replace_loading(
        &mut self,
        receiver: Receiver<thumbnails::LoadEvent>,
        ready: bool,
    ) {
        self.loading = Some(receiver);
        self.ready = ready;
    }

    #[cfg(test)]
    pub(super) fn loading_active(&self) -> bool {
        self.loading.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn with_loading(receiver: Receiver<thumbnails::LoadEvent>) -> LibraryTasks {
        LibraryTasks {
            database: PathBuf::new(),
            loading: Some(receiver),
            importing: None,
            ready: false,
            refreshing: None,
        }
    }

    fn empty_cached(database: PathBuf) -> thumbnails::LoadEvent {
        let loading = thumbnails::load(database);
        let cached = loading.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(cached, thumbnails::LoadEvent::Cached(_)));
        cached
    }

    fn receive_load(tasks: &mut LibraryTasks) -> LoadEvent {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(event) = tasks.poll_load() {
                return event;
            }
            assert!(Instant::now() < deadline, "library startup timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn cached_publication_marks_ready_and_keeps_completion_for_the_next_poll() {
        let tmp = tempfile::tempdir().unwrap();
        let cached = empty_cached(tmp.path().join("library.sqlite3"));
        let (sender, receiver) = mpsc::channel();
        let mut tasks = with_loading(receiver);
        sender.send(cached).unwrap();
        sender.send(thumbnails::LoadEvent::Complete).unwrap();
        assert!(!tasks.ready());
        assert!(matches!(tasks.poll_load(), Some(LoadEvent::Cached(_))));
        assert!(tasks.ready());
        assert!(
            tasks.loading_active(),
            "only one event is consumed per poll"
        );
        assert!(matches!(
            tasks.poll_load(),
            Some(LoadEvent::Complete {
                missing_cached: false
            })
        ));
        assert!(!tasks.loading_active());
        assert!(tasks.poll_load().is_none());
    }

    #[test]
    fn completion_and_disconnect_before_cached_publication_report_missing_cache_once() {
        for explicit_completion in [false, true] {
            let (sender, receiver) = mpsc::channel();
            let mut tasks = with_loading(receiver);
            if explicit_completion {
                sender.send(thumbnails::LoadEvent::Complete).unwrap();
            }
            drop(sender);
            assert!(matches!(
                tasks.poll_load(),
                Some(LoadEvent::Complete {
                    missing_cached: true
                })
            ));
            assert!(tasks.ready());
            assert!(!tasks.loading_active());
            assert!(tasks.poll_load().is_none());
        }
    }

    #[test]
    fn startup_error_is_terminal_and_does_not_replace_its_message_with_disconnect() {
        let (sender, receiver) = mpsc::channel();
        let mut tasks = with_loading(receiver);
        assert!(tasks.poll_load().is_none());
        assert!(!tasks.ready());
        sender
            .send(thumbnails::LoadEvent::Error("library failure".into()))
            .unwrap();
        sender.send(thumbnails::LoadEvent::Complete).unwrap();
        assert!(matches!(
            tasks.poll_load(),
            Some(LoadEvent::Error(message)) if message == "library failure"
        ));
        assert!(tasks.ready());
        assert!(!tasks.loading_active());
        assert!(tasks.poll_load().is_none());
    }

    #[test]
    fn cached_publication_followed_by_disconnect_keeps_readiness_without_missing_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let cached = empty_cached(tmp.path().join("library.sqlite3"));
        let (sender, receiver) = mpsc::channel();
        let mut tasks = with_loading(receiver);
        sender.send(cached).unwrap();
        drop(sender);
        assert!(matches!(tasks.poll_load(), Some(LoadEvent::Cached(_))));
        assert!(matches!(
            tasks.poll_load(),
            Some(LoadEvent::Complete {
                missing_cached: false
            })
        ));
    }

    #[test]
    fn import_selection_and_completion_can_drain_without_finishing_at_selection_close() {
        let (_loading_sender, loading) = mpsc::channel();
        let mut tasks = with_loading(loading);
        let (sender, importing) = mpsc::channel();
        tasks.importing = Some(importing);
        sender.send(ImportEvent::SelectionClosed).unwrap();
        sender.send(ImportEvent::Completed(Ok(None))).unwrap();
        assert!(tasks.is_importing());
        assert!(matches!(
            tasks.poll_import(),
            Some(ImportEvent::SelectionClosed)
        ));
        assert!(tasks.is_importing());
        assert!(matches!(
            tasks.poll_import(),
            Some(ImportEvent::Completed(Ok(None)))
        ));
        assert!(!tasks.is_importing());
        assert!(tasks.poll_import().is_none());
    }

    #[test]
    fn disconnected_import_retains_busy_state_and_completed_errors_clear_it() {
        let (_loading_sender, loading) = mpsc::channel();
        let mut tasks = with_loading(loading);
        let (sender, importing) = mpsc::channel();
        tasks.importing = Some(importing);
        drop(sender);
        assert!(tasks.poll_import().is_none());
        assert!(tasks.is_importing());

        let (sender, importing) = mpsc::channel();
        tasks.importing = Some(importing);
        sender
            .send(ImportEvent::Completed(Err("import failure".into())))
            .unwrap();
        assert!(matches!(
            tasks.poll_import(),
            Some(ImportEvent::Completed(Err(message))) if message == "import failure"
        ));
        assert!(!tasks.is_importing());
    }

    #[test]
    fn startup_migrates_playlists_before_publishing_cached_library() {
        let tmp = tempfile::tempdir().unwrap();
        let database = tmp.path().join("library.sqlite3");
        let (mut tasks, playlists) = LibraryTasks::start(database.clone());
        assert!(matches!(receive_load(&mut tasks), LoadEvent::Cached(_)));
        assert!(
            playlists
                .try_recv()
                .expect("playlist load must finish before cached publication")
                .unwrap()
                .is_empty()
        );
        let connection = rusqlite::Connection::open(&database).unwrap();
        for table in ["playlists", "playlist_tracks", "tracks"] {
            let count: i64 = connection
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "startup must create {table}");
        }
        assert!(matches!(
            receive_load(&mut tasks),
            LoadEvent::Complete {
                missing_cached: false
            }
        ));
    }

    #[test]
    fn playlist_load_error_does_not_block_music_library_publication() {
        let tmp = tempfile::tempdir().unwrap();
        let database = tmp.path().join("library.sqlite3");
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute_batch("CREATE TABLE playlists (unexpected TEXT);")
            .unwrap();
        drop(connection);
        let (mut tasks, playlists) = LibraryTasks::start(database);
        assert!(
            playlists
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .is_err()
        );
        assert!(matches!(receive_load(&mut tasks), LoadEvent::Cached(_)));
        assert!(tasks.ready());
        assert!(matches!(
            receive_load(&mut tasks),
            LoadEvent::Complete {
                missing_cached: false
            }
        ));
    }
}
