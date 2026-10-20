use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const EXTENSIONS: &[&str] = &["mp4", "m4v", "mov", "mkv", "webm", "avi"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Video {
    pub path: PathBuf,
    pub title: String,
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub display_aspect: f64,
    pub video_codec: String,
    pub audio_codec: Option<String>,
}

impl Video {
    /// Display aspect already accounts for anamorphic pixels and rotation metadata.
    pub fn is_landscape(&self) -> bool {
        self.display_aspect.is_finite() && self.display_aspect > 1.0
    }
    /// Bound software scaling and upload cost, preserving display aspect (SAR/rotation).
    pub fn render_size(&self) -> (u32, u32) {
        let aspect = if self.display_aspect.is_finite() && self.display_aspect > 0.0 {
            self.display_aspect
        } else {
            1.0
        };
        if aspect >= 1.0 {
            (640, (640.0 / aspect).round().clamp(1.0, 640.0) as u32)
        } else {
            ((640.0 * aspect).round().clamp(1.0, 640.0) as u32, 640)
        }
    }
}

#[derive(Default, Debug)]
pub struct ImportResult {
    pub videos: Vec<Video>,
    pub imported: usize,
    pub errors: Vec<String>,
}

pub fn load(path: &Path) -> Result<Vec<Video>, String> {
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let videos: Vec<Video> = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    if videos.iter().any(|v| {
        v.width == 0
            || v.height == 0
            || !v.duration.is_finite()
            || v.duration <= 0.0
            || !v.display_aspect.is_finite()
            || v.display_aspect <= 0.0
    }) {
        return Err("Invalid video library metadata".into());
    }
    Ok(sorted_unique(videos))
}

fn sorted_unique(videos: Vec<Video>) -> Vec<Video> {
    let mut videos: Vec<_> = videos
        .into_iter()
        .map(|v| (v.path.clone(), v))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect();
    videos
        .sort_by(|a, b| (a.title.to_lowercase(), &a.path).cmp(&(b.title.to_lowercase(), &b.path)));
    videos
}

/// Call from an import worker. Failed probes preserve existing records; a failed
/// atomic save does not report a successful import or mutate visible UI state.
pub fn import(paths: &[PathBuf], database: &Path) -> Result<ImportResult, String> {
    let mut result = ImportResult {
        videos: load(database)?,
        ..Default::default()
    };
    let mut candidates = std::collections::BTreeSet::new();
    for path in paths {
        if path.is_dir() {
            for entry in walkdir::WalkDir::new(path).follow_links(false) {
                match entry {
                    Ok(e) if e.file_type().is_file() && is_video(e.path()) => {
                        candidates.insert(e.path().to_owned());
                    }
                    Ok(_) => {}
                    Err(e) => result.errors.push(e.to_string()),
                }
            }
        } else if is_video(path) {
            candidates.insert(path.clone());
        }
    }
    for path in candidates {
        match probe(&path) {
            Ok(video) => {
                result.videos.push(video);
                result.imported += 1;
            }
            Err(e) => result.errors.push(format!("{}: {e}", path.display())),
        }
    }
    result.videos = sorted_unique(result.videos);
    save(&result.videos, database)?;
    Ok(result)
}

fn save(videos: &[Video], database: &Path) -> Result<(), String> {
    if let Some(parent) = database.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temporary = database.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec(videos).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(temporary, database).map_err(|e| e.to_string())
}

/// Metadata checks do not retain an open file or create a filesystem watch.
/// Permission and other I/O errors must not be interpreted as a deleted file.
pub fn path_missing(path: &Path) -> Result<bool, String> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(!metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.to_string()),
    }
}

/// Run on the library worker, serialized with imports. Recheck after the playback
/// worker's report so a file restored in the meantime is not removed.
pub fn remove_missing(database: &Path, path: &Path) -> Result<bool, String> {
    if !path_missing(path)? {
        return Ok(false);
    }
    let mut videos = load(database)?;
    let count = videos.len();
    videos.retain(|video| video.path != path);
    if videos.len() != count {
        save(&videos, database)?;
    }
    Ok(true)
}

pub fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|v| v.to_str())
        .is_some_and(|v| EXTENSIONS.contains(&v.to_ascii_lowercase().as_str()))
}

fn ffprobe() -> PathBuf {
    if let Some(path) = std::env::var_os("NANO_FFPROBE") {
        return path.into();
    }
    for path in ["/opt/homebrew/bin/ffprobe", "/usr/local/bin/ffprobe"] {
        if Path::new(path).is_file() {
            return path.into();
        }
    }
    "ffprobe".into()
}

pub fn probe(path: &Path) -> Result<Video, String> {
    let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !path.is_file() {
        return Err("Expected a local video file".into());
    }
    let mut child = Command::new(ffprobe()).args([
        "-v", "error", "-protocol_whitelist", "file,pipe", "-print_format", "json",
        "-show_entries", "format=duration:format_tags=title:stream=codec_type,codec_name,width,height,display_aspect_ratio,duration:stream_disposition=attached_pic:stream_side_data=rotation",
    ]).arg(&path).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
        .map_err(|e| format!("ffprobe is required for video import (macOS: brew install ffmpeg): {e}"))?;
    // Drain both pipes while waiting so metadata or diagnostics cannot fill a
    // pipe and deadlock probing. Readers cap retained output at 1 MiB each.
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    fn drain(mut pipe: impl std::io::Read) -> Vec<u8> {
        let mut output = Vec::new();
        let mut buffer = [0; 8192];
        while let Ok(count) = pipe.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let retain = count.min((1024 * 1024usize).saturating_sub(output.len()));
            output.extend_from_slice(&buffer[..retain]);
        }
        output
    }
    let out = thread::spawn(move || drain(stdout));
    let err = thread::spawn(move || drain(stderr));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < Duration::from_secs(15) => {
                thread::sleep(Duration::from_millis(20))
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(match result {
                    Err(e) => e.to_string(),
                    _ => "Video probe timed out after 15 seconds".into(),
                });
            }
        }
    };
    let output = out.join().unwrap_or_default();
    let errors = err.join().unwrap_or_default();
    if !status?.success() {
        return Err(String::from_utf8_lossy(&errors).trim().to_owned());
    }
    metadata(&path, &output)
}

fn metadata(path: &Path, bytes: &[u8]) -> Result<Video, String> {
    let json: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let streams = json["streams"].as_array().ok_or("No video streams")?;
    let stream = streams
        .iter()
        .find(|s| {
            s["codec_type"] == "video" && s["disposition"]["attached_pic"].as_u64() != Some(1)
        })
        .ok_or("No playable video stream")?;
    let width = stream["width"]
        .as_u64()
        .filter(|v| *v > 0 && *v <= 65536)
        .ok_or("Invalid video width")? as u32;
    let height = stream["height"]
        .as_u64()
        .filter(|v| *v > 0 && *v <= 65536)
        .ok_or("Invalid video height")? as u32;
    let duration = json["format"]["duration"]
        .as_str()
        .or_else(|| stream["duration"].as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n > 0.0)
        .ok_or("Video needs a finite, positive duration")?;
    let mut aspect = stream["display_aspect_ratio"]
        .as_str()
        .and_then(|s| s.split_once(':'))
        .and_then(|(w, h)| Some(w.parse::<f64>().ok()? / h.parse::<f64>().ok()?))
        .filter(|n| n.is_finite() && *n > 0.0)
        .unwrap_or(f64::from(width) / f64::from(height));
    if stream["side_data_list"].as_array().is_some_and(|list| {
        list.iter().any(|s| {
            s["rotation"]
                .as_i64()
                .is_some_and(|r| r.rem_euclid(180) == 90)
        })
    }) {
        aspect = 1.0 / aspect;
    }
    Ok(Video {
        path: path.to_owned(),
        title: json["format"]["tags"]["title"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            }),
        duration,
        width,
        height,
        display_aspect: aspect,
        video_codec: stream["codec_name"].as_str().unwrap_or("unknown").into(),
        audio_codec: streams
            .iter()
            .find(|s| s["codec_type"] == "audio")
            .and_then(|s| s["codec_name"].as_str())
            .map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_respects_anamorphic_aspect_rotation_and_rejects_cover_art() {
        let bytes = br#"{"format":{"duration":"2.5"},"streams":[{"codec_type":"video","width":720,"height":576,"display_aspect_ratio":"16:9","codec_name":"h264","side_data_list":[{"rotation":90}]}]}"#;
        let video = metadata(Path::new("portrait.mp4"), bytes).unwrap();
        assert_eq!(video.render_size(), (360, 640));
        assert!(!video.is_landscape());
        let cover = br#"{"format":{"duration":"2"},"streams":[{"codec_type":"video","width":32,"height":32,"disposition":{"attached_pic":1}}]}"#;
        assert!(metadata(Path::new("song.mp4"), cover).is_err());
    }
    #[test]
    fn candidates_and_corrupt_library() {
        assert!(is_video(Path::new("Movie.MP4")));
        assert!(!is_video(Path::new("song.mp3")));
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("videos.json");
        assert!(load(&db).unwrap().is_empty());
        fs::write(&db, b"broken").unwrap();
        assert!(import(&[], &db).is_err());
        assert_eq!(fs::read(&db).unwrap(), b"broken");
    }
}
