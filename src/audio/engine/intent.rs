//! Synchronous command intent and source identities.
use super::AudioCommand;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct SourceIdentity {
    pub(super) generation: u64,
    pub(super) serial: u64,
}

#[derive(Clone, Copy)]
pub(super) struct SourceTicket {
    pub(super) source: SourceIdentity,
    pub(super) token: Option<u64>,
}

#[derive(Default)]
pub(super) struct IntentState {
    pub(super) serial: u64,
    pub(super) epoch: u64,
    pub(super) prefetch: Option<SourceTicket>,
    pub(super) playing: Option<SourceTicket>,
    speed: Option<f32>,
}

#[derive(Default)]
pub(super) struct PlaybackIntent {
    pub(super) generation: AtomicU64,
    pub(super) state: Mutex<IntentState>,
    pub(super) reopen_output: AtomicBool,
}

pub(super) struct QueuedCommand {
    pub(super) command: AudioCommand,
    pub(super) generation: u64,
    pub(super) epoch: u64,
    pub(super) source: Option<SourceIdentity>,
    pub(super) speed: f32,
}

/// A consistent view for the pure playback reducer. No sink or decoder access
/// may occur while the intent mutex is held.
#[derive(Clone, Copy)]
pub(super) struct IntentSnapshot {
    pub(super) generation: u64,
    pub(super) epoch: u64,
    pub(super) prefetch: Option<SourceTicket>,
    pub(super) playing: Option<SourceTicket>,
}

impl PlaybackIntent {
    pub(super) fn prepare(&self, command: AudioCommand) -> QueuedCommand {
        let mut state = self.state.lock().unwrap();
        let replacing = matches!(
            command,
            AudioCommand::Load { .. }
                | AudioCommand::LoadWithGain { .. }
                | AudioCommand::LoadAt { .. }
                | AudioCommand::Stop { .. }
                | AudioCommand::Shutdown
        );
        if replacing {
            self.generation.fetch_add(1, Ordering::AcqRel);
            state.playing = None;
        }
        if replacing || matches!(command, AudioCommand::Seek(_)) {
            state.epoch = state.epoch.wrapping_add(1);
            state.prefetch = None;
        }
        match &command {
            AudioCommand::Load { .. } | AudioCommand::LoadWithGain { .. } => {
                state.speed = Some(1.0)
            }
            AudioCommand::LoadAt { speed, .. } | AudioCommand::Speed(speed) => {
                state.speed = Some(*speed);
                state.prefetch = None;
            }
            _ => {}
        }
        let generation = self.generation.load(Ordering::Acquire);
        let source = if matches!(
            command,
            AudioCommand::Load { .. }
                | AudioCommand::LoadWithGain { .. }
                | AudioCommand::LoadAt { .. }
                | AudioCommand::AppendNext { .. }
                | AudioCommand::Seek(_)
                | AudioCommand::Speed(_)
        ) {
            state.serial = state.serial.wrapping_add(1);
            Some(SourceIdentity {
                generation,
                serial: state.serial,
            })
        } else {
            None
        };
        match &command {
            AudioCommand::AppendNext { token, .. } => {
                state.prefetch = source.map(|source| SourceTicket {
                    source,
                    token: Some(*token),
                });
            }
            AudioCommand::CancelPrefetch { token }
                if state
                    .prefetch
                    .is_some_and(|ticket| ticket.token == Some(*token)) =>
            {
                state.prefetch = None;
            }
            _ => {}
        }
        QueuedCommand {
            command,
            generation,
            epoch: state.epoch,
            source,
            speed: state.speed.unwrap_or(1.0),
        }
    }

    pub(super) fn prefetch_is_live(&self, source: SourceIdentity) -> bool {
        let state = self.snapshot();
        state.generation == source.generation
            && state.prefetch.is_some_and(|ticket| ticket.source == source)
    }

    pub(super) fn snapshot(&self) -> IntentSnapshot {
        let state = self.state.lock().unwrap();
        IntentSnapshot {
            generation: self.generation.load(Ordering::Acquire),
            epoch: state.epoch,
            prefetch: state.prefetch,
            playing: state.playing,
        }
    }

    pub(super) fn seek_is_live(&self, generation: u64, epoch: u64) -> bool {
        let state = self.snapshot();
        state.generation == generation && state.epoch == epoch
    }

    pub(super) fn replace_playing(
        &self,
        source: SourceIdentity,
        epoch: u64,
        retired: &AtomicBool,
    ) -> bool {
        let mut state = self.state.lock().unwrap();
        if source.generation != self.generation.load(Ordering::Acquire) || epoch != state.epoch {
            return false;
        }
        // Retirement and the replacement latch share the first-sample lock.
        // Even an old decoder still preparing its first sample cannot relatch.
        retired.store(true, Ordering::Release);
        state.playing = Some(SourceTicket {
            source,
            token: None,
        });
        true
    }

    pub(super) fn cancel_source(&self, source: SourceIdentity) {
        let mut state = self.state.lock().unwrap();
        if state.prefetch.is_some_and(|ticket| ticket.source == source) {
            state.prefetch = None;
        }
    }
}
