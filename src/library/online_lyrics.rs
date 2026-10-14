//! Explicit online lookup. DeepSeek searches first; lyric text comes from LRCLIB,
//! never from the model's prose. Requests run outside playback and UI processing.
use super::{Track, lyrics::Lyrics};
use crate::{ai::DeepSeek, translation::Cancellation};
use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::Deserialize;
use std::{io::Read, path::PathBuf, time::Duration};

const ENDPOINT: &str = "https://lrclib.net/api/get";
const RESPONSE_LIMIT: u64 = 262_144;

#[derive(Debug, Clone)]
pub struct Request {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
}

impl Request {
    pub fn new(track: &Track) -> Self {
        Self {
            path: track.path.clone(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            duration: track.duration,
        }
    }

    pub fn matches(&self, track: &Track) -> bool {
        self.path == track.path
            && self.title == track.title
            && self.artist == track.artist
            && self.album == track.album
            && self.duration == track.duration
            && track.lyrics.is_empty()
    }
}

#[derive(Debug)]
pub struct Found {
    pub lyrics: Lyrics,
    pub source_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    id: u64,
    track_name: String,
    artist_name: String,
    album_name: String,
    duration: f64,
    instrumental: bool,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

fn normalized(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn validate_record(request: &Request, record: Record) -> Result<Found, String> {
    if normalized(&record.track_name) != normalized(&request.title)
        || normalized(&record.artist_name) != normalized(&request.artist)
        || (!request.album.trim().is_empty()
            && normalized(&record.album_name) != normalized(&request.album))
        || !record.duration.is_finite()
        || (record.duration - request.duration).abs() > 2.0
    {
        return Err("Online lyrics did not match this recording".into());
    }
    if record.instrumental {
        return Err("This recording is instrumental".into());
    }
    let mut lyrics = record
        .synced_lyrics
        .as_deref()
        .map(Lyrics::parse)
        .unwrap_or_default();
    if lyrics.timed.iter().all(|line| line.text.trim().is_empty()) {
        lyrics = record
            .plain_lyrics
            .as_deref()
            .map(Lyrics::plain_text)
            .unwrap_or_default();
    }
    if lyrics.is_empty() {
        return Err("No matching online lyrics found".into());
    }
    if lyrics
        .timed
        .iter()
        .any(|line| line.at_ms as f64 > (request.duration + 2.0) * 1000.0)
    {
        return Err("Online lyrics did not match this recording".into());
    }
    Ok(Found {
        lyrics,
        source_url: format!("{ENDPOINT}/{}", record.id),
    })
}

fn lrclib_id(source: &str) -> Option<u64> {
    let url = Url::parse(source).ok()?;
    if url.scheme() != "https" || !matches!(url.host_str(), Some("lrclib.net" | "www.lrclib.net")) {
        return None;
    }
    url.path().strip_prefix("/api/get/")?.parse().ok()
}

pub fn lookup(
    request: &Request,
    provider: &DeepSeek,
    cancellation: &Cancellation,
) -> Result<Found, String> {
    if request.title.trim().is_empty()
        || request.artist.trim().is_empty()
        || !request.duration.is_finite()
        || !(1.0..=3600.0).contains(&request.duration)
    {
        return Err("Title, artist and duration are required for online lyrics".into());
    }
    if cancellation.is_cancelled() {
        return Err("Online lyric search cancelled".into());
    }
    let sources = provider.search(&format!(
        "{} {} {} lyrics LRCLIB",
        request.title, request.artist, request.album
    ))?;
    if cancellation.is_cancelled() {
        return Err("Online lyric search cancelled".into());
    }
    // Use a native search result's LRCLIB record when present. Otherwise query
    // LRCLIB's exact signature API; never fetch arbitrary model-selected hosts.
    let id = sources.iter().find_map(|source| lrclib_id(source));
    let result = fetch(request, ENDPOINT, id);
    if cancellation.is_cancelled() {
        return Err("Online lyric search cancelled".into());
    }
    result
}

fn fetch(request: &Request, endpoint: &str, id: Option<u64>) -> Result<Found, String> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .redirect(Policy::none())
        .user_agent(concat!(
            "NanoMusicPlayer/",
            env!("CARGO_PKG_VERSION"),
            " (https://github.com/huanzichen00/Nano-Music-Player)"
        ))
        .build()
        .map_err(|_| "Cannot connect to online lyrics")?;
    let url = id.map_or_else(|| endpoint.to_owned(), |id| format!("{endpoint}/{id}"));
    let mut query = client.get(url);
    if id.is_none() {
        query = query.query(&[
            ("track_name", request.title.clone()),
            ("artist_name", request.artist.clone()),
            ("album_name", request.album.clone()),
            ("duration", request.duration.to_string()),
        ]);
    }
    let response = query
        .send()
        .map_err(|_| "Cannot connect to online lyrics")?;
    match response.status().as_u16() {
        200 => {}
        404 => return Err("No matching online lyrics found".into()),
        429 => return Err("Online lyrics rate limit reached; try again later".into()),
        _ => return Err("Online lyrics service is unavailable".into()),
    }
    let mut bytes = Vec::new();
    response
        .take(RESPONSE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read online lyrics")?;
    if bytes.len() as u64 > RESPONSE_LIMIT {
        return Err("Online lyrics response is too large".into());
    }
    let record =
        serde_json::from_slice(&bytes).map_err(|_| "Online lyrics returned an invalid response")?;
    validate_record(request, record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn request() -> Request {
        Request {
            path: "song.wav".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            duration: 60.0,
        }
    }
    fn record() -> serde_json::Value {
        json!({"id":1,"trackName":"Song","artistName":"Artist","albumName":"Album","duration":60,
            "instrumental":false,"plainLyrics":"original first\noriginal second", "syncedLyrics":"[00:01]original first\n[00:02]original second"})
    }

    #[test]
    fn http_lookup_identifies_client_bounds_responses_and_reports_missing_or_rate_limited() {
        use std::{io::Write, net::TcpListener, thread};
        for (status, body, ok) in [
            ("200 OK", record().to_string(), true),
            ("404 Not Found", "missing".to_owned(), false),
            ("429 Too Many Requests", "limited".to_owned(), false),
            ("200 OK", "x".repeat(RESPONSE_LIMIT as usize + 1), false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/api/get", listener.local_addr().unwrap());
            let server = thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    let mut buffer = [0; 1024];
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                }
                let headers = String::from_utf8(bytes).unwrap().to_lowercase();
                assert!(headers.contains("track_name=song"));
                assert!(headers.contains("artist_name=artist"));
                assert!(headers.contains("duration=60"));
                assert!(headers.contains("user-agent: nanomusicplayer/"));
                assert!(!headers.contains("authorization:"));
                assert!(!headers.contains("x-api-key:"));
                write!(socket, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            });
            assert_eq!(fetch(&request(), &endpoint, None).is_ok(), ok);
            server.join().unwrap();
        }
    }

    #[test]
    fn downloaded_lyrics_survive_reload_and_never_replace_manual_imports_or_removals() {
        use super::super::database::{Database, LyricsAssociation};
        let directory = tempfile::tempdir().unwrap();
        let db = Database::open(&directory.path().join("library.sqlite3")).unwrap();
        let audio = directory.path().join("song.wav");
        let lyrics = Lyrics::parse("[00:01]original first");
        db.set_online_lyrics(&audio, "https://lrclib.net/api/get/1", &lyrics)
            .unwrap();
        assert!(
            matches!(db.lyrics_association(&audio).unwrap(), LyricsAssociation::Manual { source, lyrics: saved } if source == "https://lrclib.net/api/get/1" && saved == lyrics)
        );
        db.set_manual_lyrics(
            &audio,
            &directory.path().join("my.lrc"),
            &Lyrics::plain_text("my version"),
        )
        .unwrap();
        assert!(
            db.set_online_lyrics(&audio, "https://lrclib.net/api/get/1", &lyrics)
                .is_err()
        );
        assert!(
            matches!(db.lyrics_association(&audio).unwrap(), LyricsAssociation::Manual { lyrics, .. } if lyrics.plain == ["my version"])
        );
        db.remove_lyrics(&audio).unwrap();
        assert!(
            db.set_online_lyrics(&audio, "https://lrclib.net/api/get/1", &lyrics)
                .is_err()
        );
        assert!(matches!(
            db.lyrics_association(&audio).unwrap(),
            LyricsAssociation::Removed
        ));
    }
    #[test]
    fn exact_record_keeps_real_timestamps_and_plain_fallback() {
        let found = validate_record(&request(), serde_json::from_value(record()).unwrap()).unwrap();
        assert_eq!(found.lyrics.timed[1].at_ms, 2000);
        assert_eq!(found.source_url, "https://lrclib.net/api/get/1");
        let mut plain = record();
        plain["syncedLyrics"] = serde_json::Value::Null;
        let found = validate_record(&request(), serde_json::from_value(plain).unwrap()).unwrap();
        assert!(found.lyrics.timed.is_empty());
        assert_eq!(found.lyrics.plain, ["original first", "original second"]);
    }
    #[test]
    fn wrong_version_instrumental_invalid_timing_and_empty_records_are_rejected() {
        for (field, value) in [
            ("trackName", json!("Wrong")),
            ("artistName", json!("Wrong")),
            ("albumName", json!("Live")),
            ("duration", json!(65)),
            ("instrumental", json!(true)),
            ("syncedLyrics", json!("[05:00]late")),
        ] {
            let mut invalid = record();
            invalid[field] = value;
            assert!(
                validate_record(&request(), serde_json::from_value(invalid).unwrap()).is_err(),
                "{field}"
            );
        }
        let mut empty = record();
        empty["syncedLyrics"] = json!(null);
        empty["plainLyrics"] = json!(null);
        assert!(
            validate_record(&request(), serde_json::from_value(empty.clone()).unwrap()).is_err()
        );
        empty["syncedLyrics"] = json!("[00:01]\n[00:02]");
        assert!(validate_record(&request(), serde_json::from_value(empty).unwrap()).is_err());
        assert_eq!(lrclib_id("https://lrclib.net/api/get/123"), Some(123));
        assert_eq!(lrclib_id("https://lrclib.net.evil.test/api/get/123"), None);
        assert_eq!(lrclib_id("http://127.0.0.1/api/get/123"), None);
    }
}
