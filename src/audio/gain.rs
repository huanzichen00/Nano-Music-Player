//! ReplayGain parsing and per-sample gain processing for the Rodio pipeline.

use rodio::Source;
use std::{path::Path, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GainMode {
    #[default]
    Off,
    Track,
    Album,
}

/// Settings supplied by the application when a track is loaded or appended.
/// They are deliberately independent from persisted settings so the audio worker
/// remains a small playback boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GainConfig {
    pub mode: GainMode,
    pub preamp_db: f32,
    pub prevent_clipping: bool,
}

impl Default for GainConfig {
    fn default() -> Self {
        Self {
            mode: GainMode::Off,
            preamp_db: 0.0,
            prevent_clipping: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ReplayGainInfo {
    pub track_gain_db: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain_db: Option<f32>,
    pub album_peak: Option<f32>,
}

impl ReplayGainInfo {
    /// Read standard ReplayGain tags using the already-present Lofty dependency.
    /// Missing, malformed, or unreadable metadata always falls back to unity gain.
    pub fn read(path: &Path) -> Self {
        use lofty::{file::TaggedFileExt, probe::Probe, tag::ItemKey};

        let Ok(file) = Probe::open(path).and_then(|probe| probe.read()) else {
            return Self::default();
        };
        let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else {
            return Self::default();
        };
        let number = |key: ItemKey| tag.get_string(&key).and_then(parse_replaygain_number);
        Self {
            track_gain_db: number(ItemKey::ReplayGainTrackGain),
            track_peak: number(ItemKey::ReplayGainTrackPeak),
            album_gain_db: number(ItemKey::ReplayGainAlbumGain),
            album_peak: number(ItemKey::ReplayGainAlbumPeak),
        }
    }

    pub fn linear_gain(self, settings: GainConfig) -> f32 {
        if settings.mode == GainMode::Off {
            return 1.0;
        }
        let (gain, peak) = match settings.mode {
            GainMode::Off => (0.0, None),
            GainMode::Track => (self.track_gain_db.unwrap_or(0.0), self.track_peak),
            GainMode::Album => (self.album_gain_db.unwrap_or(0.0), self.album_peak),
        };
        let gain_db = finite_or_zero(gain) + finite_or_zero(settings.preamp_db);
        let mut linear = 10.0_f32.powf((gain_db / 20.0).clamp(-60.0, 24.0));
        if settings.prevent_clipping
            && let Some(peak) = peak.filter(|p| p.is_finite() && *p > 0.0)
        {
            linear = linear.min(1.0 / peak);
        }
        linear.clamp(0.0, 16.0)
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn parse_replaygain_number(raw: &str) -> Option<f32> {
    let trimmed = raw.trim().trim_end_matches("dB").trim();
    trimmed
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
}

/// Rodio `Sink::set_volume` remains the user volume control. This source applies
/// normalization before the sink and caps the output as a final clipping guard.
#[derive(Debug, Clone)]
pub struct GainSource<S> {
    inner: S,
    gain: f32,
}

impl<S> GainSource<S> {
    pub fn new(inner: S, gain: f32) -> Self {
        Self {
            inner,
            gain: if gain.is_finite() { gain.max(0.0) } else { 1.0 },
        }
    }
}

impl<S: Iterator<Item = f32>> Iterator for GainSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner
            .next()
            .map(|sample| (sample * self.gain).clamp(-1.0, 1.0))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source<Item = f32>> Source for GainSource<S> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaygain_uses_safe_defaults_and_selected_mode() {
        let info = ReplayGainInfo {
            track_gain_db: Some(-6.0),
            track_peak: Some(0.5),
            album_gain_db: Some(3.0),
            album_peak: Some(2.0),
        };
        let track = GainConfig {
            mode: GainMode::Track,
            preamp_db: 0.0,
            prevent_clipping: true,
        };
        assert!((info.linear_gain(track) - 10.0_f32.powf(-6.0 / 20.0)).abs() < 0.001);
        let album = GainConfig {
            mode: GainMode::Album,
            ..track
        };
        assert!((info.linear_gain(album) - 0.5).abs() < 0.001);
        assert_eq!(ReplayGainInfo::default().linear_gain(track), 1.0);
        assert_eq!(
            ReplayGainInfo::default().linear_gain(GainConfig::default()),
            1.0
        );
    }

    #[test]
    fn malformed_gain_and_preamp_are_safe() {
        assert_eq!(parse_replaygain_number("not-a-number"), None);
        assert_eq!(parse_replaygain_number("+5.25 dB"), Some(5.25));
        let settings = GainConfig {
            mode: GainMode::Track,
            preamp_db: f32::NAN,
            prevent_clipping: false,
        };
        assert_eq!(
            ReplayGainInfo {
                track_gain_db: Some(f32::INFINITY),
                ..Default::default()
            }
            .linear_gain(settings),
            1.0
        );
    }

    #[test]
    fn gain_source_limits_samples() {
        let source = GainSource::new(rodio::source::SineWave::new(440.0), 2.0);
        // Check processing without relying on device output.
        assert!(
            source
                .take(100)
                .all(|sample| (-1.0..=1.0).contains(&sample))
        );
    }
}
