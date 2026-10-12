use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub language: super::i18n::Language,
    pub font: super::typography::Font,
    pub always_on_top: bool,
    pub romaji_visible: bool,
    pub zoom: f32,
    pub device_mode: bool,
    pub window_width: Option<f32>,
    pub window_height: Option<f32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: super::i18n::Language::English,
            font: super::typography::Font::Classic,
            always_on_top: true,
            romaji_visible: true,
            zoom: 1.25,
            device_mode: true,
            window_width: None,
            window_height: None,
        }
    }
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

pub fn config_dir() -> Result<PathBuf, String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--config-dir") => args
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| "--config-dir needs a directory".to_owned()),
        Some(_) => Err("Usage: nano-music-player [--config-dir DIRECTORY]".to_owned()),
        None => directories::ProjectDirs::from("", "Nano", "Nano Music Player")
            .map(|dirs| dirs.config_dir().to_path_buf())
            .ok_or_else(|| "Cannot locate application configuration directory".to_owned()),
    }
}
