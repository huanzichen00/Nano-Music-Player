//! Rodio ownership and output error routing. Decoder seeks never use Sink::try_seek.
use rodio::{OutputStreamBuilder, Sink};
use rodio::{cpal::traits::HostTrait, mixer::Mixer};
use std::sync::mpsc::Sender;

pub(super) struct OutputFailure {
    pub(super) stream: u64,
    pub(super) message: String,
}

pub(super) struct RodioBackend {
    stream: Option<rodio::OutputStream>,
    // Keeping a mixer handle also lets regression tests exercise the actual
    // worker and Rodio conversion without opening a device.
    pub(super) mixer: Option<Mixer>,
    pub(super) sink: Option<Sink>,
    pub(super) stream_serial: u64,
    failures: Sender<OutputFailure>,
}

impl RodioBackend {
    pub(super) fn new(failures: Sender<OutputFailure>) -> Self {
        Self {
            stream: None,
            mixer: None,
            sink: None,
            stream_serial: 0,
            failures,
        }
    }

    fn open(
        &mut self,
        builder: OutputStreamBuilder,
        fallback: bool,
    ) -> Result<rodio::OutputStream, String> {
        self.stream_serial = self.stream_serial.wrapping_add(1);
        let stream = self.stream_serial;
        let failures = self.failures.clone();
        let builder = builder.with_error_callback(move |error| {
            let _ = failures.send(OutputFailure {
                stream,
                message: error.to_string(),
            });
        });
        if fallback {
            builder.open_stream_or_fallback()
        } else {
            builder.open_stream()
        }
        .map_err(|error| error.to_string())
    }

    fn ensure_output(&mut self) -> Result<(), String> {
        if self.mixer.is_some() {
            return Ok(());
        }
        // Match Rodio's default-device then alternate-device fallback policy,
        // with our callback installed before each output attempt.
        let result = OutputStreamBuilder::from_default_device()
            .map_err(|error| error.to_string())
            .and_then(|builder| self.open(builder, false));
        let output = match result {
            Ok(output) => output,
            Err(original) => {
                let devices = rodio::cpal::default_host()
                    .output_devices()
                    .map_err(|_| original.clone())?;
                let mut output = None;
                for device in devices {
                    if let Ok(builder) = OutputStreamBuilder::from_device(device)
                        && let Ok(opened) = self.open(builder, true)
                    {
                        output = Some(opened);
                        break;
                    }
                }
                output.ok_or(original)?
            }
        };
        self.mixer = Some(output.mixer().clone());
        self.stream = Some(output);
        Ok(())
    }

    pub(super) fn new_sink(&mut self, volume: f32, paused: bool) -> Result<Sink, String> {
        self.ensure_output()?;
        let next = Sink::connect_new(self.mixer.as_ref().unwrap());
        next.set_volume(volume);
        if paused {
            next.pause();
        }
        Ok(next)
    }

    pub(super) fn stop_sink(&mut self) {
        if let Some(old) = self.sink.take() {
            old.stop();
        }
    }

    pub(super) fn stop(&mut self) {
        self.stop_sink();
        self.mixer = None;
        self.stream = None;
    }

    pub(super) fn accepts_failure(&self, failure: &OutputFailure) -> bool {
        self.mixer.is_some() && failure.stream == self.stream_serial
    }
}
