//! Worker playback transitions, independent of the Rodio sink.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct ActiveSource {
    pub(super) source: SourceIdentity,
    pub(super) id: usize,
    pub(super) epoch: u64,
}

#[derive(Default)]
pub(super) struct WorkerPlayback {
    pub(super) current: Option<ActiveSource>,
    pub(super) ended: bool,
    pub(super) finished: bool,
}

impl WorkerPlayback {
    pub(super) fn emit(&self, events: &Sender<EventEnvelope>, event: AudioEvent, use_epoch: bool) {
        if let Some(current) = self.current {
            let _ = events.send(EventEnvelope {
                event,
                generation: Some(current.source.generation),
                epoch: use_epoch.then_some(current.epoch),
            });
        }
    }

    pub(super) fn drain(
        &mut self,
        receiver: &Receiver<WorkerEvent>,
        intent: &PlaybackIntent,
        events: &Sender<EventEnvelope>,
    ) {
        while let Ok(event) = receiver.try_recv() {
            match event {
                WorkerEvent::SourceStarted {
                    source,
                    id,
                    token,
                    epoch,
                } if source.generation == intent.generation.load(Ordering::Acquire) => {
                    if let Some(current) = self.current
                        && current.source != source
                        && let Some(token) = token
                    {
                        self.current = Some(ActiveSource { source, id, epoch });
                        self.ended = false;
                        self.finished = false;
                        self.emit(
                            events,
                            AudioEvent::Transitioned {
                                from: current.id,
                                to: id,
                                token,
                            },
                            false,
                        );
                    }
                }
                WorkerEvent::SourceEnded { source, epoch }
                    if self.current.is_some_and(|current| {
                        current.source == source && current.epoch == epoch
                    }) && epoch == intent.state.lock().unwrap().epoch =>
                {
                    self.ended = true;
                }
                _ => {}
            }
        }
    }

    pub(super) fn finish_if_ready(
        &mut self,
        intent: &PlaybackIntent,
        events: &Sender<EventEnvelope>,
    ) {
        let Some(current) = self.current else { return };
        let state = intent.state.lock().unwrap();
        if self.ended
            && !self.finished
            && current.source.generation == intent.generation.load(Ordering::Acquire)
            && current.epoch == state.epoch
            && state.prefetch.is_none()
            && state
                .playing
                .is_none_or(|ticket| ticket.source == current.source)
        {
            self.finished = true;
            self.emit(events, AudioEvent::Finished(current.id), true);
        }
    }
}
