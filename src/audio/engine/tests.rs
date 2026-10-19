use super::*;
use super::{intent::*, playback::*, source::*};
use crate::audio::gain::GainSource;
use rodio::buffer::SamplesBuffer;
use rodio::{Sink, Source};

struct GatedSample {
    entered: Sender<()>,
    release: Receiver<()>,
}
impl Iterator for GatedSample {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        self.entered.send(()).unwrap();
        self.release.recv().unwrap();
        Some(0.9)
    }
}
#[test]
fn cancellation_during_first_sample_does_not_wait_for_decoder_or_publish_start() {
    let intent = Arc::new(PlaybackIntent::default());
    intent.prepare(load(1));
    let queued = intent.prepare(append(2, 7));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (source_tx, source_rx) = mpsc::channel();
    let mut source = EndNotifyingSource::new(
        GainSource::new(
            GatedSample {
                entered: entered_tx,
                release: release_rx,
            },
            1.0,
        ),
        source_tx,
        2,
        SourceTicket {
            source: queued.source.unwrap(),
            token: Some(7),
        },
        queued.epoch,
        Arc::clone(&intent),
    );
    let consuming = thread::spawn(move || source.next());
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let (canceled_tx, canceled_rx) = mpsc::channel();
    let canceling = thread::spawn(move || {
        intent.prepare(AudioCommand::CancelPrefetch { token: 7 });
        canceled_tx.send(()).unwrap();
    });
    let canceled_without_decoding = canceled_rx.recv_timeout(Duration::from_secs(1)).is_ok();
    // Release even on regression, so the test cannot leave a blocked thread.
    release_tx.send(()).unwrap();
    let sample = consuming.join().unwrap();
    canceling.join().unwrap();
    assert!(
        canceled_without_decoding,
        "cancellation waited for first-sample decoding"
    );
    assert_eq!(sample, None);
    assert!(source_rx.try_recv().is_err());
}

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

    fn queue<S: Source<Item = f32> + Send + 'static>(&mut self, queued: QueuedCommand, source: S) {
        let (id, token) = match queued.command {
            AudioCommand::Load { id, .. } | AudioCommand::LoadWithGain { id, .. } => {
                self.playback = WorkerPlayback::loaded(ActiveSource {
                    id,
                    source: queued.source.unwrap(),
                    epoch: queued.epoch,
                });
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

    fn drain(&mut self) {
        while let Ok(event) = self.source_rx.try_recv() {
            if let Some(event) = self.playback.apply(event, self.intent.snapshot()) {
                self.event_tx.send(event).unwrap();
            }
        }
    }

    fn poll(&mut self) -> Vec<AudioEvent> {
        self.drain();
        if let Some(event) = self.playback.finish_if_ready(self.intent.snapshot()) {
            self.event_tx.send(event).unwrap();
        }
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
    h.drain();
    let event = h.playback.finish_if_ready(h.intent.snapshot()).unwrap();
    h.event_tx.send(event).unwrap(); // Finished still waiting for UI.
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
    h.playback = WorkerPlayback::loaded(ActiveSource {
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

struct WorkerHarness {
    worker: worker::AudioWorker,
    intent: Arc<PlaybackIntent>,
    events: AudioEvents,
    mixer: rodio::mixer::Mixer,
    output: rodio::mixer::MixerSource,
}

impl WorkerHarness {
    fn new() -> Self {
        let intent = Arc::new(PlaybackIntent::default());
        let (tx, rx) = mpsc::channel();
        let (mixer, output) = rodio::mixer::mixer(1, 44_100);
        let mut worker = worker::AudioWorker::new(tx, Arc::clone(&intent));
        worker.backend.mixer = Some(mixer.clone());
        Self {
            worker,
            events: AudioEvents {
                receiver: rx,
                intent: Arc::clone(&intent),
            },
            intent,
            mixer,
            output,
        }
    }

    fn send(&mut self, command: AudioCommand) -> bool {
        self.worker.process(self.intent.prepare(command))
    }

    fn fixture_load(&mut self, id: usize, paused: bool) {
        self.send(AudioCommand::Load {
            id,
            path: fixture("wav"),
            volume: 0.5,
            paused,
        });
    }

    fn poll(&mut self) -> Vec<AudioEvent> {
        self.worker.poll();
        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            events.push(event);
        }
        events
    }
}

fn fixture(format: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("quiet.{format}"))
}

#[test]
fn load_at_seeks_before_output_preserves_pause_and_invalidates_old_events() {
    let mut h = WorkerHarness::new();
    h.fixture_load(1, false);
    h.worker.poll(); // An undelivered predecessor progress event is pending.
    h.send(AudioCommand::LoadAt {
        id: usize::MAX,
        path: fixture("wav"),
        volume: 0.3,
        paused: true,
        start: 0.1,
        speed: 1.0,
    });
    assert!(h.worker.backend.sink.as_ref().unwrap().is_paused());
    assert_eq!(h.worker.backend.sink.as_ref().unwrap().volume(), 0.3);
    let events = h.poll();
    assert!(
        events
            .iter()
            .all(|e| !matches!(e, AudioEvent::Progress { id: 1, .. }))
    );
    assert!(events.iter().any(|e| matches!(e, AudioEvent::Progress { id, position, status: PlaybackStatus::Paused, .. } if *id == usize::MAX && (*position - 0.1).abs() < 0.0001)));
    h.send(AudioCommand::Pause(false));
    h.output.by_ref().take(2000).for_each(drop);
    assert!(
        h.poll()
            .iter()
            .any(|e| matches!(e, AudioEvent::Progress { position, .. } if *position >= 0.1))
    );
}

#[test]
fn spoken_speed_preserves_media_time_on_changes_and_seeks_and_resets_for_music() {
    let mut h = WorkerHarness::new();
    h.send(AudioCommand::LoadAt {
        id: usize::MAX,
        path: fixture("wav"),
        volume: 0.5,
        paused: false,
        start: 0.05,
        speed: 2.0,
    });
    h.output.by_ref().take(1000).for_each(drop);
    let position = h
        .poll()
        .iter()
        .find_map(|e| {
            if let AudioEvent::Progress { position, .. } = e {
                Some(*position)
            } else {
                None
            }
        })
        .unwrap();
    // Rodio updates its clock in buffered control callbacks, so its sampled
    // position can lag the output iterator; it must advance from the resume point.
    assert!(
        position > 0.05 && position < 0.11,
        "media position at 2x: {position}"
    );
    h.send(AudioCommand::Pause(true));
    h.send(AudioCommand::Speed(0.5));
    let current = h
        .poll()
        .iter()
        .find_map(|e| {
            if let AudioEvent::Progress { position, .. } = e {
                Some(*position)
            } else {
                None
            }
        })
        .unwrap();
    assert!((current - position).abs() < 0.005);
    assert!(h.worker.backend.sink.as_ref().unwrap().is_paused());
    assert_eq!(h.worker.backend.sink.as_ref().unwrap().speed(), 0.5);
    h.send(AudioCommand::Seek(0.12));
    assert_eq!(h.worker.backend.sink.as_ref().unwrap().speed(), 0.5);
    h.fixture_load(0, true);
    assert_eq!(h.worker.backend.sink.as_ref().unwrap().speed(), 1.0);
    // A rapid seek must retain the preceding speed intent even before it ran.
    let speed = h.intent.prepare(AudioCommand::Speed(2.0));
    let seek = h.intent.prepare(AudioCommand::Seek(0.1));
    h.worker.process(speed);
    h.worker.process(seek);
    assert_eq!(h.worker.backend.sink.as_ref().unwrap().speed(), 2.0);
    // Conversely a speed change must not invalidate a seek waiting in the queue.
    let seek = h.intent.prepare(AudioCommand::Seek(0.15));
    let speed = h.intent.prepare(AudioCommand::Speed(0.5));
    h.worker.process(seek);
    h.worker.process(speed);
    assert_eq!(h.worker.backend.sink.as_ref().unwrap().speed(), 0.5);
    assert!(
        h.poll()
            .iter()
            .any(|e| matches!(e,AudioEvent::Progress{position,..} if (*position-0.15).abs()<0.005))
    );
}

#[test]
fn worker_seek_without_output_callbacks_preserves_pause_volume_and_accepts_stop() {
    let (done_tx, done_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let mut h = WorkerHarness::new();
        h.fixture_load(1, true);
        // Queued samples without any output callback must not block controls.
        let seeking = h.intent.prepare(AudioCommand::Seek(0.1));
        h.worker.process(seeking);
        let sink = h.worker.backend.sink.as_ref().unwrap();
        assert!(sink.is_paused());
        assert_eq!(sink.volume(), 0.5);
        assert!(h.poll().iter().any(|event| matches!(event,
            AudioEvent::Progress { id: 1, status: PlaybackStatus::Paused, position, .. } if (*position - 0.1).abs() < 0.0001
        )));
        h.send(AudioCommand::Stop { handoff: 42 });
        let stopped = h
            .poll()
            .iter()
            .any(|event| matches!(event, AudioEvent::Stopped { handoff: 42 }));
        let shutdown = !h.send(AudioCommand::Shutdown);
        done_tx.send(stopped && shutdown).unwrap();
    });
    assert!(
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        "seek prevented subsequent controls"
    );
    handle.join().unwrap();
}

#[test]
fn worker_seek_after_eof_reopens_identified_track_and_finishes_once() {
    let mut h = WorkerHarness::new();
    h.fixture_load(3, false);
    h.output.by_ref().take(12_000).for_each(drop);
    assert!(h.worker.backend.sink.as_ref().unwrap().empty());
    // Leave the original EOF notification unread; Seek must discard it.
    h.send(AudioCommand::Seek(0.2));
    let events = h.poll();
    assert!(!events.iter().any(|event| matches!(
        event,
        AudioEvent::Finished(_) | AudioEvent::Transitioned { .. } | AudioEvent::Error { .. }
    )));
    h.output.by_ref().take(5_000).for_each(drop);
    let events = h.poll();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AudioEvent::Finished(3)))
            .count(),
        1
    );
    assert!(
        !h.poll()
            .iter()
            .any(|event| matches!(event, AudioEvent::Finished(_)))
    );
}

#[test]
fn worker_boundary_seek_cancels_unstarted_successor_and_keeps_predecessor() {
    let mut h = WorkerHarness::new();
    h.fixture_load(1, false);
    h.send(AudioCommand::AppendNext {
        token: 7,
        id: 2,
        path: fixture("flac"),
        gain: GainConfig::default(),
    });
    let predecessor_samples = open_source(&fixture("wav"), GainConfig::default())
        .unwrap()
        .count();
    h.output
        .by_ref()
        .take(predecessor_samples - 1)
        .for_each(drop);
    h.send(AudioCommand::Seek(0.05));
    h.output.by_ref().take(2_000).for_each(drop);
    assert_eq!(h.worker.playback.current.unwrap().id, 1);
    let events = h.poll();
    assert!(!events.iter().any(|event| matches!(
        event,
        AudioEvent::Transitioned { .. } | AudioEvent::Finished(_) | AudioEvent::Error { .. }
    )));
    assert!(events.iter().any(|event| matches!(event, AudioEvent::Progress { id: 1, position, .. } if *position >= 0.05 && *position < 0.11)));
    h.output.by_ref().take(12_000).for_each(drop);
    assert_eq!(
        h.poll()
            .iter()
            .filter(|event| matches!(event, AudioEvent::Finished(1)))
            .count(),
        1
    );
}

#[test]
fn seek_of_started_successor_uses_its_path_and_does_not_repeat_transition() {
    let mut h = WorkerHarness::new();
    h.fixture_load(1, false);
    h.send(AudioCommand::AppendNext {
        token: 9,
        id: 2,
        path: fixture("flac"),
        gain: GainConfig::default(),
    });
    let samples = open_source(&fixture("wav"), GainConfig::default())
        .unwrap()
        .count();
    // Mixer conversion can prebuffer an initial span; synchronize on the
    // source latch, with a bound, instead of assuming raw-queue sample counts.
    for _ in 0..samples + 2_000 {
        h.output.next();
        if h.intent
            .snapshot()
            .playing
            .is_some_and(|ticket| ticket.token == Some(9))
        {
            break;
        }
    }
    assert_eq!(
        h.intent.snapshot().playing.map(|ticket| ticket.token),
        Some(Some(9)),
        "successor did not start at fixture boundary"
    );
    // Successor has started on the callback, but worker hasn't polled it yet.
    h.send(AudioCommand::Seek(0.1));
    let events = h.poll();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                AudioEvent::Transitioned {
                    from: 1,
                    to: 2,
                    token: 9
                }
            ))
            .count(),
        1
    );
    assert_eq!(h.worker.playback.current.unwrap().id, 2);
    h.output.by_ref().take(8_000).for_each(drop);
    assert_eq!(
        h.poll()
            .iter()
            .filter(|event| matches!(event, AudioEvent::Finished(2)))
            .count(),
        1
    );
}

#[test]
fn latest_seek_wins_and_earlier_epoch_does_not_replace_source() {
    let mut h = WorkerHarness::new();
    h.fixture_load(1, true);
    let original = h.worker.playback.current.unwrap().source;
    let old = h.intent.prepare(AudioCommand::Seek(0.02));
    let latest = h.intent.prepare(AudioCommand::Seek(0.15));
    h.worker.process(old);
    assert_eq!(h.worker.playback.current.unwrap().source, original);
    h.worker.process(latest);
    assert!(h.poll().iter().any(|event| matches!(event, AudioEvent::Progress { position, .. } if (*position - 0.15).abs() < 0.0001)));
}

#[test]
fn failed_seek_retains_active_samples_and_completion_in_new_epoch() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("loaded.wav");
    std::fs::copy(fixture("wav"), &path).unwrap();
    let mut h = WorkerHarness::new();
    h.send(AudioCommand::Load {
        id: 1,
        path: path.clone(),
        volume: 1.0,
        paused: false,
    });
    h.output.by_ref().take(100).for_each(drop);
    let identity = h.worker.playback.current.unwrap().source;
    // Existing open decoder remains usable; only reopening for seek fails.
    std::fs::remove_file(path).unwrap();
    h.send(AudioCommand::Seek(0.1));
    assert_eq!(h.worker.playback.current.unwrap().source, identity);
    assert!(
        h.poll()
            .iter()
            .any(|event| matches!(event, AudioEvent::Error { id: Some(1), .. }))
    );
    h.output.by_ref().take(12_000).for_each(drop);
    assert_eq!(
        h.poll()
            .iter()
            .filter(|event| matches!(event, AudioEvent::Finished(1)))
            .count(),
        1
    );
}

#[test]
fn failed_prefetch_preserves_active_playback_and_allows_an_alternate() {
    let mut h = WorkerHarness::new();
    h.fixture_load(1, false);
    h.send(AudioCommand::AppendNext {
        token: 1,
        id: 2,
        path: PathBuf::from("/missing-audio-file.wav"),
        gain: GainConfig::default(),
    });
    let events = h.poll();
    assert!(events.iter().any(|event| matches!(
        event,
        AudioEvent::PrefetchFailed {
            id: 2,
            token: 1,
            ..
        }
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AudioEvent::Error { .. } | AudioEvent::Finished(_)))
    );
    h.send(AudioCommand::AppendNext {
        token: 2,
        id: 3,
        path: fixture("wav"),
        gain: GainConfig::default(),
    });
    h.output.by_ref().take(24_000).for_each(drop);
    let events = h.poll();
    assert!(events.iter().any(|event| matches!(
        event,
        AudioEvent::Transitioned {
            from: 1,
            to: 3,
            token: 2
        }
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AudioEvent::Finished(3)))
    );
}

#[test]
fn output_failure_reports_once_suppresses_progress_and_allows_reload() {
    let mut h = WorkerHarness::new();
    h.fixture_load(1, false);
    h.send(AudioCommand::AppendNext {
        token: 1,
        id: 2,
        path: fixture("wav"),
        gain: GainConfig::default(),
    });
    for _ in 0..2 {
        h.worker
            .failure_sender
            .send(backend::OutputFailure {
                stream: h.worker.backend.stream_serial,
                message: "injected output loss".into(),
            })
            .unwrap();
    }
    let events = h.poll();
    assert_eq!(events.iter().filter(|event| matches!(event, AudioEvent::Error { id: Some(1), message } if message.contains("injected output loss"))).count(), 1);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AudioEvent::Progress { .. } | AudioEvent::Finished(_)))
    );
    assert!(h.intent.snapshot().prefetch.is_none());
    assert!(h.poll().is_empty());
    // Inject a replacement offline output, as a new native load would reopen it.
    h.worker.backend.mixer = Some(h.mixer.clone());
    h.worker.backend.stream_serial += 1;
    h.fixture_load(3, false);
    h.worker
        .failure_sender
        .send(backend::OutputFailure {
            stream: h.worker.backend.stream_serial - 1,
            message: "late old output error".into(),
        })
        .unwrap();
    h.output.by_ref().take(12_000).for_each(drop);
    let events = h.poll();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AudioEvent::Finished(3)))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AudioEvent::Error { .. }))
    );
}

#[test]
fn rapid_prefetch_replacement_only_outputs_latest_successor() {
    let mut h = Harness::new();
    h.buffer(load(1), vec![0.1; 2]);
    for token in 1..=100 {
        h.buffer(append(2, token), vec![0.9; 3]);
        h.intent.prepare(AudioCommand::CancelPrefetch { token });
    }
    h.buffer(append(3, 101), vec![0.2; 3]);
    assert_eq!(
        h.output.by_ref().take(5).collect::<Vec<_>>(),
        vec![0.1, 0.1, 0.2, 0.2, 0.2]
    );
    h.output.next();
    assert!(matches!(
        h.poll().as_slice(),
        [
            AudioEvent::Transitioned {
                from: 1,
                to: 3,
                token: 101
            },
            AudioEvent::Finished(3)
        ]
    ));
}

#[test]
fn seek_replacement_does_not_mix_old_samples_between_sink_control_checks() {
    let mut reference = WorkerHarness::new();
    reference.fixture_load(1, false);
    reference.send(AudioCommand::Seek(0.1));
    let expected = reference.output.by_ref().take(2_000).collect::<Vec<_>>();

    let mut h = WorkerHarness::new();
    h.fixture_load(1, false);
    h.output.by_ref().take(777).for_each(drop);
    assert!(h.intent.snapshot().playing.is_some());
    h.send(AudioCommand::Seek(0.1));
    let actual = h.output.by_ref().take(2_000).collect::<Vec<_>>();
    // A resampler may retain one frame already generated before the seek.
    assert_eq!(&actual[2..], &expected[2..]);
}

#[test]
#[ignore = "manual source-open timing profile; no performance threshold assertion"]
fn profile_source_opening() {
    for format in ["wav", "flac", "mp3", "m4a"] {
        let path = fixture(format);
        for _ in 0..20 {
            std::hint::black_box(open_source(&path, GainConfig::default()).unwrap());
        }
        let start = Instant::now();
        for _ in 0..1_000 {
            std::hint::black_box(open_source(&path, GainConfig::default()).unwrap());
        }
        eprintln!(
            "ReplayGain Off {format}: {:?} / 1000 opens",
            start.elapsed()
        );
    }
}

#[test]
fn seek_preserves_successor_prepared_after_seek_intent() {
    let mut h = WorkerHarness::new();
    h.fixture_load(1, true);
    let seeking = h.intent.prepare(AudioCommand::Seek(0.2));
    let next = h.intent.prepare(AudioCommand::AppendNext {
        token: 42,
        id: 2,
        path: fixture("wav"),
        gain: GainConfig::default(),
    });
    h.worker.process(seeking);
    h.worker.process(next);
    assert_eq!(h.intent.snapshot().prefetch.unwrap().token, Some(42));
    h.send(AudioCommand::Pause(false));
    h.output.by_ref().take(16_000).for_each(drop);
    let events = h.poll();
    assert!(events.iter().any(|event| matches!(
        event,
        AudioEvent::Transitioned {
            from: 1,
            to: 2,
            token: 42
        }
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AudioEvent::Finished(2)))
    );
}

#[test]
fn duplicate_eof_cannot_reemit_completion() {
    let mut h = Harness::new();
    h.buffer(load(1), vec![0.1]);
    h.output.next();
    h.output.next();
    assert!(matches!(h.poll().as_slice(), [AudioEvent::Finished(1)]));
    let current = h.playback.current.unwrap();
    h.source_tx
        .send(WorkerEvent::SourceEnded {
            source: current.source,
            epoch: current.epoch,
        })
        .unwrap();
    assert!(h.poll().is_empty());
}

fn write_gain(path: &std::path::Path, db: &str) {
    use lofty::{
        config::WriteOptions,
        file::TaggedFileExt,
        probe::Probe,
        tag::{ItemKey, Tag, TagExt},
    };
    let file = Probe::open(path).unwrap().read().unwrap();
    let mut tag = Tag::new(file.file_type().primary_tag_type());
    tag.insert_text(ItemKey::ReplayGainTrackGain, db.into());
    tag.insert_text(ItemKey::ReplayGainTrackPeak, "2.0".into());
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

#[test]
fn replaygain_off_preserves_tagged_decoder_samples_in_all_formats() {
    use crate::audio::gain::GainMode;
    for format in ["wav", "flac", "mp3", "m4a"] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(format!("tagged.{format}"));
        std::fs::copy(fixture(format), &path).unwrap();
        write_gain(&path, "+12.0 dB");
        let expected = rodio::Decoder::try_from(std::fs::File::open(&path).unwrap())
            .unwrap()
            .collect::<Vec<_>>();
        let actual = open_source(
            &path,
            GainConfig {
                mode: GainMode::Off,
                preamp_db: 6.0,
                prevent_clipping: true,
            },
        )
        .unwrap()
        .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{format}: Off changed samples");
    }
}

#[test]
fn seeking_keeps_loaded_gain_when_file_tags_change() {
    use crate::audio::gain::GainMode;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("tagged.wav");
    std::fs::copy(fixture("wav"), &path).unwrap();
    write_gain(&path, "-6.0 dB");
    let mut h = WorkerHarness::new();
    h.send(AudioCommand::LoadWithGain {
        id: 1,
        path: path.clone(),
        volume: 0.5,
        paused: false,
        gain: GainConfig {
            mode: GainMode::Track,
            preamp_db: 0.0,
            prevent_clipping: false,
        },
    });
    h.output.by_ref().take(777).for_each(drop);
    write_gain(&path, "+6.0 dB");
    h.send(AudioCommand::Seek(0.1));
    let actual = h.output.by_ref().take(2_000).collect::<Vec<_>>();

    let mut reference = WorkerHarness::new();
    reference.send(AudioCommand::Load {
        id: 1,
        path,
        volume: 0.5,
        paused: false,
    });
    reference.send(AudioCommand::Seek(0.1));
    let expected = reference.output.by_ref().take(2_000).collect::<Vec<_>>();
    let gain = 10.0_f32.powf(-6.0 / 20.0);
    // Ignore the one frame Rodio's resampler can have generated before seeking.
    assert_eq!(actual.len(), expected.len());
    assert!(expected[2..].iter().any(|sample| *sample != 0.0));
    for (actual, expected) in actual[2..].iter().zip(&expected[2..]) {
        assert!(
            (*actual - expected * gain).abs() < 0.000001,
            "seek changed loaded normalization"
        );
    }
}

#[test]
fn replacement_retires_a_decoder_still_preparing_its_first_sample() {
    let intent = Arc::new(PlaybackIntent::default());
    let queued = intent.prepare(load(1));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (tx, rx) = mpsc::channel();
    let mut source = EndNotifyingSource::new(
        GainSource::new(
            GatedSample {
                entered: entered_tx,
                release: release_rx,
            },
            1.0,
        ),
        tx,
        1,
        SourceTicket {
            source: queued.source.unwrap(),
            token: None,
        },
        queued.epoch,
        Arc::clone(&intent),
    );
    let retired = Arc::clone(&source.retired);
    let consuming = thread::spawn(move || source.next());
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let seeking = intent.prepare(AudioCommand::Seek(0.1));
    let replacement = seeking.source.unwrap();
    assert!(intent.replace_playing(replacement, seeking.epoch, &retired));
    release_tx.send(()).unwrap();
    assert_eq!(consuming.join().unwrap(), None);
    assert!(rx.try_recv().is_err());
    assert_eq!(intent.snapshot().playing.unwrap().source, replacement);
}
