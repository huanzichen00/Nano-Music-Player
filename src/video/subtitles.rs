//! Subtitle association and sidecar discovery primitives.
//!
//! Rendering and UI integration are owned by the video session/controller. Keep
//! explicit user choices here so automatic discovery can never override them.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, fs};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", content = "path", rename_all = "snake_case")]
pub enum SubtitlePreference {
    /// Follow libmpv's embedded track selection and sidecar discovery.
    #[default]
    Automatic,
    /// Explicitly disable subtitles.
    Off,
    /// Explicitly select an external subtitle file.
    External(PathBuf),
    /// Explicitly select an embedded track by its mpv track ID.
    Embedded(i64),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubtitleAssociations(pub BTreeMap<PathBuf, SubtitlePreference>);

impl SubtitleAssociations {
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let temporary = path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(temporary, path).map_err(|e| e.to_string())
    }

    pub fn get(&self, video: &Path) -> SubtitlePreference {
        self.0.get(video).cloned().unwrap_or_default()
    }

    pub fn set(&mut self, video: &Path, preference: SubtitlePreference) {
        self.0.insert(video.to_owned(), preference);
    }
}

/// Find matching ASS/SSA files beside a video, case-insensitively by extension.
/// The candidate must share the complete video stem; unrelated subtitles are
/// never guessed. The video-specific file is returned before language variants.
pub fn discover_sidecars(video: &Path) -> Vec<PathBuf> {
    let Some(parent) = video.parent() else {
        return Vec::new();
    };
    let Some(stem) = video.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut matches: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path.file_stem().and_then(|s| s.to_str()).is_some_and(|s| {
                    s.eq_ignore_ascii_case(stem)
                        || s.to_ascii_lowercase()
                            .starts_with(&format!("{}.", stem.to_ascii_lowercase()))
                })
                && path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ass" | "ssa"))
        })
        .collect();
    matches.sort_by(|a, b| {
        let exact_a = a.file_stem().is_some_and(|s| s.eq_ignore_ascii_case(stem));
        let exact_b = b.file_stem().is_some_and(|s| s.eq_ignore_ascii_case(stem));
        exact_b.cmp(&exact_a).then_with(|| a.cmp(b))
    });
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_exact_and_language_sidecars_only() {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("Episode 01.mkv");
        for name in [
            "Episode 01.ass",
            "episode 01.ja.SSA",
            "Episode 02.ass",
            "Episode 01.srt",
        ] {
            std::fs::write(dir.path().join(name), "fixture").unwrap();
        }
        let found = discover_sidecars(&video);
        assert_eq!(found.len(), 2);
        assert!(found[0].ends_with("Episode 01.ass"));
    }

    #[test]
    fn preference_round_trips_and_defaults_to_automatic() {
        assert_eq!(SubtitlePreference::default(), SubtitlePreference::Automatic);
        for value in [
            SubtitlePreference::Off,
            SubtitlePreference::External("captions.ass".into()),
            SubtitlePreference::Embedded(3),
        ] {
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(
                serde_json::from_str::<SubtitlePreference>(&json).unwrap(),
                value
            );
        }
    }

    #[test]
    fn associations_persist_per_video_path_and_replace_choice() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("video-subtitles.json");
        let video = dir.path().join("movie.mkv");
        let mut choices = SubtitleAssociations::default();
        choices.set(&video, SubtitlePreference::External("movie.ass".into()));
        choices.save(&db).unwrap();
        let mut loaded = SubtitleAssociations::load(&db).unwrap();
        assert_eq!(
            loaded.get(&video),
            SubtitlePreference::External("movie.ass".into())
        );
        loaded.set(&video, SubtitlePreference::Off);
        loaded.save(&db).unwrap();
        assert_eq!(
            SubtitleAssociations::load(&db).unwrap().get(&video),
            SubtitlePreference::Off
        );
    }
}
