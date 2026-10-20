use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub language: super::i18n::Language,
    pub font: super::typography::Font,
    pub always_on_top: bool,
    #[serde(default = "smooth_slides_default")]
    pub smooth_slides: bool,
    pub romaji_visible: bool,
    #[serde(default = "lyrics_default")]
    pub lyrics_visible: bool,
    #[serde(default)]
    pub translation_target: crate::translation::TargetLanguage,
    pub translation_visible: bool,
    pub zoom: f32,
    pub device_mode: bool,
    #[serde(default, skip_serializing, rename = "monitor_deleted_files")]
    pub legacy_monitor_setting: Option<serde::de::IgnoredAny>,
    #[serde(default, alias = "monitor_roots")]
    pub library_roots: Vec<PathBuf>,
    #[serde(default = "gapless_default")]
    pub gapless_playback: bool,
    #[serde(default)]
    pub replay_gain_mode: crate::audio::gain::GainMode,
    #[serde(default)]
    pub replay_gain_preamp_db: f32,
    #[serde(default = "clipping_default")]
    pub replay_gain_prevent_clipping: bool,
    pub window_width: Option<f32>,
    pub window_height: Option<f32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: super::i18n::Language::English,
            font: super::typography::Font::Classic,
            always_on_top: true,
            smooth_slides: true,
            romaji_visible: true,
            lyrics_visible: true,
            translation_target: crate::translation::TargetLanguage::English,
            translation_visible: false,
            zoom: 1.25,
            device_mode: true,
            legacy_monitor_setting: None,
            library_roots: Vec::new(),
            gapless_playback: true,
            replay_gain_mode: crate::audio::gain::GainMode::Off,
            replay_gain_preamp_db: 0.0,
            replay_gain_prevent_clipping: true,
            window_width: None,
            window_height: None,
        }
    }
}

fn smooth_slides_default() -> bool {
    true
}

fn lyrics_default() -> bool {
    true
}

fn gapless_default() -> bool {
    true
}

fn clipping_default() -> bool {
    true
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        let settings: Self = if path.exists() {
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Invalid settings.json: {e}"))?
        } else {
            Self::default()
        };
        if !settings.zoom.is_finite() || !(0.75..=3.0).contains(&settings.zoom) {
            return Err("Zoom must be between 0.75 and 3.0".to_owned());
        }
        if !settings.replay_gain_preamp_db.is_finite()
            || !(-12.0..=12.0).contains(&settings.replay_gain_preamp_db)
        {
            return Err("ReplayGain preamp must be between -12 and 12 dB".to_owned());
        }
        if settings
            .window_width
            .is_some_and(|v| !v.is_finite() || !(180.0..=3000.0).contains(&v))
            || settings
                .window_height
                .is_some_and(|v| !v.is_finite() || !(324.0..=3000.0).contains(&v))
        {
            return Err("Window dimensions are outside the supported range".to_owned());
        }
        Ok(settings)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let temporary = path.with_extension("json.tmp");
        std::fs::write(
            &temporary,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(temporary, path).map_err(|e| e.to_string())
    }
}

#[cfg(not(target_os = "ios"))]
pub fn config_dir() -> Result<PathBuf, String> {
    let mut args = std::env::args().skip(1);
    let mut directory = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--iphone-sync" => {}
            "--config-dir" if directory.is_none() => {
                directory = Some(
                    args.next()
                        .map(PathBuf::from)
                        .ok_or_else(|| "--config-dir needs a directory".to_owned())?,
                );
            }
            _ => {
                return Err(
                    "Usage: nano-music-player [--config-dir DIRECTORY] [--iphone-sync]".to_owned(),
                );
            }
        }
    }
    directory.map(Ok).unwrap_or_else(|| {
        directories::ProjectDirs::from("", "Nano", "Nano Music Player")
            .map(|dirs| dirs.config_dir().to_path_buf())
            .ok_or_else(|| "Cannot locate application configuration directory".to_owned())
    })
}

#[cfg(target_os = "ios")]
pub fn config_dir() -> Result<PathBuf, String> {
    crate::platform::ios::documents_dir().map(|directory| directory.join(".nano"))
}
