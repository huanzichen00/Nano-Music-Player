//! Desktop practice markers and cancellable, audio-ready countdowns.
//!
//! Runtime deadlines use a monotonic clock. Persisted data contains only user
//! settings and path-associated markers, never a playback index or deadline.
use crate::library::paths::normalize;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub id: u64,
    pub name: String,
    pub position: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PracticeStore {
    version: u32,
    delay_seconds: u32,
    lead_in_seconds: u32,
    next_id: u64,
    tracks: BTreeMap<PathBuf, Vec<Marker>>,
    #[serde(skip)]
    dirty: bool,
}

impl Default for PracticeStore {
    fn default() -> Self {
        Self {
            version: 1,
            delay_seconds: 3,
            lead_in_seconds: 0,
            next_id: 1,
            tracks: BTreeMap::new(),
            dirty: false,
        }
    }
}

impl PracticeStore {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.to_string()),
        };
        let mut store: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if store.version != 1 || store.delay_seconds > 30 || store.lead_in_seconds > 10 {
            return Err("Invalid practice settings".into());
        }
        let mut ids = BTreeSet::new();
        let mut tracks: BTreeMap<PathBuf, Vec<Marker>> = BTreeMap::new();
        for (path, markers) in std::mem::take(&mut store.tracks) {
            if path.as_os_str().is_empty() {
                return Err("Invalid practice track path".into());
            }
            for marker in &markers {
                validate_position(marker.position)?;
                validate_name(&marker.name)?;
                if marker.id == 0 || !ids.insert(marker.id) {
                    return Err("Invalid or duplicate practice marker ID".into());
                }
            }
            tracks.entry(normalize(&path)).or_default().extend(markers);
        }
        let highest = ids.last().copied().unwrap_or(0);
        store.next_id = store.next_id.max(
            highest
                .checked_add(1)
                .ok_or("Practice marker IDs exhausted")?,
        );
        if store.next_id == 0 {
            return Err("Invalid practice marker ID sequence".into());
        }
        for markers in tracks.values_mut() {
            sort_markers(markers);
        }
        store.tracks = tracks;
        Ok(store)
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
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::rename(&temporary, path).map_err(|e| e.to_string())?;
        self.dirty = false;
        Ok(())
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn delay_seconds(&self) -> u32 {
        self.delay_seconds
    }

    pub fn lead_in_seconds(&self) -> u32 {
        self.lead_in_seconds
    }

    pub fn set_delay_seconds(&mut self, seconds: u32) -> Result<(), String> {
        if seconds > 30 {
            return Err("Preparation delay must be between 0 and 30 seconds".into());
        }
        self.dirty |= self.delay_seconds != seconds;
        self.delay_seconds = seconds;
        Ok(())
    }

    pub fn set_lead_in_seconds(&mut self, seconds: u32) -> Result<(), String> {
        if seconds > 10 {
            return Err("Lead-in must be between 0 and 10 seconds".into());
        }
        self.dirty |= self.lead_in_seconds != seconds;
        self.lead_in_seconds = seconds;
        Ok(())
    }

    pub fn markers(&self, track: &Path) -> &[Marker] {
        self.tracks
            .get(&normalize(track))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn add_marker(
        &mut self,
        track: &Path,
        name: &str,
        position: f64,
        duration: f64,
    ) -> Result<u64, String> {
        validate_track_marker(track, name, position, duration)?;
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or("Practice marker IDs exhausted")?;
        let markers = self.tracks.entry(normalize(track)).or_default();
        markers.push(Marker {
            id,
            name: name.trim().to_owned(),
            position,
        });
        sort_markers(markers);
        self.dirty = true;
        Ok(id)
    }

    pub fn update_marker(
        &mut self,
        track: &Path,
        id: u64,
        name: &str,
        position: f64,
        duration: f64,
    ) -> Result<(), String> {
        validate_track_marker(track, name, position, duration)?;
        let markers = self
            .tracks
            .get_mut(&normalize(track))
            .ok_or("Practice marker no longer exists")?;
        let marker = markers
            .iter_mut()
            .find(|marker| marker.id == id)
            .ok_or("Practice marker no longer exists")?;
        let name = name.trim();
        if marker.name != name || marker.position != position {
            marker.name = name.to_owned();
            marker.position = position;
            sort_markers(markers);
            self.dirty = true;
        }
        Ok(())
    }

    pub fn remove_marker(&mut self, track: &Path, id: u64) -> bool {
        let path = normalize(track);
        let Some(markers) = self.tracks.get_mut(&path) else {
            return false;
        };
        let before = markers.len();
        markers.retain(|marker| marker.id != id);
        let removed = before != markers.len();
        if markers.is_empty() {
            self.tracks.remove(&path);
        }
        self.dirty |= removed;
        removed
    }
}

fn sort_markers(markers: &mut [Marker]) {
    markers.sort_by(|a, b| a.position.total_cmp(&b.position).then(a.id.cmp(&b.id)));
}

fn validate_position(position: f64) -> Result<(), String> {
    if !position.is_finite() || position < 0.0 || Duration::try_from_secs_f64(position).is_err() {
        Err("Practice time must be finite and non-negative".into())
    } else {
        Ok(())
    }
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
        Err("Marker name must contain 1 to 120 printable characters".into())
    } else {
        Ok(())
    }
}

fn validate_track_marker(
    track: &Path,
    name: &str,
    position: f64,
    duration: f64,
) -> Result<(), String> {
    if track.as_os_str().is_empty() {
        return Err("Invalid practice track path".into());
    }
    validate_name(name)?;
    validate_position(position)?;
    if !duration.is_finite() || duration < 0.0 || (duration > 0.0 && position >= duration) {
        return Err("Marker must be before the end of the song".into());
    }
    Ok(())
}

/// Accept seconds, MM:SS[.mmm], or HH:MM:SS[.mmm].
pub fn parse_time(text: &str) -> Result<f64, String> {
    let invalid = || "Enter seconds, MM:SS.mmm, or HH:MM:SS.mmm".to_owned();
    let parts: Vec<_> = text.trim().split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return Err(invalid());
    }
    let seconds = parts.last().ok_or_else(invalid)?;
    let decimal: Vec<_> = seconds.split('.').collect();
    if decimal.len() > 2
        || decimal[0].is_empty()
        || !decimal[0].bytes().all(|byte| byte.is_ascii_digit())
        || decimal.get(1).is_some_and(|fraction| {
            fraction.is_empty()
                || fraction.len() > 3
                || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(invalid());
    }
    let seconds: f64 = seconds.parse().map_err(|_| invalid())?;
    if parts.len() > 1 && seconds >= 60.0 {
        return Err(invalid());
    }
    let mut total = seconds;
    for (index, part) in parts[..parts.len() - 1].iter().rev().enumerate() {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid());
        }
        let value: f64 = part.parse().map_err(|_| invalid())?;
        if parts.len() == 3 && index == 0 && value >= 60.0 {
            return Err(invalid());
        }
        total += value * if index == 0 { 60.0 } else { 3600.0 };
    }
    validate_position(total)?;
    Ok(total)
}

pub fn format_time(seconds: f64) -> String {
    let milliseconds = if seconds.is_finite() && seconds >= 0.0 {
        (seconds * 1000.0).round() as u64
    } else {
        0
    };
    let total_seconds = milliseconds / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    let fraction = milliseconds % 1000;
    if minutes >= 60 {
        format!(
            "{}:{:02}:{seconds:02}.{fraction:03}",
            minutes / 60,
            minutes % 60
        )
    } else {
        format!("{minutes:02}:{seconds:02}.{fraction:03}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PracticeRequest {
    pub token: u64,
    pub path: PathBuf,
    pub position: f64,
}

#[derive(Debug)]
enum Phase {
    Waiting,
    Countdown(Instant),
}

#[derive(Debug)]
struct Pending {
    request: PracticeRequest,
    delay: Duration,
    phase: Phase,
}

#[derive(Debug, Default)]
pub struct PracticeSession {
    enabled: bool,
    path: Option<PathBuf>,
    selected_marker: Option<u64>,
    anchor_position: f64,
    next_token: u64,
    pending: Option<Pending>,
}

impl PracticeSession {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.cancel();
        }
    }

    /// Select the unadjusted practice anchor; lead-in belongs only to begin().
    pub fn select(
        &mut self,
        path: &Path,
        marker: Option<u64>,
        position: f64,
    ) -> Result<(), String> {
        validate_position(position)?;
        self.cancel();
        self.path = Some(normalize(path));
        self.selected_marker = marker;
        self.anchor_position = position;
        Ok(())
    }

    pub fn selected_marker(&self) -> Option<u64> {
        self.selected_marker
    }

    pub fn anchor_position(&self, path: &Path) -> f64 {
        if self.path.as_deref() == Some(normalize(path).as_path()) {
            self.anchor_position
        } else {
            0.0
        }
    }

    pub fn begin(&mut self, path: &Path, position: f64, delay_seconds: u32) -> Result<u64, String> {
        if !self.enabled {
            return Err("Practice mode is disabled".into());
        }
        validate_position(position)?;
        if path.as_os_str().is_empty() || delay_seconds > 30 {
            return Err("Invalid practice request".into());
        }
        let path = normalize(path);
        if self.path.as_ref() != Some(&path) {
            self.selected_marker = None;
            self.anchor_position = position;
            self.path = Some(path.clone());
        }
        self.next_token = self.next_token.wrapping_add(1);
        let token = self.next_token;
        self.pending = Some(Pending {
            request: PracticeRequest {
                token,
                path,
                position,
            },
            delay: Duration::from_secs(u64::from(delay_seconds)),
            phase: Phase::Waiting,
        });
        Ok(token)
    }

    pub fn pending(&self) -> Option<&PracticeRequest> {
        self.pending.as_ref().map(|pending| &pending.request)
    }

    pub fn is_waiting(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| matches!(pending.phase, Phase::Waiting))
    }

    /// Call only after the audio worker acknowledges the paused target position.
    pub fn audio_ready(&mut self, token: u64, path: &Path, now: Instant) -> bool {
        let Some(pending) = self.pending.as_mut() else {
            return false;
        };
        if !self.enabled
            || pending.request.token != token
            || pending.request.path != normalize(path)
            || !matches!(pending.phase, Phase::Waiting)
        {
            return false;
        }
        pending.phase = Phase::Countdown(now + pending.delay);
        true
    }

    pub fn remaining(&self, now: Instant) -> Option<u32> {
        match self.pending.as_ref()?.phase {
            Phase::Waiting => None,
            Phase::Countdown(deadline) => {
                let duration = deadline.saturating_duration_since(now);
                Some(
                    duration
                        .as_secs()
                        .saturating_add(u64::from(duration.subsec_nanos() > 0))
                        as u32,
                )
            }
        }
    }

    /// A changed current song invalidates pending playback even before expiry.
    pub fn tick(&mut self, path: &Path, now: Instant) -> Option<PracticeRequest> {
        let pending = self.pending.as_ref()?;
        if !self.enabled || pending.request.path != normalize(path) {
            self.cancel();
            return None;
        }
        if matches!(pending.phase, Phase::Countdown(deadline) if now >= deadline) {
            return self.pending.take().map(|pending| pending.request);
        }
        None
    }

    pub fn cancel(&mut self) -> bool {
        self.pending.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_are_sorted_path_scoped_and_survive_restart_with_stable_ids() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("practice.json");
        let song = directory.path().join("songs/../song.wav");
        let normalized = directory.path().join("song.wav");
        let other = directory.path().join("other.wav");
        let mut store = PracticeStore::load(&database).unwrap();
        let chorus = store.add_marker(&song, " 副歌 ", 62.5, 180.0).unwrap();
        let verse = store.add_marker(&normalized, "主歌", 18.0, 180.0).unwrap();
        let other_id = store.add_marker(&other, "开头", 0.0, 180.0).unwrap();
        assert_eq!(
            store
                .markers(&song)
                .iter()
                .map(|m| m.id)
                .collect::<Vec<_>>(),
            [verse, chorus]
        );
        store.set_delay_seconds(7).unwrap();
        store.set_lead_in_seconds(2).unwrap();
        store.save(&database).unwrap();
        assert!(!store.is_dirty());
        let mut loaded = PracticeStore::load(&database).unwrap();
        assert_eq!(loaded.delay_seconds(), 7);
        assert_eq!(loaded.lead_in_seconds(), 2);
        assert_eq!(loaded.markers(&normalized)[1].name, "副歌");
        loaded
            .update_marker(&song, chorus, "间奏", 10.25, 180.0)
            .unwrap();
        assert_eq!(loaded.markers(&song)[0].id, chorus);
        assert!(loaded.remove_marker(&song, verse));
        assert!(!loaded.remove_marker(&other, chorus));
        assert_eq!(loaded.markers(&other)[0].id, other_id);
        assert!(loaded.add_marker(&song, "结尾", 170.0, 180.0).unwrap() > other_id);
    }

    #[test]
    fn invalid_edits_do_not_change_data_and_failed_saves_remain_dirty() {
        let directory = tempfile::tempdir().unwrap();
        let song = Path::new("song.wav");
        let mut store = PracticeStore::default();
        let id = store.add_marker(song, "A", 12.0, 30.0).unwrap();
        for position in [-1.0, f64::NAN, f64::INFINITY, 30.0, 31.0] {
            assert!(
                store
                    .update_marker(song, id, "Changed", position, 30.0)
                    .is_err()
            );
        }
        assert!(store.add_marker(song, " ", 0.0, 30.0).is_err());
        assert!(
            store
                .add_marker(song, "Unknown duration", 50.0, 0.0)
                .is_ok()
        );
        assert!(store.set_delay_seconds(31).is_err());
        assert!(store.set_lead_in_seconds(11).is_err());
        assert_eq!(store.markers(song)[0].position, 12.0);
        assert_eq!(store.markers(song)[0].name, "A");
        assert_eq!(store.delay_seconds(), 3);
        assert!(store.save(directory.path()).is_err());
        assert!(store.is_dirty());
        let database = directory.path().join("practice.json");
        store.save(&database).unwrap();
        assert!(!store.is_dirty());
        fs::write(&database, br#"{"delay_seconds":31}"#).unwrap();
        assert!(PracticeStore::load(&database).is_err());
        assert_eq!(fs::read(&database).unwrap(), br#"{"delay_seconds":31}"#);
    }

    #[test]
    fn corrupt_duplicate_marker_ids_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("practice.json");
        fs::write(&database, br#"{"tracks":{"a.wav":[{"id":1,"name":"A","position":0}],"b.wav":[{"id":1,"name":"B","position":1}]}}"#).unwrap();
        assert!(PracticeStore::load(&database).is_err());
    }

    #[test]
    fn id_exhaustion_and_unrepresentable_times_fail_without_mutation() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("practice.json");
        fs::write(&database, br#"{"next_id":18446744073709551615}"#).unwrap();
        let mut store = PracticeStore::load(&database).unwrap();
        let song = Path::new("song.wav");
        assert!(store.add_marker(song, "A", 1.0, 3.0).is_err());
        assert!(store.markers(song).is_empty());
        assert!(!store.is_dirty());
        assert!(parse_time("999999999999999999999999999999999999999").is_err());
        assert!(store.add_marker(song, "A", f64::MAX, 0.0).is_err());
        fs::write(
            &database,
            br#"{"tracks":{"a.wav":[{"id":18446744073709551615,"name":"A","position":0}]}}"#,
        )
        .unwrap();
        assert!(PracticeStore::load(&database).is_err());
    }

    #[test]
    fn timestamp_input_and_millisecond_rounding_cover_long_songs() {
        for (text, expected) in [
            ("18.5", 18.5),
            ("01:02.003", 62.003),
            ("1:02:03.4", 3723.4),
            ("65:00", 3900.0),
        ] {
            assert_eq!(parse_time(text).unwrap(), expected);
        }
        assert_eq!(format_time(62.003), "01:02.003");
        assert_eq!(format_time(59.9996), "01:00.000");
        assert_eq!(format_time(3723.4), "1:02:03.400");
        for text in [
            "", "NaN", "inf", "-1", "+1", "1e3", "1:60", "1:60:00", "1::2", ":1", "1.", "1.0001",
            "1:2:3:4",
        ] {
            assert!(parse_time(text).is_err(), "{text}");
        }
    }

    #[test]
    fn countdown_waits_for_audio_then_uses_monotonic_time_and_fires_once() {
        let mut session = PracticeSession::default();
        let song = Path::new("song.wav");
        let now = Instant::now();
        session.set_enabled(true);
        session.select(song, Some(7), 62.0).unwrap();
        let token = session.begin(song, 60.0, 3).unwrap();
        assert!(session.is_waiting());
        assert!(session.tick(song, now + Duration::from_secs(30)).is_none());
        assert_eq!(session.remaining(now), None);
        assert!(session.audio_ready(token, song, now));
        assert!(!session.audio_ready(token, song, now + Duration::from_secs(1)));
        assert_eq!(session.remaining(now + Duration::from_millis(1)), Some(3));
        assert_eq!(
            session.remaining(now + Duration::from_millis(2200)),
            Some(1)
        );
        assert!(
            session
                .tick(song, now + Duration::from_millis(2999))
                .is_none()
        );
        let request = session.tick(song, now + Duration::from_secs(3)).unwrap();
        assert_eq!(request.token, token);
        assert_eq!(request.position, 60.0);
        assert_eq!(session.anchor_position(song), 62.0);
        assert_eq!(session.selected_marker(), Some(7));
        assert!(session.tick(song, now + Duration::from_secs(4)).is_none());
    }

    #[test]
    fn newer_requests_song_changes_disable_and_cancel_invalidate_old_starts() {
        let mut session = PracticeSession::default();
        let song = Path::new("song.wav");
        let other = Path::new("other.wav");
        let now = Instant::now();
        assert!(session.begin(song, 0.0, 0).is_err());
        session.set_enabled(true);
        let old = session.begin(song, 0.0, 3).unwrap();
        let latest = session.begin(song, 12.0, 0).unwrap();
        assert!(!session.audio_ready(old, song, now));
        assert!(!session.audio_ready(latest, other, now));
        assert!(session.audio_ready(latest, song, now));
        assert!(session.tick(other, now).is_none());
        assert!(session.pending().is_none());
        let token = session.begin(song, 0.0, 3).unwrap();
        assert!(session.audio_ready(token, song, now));
        session.set_enabled(false);
        assert!(session.tick(song, now + Duration::from_secs(10)).is_none());
        session.set_enabled(true);
        let token = session.begin(song, 1.0, 0).unwrap();
        session.audio_ready(token, song, now);
        assert!(session.cancel());
        assert!(!session.cancel());
        assert!(session.tick(song, now).is_none());
    }
}
