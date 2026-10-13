use super::{
    PlaybackStatus,
    gain::{GainConfig, GainSource, ReplayGainInfo},
};
use rodio::{Decoder, OutputStreamBuilder, Sink, Source};
use std::{
    fs::File,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

const INVALID_PREFETCH: u64 = u64::MAX;

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
    },
    /// The appended successor has become the active source. The controller should
    /// update current-track state without issuing another Load.
    Transitioned {
        from: usize,
        to: usize,
        token: u64,
    },
    /// Emitted only when the active source ends without a valid appended successor.
    Finished(usize),
    Error {
        id: Option<usize>,
        message: String,
    },
}

pub struct Engine {
    commands: Sender<AudioCommand>,
    pub events: Receiver<AudioEvent>,
    prefetch_token: Arc<AtomicU64>,
}

#[derive(Debug)]
enum WorkerEvent {
    SourceEnded { id: usize },
}

struct EndNotifyingSource<S> {
    inner: GainSource<S>,
    ended: Sender<WorkerEvent>,
    id: usize,
    token: u64,
    reported: bool,
    active_prefetch: Arc<AtomicU64>,
    active_playing: Arc<AtomicU64>,
}

impl<S> EndNotifyingSource<S> {
    fn new(
        inner: GainSource<S>,
        ended: Sender<WorkerEvent>,
        id: usize,
        token: u64,
        active_prefetch: Arc<AtomicU64>,
        active_playing: Arc<AtomicU64>,
    ) -> Self {
        Self {
            inner,
            ended,
            id,
            token,
            reported: false,
            active_prefetch,
            active_playing,
        }
    }
}

impl<S: Iterator<Item = f32>> Iterator for EndNotifyingSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        // A seek, load, stop or newer queue decision can invalidate an item which
        // was already placed behind the current source in Rodio's sink.
        let token_is_live = self.active_prefetch.load(Ordering::Acquire) == self.token
            || self.active_playing.load(Ordering::Acquire) == self.token;
        if self.token != 0 && !token_is_live {
            self.report_end();
            return None;
        }
        match self.inner.next() {
            Some(sample) => Some(sample),
            None => {
                self.report_end();
                None
            }
        }
    }
}

impl<S: rodio::Source<Item = f32>> Source for EndNotifyingSource<S> {
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
        self.inner.try_seek(pos)
    }
}

impl<S: Iterator<Item = f32>> EndNotifyingSource<S> {
    fn report_end(&mut self) {
        if !self.reported {
            self.reported = true;
            let _ = self.ended.send(WorkerEvent::SourceEnded { id: self.id });
        }
    }
}

impl Default for Engine {
    fn default() -> Self {
        let (commands, receiver) = mpsc::channel();
        let (events, event_receiver) = mpsc::channel();
        let prefetch_token = Arc::new(AtomicU64::new(INVALID_PREFETCH));
        let worker_prefetch_token = Arc::clone(&prefetch_token);
        let playing_token = Arc::new(AtomicU64::new(INVALID_PREFETCH));
        let worker_playing_token = Arc::clone(&playing_token);
        thread::spawn(move || {
            let (ended_tx, ended_rx) = mpsc::channel();
            let mut stream = None;
            let mut sink: Option<Sink> = None;
            let mut current: Option<usize> = None;
            let mut successor: Option<(usize, u64)> = None;
            let mut finished = false;
            loop {
                match receiver.recv_timeout(Duration::from_millis(50)) {
                    Ok(AudioCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Ok(command) => {
                        let loading_track = matches!(
                            command,
                            AudioCommand::Load { .. } | AudioCommand::LoadWithGain { .. }
                        );
                        let result: Result<(), String> = (|| {
                            match command {
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
                                        &mut stream,
                                        &mut sink,
                                        &mut current,
                                        &mut successor,
                                        &mut finished,
                                        &ended_tx,
                                        &worker_prefetch_token,
                                        &worker_playing_token,
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
                                        &mut stream,
                                        &mut sink,
                                        &mut current,
                                        &mut successor,
                                        &mut finished,
                                        &ended_tx,
                                        &worker_prefetch_token,
                                        &worker_playing_token,
                                    )?;
                                }
                                AudioCommand::AppendNext {
                                    token,
                                    id,
                                    path,
                                    gain,
                                } => {
                                    if worker_prefetch_token.load(Ordering::Acquire) != token {
                                        return Ok(());
                                    }
                                    if stream.is_none() {
                                        return Ok(());
                                    }
                                    let Some(active_sink) = &sink else {
                                        return Ok(());
                                    };
                                    let source = open_source(&path, gain)?;
                                    if worker_prefetch_token.load(Ordering::Acquire) != token {
                                        return Ok(());
                                    }
                                    active_sink.append(EndNotifyingSource::new(
                                        source,
                                        ended_tx.clone(),
                                        id,
                                        token,
                                        Arc::clone(&worker_prefetch_token),
                                        Arc::clone(&worker_playing_token),
                                    ));
                                    successor = Some((id, token));
                                }
                                AudioCommand::CancelPrefetch { token } => {
                                    if worker_prefetch_token.load(Ordering::Acquire) == token {
                                        worker_prefetch_token
                                            .store(INVALID_PREFETCH, Ordering::Release);
                                    }
                                    if successor.is_some_and(|(_, existing)| existing == token) {
                                        successor = None;
                                    }
                                }
                                AudioCommand::Pause(paused) => {
                                    if let Some(active_sink) = &sink {
                                        if paused {
                                            active_sink.pause();
                                        } else {
                                            active_sink.play();
                                        }
                                    }
                                }
                                AudioCommand::Stop { handoff } => {
                                    worker_prefetch_token
                                        .store(INVALID_PREFETCH, Ordering::Release);
                                    worker_playing_token.store(INVALID_PREFETCH, Ordering::Release);
                                    successor = None;
                                    if let Some(old) = sink.take() {
                                        old.stop();
                                    }
                                    stream = None;
                                    current = None;
                                    finished = true;
                                    let _ = events.send(AudioEvent::Stopped { handoff });
                                }
                                AudioCommand::Volume(volume) => {
                                    if let Some(active_sink) = &sink {
                                        active_sink.set_volume(volume);
                                    }
                                }
                                AudioCommand::Seek(seconds) => {
                                    worker_prefetch_token
                                        .store(INVALID_PREFETCH, Ordering::Release);
                                    successor = None;
                                    if let Some(active_sink) = &sink {
                                        active_sink
                                            .try_seek(Duration::from_secs_f64(seconds.max(0.0)))
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
                                successor = None;
                            }
                            let _ = events.send(AudioEvent::Error {
                                id: current,
                                message,
                            });
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                while let Ok(WorkerEvent::SourceEnded { id }) = ended_rx.try_recv() {
                    if current != Some(id) {
                        continue;
                    }
                    if let Some((next_id, next_token)) = successor.take()
                        && worker_prefetch_token.load(Ordering::Acquire) == next_token
                    {
                        current = Some(next_id);
                        worker_playing_token.store(next_token, Ordering::Release);
                        let _ = events.send(AudioEvent::Transitioned {
                            from: id,
                            to: next_id,
                            token: next_token,
                        });
                        if let Some(active_sink) = &sink {
                            let _ = events.send(AudioEvent::Progress {
                                id: next_id,
                                status: if active_sink.is_paused() {
                                    PlaybackStatus::Paused
                                } else {
                                    PlaybackStatus::Playing
                                },
                                position: 0.0,
                            });
                        }
                    } else if !finished {
                        finished = true;
                        let _ = events.send(AudioEvent::Finished(id));
                    }
                }
                if let (Some(active_sink), Some(id)) = (&sink, current) {
                    if active_sink.empty() {
                        if !finished {
                            finished = true;
                            let _ = events.send(AudioEvent::Finished(id));
                        }
                    } else {
                        let _ = events.send(AudioEvent::Progress {
                            id,
                            status: if active_sink.is_paused() {
                                PlaybackStatus::Paused
                            } else {
                                PlaybackStatus::Playing
                            },
                            position: active_sink.get_pos().as_secs_f64(),
                        });
                    }
                }
            }
        });
        Self {
            commands,
            events: event_receiver,
            prefetch_token,
        }
    }
}

fn open_source(
    path: &std::path::Path,
    gain: GainConfig,
) -> Result<GainSource<Decoder<std::io::BufReader<File>>>, String> {
    let info = ReplayGainInfo::read(path);
    let gain = info.linear_gain(gain);
    let decoder = Decoder::try_from(File::open(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(GainSource::new(decoder, gain))
}

fn load_track(
    id: usize,
    path: PathBuf,
    volume: f32,
    paused: bool,
    gain: GainConfig,
    stream: &mut Option<rodio::OutputStream>,
    sink: &mut Option<Sink>,
    current: &mut Option<usize>,
    successor: &mut Option<(usize, u64)>,
    finished: &mut bool,
    ended_tx: &Sender<WorkerEvent>,
    prefetch_token: &Arc<AtomicU64>,
    playing_token: &Arc<AtomicU64>,
) -> Result<(), String> {
    prefetch_token.store(INVALID_PREFETCH, Ordering::Release);
    playing_token.store(INVALID_PREFETCH, Ordering::Release);
    successor.take();
    if let Some(old) = sink.take() {
        old.stop();
    }
    *current = Some(id);
    *finished = false;
    if stream.is_none() {
        *stream = Some(OutputStreamBuilder::open_default_stream().map_err(|e| e.to_string())?);
    }
    let output = stream
        .as_ref()
        .ok_or_else(|| "No audio output".to_owned())?;
    let source = open_source(&path, gain)?;
    let next = Sink::connect_new(output.mixer());
    next.set_volume(volume);
    if paused {
        next.pause();
    }
    next.append(EndNotifyingSource::new(
        source,
        ended_tx.clone(),
        id,
        0,
        Arc::clone(prefetch_token),
        Arc::clone(playing_token),
    ));
    *sink = Some(next);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FiniteSource(usize);

    impl Iterator for FiniteSource {
        type Item = f32;
        fn next(&mut self) -> Option<Self::Item> {
            if self.0 == 0 {
                None
            } else {
                self.0 -= 1;
                Some(0.25)
            }
        }
    }

    impl Source for FiniteSource {
        fn current_span_len(&self) -> Option<usize> {
            Some(self.0)
        }
        fn channels(&self) -> rodio::ChannelCount {
            1
        }
        fn sample_rate(&self) -> rodio::SampleRate {
            44_100
        }
        fn total_duration(&self) -> Option<Duration> {
            Some(Duration::from_millis(1))
        }
    }

    #[test]
    fn source_end_is_reported_once() {
        let (ended_tx, ended_rx) = mpsc::channel();
        let prefetch = Arc::new(AtomicU64::new(9));
        let playing = Arc::new(AtomicU64::new(INVALID_PREFETCH));
        let mut source = EndNotifyingSource::new(
            GainSource::new(FiniteSource(2), 1.0),
            ended_tx,
            7,
            9,
            Arc::clone(&prefetch),
            Arc::clone(&playing),
        );
        assert_eq!(source.next(), Some(0.25));
        assert_eq!(source.next(), Some(0.25));
        assert_eq!(source.next(), None);
        assert_eq!(source.next(), None);
        assert!(matches!(
            ended_rx.try_recv(),
            Ok(WorkerEvent::SourceEnded { id: 7 })
        ));
        assert!(ended_rx.try_recv().is_err());
    }

    #[test]
    fn stale_successor_is_silently_dropped_at_consumption() {
        let (ended_tx, ended_rx) = mpsc::channel();
        let prefetch = Arc::new(AtomicU64::new(INVALID_PREFETCH));
        let playing = Arc::new(AtomicU64::new(INVALID_PREFETCH));
        let mut source = EndNotifyingSource::new(
            GainSource::new(FiniteSource(2), 1.0),
            ended_tx,
            8,
            10,
            prefetch,
            playing,
        );
        assert_eq!(source.next(), None);
        assert!(matches!(
            ended_rx.try_recv(),
            Ok(WorkerEvent::SourceEnded { id: 8 })
        ));
    }
}

impl Engine {
    pub fn send(&self, command: AudioCommand) -> Result<(), String> {
        match &command {
            AudioCommand::AppendNext { token, .. } => {
                self.prefetch_token.store(*token, Ordering::Release)
            }
            AudioCommand::CancelPrefetch { token } => {
                if self.prefetch_token.load(Ordering::Acquire) == *token {
                    self.prefetch_token
                        .store(INVALID_PREFETCH, Ordering::Release);
                }
            }
            AudioCommand::Load { .. }
            | AudioCommand::LoadWithGain { .. }
            | AudioCommand::Seek(_)
            | AudioCommand::Stop { .. } => self
                .prefetch_token
                .store(INVALID_PREFETCH, Ordering::Release),
            _ => {}
        }
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
