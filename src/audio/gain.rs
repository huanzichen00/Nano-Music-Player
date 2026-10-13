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
        // A file may carry ordinary metadata in its primary tag and ReplayGain
        // in a secondary APE/ID3 tag. Prefer the primary value when it is valid.
        let number = |key: ItemKey| {
            file.primary_tag()
                .into_iter()
                .chain(file.tags())
                .find_map(|tag| tag.get_string(&key).and_then(parse_replaygain_number))
        };
        Self {
            track_gain_db: number(ItemKey::ReplayGainTrackGain),
            track_peak: number(ItemKey::ReplayGainTrackPeak),
            album_gain_db: number(ItemKey::ReplayGainAlbumGain),
            album_peak: number(ItemKey::ReplayGainAlbumPeak),
        }
    }

    fn selected_gain(self, mode: GainMode) -> (Option<f32>, Option<f32>) {
        let track_gain = self.track_gain_db.filter(|gain| gain.is_finite());
        let album_gain = self.album_gain_db.filter(|gain| gain.is_finite());
        match mode {
            GainMode::Off => (None, None),
            GainMode::Track => (track_gain, self.track_peak),
            GainMode::Album if album_gain.is_some() => (
                album_gain,
                self.album_peak
                    .filter(|peak| peak.is_finite() && *peak > 0.0)
                    .or(self.track_peak),
            ),
            GainMode::Album => (track_gain, self.track_peak),
        }
    }

    pub fn has_gain(self, mode: GainMode) -> bool {
        self.selected_gain(mode).0.is_some()
    }

    pub fn linear_gain(self, settings: GainConfig) -> f32 {
        let (gain, peak) = self.selected_gain(settings.mode);
        // The preamp belongs to normalization. Untagged or malformed tracks
        // retain their original level instead of receiving a preamp-only boost.
        let Some(gain) = gain else { return 1.0 };
        let gain_db = gain + finite_or_zero(settings.preamp_db);
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
    let trimmed = raw.trim();
    let trimmed = trimmed
        .get(trimmed.len().saturating_sub(2)..)
        .filter(|suffix| suffix.eq_ignore_ascii_case("db"))
        .map_or(trimmed, |_| trimmed[..trimmed.len() - 2].trim());
    trimmed
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
}

/// Rodio `Sink::set_volume` remains the user volume control. This source applies
/// normalization before the sink. The optional final clipping guard is disabled
/// for ReplayGain Off and when the user disables clipping prevention.
#[derive(Debug, Clone)]
pub struct GainSource<S> {
    inner: S,
    gain: f32,
    prevent_clipping: bool,
}

impl<S> GainSource<S> {
    pub fn new(inner: S, gain: f32) -> Self {
        Self::with_clipping_prevention(inner, gain, false)
    }

    pub fn with_clipping_prevention(inner: S, gain: f32, prevent_clipping: bool) -> Self {
        Self {
            inner,
            gain: if gain.is_finite() { gain.max(0.0) } else { 1.0 },
            prevent_clipping,
        }
    }
}

impl<S: Iterator<Item = f32>> Iterator for GainSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|sample| {
            let sample = sample * self.gain;
            if self.prevent_clipping {
                sample.clamp(-1.0, 1.0)
            } else {
                sample
            }
        })
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
    fn clipping_prevention_is_optional_and_unity_processing_is_lossless() {
        let samples = vec![0.75, -0.75, 1.2, -1.2];
        assert_eq!(
            GainSource::new(samples.clone().into_iter(), 1.0).collect::<Vec<_>>(),
            samples
        );
        assert_eq!(
            GainSource::with_clipping_prevention(samples.clone().into_iter(), 2.0, false)
                .collect::<Vec<_>>(),
            vec![1.5, -1.5, 2.4, -2.4]
        );
        assert_eq!(
            GainSource::with_clipping_prevention(samples.into_iter(), 2.0, true)
                .collect::<Vec<_>>(),
            vec![1.0, -1.0, 1.0, -1.0]
        );
    }

    #[test]
    fn missing_metadata_preserves_unity_even_with_preamp_and_peak() {
        let settings = GainConfig {
            mode: GainMode::Track,
            preamp_db: 6.0,
            prevent_clipping: true,
        };
        assert_eq!(ReplayGainInfo::default().linear_gain(settings), 1.0);
        assert_eq!(
            ReplayGainInfo {
                track_peak: Some(2.0),
                ..Default::default()
            }
            .linear_gain(settings),
            1.0
        );
        assert_eq!(
            ReplayGainInfo {
                track_gain_db: Some(f32::NAN),
                ..Default::default()
            }
            .linear_gain(settings),
            1.0
        );
    }

    #[test]
    fn album_mode_falls_back_to_track_gain_and_its_matching_peak() {
        let settings = GainConfig {
            mode: GainMode::Album,
            preamp_db: 6.0,
            prevent_clipping: true,
        };
        let info = ReplayGainInfo {
            track_gain_db: Some(6.0),
            track_peak: Some(0.5),
            album_peak: Some(8.0),
            ..Default::default()
        };
        assert_eq!(info.linear_gain(settings), 2.0);
        let malformed_album = ReplayGainInfo {
            album_gain_db: Some(f32::NAN),
            ..info
        };
        assert_eq!(malformed_album.linear_gain(settings), 2.0);
        assert!(
            info.linear_gain(GainConfig {
                prevent_clipping: false,
                ..settings
            }) > 3.9
        );
        assert_eq!(
            info.linear_gain(GainConfig {
                mode: GainMode::Off,
                ..settings
            }),
            1.0
        );
    }

    #[test]
    fn gain_number_accepts_case_insensitive_units_and_rejects_invalid_values() {
        for raw in [" +3.5 dB ", "+3.5 db", "+3.5 DB", "+3.5"] {
            assert_eq!(parse_replaygain_number(raw), Some(3.5), "{raw}");
        }
        for raw in ["NaN", "inf", "1.0 dB extra", "dB", "音量", ""] {
            assert_eq!(parse_replaygain_number(raw), None, "{raw}");
        }
    }

    #[test]
    fn reads_actual_replaygain_tags_in_supported_formats() {
        use lofty::{
            config::WriteOptions,
            file::TaggedFileExt,
            probe::Probe,
            tag::{ItemKey, Tag, TagExt},
        };

        for format in ["wav", "flac", "mp3", "m4a"] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join(format!("tagged.{format}"));
            std::fs::copy(
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures")
                    .join(format!("quiet.{format}")),
                &path,
            )
            .unwrap();
            let file = Probe::open(&path).unwrap().read().unwrap();
            let mut tag = Tag::new(file.file_type().primary_tag_type());
            for (key, value) in [
                (ItemKey::ReplayGainTrackGain, "-6.0 dB"),
                (ItemKey::ReplayGainTrackPeak, "0.25"),
                (ItemKey::ReplayGainAlbumGain, "+3.0 dB"),
                (ItemKey::ReplayGainAlbumPeak, "0.75"),
            ] {
                tag.insert_text(key, value.to_owned());
            }
            tag.save_to_path(&path, WriteOptions::default()).unwrap();
            assert_eq!(
                ReplayGainInfo::read(&path),
                ReplayGainInfo {
                    track_gain_db: Some(-6.0),
                    track_peak: Some(0.25),
                    album_gain_db: Some(3.0),
                    album_peak: Some(0.75),
                },
                "{format}"
            );
        }
    }

    #[test]
    fn finds_replaygain_in_secondary_tag() {
        use lofty::{
            config::WriteOptions,
            tag::{ItemKey, Tag, TagExt, TagType},
        };
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tagged.mp3");
        std::fs::copy(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quiet.mp3"),
            &path,
        )
        .unwrap();
        let mut primary = Tag::new(TagType::Id3v2);
        primary.insert_text(ItemKey::TrackTitle, "ordinary title".to_owned());
        primary
            .save_to_path(&path, WriteOptions::default())
            .unwrap();
        let mut secondary = Tag::new(TagType::Ape);
        secondary.insert_text(ItemKey::ReplayGainTrackGain, "-4 dB".to_owned());
        secondary
            .save_to_path(&path, WriteOptions::default())
            .unwrap();
        assert_eq!(ReplayGainInfo::read(&path).track_gain_db, Some(-4.0));
    }
}
