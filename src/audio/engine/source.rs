//! Decoder opening, gain processing, and first-sample/EOF lifecycle.
use super::intent::{PlaybackIntent, SourceIdentity, SourceTicket};
use crate::audio::gain::{GainConfig, GainMode, GainSource, ReplayGainInfo};
use rodio::{Decoder, Source};
use std::{
    fs::File,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::Sender,
    },
    time::Duration,
};

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
    inner: GainSource<S>,
    events: Sender<WorkerEvent>,
    id: usize,
    ticket: SourceTicket,
    pub(super) epoch: Arc<AtomicU64>,
    pub(super) retired: Arc<AtomicBool>,
    started: bool,
    reported: bool,
    intent: Arc<PlaybackIntent>,
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
            epoch: Arc::new(AtomicU64::new(epoch)),
            retired: Arc::new(AtomicBool::new(false)),
            started: false,
            reported: false,
            intent,
        }
    }
}

impl<S: Iterator<Item = f32>> Iterator for EndNotifyingSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.retired.load(Ordering::Acquire)
            || self.intent.generation.load(Ordering::Acquire) != self.ticket.source.generation
        {
            return None;
        }
        if !self.started {
            // Avoid even decoding a successor already known to be canceled.
            if self.ticket.token.is_some() && !self.intent.prefetch_is_live(self.ticket.source) {
                return None;
            }
            // Packet I/O/decoding may be slow. Obtain the candidate outside the
            // intent lock, then revalidate before allowing it onto the output.
            let sample = self.inner.next();
            // Synchronize only at the first sample, not on every sample. Once
            // consumed, cancellation/replacement of *future* queue intent must
            // never truncate this active source while the worker catches up.
            let mut state = self.intent.state.lock().unwrap();
            if self.retired.load(Ordering::Acquire)
                || self.intent.generation.load(Ordering::Acquire) != self.ticket.source.generation
                || (self.ticket.token.is_some()
                    && !state
                        .prefetch
                        .is_some_and(|ticket| ticket.source == self.ticket.source))
            {
                return None;
            }
            if let Some(sample) = sample {
                self.started = true;
                state.playing = Some(self.ticket);
                if state
                    .prefetch
                    .is_some_and(|ticket| ticket.source == self.ticket.source)
                {
                    state.prefetch = None;
                }
                // Publish while the latch is locked: a seek must not observe
                // this playing identity before its start event is available.
                let _ = self.events.send(WorkerEvent::SourceStarted {
                    source: self.ticket.source,
                    id: self.id,
                    token: self.ticket.token,
                    epoch: self.epoch.load(Ordering::Acquire),
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
            return (!self.retired.load(Ordering::Acquire)).then_some(sample);
        }
        if !self.reported {
            self.reported = true;
            let _ = self.events.send(WorkerEvent::SourceEnded {
                source: self.ticket.source,
                epoch: self.epoch.load(Ordering::Acquire),
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
        self.epoch
            .store(self.intent.snapshot().epoch, Ordering::Release);
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
    // Off neither applies tags nor a final clipping guard. Avoid a second file
    // open/tag parse on this common path without changing any decoded samples.
    let info = if settings.mode == GainMode::Off {
        ReplayGainInfo::default()
    } else {
        ReplayGainInfo::read(path)
    };
    let gain = info.linear_gain(settings);
    open_decoder(
        path,
        gain,
        info.has_gain(settings.mode) && settings.prevent_clipping,
    )
}

pub(super) fn open_decoder(
    path: &std::path::Path,
    gain: f32,
    prevent_clipping: bool,
) -> Result<GainSource<Decoder<std::io::BufReader<File>>>, String> {
    // Decoder::try_from(File) supplies the file length and seekability. Rodio
    // 0.21 enables Symphonia's encoder-delay/padding trim by default.
    let decoder = Decoder::try_from(File::open(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(GainSource::with_clipping_prevention(
        decoder,
        gain,
        prevent_clipping,
    ))
}
