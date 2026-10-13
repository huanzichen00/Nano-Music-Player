//! Synchronous command intent and source identities.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

#[derive(Default)]
pub(super) struct PlaybackIntent {
    pub(super) generation: AtomicU64,
    pub(super) state: Mutex<IntentState>,
}

pub(super) struct QueuedCommand {
    pub(super) command: AudioCommand,
    pub(super) generation: u64,
    pub(super) epoch: u64,
    pub(super) source: Option<SourceIdentity>,
}

impl PlaybackIntent {
    pub(super) fn prepare(&self, command: AudioCommand) -> QueuedCommand {
        let mut state = self.state.lock().unwrap();
        let replacing = matches!(
            command,
            AudioCommand::Load { .. }
                | AudioCommand::LoadWithGain { .. }
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
        let generation = self.generation.load(Ordering::Acquire);
        let source = if matches!(
            command,
            AudioCommand::Load { .. }
                | AudioCommand::LoadWithGain { .. }
                | AudioCommand::AppendNext { .. }
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
        }
    }

    pub(super) fn prefetch_is_live(&self, source: SourceIdentity) -> bool {
        let state = self.state.lock().unwrap();
        self.generation.load(Ordering::Acquire) == source.generation
            && state.prefetch.is_some_and(|ticket| ticket.source == source)
    }

    pub(super) fn cancel_source(&self, source: SourceIdentity) {
        let mut state = self.state.lock().unwrap();
        if state.prefetch.is_some_and(|ticket| ticket.source == source) {
            state.prefetch = None;
        }
    }
}
