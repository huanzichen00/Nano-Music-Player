//! Decoder opening, gain processing, and first-sample/EOF lifecycle.
use super::*;

#[derive(Debug)]
pub(super) enum WorkerEvent {
    SourceStarted {
        source: SourceIdentity,
        id: usize,
        token: Option<u64>,
        epoch: u64,
    },
    SourceEnded {
        source: SourceIdentity,
        epoch: u64,
    },
}

pub(super) struct EndNotifyingSource<S> {
    pub(super) inner: GainSource<S>,
    pub(super) events: Sender<WorkerEvent>,
    pub(super) id: usize,
    pub(super) ticket: SourceTicket,
    pub(super) epoch: u64,
    pub(super) started: bool,
    pub(super) reported: bool,
    pub(super) intent: Arc<PlaybackIntent>,
}

impl<S> EndNotifyingSource<S> {
    pub(super) fn new(
        inner: GainSource<S>,
        events: Sender<WorkerEvent>,
        id: usize,
        ticket: SourceTicket,
        epoch: u64,
        intent: Arc<PlaybackIntent>,
    ) -> Self {
        Self {
            inner,
            events,
            id,
            ticket,
            epoch,
            started: false,
            reported: false,
            intent,
        }
    }
}

impl<S: Iterator<Item = f32>> Iterator for EndNotifyingSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.intent.generation.load(Ordering::Acquire) != self.ticket.source.generation {
            return None;
        }
        if !self.started {
            // Synchronize only at the first sample, not on every sample. Once
            // consumed, cancellation/replacement of *future* queue intent must
            // never truncate this active source while the worker catches up.
            let mut state = self.intent.state.lock().unwrap();
            if self.intent.generation.load(Ordering::Acquire) != self.ticket.source.generation
                || (self.ticket.token.is_some()
                    && !state
                        .prefetch
                        .is_some_and(|ticket| ticket.source == self.ticket.source))
            {
                return None;
            }
            if let Some(sample) = self.inner.next() {
                self.started = true;
                state.playing = Some(self.ticket);
                if state
                    .prefetch
                    .is_some_and(|ticket| ticket.source == self.ticket.source)
                {
                    state.prefetch = None;
                }
                let _ = self.events.send(WorkerEvent::SourceStarted {
                    source: self.ticket.source,
                    id: self.id,
                    token: self.ticket.token,
                    epoch: self.epoch,
                });
                return Some(sample);
            }
            // Empty successors have not started and cannot produce a transition.
            if state
                .prefetch
                .is_some_and(|ticket| ticket.source == self.ticket.source)
            {
                state.prefetch = None;
            }
        } else if let Some(sample) = self.inner.next() {
            return Some(sample);
        }
        if !self.reported {
            self.reported = true;
            let _ = self.events.send(WorkerEvent::SourceEnded {
                source: self.ticket.source,
                epoch: self.epoch,
            });
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source<Item = f32>> Source for EndNotifyingSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        let result = self.inner.try_seek(pos);
        self.epoch = self.intent.state.lock().unwrap().epoch;
        if result.is_ok() {
            self.reported = false;
        }
        result
    }
}

pub(super) fn open_source(
    path: &std::path::Path,
    settings: GainConfig,
) -> Result<GainSource<Decoder<std::io::BufReader<File>>>, String> {
    let info = ReplayGainInfo::read(path);
    let gain = info.linear_gain(settings);
    // Decoder::try_from(File) supplies the file length and seekability. Rodio
    // 0.21 enables Symphonia's encoder-delay/padding trim by default.
    let decoder = Decoder::try_from(File::open(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(GainSource::with_clipping_prevention(
        decoder,
        gain,
        info.has_gain(settings.mode) && settings.prevent_clipping,
    ))
}
