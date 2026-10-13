use super::library::{self, ImportResult, Video};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
};

pub enum Event {
    SelectionClosed,
    Completed(Result<Option<ImportResult>, String>),
}

pub fn load(database: PathBuf) -> Receiver<Result<Vec<Video>, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(library::load(&database));
    });
    receiver
}

pub fn start(database: PathBuf, title: String, paths: Option<Vec<PathBuf>>) -> Receiver<Event> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
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
    });
    receiver
}
