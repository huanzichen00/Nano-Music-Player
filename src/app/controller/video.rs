//! Background video-library and subtitle-picker tasks. Playback, UI state, and
//! subtitle application remain with the controller.

use crate::video::{
    import::{self, Event},
    library::Video,
};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};

pub(super) struct SubtitleSelection {
    pub(super) generation: u64,
    pub(super) video_path: PathBuf,
    pub(super) selected: Option<PathBuf>,
}

pub(super) struct ImportUpdate {
    pub(super) event: Event,
    /// Only the task that opened the native picker can release the modal gate.
    pub(super) picker_closed: bool,
}

/// Owns the one-shot load, import event stream, and captured picker identity.
/// Workers keep their existing detached lifetime: dropping a receiver never
/// blocks the UI waiting for file probing or a native picker to finish.
pub(super) struct VideoTasks {
    database: PathBuf,
    loading: Option<Receiver<Result<Vec<Video>, String>>>,
    importing: bool,
    import_picker_open: bool,
    library: import::Worker,
    library_stopped: bool,
    subtitle_picker: Option<Receiver<SubtitleSelection>>,
}

impl VideoTasks {
    pub(super) fn new(directory: PathBuf) -> Self {
        let database = directory.join("videos.json");
        let loading = Some(import::load(database.clone()));
        let library = import::Worker::new(database.clone());
        Self {
            database,
            loading,
            importing: false,
            import_picker_open: false,
            library,
            library_stopped: false,
            subtitle_picker: None,
        }
    }

    pub(super) fn start_import(&mut self, title: String, paths: Option<Vec<PathBuf>>) {
        if self.importing {
            return;
        }
        self.restart_library();
        self.importing = true;
        self.import_picker_open = paths.is_none();
        self.library.start_import(title, paths);
    }

    pub(super) fn remove_missing(&mut self, path: PathBuf) {
        self.restart_library();
        self.library.remove_missing(path);
    }

    fn restart_library(&mut self) {
        if self.library_stopped {
            self.library = import::Worker::new(self.database.clone());
            self.library_stopped = false;
        }
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

    pub(super) fn poll_subtitle(&mut self) -> Option<Result<SubtitleSelection, String>> {
        let result = match self.subtitle_picker.as_ref()?.try_recv() {
            Ok(result) => Ok(result),
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                Err("Subtitle selection stopped unexpectedly".into())
            }
        };
        self.subtitle_picker = None;
        Some(result)
    }

    pub(super) fn poll_loading(&mut self) -> Option<Result<Vec<Video>, String>> {
        let result = match self.loading.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("Video loading stopped unexpectedly".into()),
        };
        self.loading = None;
        Some(result)
    }

    pub(super) fn poll_import(&mut self) -> Option<ImportUpdate> {
        if self.library_stopped {
            return None;
        }
        let event = match self.library.events.try_recv() {
            Ok(event) => event,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                self.importing = false;
                self.library_stopped = true;
                Event::WorkerStopped
            }
        };
        if matches!(&event, Event::Completed(_)) {
            self.importing = false;
        }
        let picker_closed = matches!(
            &event,
            Event::SelectionClosed | Event::Completed(_) | Event::WorkerStopped
        ) && std::mem::take(&mut self.import_picker_open);
        Some(ImportUpdate {
            event,
            picker_closed,
        })
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
            database: directory.join("videos.json"),
            loading: None,
            importing: false,
            import_picker_open: false,
            library: import::Worker::new(directory.join("videos.json")),
            library_stopped: false,
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
            wait_for(|| tasks.poll_import()).event,
            Event::SelectionClosed
        ));
        assert!(tasks.importing);
        let Event::Completed(Ok(Some(result))) = wait_for(|| tasks.poll_import()).event else {
            panic!("expected successful import completion");
        };
        assert_eq!(result.videos, videos);
        assert_eq!(result.imported, 0);
        assert!(result.errors.is_empty());
        assert!(!tasks.importing);
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
            wait_for(|| tasks.poll_import()).event,
            Event::SelectionClosed
        ));
        assert!(matches!(
            wait_for(|| tasks.poll_import()).event,
            Event::Completed(Err(_))
        ));
        assert!(!tasks.importing);
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
        let result = wait_for(|| tasks.poll_subtitle()).unwrap();
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

        let result = wait_for(|| tasks.poll_subtitle()).unwrap();
        assert_eq!(result.generation, 8);
        assert_eq!(result.video_path, path);
        assert!(result.selected.is_none());
        assert!(!tasks.subtitle_pending());
    }

    #[test]
    fn disconnected_receivers_report_once_release_pending_and_allow_retry() {
        let directory = tempfile::tempdir().unwrap();
        let mut tasks = empty_tasks(directory.path());
        let (sender, receiver) = mpsc::channel();
        tasks.loading = Some(receiver);
        drop(sender);
        let (sender, receiver) = mpsc::channel();
        tasks.subtitle_picker = Some(receiver);
        drop(sender);

        assert!(matches!(tasks.poll_loading(), Some(Err(message))
            if message == "Video loading stopped unexpectedly"));
        assert!(tasks.loading.is_none());
        assert!(tasks.poll_loading().is_none());
        assert!(tasks.poll_import().is_none());
        assert!(!tasks.importing);
        assert!(matches!(tasks.poll_subtitle(), Some(Err(message))
            if message == "Subtitle selection stopped unexpectedly"));
        assert!(tasks.poll_subtitle().is_none());
        assert!(!tasks.subtitle_pending());
        tasks.start_subtitle_picker(9, directory.path().join("retry.mp4"), || None);
        assert_eq!(wait_for(|| tasks.poll_subtitle()).unwrap().generation, 9);

        let (sender, receiver) = mpsc::channel();
        tasks.library.events = receiver;
        tasks.importing = true;
        assert!(tasks.poll_import().is_none());
        assert!(tasks.importing);
        drop(sender);
        assert!(matches!(
            tasks.poll_import(),
            Some(ImportUpdate {
                event: Event::WorkerStopped,
                picker_closed: false
            })
        ));
        assert!(!tasks.importing);
        assert!(tasks.poll_import().is_none());
        tasks.start_import(String::new(), Some(Vec::new()));
        assert!(!tasks.library_stopped);
        assert!(matches!(
            wait_for(|| tasks.poll_import()).event,
            Event::SelectionClosed
        ));
        assert!(matches!(
            wait_for(|| tasks.poll_import()).event,
            Event::Completed(Ok(Some(_)))
        ));
        assert!(!tasks.importing);
    }

    #[test]
    fn background_worker_failure_keeps_the_subtitle_picker_and_active_video_owner() {
        use crate::{app::controller::Controller, ui::AppWindow, video::Status};
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        let ui = AppWindow::new().unwrap();
        let mut controller = Controller::new(directory.path().into());
        controller.video_tasks = empty_tasks(directory.path());
        controller
            .state
            .merge_videos(vec![video(directory.path().join("active.mp4"))]);
        controller.state.video_current = Some(0);
        controller.state.video_active = true;
        controller.state.video_generation = 8;
        controller.state.video_status = Status::Playing;
        controller.state.video_position = 2.0;
        let (subtitle_sender, subtitle_receiver) = mpsc::channel();
        controller.video_tasks.subtitle_picker = Some(subtitle_receiver);
        controller.dialog_open = true;

        let (sender, receiver) = mpsc::channel();
        controller.video_tasks.library.events = receiver;
        drop(sender);
        controller.poll_videos(&ui);
        assert!(controller.dialog_open);
        assert!(controller.video_tasks.subtitle_pending());
        assert!(controller.state.video_active);
        assert_eq!(controller.state.video_current, Some(0));
        assert_eq!(controller.state.video_generation, 8);
        assert_eq!(controller.state.video_status, Status::Playing);
        assert_eq!(controller.state.video_position, 2.0);

        // The import failure is terminal; a later notice must survive polling.
        controller.state.notice = "A newer operation".into();
        controller.poll_videos(&ui);
        assert_eq!(controller.state.notice, "A newer operation");
        assert!(controller.dialog_open);

        // Only completion of the actual subtitle picker releases its gate.
        drop(subtitle_sender);
        controller.poll_videos(&ui);
        assert!(!controller.dialog_open);
        assert!(!controller.video_tasks.subtitle_pending());
        assert_eq!(
            controller.state.notice,
            "Subtitle selection stopped unexpectedly"
        );
    }

    #[test]
    fn video_import_disconnect_releases_only_its_still_open_picker() {
        use crate::{app::controller::Controller, ui::AppWindow};
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        let ui = AppWindow::new().unwrap();
        let mut controller = Controller::new(directory.path().into());

        for selection_closed in [false, true] {
            controller.video_tasks = empty_tasks(directory.path());
            let (sender, receiver) = mpsc::channel();
            controller.video_tasks.library.events = receiver;
            controller.video_tasks.importing = true;
            controller.video_tasks.import_picker_open = true;
            controller.state.video_importing = true;
            controller.dialog_open = true;
            if selection_closed {
                sender.send(Event::SelectionClosed).unwrap();
                controller.poll_videos(&ui);
                assert!(!controller.dialog_open);
                assert!(controller.state.video_importing);
                // A different picker can open while the video scan continues.
                controller.dialog_open = true;
            }
            drop(sender);
            controller.poll_videos(&ui);
            assert_eq!(controller.dialog_open, selection_closed);
            assert!(!controller.state.video_importing);
            assert!(!controller.video_tasks.importing);
            assert!(!controller.video_tasks.import_picker_open);
        }
    }

    #[test]
    fn programmatic_video_import_never_releases_an_unrelated_picker() {
        use crate::{app::controller::Controller, ui::AppWindow};
        i_slint_backend_testing::init_no_event_loop();
        let directory = tempfile::tempdir().unwrap();
        let ui = AppWindow::new().unwrap();
        let mut controller = Controller::new(directory.path().into());
        controller.video_tasks = empty_tasks(directory.path());
        controller.dialog_open = true;
        controller.import_videos(Vec::new());
        let deadline = Instant::now() + Duration::from_secs(5);
        while controller.state.video_importing {
            controller.poll_videos(&ui);
            assert!(controller.dialog_open);
            assert!(Instant::now() < deadline, "video import timed out");
            std::thread::yield_now();
        }
        assert!(!controller.video_tasks.import_picker_open);
        assert_eq!(controller.state.notice, "Imported 0 videos");
    }
}

#[cfg(test)]
mod progress_tests {
    use super::*;
    use crate::{app::controller::Controller, video::Status};

    #[test]
    fn periodic_save_and_shutdown_restore_latest_video_position_without_import_overwrite() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("movie.mp4");
        let video = Video {
            path: path.clone(),
            title: "Movie".into(),
            duration: 120.0,
            width: 320,
            height: 180,
            display_aspect: 16.0 / 9.0,
            video_codec: "h264".into(),
            audio_codec: None,
        };
        let mut controller = Controller::new(directory.path().into());
        controller.state.merge_videos(vec![video.clone()]);
        controller.state.start_video(0);
        controller.state.video_status = Status::Playing;
        controller.state.video_position = 24.5;
        controller.video_progress_saved -= std::time::Duration::from_secs(6);
        controller.save_video_progress(false);
        let database = directory.path().join("video-progress.json");
        assert_eq!(
            crate::video::progress::WatchProgress::load(&database)
                .unwrap()
                .resume(&path, 120.0),
            24.5
        );

        controller.state.merge_videos(vec![video.clone()]);
        controller.state.video_position = 31.75;
        drop(controller);
        let mut reopened = Controller::new(directory.path().into());
        reopened.state.merge_videos(vec![video]);
        reopened.state.start_video(0);
        assert_eq!(reopened.state.video_position, 31.75);
    }
}
