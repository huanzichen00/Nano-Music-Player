use super::{PlaybackStatus, gain::GainConfig};

mod backend;
mod intent;
mod playback;
mod source;
mod worker;

use intent::{PlaybackIntent, QueuedCommand};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub enum AudioCommand {
    /// Backward-compatible load with ReplayGain disabled.
    Load {
        id: usize,
        path: PathBuf,
        volume: f32,
        paused: bool,
    },
    /// Load using the current persisted ReplayGain settings.
    LoadWithGain {
        id: usize,
        path: PathBuf,
        volume: f32,
        paused: bool,
        gain: GainConfig,
    },
    /// Decode and queue the intended successor on the same sink for gapless playback.
    /// Tokens are owned by the caller and should change whenever queue intent changes.
    AppendNext {
        token: u64,
        id: usize,
        path: PathBuf,
        gain: GainConfig,
    },
    /// Invalidate an append request that has not started yet.
    CancelPrefetch {
        token: u64,
    },
    Pause(bool),
    Volume(f32),
    Seek(f64),
    Stop {
        handoff: u64,
    },
    Shutdown,
}

#[derive(Debug)]
pub enum AudioEvent {
    Stopped {
        handoff: u64,
    },
    Progress {
        id: usize,
        status: PlaybackStatus,
        position: f64,
        observed_at: Instant,
    },
    /// The appended successor has become the active source. The controller should
    /// update current-track state without issuing another Load.
    Transitioned {
        from: usize,
        to: usize,
        token: u64,
    },
    /// A successor failed to decode; the active track can continue normally.
    PrefetchFailed {
        id: usize,
        token: u64,
        message: String,
    },
    /// Emitted only when the active source ends without a valid appended successor.
    Finished(usize),
    Error {
        id: Option<usize>,
        message: String,
    },
}

pub struct Engine {
    commands: Sender<QueuedCommand>,
    pub events: AudioEvents,
    intent: Arc<PlaybackIntent>,
}

/// Filters events invalidated by a new load/stop or seek before the UI polls.
/// Callers retain the usual `engine.events.try_recv()` event interface.
pub struct AudioEvents {
    receiver: Receiver<EventEnvelope>,
    intent: Arc<PlaybackIntent>,
}

impl AudioEvents {
    pub fn try_recv(&self) -> Result<AudioEvent, mpsc::TryRecvError> {
        loop {
            let envelope = self.receiver.try_recv()?;
            let state = self.intent.snapshot();
            if envelope
                .generation
                .is_none_or(|generation| generation == state.generation)
                && envelope.epoch.is_none_or(|epoch| epoch == state.epoch)
            {
                return Ok(envelope.event);
            }
        }
    }
}

struct EventEnvelope {
    event: AudioEvent,
    generation: Option<u64>,
    epoch: Option<u64>,
}

impl Default for Engine {
    fn default() -> Self {
        let (commands, receiver) = mpsc::channel::<QueuedCommand>();
        let (events, event_receiver) = mpsc::channel();
        let intent = Arc::new(PlaybackIntent::default());
        let worker_intent = Arc::clone(&intent);
        thread::spawn(move || worker::run(receiver, events, worker_intent));
        Self {
            commands,
            events: AudioEvents {
                receiver: event_receiver,
                intent: Arc::clone(&intent),
            },
            intent,
        }
    }
}

impl Engine {
    pub fn send(&self, command: AudioCommand) -> Result<(), String> {
        if let AudioCommand::Seek(seconds) = &command {
            Duration::try_from_secs_f64(seconds.max(0.0))
                .map_err(|_| "Seek position must be finite and representable".to_owned())?;
            if !seconds.is_finite() {
                return Err("Seek position must be finite and representable".to_owned());
            }
        }
        self.commands
            .send(self.intent.prepare(command))
            .map_err(|_| "Audio worker is unavailable".to_owned())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.send(AudioCommand::Shutdown);
    }
}

#[cfg(test)]
mod tests;
