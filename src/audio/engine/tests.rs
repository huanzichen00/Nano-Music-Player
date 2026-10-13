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

    fn queue<S: Source<Item = f32> + Send + 'static>(&mut self, queued: QueuedCommand, source: S) {
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
