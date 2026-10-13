//! Debounced notifications for supported audio files below imported roots.
//!
//! Notify callbacks only enqueue OS events. Path checks, watch maintenance, and
//! event reconciliation run on this module's worker thread; callers can poll
//! `Monitor::events` from their normal controller loop and schedule scanner work
//! elsewhere.

use notify::{
    Event, EventKind, RecursiveMode, Watcher,
    event::{CreateKind, ModifyKind, RemoveKind, RenameMode},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// A filesystem hint for a supported audio file or an imported root.
///
/// Native rename notifications are surfaced as `Renamed`; when a backend only
/// reports create/remove paths, callers receive the equivalent `Removed` plus
/// `Added` pair. Treat each path as a hint and refresh it on a background worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryEvent {
    Added(PathBuf),
    Removed(PathBuf),
    Renamed {
        from: PathBuf,
        to: PathBuf,
    },
    Modified(PathBuf),
    /// Reconcile this normalized imported root on a background scanner. Emitted
    /// on watch attachment/recovery, directory changes, and lost notifications.
    /// Preserve cached tracks if the root is unavailable or traversal fails;
    /// this hint by itself never confirms a deletion.
    RefreshRoot(PathBuf),
    Error(String),
}

enum Command {
    AddRoots(Vec<PathBuf>),
    #[cfg(test)]
    Ready(Sender<()>),
}

/// Handle to a background filesystem watcher.
pub struct Monitor {
    pub events: Receiver<LibraryEvent>,
    commands: Sender<Command>,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        // Shutdown is asynchronous: slow filesystem operations must not block
        // the UI. Dropping the handle detaches the worker, which sees the flag
        // both while polling and while waiting for output queue capacity.
        self.worker.take();
    }
}

struct Output {
    sender: SyncSender<LibraryEvent>,
    stopped: Arc<AtomicBool>,
}

impl Output {
    fn send(&self, mut event: LibraryEvent) -> bool {
        while !self.stopped.load(Ordering::Relaxed) {
            match self.sender.try_send(event) {
                Ok(()) => return true,
                Err(mpsc::TrySendError::Full(pending)) => {
                    event = pending;
                    thread::sleep(Duration::from_millis(10));
                }
                Err(mpsc::TrySendError::Disconnected(_)) => return false,
            }
        }
        false
    }
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
    start_monitor(roots, 4096)
}

fn start_monitor(roots: Vec<PathBuf>, output_capacity: usize) -> Monitor {
    let (output, events) = mpsc::sync_channel(output_capacity);
    let (commands, command_receiver) = mpsc::channel();
    let stopped = Arc::new(AtomicBool::new(false));
    let worker_output = Output {
        sender: output,
        stopped: Arc::clone(&stopped),
    };
    let worker = thread::spawn(move || {
        run_worker(roots, command_receiver, worker_output);
    });
    Monitor {
        events,
        commands,
        stopped,
        worker: Some(worker),
    }
}

fn run_worker(roots: Vec<PathBuf>, command_receiver: Receiver<Command>, output: Output) {
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
    loop {
        if output.stopped.load(Ordering::Relaxed) {
            return;
        }
        #[cfg(test)]
        let mut readiness = Vec::new();
        loop {
            match command_receiver.try_recv() {
                Ok(Command::AddRoots(paths)) => {
                    roots.extend(
                        paths
                            .iter()
                            .map(|path| super::deletion_monitor::normalize(path)),
                    );
                }
                #[cfg(test)]
                Ok(Command::Ready(sender)) => readiness.push(sender),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
        let attached = sync_watches(&mut watcher, &mut recursive, &mut shallow, &roots, &output);
        for root in attached {
            if !output.send(LibraryEvent::RefreshRoot(root)) {
                return;
            }
        }
        #[cfg(test)]
        for sender in readiness {
            let _ = sender.send(());
        }

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
                let mut events = Vec::new();
                let mut lost_events = false;
                for result in batch {
                    match result {
                        Ok(event) => events.push(event),
                        Err(error) => {
                            lost_events = true;
                            if !output.send(LibraryEvent::Error(error.to_string())) {
                                return;
                            }
                        }
                    }
                }
                // Native backends can detach their watches when a directory
                // disappears, even if it has returned before this batch runs.
                let rearm = roots_to_rearm(&events, &roots, lost_events);
                for root in &rearm {
                    if recursive.remove(root) {
                        let _ = watcher.unwatch(root);
                    }
                }
                let attached =
                    sync_watches(&mut watcher, &mut recursive, &mut shallow, &roots, &output);
                let mut reconciled = reconcile_events(events, &roots);
                reconciled.extend(
                    attached
                        .into_iter()
                        .chain(rearm.into_iter().filter(|root| root.is_dir()))
                        .map(LibraryEvent::RefreshRoot),
                );
                sort_events(&mut reconciled);
                for event in reconciled {
                    if !output.send(event) {
                        return;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }

        if overflow.swap(false, Ordering::Relaxed) {
            // Reattach as well as rescan: a lost root/subdirectory event may
            // have invalidated the backend's recursive watch tree.
            for root in std::mem::take(&mut recursive) {
                let _ = watcher.unwatch(&root);
            }
            sync_watches(&mut watcher, &mut recursive, &mut shallow, &roots, &output);
            for event in refresh_roots(&roots) {
                if !output.send(event) {
                    return;
                }
            }
            if !output.send(LibraryEvent::Error(
                "Filesystem event burst exceeded the monitor queue; root reconciliation requested"
                    .to_owned(),
            )) {
                return;
            }
        }
    }
}

fn sync_watches(
    watcher: &mut notify::RecommendedWatcher,
    recursive: &mut BTreeSet<PathBuf>,
    shallow: &mut BTreeSet<PathBuf>,
    roots: &BTreeSet<PathBuf>,
    output: &Output,
) -> BTreeSet<PathBuf> {
    let mut attached = BTreeSet::new();
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
        if output.stopped.load(Ordering::Relaxed) {
            break;
        }
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
                if watch_directory(
                    watcher,
                    recursive,
                    shallow,
                    root.clone(),
                    RecursiveMode::Recursive,
                    output,
                ) {
                    attached.insert(root.clone());
                }
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
    attached
}

fn watch_directory(
    watcher: &mut notify::RecommendedWatcher,
    recursive: &mut BTreeSet<PathBuf>,
    shallow: &mut BTreeSet<PathBuf>,
    path: PathBuf,
    mode: RecursiveMode,
    output: &Output,
) -> bool {
    if !path.is_dir() {
        return false;
    }
    let watched = match mode {
        RecursiveMode::Recursive => recursive.contains(&path),
        RecursiveMode::NonRecursive => recursive.contains(&path) || shallow.contains(&path),
    };
    if watched {
        return false;
    }
    if mode == RecursiveMode::Recursive && shallow.remove(&path) {
        let _ = watcher.unwatch(&path);
    }
    if let Err(error) = watcher.watch(&path, mode) {
        let _ = output.send(LibraryEvent::Error(format!("{}: {error}", path.display())));
        return false;
    }
    match mode {
        RecursiveMode::Recursive => {
            recursive.insert(path);
        }
        RecursiveMode::NonRecursive => {
            shallow.insert(path);
        }
    }
    true
}

/// Convert a debounced batch into stable path events. All filesystem existence
/// checks happen on the watcher worker, never in a notify callback or UI poll.
fn reconcile_events(
    events: impl IntoIterator<Item = Event>,
    roots: &BTreeSet<PathBuf>,
) -> Vec<LibraryEvent> {
    let mut renamed = BTreeSet::new();
    // A create hint means metadata must be imported if the file still exists.
    // Existence at the end of the batch decides removal, regardless of event
    // ordering (including remove/create/remove and atomic replacements).
    let mut candidates = BTreeMap::<PathBuf, bool>::new();
    let mut refresh = BTreeSet::new();
    for event in events {
        if event.need_rescan() || (event.kind == EventKind::Any && event.paths.is_empty()) {
            refresh.extend(roots.iter().cloned());
        }
        if !is_mutation(event.kind) {
            continue;
        }
        let paths: Vec<_> = event
            .paths
            .iter()
            .map(|path| super::deletion_monitor::normalize(path))
            .collect();

        // Filter each side only after examining a rename pair: moving out of a
        // root still removes the old path, and moving in still imports the new.
        if matches!(
            event.kind,
            EventKind::Modify(ModifyKind::Name(RenameMode::Both))
        ) && paths.len() == 2
            && paths.iter().all(|path| is_in_roots(path, roots))
            && paths
                .iter()
                .all(|path| super::deletion_monitor::is_audio(path))
            && matches!(path_state(&paths[0]), PathState::Missing)
            && matches!(path_state(&paths[1]), PathState::File)
        {
            renamed.insert((paths[0].clone(), paths[1].clone()));
        }
        for (index, path) in paths.into_iter().enumerate() {
            if roots.iter().any(|root| root.starts_with(&path)) {
                refresh.extend(roots.iter().filter(|root| root.starts_with(&path)).cloned());
            }
            if !is_in_roots(&path, roots) {
                continue;
            }
            let state = path_state(&path);
            let audio = super::deletion_monitor::is_audio(&path);
            if is_directory_hint(event.kind, &path, state) {
                refresh.extend(roots_for_path(&path, roots).cloned());
            } else if audio {
                let added = matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(RenameMode::To))
                ) || (matches!(
                    event.kind,
                    EventKind::Modify(ModifyKind::Name(RenameMode::Both))
                ) && index == 1);
                *candidates.entry(path).or_default() |= added;
            }
        }
    }

    let mut output = Vec::new();
    for (from, to) in renamed {
        // Verify both sides again: earlier rename hints may be stale by the end
        // of a burst, or the source may have been recreated independently.
        if available_root(&from, roots)
            && available_root(&to, roots)
            && matches!(path_state(&from), PathState::Missing)
            && matches!(path_state(&to), PathState::File)
        {
            candidates.remove(&from);
            candidates.remove(&to);
            output.push(LibraryEvent::Renamed { from, to });
        }
    }
    for (path, added) in candidates {
        let state = path_state(&path);
        // Permission/I/O errors and a disconnected root cannot confirm removal.
        if !available_root(&path, roots) {
            continue;
        }
        match state {
            PathState::File => output.push(if added {
                LibraryEvent::Added(path)
            } else {
                LibraryEvent::Modified(path)
            }),
            PathState::Missing => output.push(LibraryEvent::Removed(path)),
            PathState::Directory | PathState::Unknown => {
                refresh.extend(roots_for_path(&path, roots).cloned());
            }
            PathState::Other => {}
        }
    }
    output.extend(refresh_roots(&refresh));
    sort_events(&mut output);
    output
}

#[derive(Clone, Copy)]
enum PathState {
    File,
    Directory,
    Other,
    Missing,
    Unknown,
}

fn path_state(path: &Path) -> PathState {
    classify_metadata(std::fs::metadata(path))
}

fn classify_metadata(result: io::Result<std::fs::Metadata>) -> PathState {
    match result {
        Ok(metadata) if metadata.is_file() => PathState::File,
        Ok(metadata) if metadata.is_dir() => PathState::Directory,
        Ok(_) => PathState::Other,
        Err(error) if error.kind() == io::ErrorKind::NotFound => PathState::Missing,
        Err(_) => PathState::Unknown,
    }
}

fn is_mutation(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_) | EventKind::Any
    )
}

fn is_directory_hint(kind: EventKind, path: &Path, state: PathState) -> bool {
    matches!(
        kind,
        EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder)
    ) || matches!(state, PathState::Directory | PathState::Unknown)
        || (!super::deletion_monitor::is_audio(path)
            && matches!(state, PathState::Missing)
            && !matches!(
                kind,
                EventKind::Create(CreateKind::File) | EventKind::Remove(RemoveKind::File)
            ))
}

fn available_root(path: &Path, roots: &BTreeSet<PathBuf>) -> bool {
    roots_for_path(path, roots).any(|root| root.is_dir())
}

fn refresh_roots(roots: &BTreeSet<PathBuf>) -> Vec<LibraryEvent> {
    roots
        .iter()
        .filter(|root| root.is_dir())
        .cloned()
        .map(LibraryEvent::RefreshRoot)
        .collect()
}

fn roots_to_rearm(
    events: &[Event],
    roots: &BTreeSet<PathBuf>,
    lost_events: bool,
) -> BTreeSet<PathBuf> {
    roots
        .iter()
        .filter(|root| {
            lost_events
                || events.iter().any(|event| {
                    event.need_rescan()
                        || (event.kind == EventKind::Any && event.paths.is_empty())
                        || (matches!(
                            event.kind,
                            EventKind::Create(_)
                                | EventKind::Remove(_)
                                | EventKind::Modify(ModifyKind::Name(_))
                                | EventKind::Any
                        ) && event
                            .paths
                            .iter()
                            .any(|path| root.starts_with(super::deletion_monitor::normalize(path))))
                        || (is_mutation(event.kind)
                            && event.paths.iter().any(|path| {
                                let path = super::deletion_monitor::normalize(path);
                                path.starts_with(root)
                                    && is_directory_hint(event.kind, &path, path_state(&path))
                            }))
                })
        })
        .cloned()
        .collect()
}

fn sort_events(events: &mut Vec<LibraryEvent>) {
    // Include the variant in the ordering so duplicate root hints remain
    // adjacent even when another event has the same path.
    events.sort_by(|a, b| {
        event_path(a)
            .cmp(event_path(b))
            .then_with(|| event_order(a).cmp(&event_order(b)))
    });
    events.dedup();
}

fn event_order(event: &LibraryEvent) -> u8 {
    match event {
        LibraryEvent::Removed(_) => 0,
        LibraryEvent::Renamed { .. } => 1,
        LibraryEvent::Added(_) => 2,
        LibraryEvent::Modified(_) => 3,
        LibraryEvent::RefreshRoot(_) => 4,
        LibraryEvent::Error(_) => 5,
    }
}

fn event_path(event: &LibraryEvent) -> &Path {
    match event {
        LibraryEvent::Added(path)
        | LibraryEvent::Removed(path)
        | LibraryEvent::Modified(path)
        | LibraryEvent::RefreshRoot(path) => path,
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

    #[test]
    fn recreate_batches_use_final_file_state() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let path = root.join("track.mp3");
        let roots = BTreeSet::from([root]);
        let hints = [
            event(EventKind::Create(CreateKind::File), [path.clone()]),
            event(EventKind::Remove(RemoveKind::File), [path.clone()]),
            event(EventKind::Create(CreateKind::File), [path.clone()]),
            event(EventKind::Remove(RemoveKind::File), [path.clone()]),
        ];
        assert_eq!(
            reconcile_events(hints.clone(), &roots),
            vec![LibraryEvent::Removed(path.clone())]
        );
        fs::write(&path, b"recreated audio").unwrap();
        assert_eq!(
            reconcile_events(hints, &roots),
            vec![LibraryEvent::Added(path)]
        );
    }

    #[test]
    fn removal_hint_for_a_recreated_file_refreshes_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("track.mp3");
        fs::write(&path, b"replacement audio").unwrap();
        assert_eq!(
            reconcile_events(
                [event(EventKind::Remove(RemoveKind::File), [path.clone()])],
                &BTreeSet::from([directory.path().to_path_buf()]),
            ),
            vec![LibraryEvent::Modified(path)]
        );
    }

    #[test]
    fn rename_crossing_root_boundary_reconciles_each_side() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        fs::create_dir(&root).unwrap();
        let inside = root.join("track.mp3");
        let outside = directory.path().join("track.mp3");
        let roots = BTreeSet::from([root]);
        fs::write(&outside, b"audio").unwrap();
        assert_eq!(
            reconcile_events(
                [event(
                    EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                    [inside.clone(), outside.clone()]
                )],
                &roots,
            ),
            vec![LibraryEvent::Removed(inside.clone())]
        );
        fs::rename(&outside, &inside).unwrap();
        assert_eq!(
            reconcile_events(
                [event(
                    EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                    [outside, inside.clone()]
                )],
                &roots,
            ),
            vec![LibraryEvent::Added(inside)]
        );
    }

    #[test]
    fn recreated_rename_source_is_kept_and_destination_is_added() {
        let directory = tempfile::tempdir().unwrap();
        let from = directory.path().join("old.mp3");
        let to = directory.path().join("new.mp3");
        fs::write(&from, b"replacement").unwrap();
        fs::write(&to, b"renamed audio").unwrap();
        assert_eq!(
            reconcile_events(
                [event(
                    EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                    [from.clone(), to.clone()]
                )],
                &BTreeSet::from([directory.path().to_path_buf()]),
            ),
            vec![LibraryEvent::Added(to), LibraryEvent::Modified(from)]
        );
    }

    #[test]
    fn directory_create_rename_remove_request_one_root_refresh() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let created = root.join("new.mp3"); // A folder can have an audio extension.
        fs::create_dir(&created).unwrap();
        let before = root.join("before");
        let after = root.join("after");
        fs::create_dir(&after).unwrap();
        let roots = BTreeSet::from([root.clone()]);
        let hints = [
            event(EventKind::Create(CreateKind::Folder), [created]),
            event(
                EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                [before, after],
            ),
            event(
                EventKind::Remove(RemoveKind::Folder),
                [root.join("removed")],
            ),
        ];
        assert_eq!(roots_to_rearm(&hints, &roots, false), roots);
        assert_eq!(
            reconcile_events(hints, &roots),
            vec![LibraryEvent::RefreshRoot(root)]
        );
    }

    #[test]
    fn rescan_flags_and_unknown_loss_request_available_roots() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let roots = BTreeSet::from([root.clone(), root.join("unavailable")]);
        let hint = event(EventKind::Other, []).set_flag(notify::event::Flag::Rescan);
        assert_eq!(
            roots_to_rearm(std::slice::from_ref(&hint), &roots, false),
            roots
        );
        assert_eq!(
            reconcile_events([hint], &roots),
            vec![LibraryEvent::RefreshRoot(root.clone())]
        );
        assert_eq!(refresh_roots(&roots), vec![LibraryEvent::RefreshRoot(root)]);
        assert_eq!(roots_to_rearm(&[], &roots, true), roots);
    }

    #[test]
    fn inaccessible_metadata_is_not_a_confirmed_missing_file() {
        for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
            assert!(matches!(
                classify_metadata(Err(io::Error::from(kind))),
                PathState::Unknown
            ));
        }
        assert!(matches!(
            classify_metadata(Err(io::Error::from(io::ErrorKind::NotFound))),
            PathState::Missing
        ));
    }

    fn await_event(
        monitor: &Monitor,
        mut predicate: impl FnMut(&LibraryEvent) -> bool,
    ) -> LibraryEvent {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut seen = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(
                !remaining.is_zero(),
                "native watcher timed out; received {seen:?}"
            );
            match monitor.events.recv_timeout(remaining) {
                Ok(event) => {
                    // Native backends may report a transient ENOENT while a
                    // directory is moved/deleted. Require the typed result to
                    // arrive within the deadline despite those warnings.
                    if predicate(&event) {
                        return event;
                    }
                    seen.push(event);
                }
                Err(error) => {
                    panic!("native watcher timed out/disconnected: {error}; received {seen:?}")
                }
            }
        }
    }

    fn await_root(monitor: &Monitor, root: &Path) {
        await_event(
            monitor,
            |event| matches!(event, LibraryEvent::RefreshRoot(path) if path == root),
        );
    }

    fn await_watches(monitor: &Monitor) {
        let (sender, ready) = mpsc::channel();
        monitor.commands.send(Command::Ready(sender)).unwrap();
        ready.recv_timeout(Duration::from_secs(3)).unwrap();
    }

    #[test]
    fn native_leaf_create_modify_rename_remove() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let path = root.join("track.mp3");
        let to = root.join("renamed.mp3");
        let monitor = watch(vec![root.clone()]);
        await_root(&monitor, &root); // Watch attachment is the readiness barrier.
        fs::write(&path, b"audio").unwrap();
        await_event(
            &monitor,
            |event| matches!(event, LibraryEvent::Added(found) if found == &path),
        );
        fs::write(&path, b"changed audio content").unwrap();
        await_event(
            &monitor,
            |event| matches!(event, LibraryEvent::Modified(found) if found == &path),
        );
        fs::rename(&path, &to).unwrap();
        let mut old_removed = false;
        let mut new_added = false;
        await_event(&monitor, |event| {
            match event {
                LibraryEvent::Renamed { from, to: found } if from == &path && found == &to => {
                    old_removed = true;
                    new_added = true;
                }
                LibraryEvent::Added(found) if found == &to => new_added = true,
                LibraryEvent::Removed(found) if found == &path => old_removed = true,
                _ => {}
            }
            old_removed && new_added
        });
        fs::remove_file(&to).unwrap();
        await_event(
            &monitor,
            |event| matches!(event, LibraryEvent::Removed(found) if found == &to),
        );
    }

    #[test]
    fn native_directory_move_remove_and_child_events_after_reattachment() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("library");
        let incoming = directory.path().join("incoming");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&incoming).unwrap();
        fs::write(incoming.join("track.mp3"), b"audio").unwrap();
        let monitor = watch(vec![root.clone()]);
        await_root(&monitor, &root);
        let album = root.join("album");
        fs::rename(&incoming, &album).unwrap();
        await_root(&monitor, &root);
        let path = album.join("track.mp3");
        fs::write(&path, b"changed after folder import").unwrap();
        await_event(
            &monitor,
            |event| matches!(event, LibraryEvent::Modified(found) if found == &path),
        );
        let renamed = root.join("renamed");
        fs::rename(&album, &renamed).unwrap();
        await_root(&monitor, &root);
        let path = renamed.join("track.mp3");
        fs::write(&path, b"changed after folder rename").unwrap();
        await_event(
            &monitor,
            |event| matches!(event, LibraryEvent::Modified(found) if found == &path),
        );
        fs::remove_dir_all(&renamed).unwrap();
        await_root(&monitor, &root);
    }

    #[test]
    fn native_root_returns_with_preexisting_and_new_audio() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("parent").join("library");
        let monitor = watch(vec![root.clone()]);
        await_watches(&monitor);
        // Reattachment is polled even if the entire parent chain appears at
        // once and shallow native notifications do not cover the final root.
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("preexisting.mp3"), b"audio").unwrap();
        await_root(&monitor, &root);
        let hidden = directory.path().join("hidden");
        fs::rename(root.parent().unwrap(), &hidden).unwrap();
        // Keep the root absent beyond one reconciliation pass.
        thread::sleep(Duration::from_millis(700));
        assert!(
            !monitor
                .events
                .try_iter()
                .any(|event| matches!(event, LibraryEvent::Removed(_)))
        );
        fs::rename(&hidden, root.parent().unwrap()).unwrap();
        await_root(&monitor, &root);
        let path = root.join("new.mp3");
        fs::write(&path, b"new audio after recovery").unwrap();
        await_event(
            &monitor,
            |event| matches!(event, LibraryEvent::Added(found) if found == &path),
        );
    }

    #[test]
    fn native_root_recreated_inside_one_debounce_batch_is_reattached() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("library");
        fs::create_dir(&root).unwrap();
        let monitor = watch(vec![root.clone()]);
        await_root(&monitor, &root);
        fs::remove_dir(&root).unwrap();
        fs::create_dir(&root).unwrap();
        let path = root.join("restored.mp3");
        fs::write(&path, b"restored during debounce").unwrap();
        await_root(&monitor, &root);
        fs::write(&path, b"modified on the reattached watch").unwrap();
        await_event(
            &monitor,
            |event| matches!(event, LibraryEvent::Modified(found) if found == &path),
        );
    }

    #[test]
    fn native_added_root_is_refreshed_and_watched() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let monitor = watch(Vec::new());
        await_watches(&monitor);
        monitor.add_roots([root.clone()]);
        await_root(&monitor, &root);
        let path = root.join("added-root.mp3");
        fs::write(&path, b"audio").unwrap();
        await_event(
            &monitor,
            |event| matches!(event, LibraryEvent::Added(found) if found == &path),
        );
    }

    #[test]
    fn dropping_monitor_stops_idle_worker() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let mut monitor = watch(vec![root.clone()]);
        await_root(&monitor, &root);
        let worker = monitor.worker.take().unwrap();
        drop(monitor);
        await_worker_exit(worker);
    }

    #[test]
    fn shutdown_and_disconnect_stop_a_full_output_queue() {
        let directory = tempfile::tempdir().unwrap();
        let roots: Vec<_> = ["one", "two", "three"]
            .map(|name| directory.path().join(name))
            .into_iter()
            .collect();
        for root in &roots {
            fs::create_dir(root).unwrap();
        }
        let mut monitor = start_monitor(roots, 1);
        monitor.events.recv_timeout(Duration::from_secs(3)).unwrap();
        // The next root fills the one-slot queue, and the third root blocks
        // publication. Drop must stop that worker without polling its output.
        thread::sleep(Duration::from_millis(30));
        let worker = monitor.worker.take().unwrap();
        assert!(!worker.is_finished());
        drop(monitor);
        await_worker_exit(worker);

        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        assert!(
            !Output {
                sender,
                stopped: Arc::new(AtomicBool::new(false))
            }
            .send(LibraryEvent::Error("closed".into()))
        );
    }

    fn await_worker_exit(worker: JoinHandle<()>) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !worker.is_finished() {
            assert!(Instant::now() < deadline, "watcher worker did not stop");
            thread::sleep(Duration::from_millis(10));
        }
        worker.join().unwrap();
    }
}
