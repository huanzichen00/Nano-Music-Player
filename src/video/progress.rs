//! Per-path watch positions, kept separate from imported metadata.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path, path::PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WatchProgress {
    positions: BTreeMap<PathBuf, f64>,
    #[serde(skip)]
    dirty: bool,
}

impl WatchProgress {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.to_string()),
        };
        let progress: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if progress
            .positions
            .values()
            .any(|p| !p.is_finite() || *p < 0.0)
        {
            return Err("Invalid video watch position".into());
        }
        Ok(progress)
    }

    pub fn resume(&self, video: &Path, duration: f64) -> f64 {
        self.positions
            .get(video)
            .copied()
            .filter(|p| *p < duration)
            .unwrap_or(0.0)
    }

    pub fn record(&mut self, video: &Path, position: f64, duration: f64) {
        if !position.is_finite() || !duration.is_finite() || duration <= 0.0 {
            return;
        }
        let position = position.clamp(0.0, duration);
        if self.positions.get(video).copied().unwrap_or(0.0) != position {
            self.positions.insert(video.to_owned(), position);
            self.dirty = true;
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let temporary = path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(temporary, path).map_err(|e| e.to_string())?;
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_survive_restart_and_completed_videos_replay_from_zero() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("video-progress.json");
        let first = Path::new("first.mp4");
        let second = Path::new("second.mp4");
        let mut progress = WatchProgress::load(&database).unwrap();
        progress.record(first, 42.5, 100.0);
        progress.record(second, 20.0, 20.0);
        progress.save(&database).unwrap();
        let mut progress = WatchProgress::load(&database).unwrap();
        assert!(!progress.is_dirty());
        assert_eq!(progress.resume(first, 100.0), 42.5);
        assert_eq!(progress.resume(second, 20.0), 0.0);
        assert_eq!(progress.resume(first, 30.0), 0.0);
        progress.record(first, 0.0, 100.0);
        progress.save(&database).unwrap();
        assert_eq!(
            WatchProgress::load(&database).unwrap().resume(first, 100.0),
            0.0
        );
    }

    #[test]
    fn invalid_positions_and_failed_saves_preserve_watch_history() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("video-progress.json");
        let video = Path::new("movie.mp4");
        let mut progress = WatchProgress::default();
        progress.record(video, 12.0, 100.0);
        progress.record(video, f64::NAN, 100.0);
        progress.record(video, 0.0, f64::INFINITY);
        assert_eq!(progress.resume(video, 100.0), 12.0);
        assert!(progress.save(directory.path()).is_err());
        assert!(progress.is_dirty());
        progress.save(&database).unwrap();
        fs::write(&database, br#"{"positions":{"movie.mp4":-1}}"#).unwrap();
        assert!(WatchProgress::load(&database).is_err());
        assert_eq!(
            fs::read(&database).unwrap(),
            br#"{"positions":{"movie.mp4":-1}}"#
        );
    }
}
