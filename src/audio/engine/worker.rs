//! Command processing. All file opening and decoder seeking belong to this worker.
use super::backend::{OutputFailure, RodioBackend};
use super::{
    AudioCommand, AudioEvent, EventEnvelope,
    intent::{PlaybackIntent, QueuedCommand, SourceIdentity, SourceTicket},
    playback::{ActiveSource, PlaybackPhase, WorkerPlayback},
    source::{EndNotifyingSource, WorkerEvent, open_decoder, open_source},
};
use crate::audio::{
    PlaybackStatus,
    gain::{GainConfig, GainSource},
};
use rodio::{Decoder, Sink, Source};
use std::collections::HashMap;
use std::{
    fs::File,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

struct LoadOptions {
    volume: f32,
    paused: bool,
    start: f64,
    speed: f32,
}

type DecodedSource = GainSource<Decoder<std::io::BufReader<File>>>;

struct TrackRecord {
    path: PathBuf,
    gain: f32,
    prevent_clipping: bool,
    epoch: Arc<AtomicU64>,
    retired: Arc<AtomicBool>,
}

pub(super) struct AudioWorker {
    pub(super) backend: RodioBackend,
    pub(super) playback: WorkerPlayback,
    tracks: HashMap<SourceIdentity, TrackRecord>,
    source_events: Sender<WorkerEvent>,
    source_receiver: Receiver<WorkerEvent>,
    failure_receiver: Receiver<OutputFailure>,
    #[cfg(test)]
    pub(super) failure_sender: Sender<OutputFailure>,
    events: Sender<EventEnvelope>,
    intent: Arc<PlaybackIntent>,
    // Rodio reports elapsed time in the replacement source. Keep the seek
    // offset here, and reset it on a genuine successor transition.
    position_offset: Duration,
    speed: f32,
}

impl AudioWorker {
    pub(super) fn new(events: Sender<EventEnvelope>, intent: Arc<PlaybackIntent>) -> Self {
        let (source_events, source_receiver) = mpsc::channel();
        let (failure_sender, failure_receiver) = mpsc::channel();
        Self {
            backend: RodioBackend::new(failure_sender.clone()),
            playback: WorkerPlayback::default(),
            tracks: HashMap::new(),
            source_events,
            source_receiver,
            failure_receiver,
            #[cfg(test)]
            failure_sender,
            events,
            intent,
            position_offset: Duration::ZERO,
            speed: 1.0,
        }
    }

    fn publish(&self, envelope: Option<EventEnvelope>) {
        if let Some(envelope) = envelope {
            let _ = self.events.send(envelope);
        }
    }

    fn drain_sources(&mut self) {
        while let Ok(event) = self.source_receiver.try_recv() {
            if let Some(envelope) = self.playback.apply(event, self.intent.snapshot()) {
                self.position_offset = Duration::ZERO;
                self.publish(Some(envelope));
            }
        }
    }

    fn output_failures(&mut self) {
        while let Ok(failure) = self.failure_receiver.try_recv() {
            if !self.backend.accepts_failure(&failure) {
                continue;
            }
            self.backend.stop();
            // Stopping output first completes any in-flight callback. Its
            // source-start notification is authoritative even when failure
            // follows immediately, so adopt it before reporting the error.
            self.drain_sources();
            let state = self.intent.snapshot();
            if let Some(current) = self.playback.current {
                if current.source.generation == state.generation
                    && let Some(ticket) = state.prefetch
                {
                    self.intent.cancel_source(ticket.source);
                }
                self.publish(self.playback.envelope(
                    AudioEvent::Error {
                        id: Some(current.id),
                        message: format!("Audio output failed: {}", failure.message),
                    },
                    false,
                ));
            }
            self.playback.phase = PlaybackPhase::Failed;
            self.tracks.clear();
        }
    }

    fn stopped(&self, handoff: u64) {
        let _ = self.events.send(EventEnvelope {
            event: AudioEvent::Stopped { handoff },
            generation: None,
            epoch: None,
        });
    }

    // Wrap before queueing so the worker retains the path, resolved gain and
    // epoch handle of every potentially active source, including successors.
    fn tracked_source(
        &mut self,
        source: DecodedSource,
        path: PathBuf,
        current: ActiveSource,
        token: Option<u64>,
    ) -> EndNotifyingSource<Decoder<std::io::BufReader<File>>> {
        let (gain, prevent_clipping) = source.processing();
        let source = EndNotifyingSource::new(
            source,
            self.source_events.clone(),
            current.id,
            SourceTicket {
                source: current.source,
                token,
            },
            current.epoch,
            Arc::clone(&self.intent),
        );
        self.tracks.insert(
            current.source,
            TrackRecord {
                path,
                gain,
                prevent_clipping,
                epoch: Arc::clone(&source.epoch),
                retired: Arc::clone(&source.retired),
            },
        );
        source
    }

    fn load(
        &mut self,
        current: ActiveSource,
        path: PathBuf,
        gain: GainConfig,
        options: LoadOptions,
    ) -> Result<(), String> {
        let LoadOptions {
            volume,
            paused,
            start,
            speed,
        } = options;
        crate::shared::diagnostics::record(
            "audio_load_begin",
            serde_json::json!({"session": current.id, "paused": paused}),
        );
        if self.intent.reopen_output.swap(false, Ordering::AcqRel) {
            crate::shared::diagnostics::record(
                "audio_output_reopen",
                serde_json::json!({"session": current.id}),
            );
            self.backend.stop();
        } else {
            self.backend.stop_sink();
        }
        self.tracks.clear();
        self.position_offset = Duration::ZERO;
        self.playback = WorkerPlayback {
            current: Some(current),
            phase: PlaybackPhase::Failed,
        };
        let mut source = open_source(&path, gain)?;
        let target = Duration::from_secs_f64(start.max(0.0));
        if !target.is_zero() {
            source.try_seek(target).map_err(|e| e.to_string())?;
        }
        self.position_offset = target;
        self.speed = speed;
        crate::shared::diagnostics::record(
            "audio_decode_ready",
            serde_json::json!({"session": current.id}),
        );
        if self.intent.snapshot().generation != current.source.generation {
            return Ok(());
        }
        let next = self.backend.new_sink(volume, paused)?;
        next.set_speed(speed);
        crate::shared::diagnostics::record(
            "audio_sink_ready",
            serde_json::json!({"session": current.id}),
        );
        // Output opening can itself take time; never publish a stale load.
        if self.intent.snapshot().generation != current.source.generation {
            return Ok(());
        }
        let source = self.tracked_source(source, path, current, None);
        self.playback = WorkerPlayback::loaded(current);
        next.append(source);
        self.backend.sink = Some(next);
        Ok(())
    }

    fn append(&mut self, current: ActiveSource, token: u64, path: PathBuf, gain: GainConfig) {
        if !self.intent.prefetch_is_live(current.source) {
            return;
        }
        if matches!(
            self.playback.phase,
            PlaybackPhase::Finished | PlaybackPhase::Failed
        ) || self.backend.sink.is_none()
        {
            self.intent.cancel_source(current.source);
            return;
        }
        let source = match open_source(&path, gain) {
            Ok(source) => source,
            Err(message) => {
                self.intent.cancel_source(current.source);
                let _ = self.events.send(EventEnvelope {
                    event: AudioEvent::PrefetchFailed {
                        id: current.id,
                        token,
                        message,
                    },
                    generation: Some(current.source.generation),
                    epoch: Some(current.epoch),
                });
                return;
            }
        };
        if self.intent.prefetch_is_live(current.source) {
            let source = self.tracked_source(source, path, current, Some(token));
            self.backend.sink.as_ref().unwrap().append(source);
        }
    }

    fn seek(
        &mut self,
        source_id: SourceIdentity,
        epoch: u64,
        seconds: f64,
        speed: Option<f32>,
    ) -> Result<(), String> {
        if !self.intent.seek_is_live(source_id.generation, epoch) {
            return Ok(());
        }
        let Some(current) = self.playback.current else {
            return Ok(());
        };
        let Some(record) = self.tracks.get(&current.source) else {
            return Ok(());
        };
        let Some(sink) = &self.backend.sink else {
            return Ok(());
        };
        let volume = sink.volume();
        let paused = sink.is_paused();
        let path = record.path.clone();
        let target = Duration::try_from_secs_f64(seconds.max(0.0)).map_err(|e| e.to_string())?;
        let prepared = (|| {
            let mut source = open_decoder(&path, record.gain, record.prevent_clipping)?;
            let target = source
                .total_duration()
                .map_or(target, |duration| target.min(duration));
            // Seek an unqueued decoder on the worker. Sink::try_seek can block
            // forever if EOF or a lost device removes its feedback callback.
            source.try_seek(target).map_err(|e| e.to_string())?;
            Ok::<_, String>((source, target))
        })();
        if !self.intent.seek_is_live(source_id.generation, epoch) {
            return Ok(());
        }
        let (source, target) = match prepared {
            Ok(prepared) => prepared,
            Err(message) => {
                // Keep the existing source after a failed seek. Its future EOF
                // must use the new epoch even though no decoder seek occurred.
                record.epoch.store(epoch, Ordering::Release);
                self.playback.current.as_mut().unwrap().epoch = epoch;
                if sink.empty() && self.playback.phase != PlaybackPhase::Finished {
                    self.playback.phase = PlaybackPhase::Ended;
                }
                return Err(message);
            }
        };
        let next = self.backend.new_sink(volume, paused)?;
        let speed = speed.unwrap_or(self.speed);
        next.set_speed(speed);
        if !self.intent.replace_playing(
            source_id,
            epoch,
            &self.tracks.get(&current.source).unwrap().retired,
        ) {
            return Ok(());
        }
        // Sink::stop is observed only at Rodio's periodic control checks. Retire
        // the predecessor before queueing replacement samples to prevent overlap.
        let current = ActiveSource {
            source: source_id,
            id: current.id,
            epoch,
        };
        let source = self.tracked_source(source, path, current, None);
        self.backend.stop_sink();
        self.playback = WorkerPlayback::loaded(current);
        self.position_offset = target;
        self.speed = speed;
        next.append(source);
        self.backend.sink = Some(next);
        Ok(())
    }

    /// Returns false only for shutdown. Intent changes are never repeated here.
    pub(super) fn process(&mut self, queued: QueuedCommand) -> bool {
        self.output_failures();
        self.drain_sources();
        if matches!(queued.command, AudioCommand::Shutdown) {
            return false;
        }
        if queued.generation != self.intent.snapshot().generation {
            if let AudioCommand::Stop { handoff } = queued.command {
                self.stopped(handoff);
            }
            return true;
        }
        let loading = matches!(
            queued.command,
            AudioCommand::Load { .. }
                | AudioCommand::LoadWithGain { .. }
                | AudioCommand::LoadAt { .. }
        );
        let current = |id| ActiveSource {
            source: queued.source.unwrap(),
            id,
            epoch: queued.epoch,
        };
        let result = match queued.command {
            AudioCommand::Load {
                id,
                path,
                volume,
                paused,
            } => self.load(
                current(id),
                path,
                GainConfig::default(),
                LoadOptions {
                    volume,
                    paused,
                    start: 0.0,
                    speed: 1.0,
                },
            ),
            AudioCommand::LoadWithGain {
                id,
                path,
                volume,
                paused,
                gain,
            } => self.load(
                current(id),
                path,
                gain,
                LoadOptions {
                    volume,
                    paused,
                    start: 0.0,
                    speed: 1.0,
                },
            ),
            AudioCommand::LoadAt {
                id,
                path,
                volume,
                paused,
                start,
                speed,
            } => self.load(
                current(id),
                path,
                GainConfig::default(),
                LoadOptions {
                    volume,
                    paused,
                    start,
                    speed,
                },
            ),
            AudioCommand::AppendNext {
                token,
                id,
                path,
                gain,
            } => {
                self.append(current(id), token, path, gain);
                Ok(())
            }
            AudioCommand::CancelPrefetch { .. } => Ok(()),
            AudioCommand::Pause(paused) => {
                if let Some(sink) = &self.backend.sink {
                    if paused {
                        sink.pause();
                    } else {
                        sink.play();
                    }
                }
                Ok(())
            }
            AudioCommand::Volume(volume) => {
                if let Some(sink) = &self.backend.sink {
                    sink.set_volume(volume);
                }
                Ok(())
            }
            AudioCommand::Seek(seconds) => self.seek(
                queued.source.unwrap(),
                queued.epoch,
                seconds,
                Some(queued.speed),
            ),
            AudioCommand::Speed(speed) => {
                let seconds = self.position_offset.as_secs_f64()
                    + self
                        .backend
                        .sink
                        .as_ref()
                        .map_or(0.0, |s| s.get_pos().as_secs_f64() * self.speed as f64);
                self.seek(queued.source.unwrap(), queued.epoch, seconds, Some(speed))
            }
            AudioCommand::Stop { handoff } => {
                self.backend.stop();
                self.playback = WorkerPlayback::default();
                self.tracks.clear();
                self.position_offset = Duration::ZERO;
                self.stopped(handoff);
                Ok(())
            }
            AudioCommand::Shutdown => unreachable!(),
        };
        if let Err(message) = result {
            crate::shared::diagnostics::record(
                "audio_worker_error",
                serde_json::json!({"session": self.playback.current.map(|current| current.id), "message": message.chars().take(512).collect::<String>()}),
            );
            let _ = self.events.send(EventEnvelope {
                event: AudioEvent::Error {
                    id: self.playback.current.map(|current| current.id),
                    message,
                },
                generation: Some(queued.generation),
                epoch: (!loading).then_some(queued.epoch),
            });
        }
        true
    }

    pub(super) fn poll(&mut self) {
        self.output_failures();
        self.drain_sources();
        let state = self.intent.snapshot();
        // An EOF notification may have been invalidated by a failed seek. The
        // empty sink is an independent completion observation in that epoch.
        if self.playback.phase == PlaybackPhase::Active
            && self.backend.sink.as_ref().is_some_and(Sink::empty)
        {
            self.playback.phase = PlaybackPhase::Ended;
        }
        let finished = self.playback.finish_if_ready(state);
        self.publish(finished);
        if self.playback.can_report_progress(state)
            && let (Some(sink), Some(current)) = (&self.backend.sink, self.playback.current)
        {
            let position = self
                .position_offset
                .saturating_add(sink.get_pos().mul_f64(self.speed as f64))
                .as_secs_f64();
            // The callback can switch sources while get_pos runs. Recheck the
            // source latch so successor positions never get labeled as predecessor progress.
            if self.playback.can_report_progress(self.intent.snapshot()) {
                self.publish(self.playback.envelope(
                    AudioEvent::Progress {
                        id: current.id,
                        status: if sink.is_paused() {
                            PlaybackStatus::Paused
                        } else {
                            PlaybackStatus::Playing
                        },
                        position,
                        observed_at: Instant::now(),
                    },
                    true,
                ));
            }
        }
        self.tracks.retain(|source, _| {
            self.playback
                .current
                .is_some_and(|current| current.source == *source)
                || state
                    .prefetch
                    .is_some_and(|ticket| ticket.source == *source)
                || state.playing.is_some_and(|ticket| ticket.source == *source)
        });
    }
}

pub(super) fn run(
    receiver: Receiver<QueuedCommand>,
    events: Sender<EventEnvelope>,
    intent: Arc<PlaybackIntent>,
) {
    struct WorkerExit;
    impl Drop for WorkerExit {
        fn drop(&mut self) {
            crate::shared::diagnostics::record(
                "audio_worker_exit",
                serde_json::json!({"panicking": std::thread::panicking()}),
            );
        }
    }
    let _exit = WorkerExit;
    crate::shared::diagnostics::record("audio_worker_start", serde_json::json!({}));
    let mut worker = AudioWorker::new(events, intent);
    loop {
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(queued) => {
                if !worker.process(queued) {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        worker.poll();
    }
    worker.backend.stop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_reopen_survives_a_superseded_load_and_retires_the_old_mixer() {
        let intent = Arc::new(PlaybackIntent::default());
        let (tx, _rx) = mpsc::channel();
        let mut worker = AudioWorker::new(tx, Arc::clone(&intent));
        let (mixer, _output) = rodio::mixer::mixer(1, 44_100);
        worker.backend.mixer = Some(mixer);
        let missing = PathBuf::from("/no-such-nano-test-source.wav");
        let obsolete = intent.prepare(AudioCommand::Load {
            id: 1,
            path: missing.clone(),
            volume: 1.0,
            paused: false,
        });
        intent.reopen_output.store(true, Ordering::Release);
        let current = intent.prepare(AudioCommand::Load {
            id: 2,
            path: missing,
            volume: 1.0,
            paused: false,
        });
        worker.process(obsolete);
        assert!(intent.reopen_output.load(Ordering::Acquire));
        assert!(worker.backend.mixer.is_some());
        worker.process(current);
        assert!(!intent.reopen_output.load(Ordering::Acquire));
        assert!(
            worker.backend.mixer.is_none(),
            "recovery must release the prior device, even after coalescing"
        );
        assert!(worker.backend.sink.is_none());
        assert_eq!(worker.playback.current.unwrap().id, 2);
    }

    #[test]
    fn output_loss_after_unread_successor_start_reports_the_successor() {
        let intent = Arc::new(PlaybackIntent::default());
        let (tx, rx) = mpsc::channel();
        let mut worker = AudioWorker::new(tx, Arc::clone(&intent));
        let (mixer, mut output) = rodio::mixer::mixer(1, 44_100);
        worker.backend.mixer = Some(mixer);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quiet.wav");
        worker.process(intent.prepare(AudioCommand::Load {
            id: 1,
            path: path.clone(),
            volume: 1.0,
            paused: false,
        }));
        worker.process(intent.prepare(AudioCommand::AppendNext {
            token: 9,
            id: 2,
            path,
            gain: GainConfig::default(),
        }));
        for _ in 0..15_000 {
            output.next();
            if intent
                .snapshot()
                .playing
                .is_some_and(|ticket| ticket.token == Some(9))
            {
                break;
            }
        }
        assert_eq!(intent.snapshot().playing.unwrap().token, Some(9));
        worker
            .failure_sender
            .send(OutputFailure {
                stream: worker.backend.stream_serial,
                message: "output disconnected".into(),
            })
            .unwrap();
        worker.poll();
        let events = super::super::AudioEvents {
            receiver: rx,
            intent,
        };
        assert!(matches!(
            events.try_recv().unwrap(),
            AudioEvent::Transitioned {
                from: 1,
                to: 2,
                token: 9
            }
        ));
        assert!(matches!(
            events.try_recv().unwrap(),
            AudioEvent::Error { id: Some(2), .. }
        ));
        assert!(events.try_recv().is_err());
    }
}
