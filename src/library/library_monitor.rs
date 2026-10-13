//! Debounced notifications for supported audio files below imported roots.
//!
//! Notify callbacks only enqueue OS events. Path checks, watch maintenance, and
//! event reconciliation run on this module's worker thread; callers can poll
//! `Monitor::events` from their normal controller loop and schedule scanner work
//! elsewhere.

use notify::{Event, EventKind, RecursiveMode, Watcher, event::ModifyKind};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

/// A filesystem change to a supported audio file.
///
/// Native rename notifications are surfaced as `Renamed`; when a backend only
/// reports create/remove paths, callers receive the equivalent `Removed` plus
/// `Added` pair. Treat each path as a hint and refresh it on a background worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryEvent {
    Added(PathBuf),
    Removed(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
    Modified(PathBuf),
    Error(String),
}

enum Command {
    AddRoots(Vec<PathBuf>),
}

/// Handle to a background filesystem watcher.
pub struct Monitor {
    pub events: Receiver<LibraryEvent>,
    commands: Sender<Command>,
}

impl Monitor {
    /// Add imported music roots without blocking the caller.
    pub fn add_roots(&self, roots: impl IntoIterator<Item = PathBuf>) {
        let _ = self
            .commands
            .send(Command::AddRoots(roots.into_iter().collect()));
    }
}

/// Start a background watcher for imported roots.
///
/// Roots that are temporarily unavailable are not recursively watched and do
/// not generate synthetic removals. Their nearest existing ancestor is watched
/// shallowly so the root can be attached when it returns.
pub fn watch(roots: Vec<PathBuf>) -> Monitor {
    let (output, events) = mpsc::sync_channel(4096);
    let (commands, command_receiver) = mpsc::channel();
    thread::spawn(move || {
        run_worker(roots, command_receiver, output);
    });
    Monitor { events, commands }
}

fn run_worker(
    roots: Vec<PathBuf>,
    command_receiver: Receiver<Command>,
    output: SyncSender<LibraryEvent>,
) {
    const INPUT_CAPACITY: usize = 4096;
    let (tx, rx) = mpsc::sync_channel(INPUT_CAPACITY);
    let overflow = Arc::new(AtomicBool::new(false));
    let callback_overflow = Arc::clone(&overflow);
    let mut watcher = match notify::recommended_watcher(move |event| {
        if tx.try_send(event).is_err() {
            callback_overflow.store(true, Ordering::Relaxed);
        }
    }) {
        Ok(watcher) => watcher,
        Err(error) => {
            let _ = output.send(LibraryEvent::Error(error.to_string()));
            return;
        }
    };

    let mut roots: BTreeSet<_> = roots
        .iter()
        .map(|path| super::deletion_monitor::normalize(path))
        .collect();
    let mut recursive = BTreeSet::new();
    let mut shallow = BTreeSet::new();
    sync_watches(&mut watcher, &mut recursive, &mut shallow, &roots, &output);

    loop {
        while let Ok(command) = command_receiver.try_recv() {
            match command {
                Command::AddRoots(paths) => {
                    roots.extend(
                        paths
                            .iter()
                            .map(|path| super::deletion_monitor::normalize(path)),
                    );
                }
            }
        }
        sync_watches(&mut watcher, &mut recursive, &mut shallow, &roots, &output);

        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(first) => {
                let mut batch = vec![first];
                let deadline = Instant::now() + Duration::from_millis(240);
                while batch.len() < 2048 {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match rx.recv_timeout(remaining) {
                        Ok(event) => batch.push(event),
                        Err(_) => break,
                    }
                }
                let events = batch.into_iter().filter_map(|result| match result {
                    Ok(event) => Some(event),
                    Err(error) => {
                        let _ = output.send(LibraryEvent::Error(error.to_string()));
                        None
                    }
                });
                let reconciled = reconcile_events(events, &roots);
                for event in reconciled {
                    let _ = output.send(event);
                }
                sync_watches(&mut watcher, &mut recursive, &mut shallow, &roots, &output);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }

        if overflow.swap(false, Ordering::Relaxed) {
            // Without a full library snapshot, an overflow cannot be safely
            // reconciled into additions or deletions. Tell the owner to schedule
            // a background root refresh instead of guessing.
            let _ = output.send(LibraryEvent::Error(
                "Filesystem event burst exceeded the monitor queue; schedule a background library refresh"
                    .to_owned(),
            ));
        }
    }
}

fn sync_watches(
    watcher: &mut notify::RecommendedWatcher,
    recursive: &mut BTreeSet<PathBuf>,
    shallow: &mut BTreeSet<PathBuf>,
    roots: &BTreeSet<PathBuf>,
    output: &SyncSender<LibraryEvent>,
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
        if root.is_dir() {
            if !recursive.contains(root) {
                // A root may have disappeared and returned between polls.
                if shallow.remove(root) {
                    let _ = watcher.unwatch(root);
                }
                watch_directory(
                    watcher,
                    recursive,
                    shallow,
                    root.clone(),
                    RecursiveMode::Recursive,
                    output,
                );
            }
        } else {
            // Only watch one nearest existing ancestor, non-recursively. This
            // detects recreation without recursively scanning unrelated files.
            let mut ancestor = root.parent().map(Path::to_path_buf);
            while let Some(path) = ancestor {
                if path.is_dir() {
                    watch_directory(
                        watcher,
                        recursive,
                        shallow,
                        path.clone(),
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

fn watch_directory(
    watcher: &mut notify::RecommendedWatcher,
    recursive: &mut BTreeSet<PathBuf>,
    shallow: &mut BTreeSet<PathBuf>,
    path: PathBuf,
    mode: RecursiveMode,
    output: &SyncSender<LibraryEvent>,
) {
    if !path.is_dir() {
        return;
    }
    let watched = match mode {
        RecursiveMode::Recursive => recursive.contains(&path),
        RecursiveMode::NonRecursive => recursive.contains(&path) || shallow.contains(&path),
    };
    if watched {
        return;
    }
    if mode == RecursiveMode::Recursive && shallow.remove(&path) {
        let _ = watcher.unwatch(&path);
    }
    if let Err(error) = watcher.watch(&path, mode) {
        let _ = output.send(LibraryEvent::Error(format!("{}: {error}", path.display())));
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

/// Convert a debounced batch into stable path events. All filesystem existence
/// checks happen on the watcher worker, never in a notify callback or UI poll.
fn reconcile_events(
    events: impl IntoIterator<Item = Event>,
    roots: &BTreeSet<PathBuf>,
) -> Vec<LibraryEvent> {
    let mut renamed = BTreeSet::new();
    let mut candidates = BTreeMap::<PathBuf, EventKind>::new();
    for event in events {
        let paths: Vec<_> = event
            .paths
            .iter()
            .map(|path| super::deletion_monitor::normalize(path))
            .filter(|path| is_in_roots(path, roots))
            .collect();

        if matches!(event.kind, EventKind::Modify(ModifyKind::Name(_))) && paths.len() >= 2 {
            for pair in paths.chunks_exact(2) {
                let from = &pair[0];
                let to = &pair[1];
                // Do not treat a missing/unmounted root as file deletions.
                let from_audio = super::deletion_monitor::is_audio(from);
                let to_audio = super::deletion_monitor::is_audio(to);
                if from_audio
                    && to_audio
                    && roots_for_path(from, roots).any(|root| root.is_dir())
                    && roots_for_path(to, roots).any(|root| root.is_dir())
                {
                    renamed.insert((from.clone(), to.clone()));
                } else {
                    if from_audio && roots_for_path(from, roots).any(|root| root.is_dir()) {
                        candidates.insert(
                            from.clone(),
                            EventKind::Remove(notify::event::RemoveKind::Any),
                        );
                    }
                    if to_audio && roots_for_path(to, roots).any(|root| root.is_dir()) {
                        candidates.insert(
                            to.clone(),
                            EventKind::Create(notify::event::CreateKind::Any),
                        );
                    }
                }
            }
            continue;
        }

        for path in paths {
            if !super::deletion_monitor::is_audio(&path) {
                continue;
            }
            let kind = match event.kind {
                EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::From)) => {
                    EventKind::Remove(notify::event::RemoveKind::Any)
                }
                EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::To)) => {
                    EventKind::Create(notify::event::CreateKind::Any)
                }
                kind => kind,
            };
            let replace = match candidates.get(&path) {
                None => true,
                Some(previous) => event_priority(kind) > event_priority(*previous),
            };
            if replace {
                candidates.insert(path, kind);
            }
        }
    }

    let mut output = Vec::new();
    for (from, to) in renamed {
        if !from.exists() && to.is_file() {
            output.push(LibraryEvent::Renamed { from, to });
        } else {
            if roots_for_path(&from, roots).any(|root| root.is_dir()) && !from.is_file() {
                output.push(LibraryEvent::Removed(from));
            }
            if roots_for_path(&to, roots).any(|root| root.is_dir()) && to.is_file() {
                output.push(LibraryEvent::Added(to));
            }
        }
    }
    for (path, kind) in candidates {
        // A disconnected root must never make every path below it look deleted.
        if !roots_for_path(&path, roots).any(|root| root.is_dir()) {
            continue;
        }
        let event = match kind {
            EventKind::Create(_) => path.is_file().then_some(LibraryEvent::Added(path)),
            EventKind::Remove(_) => (!path.is_file()).then_some(LibraryEvent::Removed(path)),
            EventKind::Modify(_) | EventKind::Any => {
                path.is_file().then_some(LibraryEvent::Modified(path))
            }
            _ => None,
        };
        if let Some(event) = event {
            output.push(event);
        }
    }
    output.sort_by(|a, b| event_path(a).cmp(event_path(b)));
    output.dedup();
    output
}

fn event_priority(kind: EventKind) -> u8 {
    match kind {
        EventKind::Create(_) => 3,
        EventKind::Remove(_) => 2,
        EventKind::Modify(_) => 1,
        _ => 0,
    }
}

fn event_path(event: &LibraryEvent) -> &Path {
    match event {
        LibraryEvent::Added(path) | LibraryEvent::Removed(path) | LibraryEvent::Modified(path) => {
            path
        }
        LibraryEvent::Renamed { to, .. } => to,
        LibraryEvent::Error(_) => Path::new(""),
    }
}

fn is_in_roots(path: &Path, roots: &BTreeSet<PathBuf>) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

fn roots_for_path<'a>(
    path: &'a Path,
    roots: &'a BTreeSet<PathBuf>,
) -> impl Iterator<Item = &'a PathBuf> {
    roots.iter().filter(move |root| path.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_dir() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rust-music-monitor-{stamp}"))
    }

    fn event(kind: EventKind, paths: impl IntoIterator<Item = PathBuf>) -> Event {
        Event {
            kind,
            paths: paths.into_iter().collect(),
            attrs: Default::default(),
        }
    }

    #[test]
    fn supported_audio_extensions_match_scanner() {
        for name in [
            "one.mp3",
            "two.flac",
            "three.wav",
            "four.m4a",
            "five.aac",
            "six.ogg",
            "seven.aiff",
        ] {
            assert!(
                super::super::deletion_monitor::is_audio(Path::new(name)),
                "{name}"
            );
        }
        assert!(!super::super::deletion_monitor::is_audio(Path::new(
            "cover.jpg"
        )));
    }

    #[test]
    fn create_modify_burst_is_debounced_as_one_added_event() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let path = root.join("new.mp3");
        fs::write(&path, b"audio").unwrap();
        let roots = BTreeSet::from([root.clone()]);
        let result = reconcile_events(
            [
                event(
                    EventKind::Create(notify::event::CreateKind::File),
                    [path.clone()],
                ),
                event(
                    EventKind::Modify(notify::event::ModifyKind::Data(
                        notify::event::DataChange::Content,
                    )),
                    [path.clone()],
                ),
            ],
            &roots,
        );
        assert_eq!(result, vec![LibraryEvent::Added(path)]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rename_notification_produces_renamed_event() {
        let root = temp_dir();
        fs::create_dir_all(&root).unwrap();
        let from = root.join("old.mp3");
        let to = root.join("new.mp3");
        fs::write(&from, b"audio").unwrap();
        fs::rename(&from, &to).unwrap();
        let roots = BTreeSet::from([root.clone()]);
        let result = reconcile_events(
            [event(
                EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Both)),
                [from.clone(), to.clone()],
            )],
            &roots,
        );
        assert_eq!(result, vec![LibraryEvent::Renamed { from, to }]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unavailable_root_never_emits_mass_removals() {
        let root = temp_dir();
        let path = root.join("gone.mp3");
        let roots = BTreeSet::from([root]);
        let result = reconcile_events(
            [event(
                EventKind::Remove(notify::event::RemoveKind::File),
                [path],
            )],
            &roots,
        );
        assert!(result.is_empty());
    }

    #[test]
    fn irrelevant_extensions_and_paths_outside_roots_are_ignored() {
        let root = temp_dir();
        let outside_root = temp_dir();
        let outside = outside_root.join("track.mp3");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside_root).unwrap();
        fs::write(root.join("cover.jpg"), b"image").unwrap();
        fs::write(&outside, b"audio").unwrap();
        let roots = BTreeSet::from([root.clone()]);
        let result = reconcile_events(
            [event(
                EventKind::Create(notify::event::CreateKind::File),
                [root.join("cover.jpg"), outside.clone()],
            )],
            &roots,
        );
        assert!(result.is_empty());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside_root).unwrap();
    }
}
