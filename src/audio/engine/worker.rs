//! Command processing and ownership of the Rodio output stream and sink.
use super::*;

pub(super) fn run(
    receiver: Receiver<QueuedCommand>,
    events: Sender<EventEnvelope>,
    worker_intent: Arc<PlaybackIntent>,
) {
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
                                        event: AudioEvent::PrefetchFailed { id, token, message },
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
