use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

/// A watcher event is only a hint. Deletions are accepted only after checking the path.
pub fn confirmed_missing_audio(event: &Event, audio_extensions: &BTreeSet<String>) -> Vec<PathBuf> {
    if !matches!(event.kind, EventKind::Remove(_)) {
        return Vec::new();
    }
    event
        .paths
        .iter()
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| audio_extensions.contains(&extension.to_ascii_lowercase()))
                && !path.exists()
        })
        .cloned()
        .collect()
}

/// Conservative policy: unavailable roots are suspended, never interpreted as mass deletion.
pub fn root_is_available(root: &Path) -> bool {
    root.is_dir()
}

pub fn temporary_watcher(root: &Path) -> notify::Result<Receiver<notify::Result<Event>>> {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = sender.send(event);
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    // notify's watcher owns OS resources; retain it on a detached keepalive thread.
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(60));
            std::hint::black_box(&watcher);
        }
    });
    Ok(receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::RemoveKind;
    use std::{fs, thread, time::Instant};

    fn audio_extensions() -> BTreeSet<String> {
        ["mp3", "flac", "wav", "m4a", "aac", "ogg", "aiff"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn only_confirmed_audio_removals_are_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let deleted = dir.path().join("gone.MP3");
        let still_present = dir.path().join("present.flac");
        fs::write(&still_present, b"fixture").unwrap();
        let event = Event::new(EventKind::Remove(RemoveKind::File))
            .add_path(deleted.clone())
            .add_path(still_present);
        assert_eq!(
            confirmed_missing_audio(&event, &audio_extensions()),
            vec![deleted]
        );
        assert!(
            confirmed_missing_audio(
                &Event::new(EventKind::Create(notify::event::CreateKind::File))
                    .add_path(dir.path().join("other.mp3")),
                &audio_extensions()
            )
            .is_empty()
        );
    }

    #[test]
    fn root_disappearance_is_suspension_and_restoration_is_recoverable() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("removable-drive");
        fs::create_dir(&root).unwrap();
        assert!(root_is_available(&root));
        fs::remove_dir(&root).unwrap();
        assert!(!root_is_available(&root));
        fs::create_dir(&root).unwrap();
        assert!(root_is_available(&root));
    }

    #[test]
    fn real_watcher_observes_temporary_fixture_removal() {
        let dir = tempfile::tempdir().unwrap();
        let receiver = temporary_watcher(dir.path()).unwrap();
        let fixture = dir.path().join("temporary.wav");
        fs::write(&fixture, b"never user media").unwrap();
        thread::sleep(Duration::from_millis(100));
        fs::remove_file(&fixture).unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut saw_confirmed = false;
        while Instant::now() < deadline {
            if let Ok(Ok(event)) = receiver.recv_timeout(Duration::from_millis(100))
                && !confirmed_missing_audio(&event, &audio_extensions()).is_empty()
            {
                saw_confirmed = true;
                break;
            }
        }
        assert!(
            saw_confirmed,
            "watcher should report a confirmed temporary-file removal"
        );
    }
}
