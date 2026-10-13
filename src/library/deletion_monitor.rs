use notify::{RecursiveMode, Watcher};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

#[derive(Debug)]
pub enum EventMessage {
    Removed(PathBuf),
    Restored(PathBuf),
    RootChanged(PathBuf),
    Error(String),
}

pub struct Monitor {
    pub events: Receiver<EventMessage>,
    roots: Sender<PathBuf>,
}

impl Monitor {
    pub fn add_roots(&self, roots: impl IntoIterator<Item = PathBuf>) {
        for root in roots {
            let _ = self.roots.send(root);
        }
    }
}

fn watch_root(
    watcher: &mut notify::RecommendedWatcher,
    watched: &mut BTreeSet<PathBuf>,
    root: PathBuf,
    output: &mpsc::Sender<EventMessage>,
) {
    let root = normalize(&root);
    if root.is_dir()
        && watched.insert(root.clone())
        && let Err(error) = watcher.watch(&root, RecursiveMode::Recursive)
    {
        let _ = output.send(EventMessage::Error(format!("{}: {error}", root.display())));
    }
}

/// Watches roots on a worker and coalesces notifications before checking disk state.
pub fn watch(roots: Vec<PathBuf>) -> Monitor {
    let (output, receiver) = mpsc::channel();
    let (root_sender, root_receiver) = mpsc::channel();
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
        let mut watched = BTreeSet::new();
        for root in roots {
            watch_root(&mut watcher, &mut watched, root, &output);
        }
        loop {
            match root_receiver.try_recv() {
                Ok(root) => watch_root(&mut watcher, &mut watched, root, &output),
                Err(mpsc::TryRecvError::Disconnected) => return,
                Err(mpsc::TryRecvError::Empty) => {}
            }
            for root in root_receiver.try_iter() {
                watch_root(&mut watcher, &mut watched, root, &output);
            }
            let first = match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(first) => first,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            };
            let mut batch = vec![first];
            while batch.len() < 2048 {
                match rx.recv_timeout(Duration::from_millis(220)) {
                    Ok(event) => batch.push(event),
                    Err(_) => break,
                }
            }
            let mut candidates = BTreeSet::new();
            for event in batch {
                match event {
                    Ok(event) => {
                        for path in event.paths.into_iter().map(|path| normalize(&path)) {
                            if path.is_dir() {
                                let _ = output.send(EventMessage::RootChanged(path));
                                continue;
                            }
                            if !is_audio(&path) {
                                continue;
                            }
                            if path.is_file() {
                                let _ = output.send(EventMessage::Restored(path));
                            } else if !path.exists() {
                                // Some providers express removal as a generic name/modify event.
                                // The post-event filesystem check remains the deletion proof.
                                candidates.insert(path);
                            }
                        }
                    }
                    Err(error) => {
                        let _ = output.send(EventMessage::Error(error.to_string()));
                    }
                }
            }
            for path in candidates {
                if is_audio(&path) && !path.exists() {
                    let _ = output.send(EventMessage::Removed(path));
                }
            }
            if overflow.swap(false, Ordering::Relaxed) {
                // Reconcile known paths under healthy roots after dropping excess OS hints.
                for root in &watched {
                    let _ = output.send(EventMessage::RootChanged(root.clone()));
                }
                let _ = output.send(EventMessage::Error(
                    "Filesystem event burst exceeded the monitor queue; library paths were reconciled"
                        .to_owned(),
                ));
            }
        }
    });
    Monitor {
        events: receiver,
        roots: root_sender,
    }
}

pub fn normalize(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        };
        let mut normalized = PathBuf::new();
        for component in absolute.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                other => normalized.push(other.as_os_str()),
            }
        }
        normalized
    })
}

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp3" | "flac" | "wav" | "m4a" | "aac" | "ogg" | "aiff"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_keeps_missing_path_identity_and_audio_filter_is_case_insensitive() {
        let path = PathBuf::from("/tmp/not-created/../not-created/track.MP3");
        assert!(normalize(&path).ends_with("not-created/track.MP3"));
        assert!(is_audio(Path::new("track.MP3")));
        assert!(!is_audio(Path::new("cover.jpg")));
    }
}
