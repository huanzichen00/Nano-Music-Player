use super::library::{self, ImportResult, Video};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

pub enum Event {
    SelectionClosed,
    Completed(Result<Option<ImportResult>, String>),
    RemovedMissing {
        path: PathBuf,
        result: Result<bool, String>,
    },
}

enum Command {
    Import {
        title: String,
        paths: Option<Vec<PathBuf>>,
    },
    RemoveMissing(PathBuf),
}

/// A single queue serializes metadata writes and their published results. A
/// delayed import can never republish a row after its missing-path removal.
pub struct Worker {
    commands: Sender<Command>,
    pub events: Receiver<Event>,
}

impl Worker {
    pub fn new(database: PathBuf) -> Self {
        let (commands, receiver) = mpsc::channel();
        let (sender, events) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(command) = receiver.recv() {
                match command {
                    Command::Import { title, paths } => {
                        let paths = paths.or_else(|| {
                            rfd::FileDialog::new()
                                .set_title(&title)
                                .add_filter("Videos", library::EXTENSIONS)
                                .pick_files()
                        });
                        let _ = sender.send(Event::SelectionClosed);
                        let result = paths
                            .map(|paths| library::import(&paths, &database))
                            .transpose();
                        let _ = sender.send(Event::Completed(result));
                    }
                    Command::RemoveMissing(path) => {
                        let result = library::remove_missing(&database, &path);
                        let _ = sender.send(Event::RemovedMissing { path, result });
                    }
                }
            }
        });
        Self { commands, events }
    }

    pub fn start_import(&self, title: String, paths: Option<Vec<PathBuf>>) {
        let _ = self.commands.send(Command::Import { title, paths });
    }

    pub fn remove_missing(&self, path: PathBuf) {
        let _ = self.commands.send(Command::RemoveMissing(path));
    }
}

pub fn load(database: PathBuf) -> Receiver<Result<Vec<Video>, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(library::load(&database));
    });
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn missing_removal_follows_import_and_preserves_restored_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing.mp4");
        let restored = directory.path().join("restored.mp4");
        let database = directory.path().join("videos.json");
        let video = |path: PathBuf| Video {
            path,
            title: "Movie".into(),
            duration: 10.0,
            width: 320,
            height: 180,
            display_aspect: 16.0 / 9.0,
            video_codec: "h264".into(),
            audio_codec: None,
        };
        std::fs::write(
            &database,
            serde_json::to_vec(&vec![video(path.clone()), video(restored.clone())]).unwrap(),
        )
        .unwrap();
        std::fs::write(&restored, b"restored fixture").unwrap();
        let worker = Worker::new(database.clone());
        worker.start_import(String::new(), Some(Vec::new()));
        worker.remove_missing(path.clone());
        worker.remove_missing(restored.clone());
        let next = || worker.events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(next(), Event::SelectionClosed));
        assert!(matches!(next(), Event::Completed(Ok(Some(result))) if result.videos.len() == 2));
        assert!(
            matches!(next(), Event::RemovedMissing { path: removed, result: Ok(true) } if removed == path)
        );
        assert!(
            matches!(next(), Event::RemovedMissing { path, result: Ok(false) } if path == restored)
        );
        let library = library::load(&database).unwrap();
        assert_eq!(library.len(), 1);
        assert_eq!(library[0].path, restored);
        assert!(restored.is_file());
    }
}
