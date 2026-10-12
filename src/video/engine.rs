use super::{Generation, Status, library::Video, mpv::Session};
use std::{
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Debug)]
pub enum Command {
    Load {
        generation: Generation,
        video: Video,
        volume: f32,
        paused: bool,
    },
    Pause {
        generation: Generation,
        paused: bool,
    },
    Seek {
        generation: Generation,
        seconds: f64,
    },
    Volume {
        generation: Generation,
        volume: f32,
    },
    /// Reply only after the native player and its audio output are destroyed.
    Stop {
        handoff: u64,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub generation: Generation,
    pub status: Status,
    pub position: f64,
    pub duration: f64,
    /// Native audio output name, for diagnostics and device-independent tests.
    pub audio_output: Option<String>,
}

#[derive(Debug)]
pub enum Event {
    Stopped {
        handoff: u64,
    },
    Finished(Generation),
    Error {
        generation: Generation,
        message: String,
    },
}

#[derive(Debug)]
pub struct Frame {
    pub generation: Generation,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Default)]
pub(super) struct Latest {
    pub frame: Option<Frame>,
    pub progress: Option<Progress>,
}

pub struct Engine {
    commands: mpsc::Sender<Command>,
    pub events: mpsc::Receiver<Event>,
    latest: Arc<Mutex<Latest>>,
    worker: Option<JoinHandle<()>>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Engine {
    /// `silent` uses mpv's timed null audio output for device-independent tests.
    pub fn new(silent: bool) -> Self {
        let (commands, receiver) = mpsc::channel();
        let (events, event_receiver) = mpsc::channel();
        let latest = Arc::new(Mutex::new(Latest::default()));
        let output = latest.clone();
        let worker = thread::spawn(move || {
            let mut session: Option<Session> = None;
            loop {
                match receiver.recv_timeout(Duration::from_millis(8)) {
                    Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Ok(Command::Stop { handoff }) => {
                        session = None;
                        *output.lock().unwrap() = Latest::default();
                        let _ = events.send(Event::Stopped { handoff });
                    }
                    Ok(Command::Load {
                        generation,
                        video,
                        volume,
                        paused,
                    }) => {
                        // Destroy before opening: native events and callbacks cannot cross sessions.
                        session = None;
                        *output.lock().unwrap() = Latest::default();
                        match Session::open(generation, &video, volume, paused, silent) {
                            Ok(next) => session = Some(next),
                            Err(message) => {
                                let _ = events.send(Event::Error {
                                    generation,
                                    message,
                                });
                            }
                        }
                    }
                    Ok(command) => {
                        if let Some(active) = &mut session
                            && let Err(message) = active.command(command)
                        {
                            let _ = events.send(Event::Error {
                                generation: active.generation,
                                message,
                            });
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if let Some(active) = &mut session
                    && let Err(message) = active.poll(&output, &events)
                {
                    let _ = events.send(Event::Error {
                        generation: active.generation,
                        message,
                    });
                    session = None;
                }
            }
            // Session frees the renderer before destroying mpv, on its owner thread.
        });
        Self {
            commands,
            events: event_receiver,
            latest,
            worker: Some(worker),
        }
    }

    pub fn send(&self, command: Command) -> Result<(), String> {
        self.commands
            .send(command)
            .map_err(|_| "Video worker is unavailable".into())
    }

    pub fn take_frame(&self) -> Option<Frame> {
        self.latest.lock().unwrap().frame.take()
    }
    pub fn take_progress(&self) -> Option<Progress> {
        self.latest.lock().unwrap().progress.take()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        // Never hold Latest while joining. All native cleanup completes before return.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
