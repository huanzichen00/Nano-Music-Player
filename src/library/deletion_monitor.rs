pub use super::paths::{is_audio, normalize};
use notify::{RecursiveMode, Watcher};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread,
    time::Duration,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventMessage {
    Removed(PathBuf),
    Restored(PathBuf),
    Error(String),
}

enum Command {
    AddRoots(Vec<PathBuf>),
    SetTracks(Vec<(PathBuf, bool)>),
}

pub struct Monitor {
    pub events: Receiver<EventMessage>,
    commands: Sender<Command>,
}

impl Monitor {
    pub fn add_roots(&self, roots: impl IntoIterator<Item = PathBuf>) {
        let _ = self
            .commands
            .send(Command::AddRoots(roots.into_iter().collect()));
    }

    /// Replaces the worker's path snapshot. `available` is the controller's last
    /// accepted state; the worker verifies it on its own thread and emits changes.
    pub fn set_tracks(&self, tracks: impl IntoIterator<Item = (PathBuf, bool)>) {
        let _ = self
            .commands
            .send(Command::SetTracks(tracks.into_iter().collect()));
    }

    #[cfg(test)]
    pub(crate) fn testing_channel() -> (Self, Sender<EventMessage>) {
        let (event_sender, events) = mpsc::channel();
        let (commands, _receiver) = mpsc::channel();
        (Self { events, commands }, event_sender)
    }
}

fn watch_directory(
    watcher: &mut notify::RecommendedWatcher,
    recursive: &mut BTreeSet<PathBuf>,
    shallow: &mut BTreeSet<PathBuf>,
    path: PathBuf,
    mode: RecursiveMode,
    output: &SyncSender<EventMessage>,
) {
    let path = normalize(&path);
    if !path.is_dir() {
        return;
    }
    match mode {
        RecursiveMode::Recursive if recursive.contains(&path) => return,
        RecursiveMode::NonRecursive if recursive.contains(&path) || shallow.contains(&path) => {
            return;
        }
        _ => {}
    }
    if mode == RecursiveMode::Recursive && shallow.remove(&path) {
        let _ = watcher.unwatch(&path);
    }
    if let Err(error) = watcher.watch(&path, mode) {
        let _ = output.send(EventMessage::Error(format!("{}: {error}", path.display())));
        return;
    }
    match mode {
        RecursiveMode::Recursive => {
            recursive.insert(path);
        }
        RecursiveMode::NonRecursive => {
            shallow.insert(path);
        }
    }
}

fn sync_watches(
    watcher: &mut notify::RecommendedWatcher,
    recursive: &mut BTreeSet<PathBuf>,
    shallow: &mut BTreeSet<PathBuf>,
    roots: &BTreeSet<PathBuf>,
    output: &SyncSender<EventMessage>,
) {
    let stale: Vec<_> = recursive
        .iter()
        .chain(shallow.iter())
        .filter(|path| !path.is_dir())
        .cloned()
        .collect();
    for path in stale {
        let _ = watcher.unwatch(&path);
        recursive.remove(&path);
        shallow.remove(&path);
    }
    for root in roots {
        if root.is_dir() {
            watch_directory(
                watcher,
                recursive,
                shallow,
                root.clone(),
                RecursiveMode::Recursive,
                output,
            );
            if let Some(parent) = root.parent() {
                watch_directory(
                    watcher,
                    recursive,
                    shallow,
                    parent.to_path_buf(),
                    RecursiveMode::NonRecursive,
                    output,
                );
            }
        } else {
            // A shallow nearest-ancestor watch detects the next directory in a
            // disconnected/deleted root being recreated without watching that
            // whole ancestor tree recursively.
            let mut ancestor = root.parent().map(Path::to_path_buf);
            while let Some(path) = ancestor {
                if path.is_dir() {
                    watch_directory(
                        watcher,
                        recursive,
                        shallow,
                        path,
                        RecursiveMode::NonRecursive,
                        output,
                    );
                    break;
                }
                ancestor = path.parent().map(Path::to_path_buf);
            }
        }
    }
}

fn reconcile_paths(
    paths: impl IntoIterator<Item = PathBuf>,
    force_restored: &BTreeSet<PathBuf>,
    known: &mut BTreeMap<PathBuf, bool>,
    output: &SyncSender<EventMessage>,
) {
    for path in paths {
        let exists = path.is_file();
        if known.get(&path).is_some_and(|previous| *previous == exists)
            && !(exists && force_restored.contains(&path))
        {
            continue;
        }
        known.insert(path.clone(), exists);
        let event = if exists {
            EventMessage::Restored(path)
        } else {
            EventMessage::Removed(path)
        };
        let _ = output.send(event);
    }
}

fn reconcile_event_paths(
    event_paths: impl IntoIterator<Item = PathBuf>,
    roots: &BTreeSet<PathBuf>,
    known: &mut BTreeMap<PathBuf, bool>,
    output: &SyncSender<EventMessage>,
) {
    let mut affected_tracks = BTreeSet::new();
    let mut force_restored = BTreeSet::new();
    for event_path in event_paths.into_iter().map(|path| normalize(&path)) {
        if is_audio(&event_path) && known.contains_key(&event_path) {
            affected_tracks.insert(event_path.clone());
            force_restored.insert(event_path.clone());
        }
        for root in roots {
            // Includes a missing root path itself and its nearest watched
            // ancestor. Do not require the event path to currently be a dir.
            if event_path.starts_with(root) || root.starts_with(&event_path) {
                affected_tracks.extend(
                    known
                        .keys()
                        .filter(|track| track.starts_with(root))
                        .cloned(),
                );
            }
        }
    }
    reconcile_paths(affected_tracks, &force_restored, known, output);
}

/// Watches imported roots on one worker. OS notifications are hints: all file
/// existence checks, including directory reconciliation, stay on this worker.
pub fn watch(roots: Vec<PathBuf>) -> Monitor {
    let (output, receiver) = mpsc::sync_channel(4096);
    let (command_sender, command_receiver) = mpsc::channel();
    thread::spawn(move || {
        const EVENT_CAPACITY: usize = 4096;
        let (tx, rx) = mpsc::sync_channel(EVENT_CAPACITY);
        let overflow = Arc::new(AtomicBool::new(false));
        let callback_overflow = Arc::clone(&overflow);
        let mut watcher = match notify::recommended_watcher(move |event| {
            if tx.try_send(event).is_err() {
                callback_overflow.store(true, Ordering::Relaxed);
            }
        }) {
            Ok(watcher) => watcher,
            Err(error) => {
                let _ = output.send(EventMessage::Error(error.to_string()));
                return;
            }
        };
        let mut roots: BTreeSet<_> = roots.into_iter().map(|path| normalize(&path)).collect();
        let mut known = BTreeMap::<PathBuf, bool>::new();
        let mut recursive = BTreeSet::new();
        let mut shallow = BTreeSet::new();
        sync_watches(&mut watcher, &mut recursive, &mut shallow, &roots, &output);
        loop {
            let mut changed = false;
            while let Ok(command) = command_receiver.try_recv() {
                changed = true;
                match command {
                    Command::AddRoots(new_roots) => {
                        roots.extend(new_roots.into_iter().map(|path| normalize(&path)));
                    }
                    Command::SetTracks(tracks) => {
                        let updated: BTreeMap<_, _> = tracks
                            .into_iter()
                            .map(|(path, available)| (normalize(&path), available))
                            .collect();
                        known = updated;
                    }
                }
            }
            if changed {
                sync_watches(&mut watcher, &mut recursive, &mut shallow, &roots, &output);
                // Verify the initial snapshot off-thread. This also catches a
                // path that changed before the watcher was attached.
                reconcile_paths(
                    known.keys().cloned().collect::<Vec<_>>(),
                    &BTreeSet::new(),
                    &mut known,
                    &output,
                );
            }
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(first) => {
                    let mut batch = vec![first];
                    let debounce_deadline = std::time::Instant::now() + Duration::from_millis(220);
                    while batch.len() < 2048 {
                        let remaining =
                            debounce_deadline.saturating_duration_since(std::time::Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        match rx.recv_timeout(remaining) {
                            Ok(event) => batch.push(event),
                            Err(_) => break,
                        }
                    }
                    let mut paths = BTreeSet::new();
                    for event in batch {
                        match event {
                            Ok(event) => paths.extend(event.paths),
                            Err(error) => {
                                let _ = output.send(EventMessage::Error(error.to_string()));
                            }
                        }
                    }
                    reconcile_event_paths(paths.iter().cloned(), &roots, &mut known, &output);
                    // Some native backends detach a recursive watch when a root
                    // is removed and recreated between UI polling turns.
                    for root in &roots {
                        if root.is_dir()
                            && paths
                                .iter()
                                .any(|path| root == path || root.starts_with(path))
                            && recursive.remove(root)
                        {
                            let _ = watcher.unwatch(root);
                        }
                    }
                    sync_watches(&mut watcher, &mut recursive, &mut shallow, &roots, &output);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            if overflow.swap(false, Ordering::Relaxed) {
                reconcile_paths(
                    known.keys().cloned().collect::<Vec<_>>(),
                    &BTreeSet::new(),
                    &mut known,
                    &output,
                );
                let _ = output.send(EventMessage::Error(
                    "Filesystem event burst exceeded the monitor queue; known paths were reconciled"
                        .to_owned(),
                ));
            }
        }
    });
    Monitor {
        events: receiver,
        commands: command_sender,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, thread};

    fn next_change(monitor: &Monitor, path: &Path) -> EventMessage {
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        while std::time::Instant::now() < deadline {
            if let Ok(event) = monitor.events.recv_timeout(Duration::from_millis(100))
                && matches!(&event, EventMessage::Removed(p) | EventMessage::Restored(p) if p == path)
            {
                return event;
            }
        }
        panic!(
            "timed out waiting for a state change for {}",
            path.display()
        );
    }

    #[test]
    fn healthy_ancestor_reports_directory_removal_and_same_path_restore() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("music");
        fs::create_dir(&root).unwrap();
        let audio = root.join("track.wav");
        fs::write(&audio, include_bytes!("../../tests/fixtures/quiet.wav")).unwrap();
        let monitor = watch(vec![root.clone()]);
        monitor.set_tracks([(audio.clone(), false)]);
        assert_eq!(
            next_change(&monitor, &audio),
            EventMessage::Restored(audio.clone())
        );
        // Consume any duplicate initial-state notification before changing files.
        while monitor.events.try_recv().is_ok() {}
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            next_change(&monitor, &audio),
            EventMessage::Removed(audio.clone())
        );
        fs::create_dir(&root).unwrap();
        fs::write(&audio, include_bytes!("../../tests/fixtures/quiet.wav")).unwrap();
        assert_eq!(next_change(&monitor, &audio), EventMessage::Restored(audio));
        thread::yield_now();
    }

    #[test]
    fn missing_root_under_reachable_ancestor_is_not_a_mass_deletion_and_can_reconnect() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("disconnected").join("music");
        let audio = root.join("track.wav");
        let monitor = watch(vec![root.clone()]);
        monitor.set_tracks([(audio.clone(), false)]);
        assert!(
            monitor
                .events
                .recv_timeout(Duration::from_millis(300))
                .is_err()
        );

        fs::create_dir_all(&root).unwrap();
        fs::write(&audio, include_bytes!("../../tests/fixtures/quiet.wav")).unwrap();
        assert_eq!(next_change(&monitor, &audio), EventMessage::Restored(audio));
    }

    #[test]
    fn normalization_keeps_missing_path_identity_and_audio_filter_is_case_insensitive() {
        let path = PathBuf::from("/tmp/not-created/../not-created/track.MP3");
        assert!(normalize(&path).ends_with("not-created/track.MP3"));
        assert!(is_audio(Path::new("track.MP3")));
        assert!(!is_audio(Path::new("cover.jpg")));
    }
}
