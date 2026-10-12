use super::PlaybackStatus;
use rodio::{Decoder, OutputStreamBuilder, Sink};
use std::{
    fs::File,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

#[derive(Debug)]
pub enum AudioCommand {
    Load {
        id: usize,
        path: PathBuf,
        volume: f32,
        paused: bool,
    },
    Pause(bool),
    Volume(f32),
    Seek(f64),
    Shutdown,
}

#[derive(Debug)]
pub enum AudioEvent {
    Progress {
        id: usize,
        status: PlaybackStatus,
        position: f64,
    },
    Finished(usize),
    Error {
        id: Option<usize>,
        message: String,
    },
}

pub struct Engine {
    commands: Sender<AudioCommand>,
    pub events: Receiver<AudioEvent>,
}

impl Default for Engine {
    fn default() -> Self {
        let (commands, receiver) = mpsc::channel();
        let (events, event_receiver) = mpsc::channel();
        thread::spawn(move || {
            // The stream stays on this worker for its entire lifetime. It is opened lazily.
            let mut stream = None;
            let mut sink: Option<Sink> = None;
            let mut current = None;
            let mut finished = false;
            loop {
                match receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(AudioCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Ok(command) => {
                        let loading_track = matches!(command, AudioCommand::Load { .. });
                        let result: Result<(), String> = (|| {
                            match command {
                                AudioCommand::Load {
                                    id,
                                    path,
                                    volume,
                                    paused,
                                } => {
                                    if let Some(old) = sink.take() {
                                        old.stop();
                                    }
                                    current = Some(id);
                                    finished = false;
                                    if stream.is_none() {
                                        stream = Some(
                                            OutputStreamBuilder::open_default_stream()
                                                .map_err(|e| e.to_string())?,
                                        );
                                    }
                                    let Some(output) = &stream else {
                                        return Err("No audio output".to_owned());
                                    };
                                    let source = Decoder::try_from(
                                        File::open(path).map_err(|e| e.to_string())?,
                                    )
                                    .map_err(|e| e.to_string())?;
                                    let next = Sink::connect_new(output.mixer());
                                    next.set_volume(volume);
                                    if paused {
                                        next.pause();
                                    }
                                    next.append(source);
                                    sink = Some(next);
                                }
                                AudioCommand::Pause(paused) => {
                                    if let Some(sink) = &sink {
                                        if paused {
                                            sink.pause();
                                        } else {
                                            sink.play();
                                        }
                                    }
                                }
                                AudioCommand::Volume(volume) => {
                                    if let Some(sink) = &sink {
                                        sink.set_volume(volume);
                                    }
                                }
                                AudioCommand::Seek(seconds) => {
                                    if let Some(sink) = &sink {
                                        sink.try_seek(Duration::from_secs_f64(seconds.max(0.0)))
                                            .map_err(|e| e.to_string())?;
                                    }
                                }
                                AudioCommand::Shutdown => {}
                            }
                            Ok(())
                        })();
                        if let Err(message) = result {
                            if loading_track {
                                finished = true;
                            }
                            let _ = events.send(AudioEvent::Error {
                                id: current,
                                message,
                            });
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if let (Some(sink), Some(id)) = (&sink, current) {
                    if sink.empty() {
                        if !finished {
                            finished = true;
                            let _ = events.send(AudioEvent::Finished(id));
                        }
                    } else {
                        let _ = events.send(AudioEvent::Progress {
                            id,
                            status: if sink.is_paused() {
                                PlaybackStatus::Paused
                            } else {
                                PlaybackStatus::Playing
                            },
                            position: sink.get_pos().as_secs_f64(),
                        });
                    }
                }
            }
        });
        Self {
            commands,
            events: event_receiver,
        }
    }
}

impl Engine {
    pub fn send(&self, command: AudioCommand) -> Result<(), String> {
        self.commands
            .send(command)
            .map_err(|_| "Audio worker is unavailable".to_owned())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.commands.send(AudioCommand::Shutdown);
    }
}
