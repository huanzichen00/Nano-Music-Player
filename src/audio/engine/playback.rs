//! Pure playback reducer: no locks, channels, decoder, or output-device calls.
use super::{
    AudioEvent, EventEnvelope,
    intent::{IntentSnapshot, SourceIdentity},
    source::WorkerEvent,
};

#[derive(Clone, Copy)]
pub(super) struct ActiveSource {
    pub(super) source: SourceIdentity,
    pub(super) id: usize,
    pub(super) epoch: u64,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlaybackPhase {
    Active,
    Ended,
    #[default]
    Finished,
    Failed,
}

#[derive(Default)]
pub(super) struct WorkerPlayback {
    pub(super) current: Option<ActiveSource>,
    pub(super) phase: PlaybackPhase,
}

impl WorkerPlayback {
    pub(super) fn loaded(current: ActiveSource) -> Self {
        Self {
            current: Some(current),
            phase: PlaybackPhase::Active,
        }
    }

    pub(super) fn envelope(&self, event: AudioEvent, use_epoch: bool) -> Option<EventEnvelope> {
        self.current.map(|current| EventEnvelope {
            event,
            generation: Some(current.source.generation),
            epoch: use_epoch.then_some(current.epoch),
        })
    }

    pub(super) fn apply(
        &mut self,
        event: WorkerEvent,
        intent: IntentSnapshot,
    ) -> Option<EventEnvelope> {
        let current = self.current?;
        // A failed load/output cannot be revived by a late callback.
        if self.phase == PlaybackPhase::Failed || current.source.generation != intent.generation {
            return None;
        }
        match event {
            WorkerEvent::SourceStarted {
                source,
                id,
                token: Some(token),
                epoch,
            } if source.generation == intent.generation
                && source.serial > current.source.serial =>
            {
                *self = Self::loaded(ActiveSource { source, id, epoch });
                self.envelope(
                    AudioEvent::Transitioned {
                        from: current.id,
                        to: id,
                        token,
                    },
                    false,
                )
            }
            WorkerEvent::SourceEnded { source, epoch }
                if self.phase == PlaybackPhase::Active
                    && source == current.source
                    && epoch == current.epoch
                    && epoch == intent.epoch =>
            {
                self.phase = PlaybackPhase::Ended;
                None
            }
            _ => None,
        }
    }

    pub(super) fn finish_if_ready(&mut self, intent: IntentSnapshot) -> Option<EventEnvelope> {
        let current = self.current?;
        if self.phase == PlaybackPhase::Ended
            && current.source.generation == intent.generation
            && current.epoch == intent.epoch
            && intent.prefetch.is_none()
            && intent
                .playing
                .is_none_or(|ticket| ticket.source == current.source)
        {
            self.phase = PlaybackPhase::Finished;
            return self.envelope(AudioEvent::Finished(current.id), true);
        }
        None
    }

    pub(super) fn can_report_progress(&self, intent: IntentSnapshot) -> bool {
        self.phase == PlaybackPhase::Active
            && self.current.is_some_and(|current| {
                current.source.generation == intent.generation
                    && current.epoch == intent.epoch
                    && intent
                        .playing
                        .is_none_or(|ticket| ticket.source == current.source)
            })
    }
}
