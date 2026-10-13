use super::{
    PlaybackStatus,
    gain::{GainConfig, GainSource, ReplayGainInfo},
};
use rodio::{Decoder, OutputStreamBuilder, Sink, Source};
use std::{
    fs::File,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
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
            let state = self.intent.state.lock().unwrap();
            if envelope.generation.is_none_or(|generation| {
                generation == self.intent.generation.load(Ordering::Acquire)
            }) && envelope.epoch.is_none_or(|epoch| epoch == state.epoch)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceIdentity {
    generation: u64,
    serial: u64,
}

#[derive(Clone, Copy)]
struct SourceTicket {
    source: SourceIdentity,
    token: Option<u64>,
}

#[derive(Default)]
struct IntentState {
    serial: u64,
    epoch: u64,
    prefetch: Option<SourceTicket>,
    playing: Option<SourceTicket>,
}

#[derive(Default)]
struct PlaybackIntent {
    generation: AtomicU64,
    state: Mutex<IntentState>,
}

struct QueuedCommand {
    command: AudioCommand,
    generation: u64,
    epoch: u64,
    source: Option<SourceIdentity>,
}

impl PlaybackIntent {
    fn prepare(&self, command: AudioCommand) -> QueuedCommand {
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

    fn prefetch_is_live(&self, source: SourceIdentity) -> bool {
        let state = self.state.lock().unwrap();
        self.generation.load(Ordering::Acquire) == source.generation
            && state.prefetch.is_some_and(|ticket| ticket.source == source)
    }

    fn cancel_source(&self, source: SourceIdentity) {
        let mut state = self.state.lock().unwrap();
        if state.prefetch.is_some_and(|ticket| ticket.source == source) {
            state.prefetch = None;
        }
    }
}

#[derive(Debug)]
enum WorkerEvent {
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

struct EndNotifyingSource<S> {
    inner: GainSource<S>,
    events: Sender<WorkerEvent>,
    id: usize,
    ticket: SourceTicket,
    epoch: u64,
    started: bool,
    reported: bool,
    intent: Arc<PlaybackIntent>,
}

impl<S> EndNotifyingSource<S> {
    fn new(
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

#[derive(Clone, Copy)]
struct ActiveSource {
    source: SourceIdentity,
    id: usize,
    epoch: u64,
}

#[derive(Default)]
struct WorkerPlayback {
    current: Option<ActiveSource>,
    ended: bool,
    finished: bool,
}

impl WorkerPlayback {
    fn emit(&self, events: &Sender<EventEnvelope>, event: AudioEvent, use_epoch: bool) {
        if let Some(current) = self.current {
            let _ = events.send(EventEnvelope {
                event,
                generation: Some(current.source.generation),
                epoch: use_epoch.then_some(current.epoch),
            });
        }
    }

    fn drain(
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

    fn finish_if_ready(&mut self, intent: &PlaybackIntent, events: &Sender<EventEnvelope>) {
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

impl Default for Engine {
    fn default() -> Self {
        let (commands, receiver) = mpsc::channel::<QueuedCommand>();
        let (events, event_receiver) = mpsc::channel();
        let intent = Arc::new(PlaybackIntent::default());
        let worker_intent = Arc::clone(&intent);
        thread::spawn(move || {
            let (source_events, source_receiver) = mpsc::channel();
            let mut stream = None;
            let mut sink: Option<Sink> = None;
            let mut playback = WorkerPlayback::default();
            loop {
                match receiver.recv_timeout(Duration::from_millis(50)) {
                    Ok(queued) => {
                        // A transition may have started before a seek/cancel arrived.
                        playback.drain(&source_receiver, &worker_intent, &events);
                        if matches!(queued.command, AudioCommand::Shutdown) {
                            break;
                        }
                        if queued.generation != worker_intent.generation.load(Ordering::Acquire) {
                            if let AudioCommand::Stop { handoff } = queued.command {
                                let _ = events.send(EventEnvelope {
                                    event: AudioEvent::Stopped { handoff },
                                    generation: None,
                                    epoch: None,
                                });
                            }
                            continue;
                        }
                        let loading_track = matches!(
                            queued.command,
                            AudioCommand::Load { .. } | AudioCommand::LoadWithGain { .. }
                        );
                        let result: Result<(), String> = (|| {
                            match queued.command {
                                AudioCommand::Load {
                                    id,
                                    path,
                                    volume,
                                    paused,
                                } => {
                                    load_track(
                                        id,
                                        path,
                                        volume,
                                        paused,
                                        GainConfig::default(),
                                        queued.source.unwrap(),
                                        queued.epoch,
                                        &mut stream,
                                        &mut sink,
                                        &mut playback,
                                        &source_events,
                                        &worker_intent,
                                    )?;
                                }
                                AudioCommand::LoadWithGain {
                                    id,
                                    path,
                                    volume,
                                    paused,
                                    gain,
                                } => {
                                    load_track(
                                        id,
                                        path,
                                        volume,
                                        paused,
                                        gain,
                                        queued.source.unwrap(),
                                        queued.epoch,
                                        &mut stream,
                                        &mut sink,
                                        &mut playback,
                                        &source_events,
                                        &worker_intent,
                                    )?;
                                }
                                AudioCommand::AppendNext {
                                    token,
                                    id,
                                    path,
                                    gain,
                                } => {
                                    let source_id = queued.source.unwrap();
                                    if !worker_intent.prefetch_is_live(source_id) {
                                        return Ok(());
                                    }
                                    if playback.finished {
                                        worker_intent.cancel_source(source_id);
                                        return Ok(());
                                    }
                                    let Some(active_sink) = &sink else {
                                        worker_intent.cancel_source(source_id);
                                        return Ok(());
                                    };
                                    let source = match open_source(&path, gain) {
                                        Ok(source) => source,
                                        Err(message) => {
                                            worker_intent.cancel_source(source_id);
                                            let _ = events.send(EventEnvelope {
                                                event: AudioEvent::PrefetchFailed {
                                                    id,
                                                    token,
                                                    message,
                                                },
                                                generation: Some(queued.generation),
                                                epoch: Some(queued.epoch),
                                            });
                                            return Ok(());
                                        }
                                    };
                                    if worker_intent.prefetch_is_live(source_id) {
                                        active_sink.append(EndNotifyingSource::new(
                                            source,
                                            source_events.clone(),
                                            id,
                                            SourceTicket {
                                                source: source_id,
                                                token: Some(token),
                                            },
                                            queued.epoch,
                                            Arc::clone(&worker_intent),
                                        ));
                                    }
                                }
                                // Intent changes happen synchronously in send(), so they
                                // must not be repeated here and erase a newer append.
                                AudioCommand::CancelPrefetch { .. } => {}
                                AudioCommand::Pause(paused) => {
                                    if let Some(active_sink) = &sink {
                                        if paused {
                                            active_sink.pause()
                                        } else {
                                            active_sink.play()
                                        }
                                    }
                                }
                                AudioCommand::Stop { handoff } => {
                                    if let Some(old) = sink.take() {
                                        old.stop()
                                    }
                                    stream = None;
                                    playback = WorkerPlayback::default();
                                    let _ = events.send(EventEnvelope {
                                        event: AudioEvent::Stopped { handoff },
                                        generation: None,
                                        epoch: None,
                                    });
                                }
                                AudioCommand::Volume(volume) => {
                                    if let Some(active_sink) = &sink {
                                        active_sink.set_volume(volume)
                                    }
                                }
                                AudioCommand::Seek(seconds) => {
                                    if !seconds.is_finite() {
                                        return Err("Seek position must be finite".to_owned());
                                    }
                                    if let Some(active_sink) = &sink {
                                        let result = active_sink
                                            .try_seek(
                                                Duration::try_from_secs_f64(seconds.max(0.0))
                                                    .map_err(|e| e.to_string())?,
                                            )
                                            .map_err(|e| e.to_string());
                                        if let Some(current) = &mut playback.current {
                                            current.epoch = queued.epoch;
                                        }
                                        playback.ended = active_sink.empty();
                                        playback.finished = false;
                                        result?;
                                    }
                                }
                                AudioCommand::Shutdown => {}
                            }
                            Ok(())
                        })();
                        if let Err(message) = result {
                            let _ = events.send(EventEnvelope {
                                event: AudioEvent::Error {
                                    id: playback.current.map(|current| current.id),
                                    message,
                                },
                                generation: Some(queued.generation),
                                epoch: (!loading_track).then_some(queued.epoch),
                            });
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                playback.drain(&source_receiver, &worker_intent, &events);
                playback.finish_if_ready(&worker_intent, &events);
                if let (Some(active_sink), Some(current)) = (&sink, playback.current) {
                    let state = worker_intent.state.lock().unwrap();
                    if !playback.ended
                        && !playback.finished
                        && state
                            .playing
                            .is_none_or(|ticket| ticket.source == current.source)
                    {
                        playback.emit(
                            &events,
                            AudioEvent::Progress {
                                id: current.id,
                                status: if active_sink.is_paused() {
                                    PlaybackStatus::Paused
                                } else {
                                    PlaybackStatus::Playing
                                },
                                position: active_sink.get_pos().as_secs_f64(),
                                observed_at: Instant::now(),
                            },
                            true,
                        );
                    }
                }
            }
        });
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

fn open_source(
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

#[allow(clippy::too_many_arguments)]
fn load_track(
    id: usize,
    path: PathBuf,
    volume: f32,
    paused: bool,
    gain: GainConfig,
    source_id: SourceIdentity,
    epoch: u64,
    stream: &mut Option<rodio::OutputStream>,
    sink: &mut Option<Sink>,
    playback: &mut WorkerPlayback,
    source_events: &Sender<WorkerEvent>,
    intent: &Arc<PlaybackIntent>,
) -> Result<(), String> {
    // send() has already invalidated the old generation. Do not invalidate
    // prefetch here: a successor for this load may already be waiting in the channel.
    if let Some(old) = sink.take() {
        old.stop()
    }
    *playback = WorkerPlayback {
        current: Some(ActiveSource {
            source: source_id,
            id,
            epoch,
        }),
        ended: false,
        finished: true,
    };
    let source = open_source(&path, gain)?;
    if intent.generation.load(Ordering::Acquire) != source_id.generation {
        return Ok(());
    }
    if stream.is_none() {
        *stream = Some(OutputStreamBuilder::open_default_stream().map_err(|e| e.to_string())?);
    }
    let output = stream
        .as_ref()
        .ok_or_else(|| "No audio output".to_owned())?;
    let next = Sink::connect_new(output.mixer());
    next.set_volume(volume);
    if paused {
        next.pause()
    }
    next.append(EndNotifyingSource::new(
        source,
        source_events.clone(),
        id,
        SourceTicket {
            source: source_id,
            token: None,
        },
        epoch,
        Arc::clone(intent),
    ));
    *sink = Some(next);
    playback.finished = false;
    Ok(())
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
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    fn load(id: usize) -> AudioCommand {
        AudioCommand::Load {
            id,
            path: PathBuf::new(),
            volume: 1.0,
            paused: false,
        }
    }

    fn append(id: usize, token: u64) -> AudioCommand {
        AudioCommand::AppendNext {
            id,
            token,
            path: PathBuf::new(),
            gain: GainConfig::default(),
        }
    }

    struct Harness {
        intent: Arc<PlaybackIntent>,
        source_tx: Sender<WorkerEvent>,
        source_rx: Receiver<WorkerEvent>,
        event_tx: Sender<EventEnvelope>,
        events: AudioEvents,
        playback: WorkerPlayback,
        sink: Sink,
        output: rodio::queue::SourcesQueueOutput,
    }

    impl Harness {
        fn new() -> Self {
            let intent = Arc::new(PlaybackIntent::default());
            let (source_tx, source_rx) = mpsc::channel();
            let (event_tx, event_rx) = mpsc::channel();
            let (sink, output) = Sink::new();
            Self {
                events: AudioEvents {
                    receiver: event_rx,
                    intent: Arc::clone(&intent),
                },
                intent,
                source_tx,
                source_rx,
                event_tx,
                playback: WorkerPlayback::default(),
                sink,
                output,
            }
        }

        fn queue<S: Source<Item = f32> + Send + 'static>(
            &mut self,
            queued: QueuedCommand,
            source: S,
        ) {
            let (id, token) = match queued.command {
                AudioCommand::Load { id, .. } | AudioCommand::LoadWithGain { id, .. } => {
                    self.playback = WorkerPlayback {
                        current: Some(ActiveSource {
                            id,
                            source: queued.source.unwrap(),
                            epoch: queued.epoch,
                        }),
                        ended: false,
                        finished: false,
                    };
                    (id, None)
                }
                AudioCommand::AppendNext { id, token, .. } => (id, Some(token)),
                _ => panic!("expected a source command"),
            };
            self.sink.append(EndNotifyingSource::new(
                GainSource::new(source, 1.0),
                self.source_tx.clone(),
                id,
                SourceTicket {
                    source: queued.source.unwrap(),
                    token,
                },
                queued.epoch,
                Arc::clone(&self.intent),
            ));
        }

        fn buffer(&mut self, command: AudioCommand, samples: Vec<f32>) {
            let queued = self.intent.prepare(command);
            self.queue(queued, SamplesBuffer::new(1, 1000, samples));
        }

        fn poll(&mut self) -> Vec<AudioEvent> {
            self.playback
                .drain(&self.source_rx, &self.intent, &self.event_tx);
            self.playback.finish_if_ready(&self.intent, &self.event_tx);
            let mut events = Vec::new();
            while let Ok(event) = self.events.try_recv() {
                events.push(event)
            }
            events
        }
    }

    #[test]
    fn same_sink_outputs_successor_samples_without_silence() {
        let mut h = Harness::new();
        // Prepare both commands before the load is processed, as the UI does.
        let loading = h.intent.prepare(load(1));
        let next = h.intent.prepare(append(2, 0));
        h.queue(loading, SamplesBuffer::new(1, 1000, vec![0.1, 0.2]));
        h.queue(next, SamplesBuffer::new(1, 1000, vec![0.3, 0.4]));
        assert_eq!(
            h.output.by_ref().take(4).collect::<Vec<_>>(),
            vec![0.1, 0.2, 0.3, 0.4]
        );
        assert!(matches!(
            h.poll().as_slice(),
            [AudioEvent::Transitioned {
                from: 1,
                to: 2,
                token: 0
            }]
        ));
        h.output.next(); // EOF is observed after the last sample, not before it.
        assert!(matches!(h.poll().as_slice(), [AudioEvent::Finished(2)]));
        assert!(h.poll().is_empty());
    }

    #[test]
    fn short_repeat_with_same_track_id_has_distinct_source_end_events() {
        let mut h = Harness::new();
        h.buffer(load(7), vec![0.1]);
        h.buffer(append(7, 9), vec![0.2, 0.3]);
        assert_eq!(
            h.output.by_ref().take(3).collect::<Vec<_>>(),
            vec![0.1, 0.2, 0.3]
        );
        h.output.next();
        assert!(matches!(
            h.poll().as_slice(),
            [
                AudioEvent::Transitioned {
                    from: 7,
                    to: 7,
                    token: 9
                },
                AudioEvent::Finished(7)
            ]
        ));
        assert!(h.poll().is_empty());
    }

    #[test]
    fn canceled_and_replaced_successors_never_output_samples() {
        let mut h = Harness::new();
        h.buffer(load(1), vec![0.1, 0.2]);
        h.buffer(append(2, 5), vec![0.8; 5]);
        h.intent.prepare(AudioCommand::CancelPrefetch { token: 5 });
        h.buffer(append(3, 6), vec![0.3, 0.4]);
        assert_eq!(
            h.output.by_ref().take(4).collect::<Vec<_>>(),
            vec![0.1, 0.2, 0.3, 0.4]
        );
        assert!(matches!(
            h.poll().as_slice(),
            [AudioEvent::Transitioned {
                from: 1,
                to: 3,
                token: 6
            }]
        ));
    }

    #[test]
    fn started_successor_survives_late_cancel_and_new_prefetch_before_worker_poll() {
        let mut h = Harness::new();
        h.buffer(load(1), vec![0.1]);
        h.buffer(append(2, 8), vec![0.2, 0.3, 0.4]);
        assert_eq!(h.output.next(), Some(0.1));
        assert_eq!(h.output.next(), Some(0.2));
        h.intent.prepare(AudioCommand::CancelPrefetch { token: 8 });
        h.buffer(append(3, 9), vec![0.5]);
        assert_eq!(
            h.output.by_ref().take(3).collect::<Vec<_>>(),
            vec![0.3, 0.4, 0.5]
        );
        assert!(matches!(
            h.poll().as_slice(),
            [
                AudioEvent::Transitioned {
                    from: 1,
                    to: 2,
                    token: 8
                },
                AudioEvent::Transitioned {
                    from: 2,
                    to: 3,
                    token: 9
                }
            ]
        ));
    }

    #[test]
    fn canceled_successor_does_not_report_a_transition_or_duplicate_finish() {
        let mut h = Harness::new();
        h.buffer(load(1), vec![0.1]);
        h.buffer(append(2, 10), vec![0.8]);
        h.intent.prepare(AudioCommand::CancelPrefetch { token: 10 });
        assert_eq!(h.output.next(), Some(0.1));
        assert_eq!(h.output.next(), Some(0.0));
        assert!(matches!(h.poll().as_slice(), [AudioEvent::Finished(1)]));
        assert!(h.poll().is_empty());
    }

    #[test]
    fn empty_successor_does_not_transition() {
        let mut h = Harness::new();
        h.buffer(load(1), vec![0.1]);
        h.buffer(append(2, 1), Vec::new());
        assert_eq!(h.output.next(), Some(0.1));
        h.output.next();
        assert!(matches!(h.poll().as_slice(), [AudioEvent::Finished(1)]));
    }

    #[test]
    fn reload_same_id_invalidates_old_samples_end_events_and_published_events() {
        let mut h = Harness::new();
        h.buffer(load(4), vec![0.1; 2]);
        h.output.by_ref().take(2).for_each(drop);
        h.output.next();
        h.playback.drain(&h.source_rx, &h.intent, &h.event_tx);
        h.playback.finish_if_ready(&h.intent, &h.event_tx); // Finished still waiting for UI.
        h.buffer(load(4), vec![0.3; 2]);
        // The sink already entered its keep-alive silence; use a new hardware-free
        // sink as production load_track does, leaving the stale events queued.
        let queued = h.intent.prepare(load(4));
        let source = queued.source.unwrap();
        let (sink, output) = Sink::new();
        h.sink = sink;
        h.output = output;
        h.queue(queued, SamplesBuffer::new(1, 1000, vec![0.4; 2]));
        h.source_tx
            .send(WorkerEvent::SourceEnded {
                source: SourceIdentity {
                    generation: source.generation - 1,
                    serial: source.serial - 1,
                },
                epoch: h.intent.state.lock().unwrap().epoch,
            })
            .unwrap();
        assert!(h.poll().is_empty());
        assert_eq!(h.output.by_ref().take(2).collect::<Vec<_>>(), vec![0.4; 2]);
        assert!(h.poll().is_empty());
    }

    #[test]
    fn new_load_invalidates_even_a_started_source() {
        let intent = Arc::new(PlaybackIntent::default());
        let queued = intent.prepare(load(1));
        let (tx, rx) = mpsc::channel();
        let mut source = EndNotifyingSource::new(
            GainSource::new(vec![0.2; 2].into_iter(), 1.0),
            tx,
            1,
            SourceTicket {
                source: queued.source.unwrap(),
                token: None,
            },
            queued.epoch,
            Arc::clone(&intent),
        );
        assert_eq!(source.next(), Some(0.2));
        intent.prepare(load(1));
        assert_eq!(source.next(), None);
        assert!(matches!(
            rx.try_recv(),
            Ok(WorkerEvent::SourceStarted { .. })
        ));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn seek_discards_old_end_event_and_cancels_successor_without_invalidating_active() {
        let mut h = Harness::new();
        let queued = h.intent.prepare(load(1));
        let mut source = EndNotifyingSource::new(
            GainSource::new(SamplesBuffer::new(1, 1000, vec![0.2]), 1.0),
            h.source_tx.clone(),
            1,
            SourceTicket {
                source: queued.source.unwrap(),
                token: None,
            },
            queued.epoch,
            Arc::clone(&h.intent),
        );
        h.playback.current = Some(ActiveSource {
            source: queued.source.unwrap(),
            id: 1,
            epoch: queued.epoch,
        });
        assert_eq!(source.next(), Some(0.2));
        assert_eq!(source.next(), None);
        h.intent.prepare(append(2, 4));
        let seeking = h.intent.prepare(AudioCommand::Seek(0.0));
        source.try_seek(Duration::ZERO).unwrap();
        h.playback.current.as_mut().unwrap().epoch = seeking.epoch;
        assert!(h.intent.state.lock().unwrap().prefetch.is_none());
        assert!(h.poll().is_empty());
        assert_eq!(source.next(), Some(0.2));
        assert_eq!(source.next(), None);
        assert!(matches!(h.poll().as_slice(), [AudioEvent::Finished(1)]));
    }

    #[test]
    fn pause_preserves_queue_and_does_not_start_successor() {
        let mut h = Harness::new();
        h.sink.pause();
        h.buffer(load(1), vec![0.1; 10]);
        h.buffer(append(2, 4), vec![0.2; 10]);
        assert!(h.output.by_ref().take(10).all(|sample| sample == 0.0));
        assert!(h.intent.state.lock().unwrap().playing.is_none());
        h.sink.play();
        // Sink applies pause controls at its 5 ms periodic access boundaries.
        assert_eq!(
            h.output.by_ref().take(20).collect::<Vec<_>>(),
            [vec![0.1; 10], vec![0.2; 10]].concat()
        );
        assert!(matches!(
            h.poll().as_slice(),
            [AudioEvent::Transitioned {
                from: 1,
                to: 2,
                token: 4
            }]
        ));
    }

    #[test]
    fn successor_position_is_relative_to_new_track_on_same_sink() {
        let mut h = Harness::new();
        h.buffer(load(1), vec![0.1; 20]);
        h.buffer(append(2, 4), vec![0.2; 30]);
        h.output.by_ref().take(28).for_each(drop);
        let position = h.sink.get_pos().as_secs_f64();
        assert!(
            (0.004..0.01).contains(&position),
            "relative position: {position}"
        );
        assert!(matches!(
            h.poll().as_slice(),
            [AudioEvent::Transitioned {
                from: 1,
                to: 2,
                token: 4
            }]
        ));
    }

    #[test]
    fn actual_fixture_decoders_preserve_samples_at_same_sink_boundaries() {
        for format in ["wav", "flac", "mp3", "m4a"] {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(format!("quiet.{format}"));
            let expected = open_source(&path, GainConfig::default())
                .unwrap()
                .collect::<Vec<_>>();
            assert!(!expected.is_empty());
            let mut h = Harness::new();
            let first = h.intent.prepare(load(1));
            let next = h.intent.prepare(append(2, 4));
            h.queue(first, open_source(&path, GainConfig::default()).unwrap());
            h.queue(next, open_source(&path, GainConfig::default()).unwrap());
            let actual = h
                .output
                .by_ref()
                .take(expected.len() * 2)
                .collect::<Vec<_>>();
            assert_eq!(
                actual,
                [expected.clone(), expected].concat(),
                "{format} queue boundary"
            );
            assert!(matches!(
                h.poll().as_slice(),
                [AudioEvent::Transitioned {
                    from: 1,
                    to: 2,
                    token: 4
                }]
            ));
        }
    }

    #[test]
    fn malformed_seek_is_rejected_without_changing_intent() {
        let (commands, _receiver) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let intent = Arc::new(PlaybackIntent::default());
        let engine = Engine {
            commands,
            events: AudioEvents {
                receiver: event_rx,
                intent: Arc::clone(&intent),
            },
            intent,
        };
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            assert!(engine.send(AudioCommand::Seek(value)).is_err());
        }
        assert_eq!(engine.intent.state.lock().unwrap().epoch, 0);
        drop(event_tx);
    }
}
