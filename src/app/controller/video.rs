//! Background video-library and subtitle-picker tasks. Playback, UI state, and
//! subtitle application remain with the controller.

use crate::video::{
    import::{self, Event},
    library::Video,
};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
};

pub(super) struct SubtitleSelection {
    pub(super) generation: u64,
    pub(super) video_path: PathBuf,
    pub(super) selected: Option<PathBuf>,
}

/// Owns the one-shot load, import event stream, and captured picker identity.
/// Workers keep their existing detached lifetime: dropping a receiver never
/// blocks the UI waiting for file probing or a native picker to finish.
pub(super) struct VideoTasks {
    directory: PathBuf,
    loading: Option<Receiver<Result<Vec<Video>, String>>>,
    importing: Option<Receiver<Event>>,
    subtitle_picker: Option<Receiver<SubtitleSelection>>,
}

impl VideoTasks {
    pub(super) fn new(directory: PathBuf) -> Self {
        let loading = Some(import::load(directory.join("videos.json")));
        Self {
            directory,
            loading,
            importing: None,
            subtitle_picker: None,
        }
    }

    pub(super) fn start_import(&mut self, title: String, paths: Option<Vec<PathBuf>>) {
        self.importing = Some(import::start(
            self.directory.join("videos.json"),
            title,
            paths,
        ));
    }

    pub(super) fn subtitle_pending(&self) -> bool {
        self.subtitle_picker.is_some()
    }

    pub(super) fn start_subtitle_import(
        &mut self,
        generation: u64,
        video_path: PathBuf,
        title: String,
    ) {
        self.start_subtitle_picker(generation, video_path, move || {
            rfd::FileDialog::new()
                .set_title(&title)
                .add_filter("ASS / SSA Subtitles", &["ass", "ssa"])
                .pick_file()
        });
    }

    fn start_subtitle_picker(
        &mut self,
        generation: u64,
        video_path: PathBuf,
        pick: impl FnOnce() -> Option<PathBuf> + Send + 'static,
    ) {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(SubtitleSelection {
                generation,
                video_path,
                selected: pick(),
            });
        });
        self.subtitle_picker = Some(receiver);
    }

    pub(super) fn poll_subtitle(&mut self) -> Option<SubtitleSelection> {
        let result = self.subtitle_picker.as_ref()?.try_recv().ok()?;
        self.subtitle_picker = None;
        Some(result)
    }

    pub(super) fn poll_loading(&mut self) -> Option<Result<Vec<Video>, String>> {
        let result = self.loading.as_ref()?.try_recv().ok()?;
        self.loading = None;
        Some(result)
    }

    pub(super) fn poll_import(&mut self) -> Option<Event> {
        let event = self.importing.as_ref()?.try_recv().ok()?;
        if matches!(&event, Event::Completed(_)) {
            self.importing = None;
        }
        Some(event)
    }
}

/// A picker result is valid only for the active video that opened the picker.
pub(super) fn subtitle_import_matches(
    generation: u64,
    video_path: &Path,
    video_active: bool,
    current_generation: u64,
    current_path: Option<&Path>,
) -> bool {
    video_active && current_generation == generation && current_path == Some(video_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn empty_tasks(directory: &Path) -> VideoTasks {
        VideoTasks {
            directory: directory.to_owned(),
            loading: None,
            importing: None,
            subtitle_picker: None,
        }
    }

    fn wait_for<T>(mut poll: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(value) = poll() {
                return value;
            }
            assert!(Instant::now() < deadline, "background task timed out");
            std::thread::yield_now();
        }
    }

    fn video(path: PathBuf) -> Video {
        Video {
            path,
            title: "Existing video".into(),
            duration: 4.0,
            width: 320,
            height: 180,
            display_aspect: 16.0 / 9.0,
            video_codec: "h264".into(),
            audio_codec: None,
        }
    }

    #[test]
    fn startup_load_publishes_fixture_once_without_rewriting_it() {
        let directory = tempfile::tempdir().unwrap();
        let videos = vec![video(directory.path().join("existing.mp4"))];
        let database = directory.path().join("videos.json");
        let bytes = serde_json::to_vec(&videos).unwrap();
        std::fs::write(&database, &bytes).unwrap();
        let mut tasks = VideoTasks::new(directory.path().to_owned());

        assert_eq!(wait_for(|| tasks.poll_loading()).unwrap(), videos);
        assert!(tasks.poll_loading().is_none());
        assert!(tasks.loading.is_none());
        assert_eq!(std::fs::read(database).unwrap(), bytes);
    }

    #[test]
    fn corrupt_startup_library_reports_error_once_and_preserves_file() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("videos.json");
        std::fs::write(&database, b"broken").unwrap();
        let mut tasks = VideoTasks::new(directory.path().to_owned());

        assert!(wait_for(|| tasks.poll_loading()).is_err());
        assert!(tasks.poll_loading().is_none());
        assert_eq!(std::fs::read(database).unwrap(), b"broken");
    }

    #[test]
    fn import_worker_closes_selection_before_completion_and_keeps_existing_videos() {
        let directory = tempfile::tempdir().unwrap();
        let videos = vec![video(directory.path().join("existing.mp4"))];
        let database = directory.path().join("videos.json");
        std::fs::write(&database, serde_json::to_vec(&videos).unwrap()).unwrap();
        let mut tasks = empty_tasks(directory.path());
        tasks.start_import(String::new(), Some(Vec::new()));

        assert!(matches!(
            wait_for(|| tasks.poll_import()),
            Event::SelectionClosed
        ));
        assert!(tasks.importing.is_some());
        let Event::Completed(Ok(Some(result))) = wait_for(|| tasks.poll_import()) else {
            panic!("expected successful import completion");
        };
        assert_eq!(result.videos, videos);
        assert_eq!(result.imported, 0);
        assert!(result.errors.is_empty());
        assert!(tasks.importing.is_none());
        assert!(tasks.poll_import().is_none());
        assert_eq!(crate::video::library::load(&database).unwrap(), videos);
    }

    #[test]
    fn failed_import_completes_after_selection_and_preserves_corrupt_library() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("videos.json");
        std::fs::write(&database, b"broken").unwrap();
        let mut tasks = empty_tasks(directory.path());
        tasks.start_import(String::new(), Some(Vec::new()));

        assert!(matches!(
            wait_for(|| tasks.poll_import()),
            Event::SelectionClosed
        ));
        assert!(matches!(
            wait_for(|| tasks.poll_import()),
            Event::Completed(Err(_))
        ));
        assert!(tasks.importing.is_none());
        assert_eq!(std::fs::read(database).unwrap(), b"broken");
    }

    #[test]
    fn picker_preserves_request_identity_and_rejects_changed_video_or_generation() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.mp4");
        let second = directory.path().join("second.mp4");
        let subtitle = directory.path().join("first.ass");
        let (selected, selection) = mpsc::channel();
        let mut tasks = empty_tasks(directory.path());
        tasks.start_subtitle_picker(8, first.clone(), move || selection.recv().unwrap());

        assert!(tasks.subtitle_pending());
        assert!(tasks.poll_subtitle().is_none());
        selected.send(Some(subtitle.clone())).unwrap();
        let result = wait_for(|| tasks.poll_subtitle());
        assert_eq!(result.generation, 8);
        assert_eq!(result.video_path, first);
        assert_eq!(result.selected, Some(subtitle));
        assert!(subtitle_import_matches(8, &first, true, 8, Some(&first)));
        assert!(!subtitle_import_matches(8, &first, true, 9, Some(&first)));
        assert!(!subtitle_import_matches(8, &first, true, 8, Some(&second)));
        assert!(!subtitle_import_matches(8, &first, false, 8, Some(&first)));
        assert!(!subtitle_import_matches(8, &first, true, 8, None));
        assert!(!tasks.subtitle_pending());
        assert!(tasks.poll_subtitle().is_none());
    }

    #[test]
    fn cancelled_picker_still_completes_and_releases_pending_request() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("first.mp4");
        let mut tasks = empty_tasks(directory.path());
        tasks.start_subtitle_picker(8, path.clone(), || None);

        let result = wait_for(|| tasks.poll_subtitle());
        assert_eq!(result.generation, 8);
        assert_eq!(result.video_path, path);
        assert!(result.selected.is_none());
        assert!(!tasks.subtitle_pending());
    }

    #[test]
    fn disconnected_receivers_preserve_existing_pending_semantics() {
        let directory = tempfile::tempdir().unwrap();
        let mut tasks = empty_tasks(directory.path());
        let (sender, receiver) = mpsc::channel();
        tasks.loading = Some(receiver);
        drop(sender);
        let (sender, receiver) = mpsc::channel();
        tasks.importing = Some(receiver);
        drop(sender);
        let (sender, receiver) = mpsc::channel();
        tasks.subtitle_picker = Some(receiver);
        drop(sender);

        assert!(tasks.poll_loading().is_none());
        assert!(tasks.loading.is_some());
        assert!(tasks.poll_import().is_none());
        assert!(tasks.importing.is_some());
        assert!(tasks.poll_subtitle().is_none());
        assert!(tasks.subtitle_pending());
    }
}
