use super::*;
use serde_json::json;

fn as_record(value: serde_json::Value) -> Record {
    serde_json::from_value(value).unwrap()
}

#[test]
fn discovery_urls_fetch_each_record_only_once() {
    let sources = [
        "https://lrclib.net/api/get/1",
        "https://lrclib.net/api/get/1?ref=search",
        "https://www.lrclib.net/api/get/1",
        "https://lrclib.net/api/get/2",
        "https://lrclib.net/api/get/3",
        "https://lrclib.net/api/get/4",
    ]
    .map(str::to_owned);
    assert_eq!(source_ids(&sources), [1, 2, 3]);
}

#[test]
fn matching_handles_typography_release_tags_and_moved_guest_credits() {
    for (title, artist, album, found_title, found_artist, found_album) in [
        (
            "Don’t Stop",
            "Artist",
            "Album",
            "Don't Stop",
            "artist",
            "Album (Deluxe Edition)",
        ),
        (
            "Ｃａｆé",
            "Artist",
            "Album - EP (FLAC)",
            "Cafe\u{301}",
            "Artist",
            "Album",
        ),
        (
            "Song (Live)",
            "Artist",
            "Album",
            "Song – Live",
            "Artist",
            "Album",
        ),
        (
            "Song (2007 Stereo Mix)",
            "Artist",
            "Album",
            "Song - 2007 Stereo Mix",
            "Artist",
            "Album",
        ),
        (
            "Song (feat. Guest)",
            "Artist",
            "Album",
            "Song",
            "Artist featuring Guest",
            "Album",
        ),
        (
            "Song",
            "Artist ft. Guest",
            "Album",
            "Song (feat Guest)",
            "Artist",
            "Album",
        ),
        (
            "Song (feat. Guest) (Live)",
            "Artist",
            "Album Live",
            "Song - Live",
            "Artist feat Guest",
            "Album Live",
        ),
    ] {
        let request = Request {
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            ..request()
        };
        let mut candidate = record();
        candidate["trackName"] = json!(found_title);
        candidate["artistName"] = json!(found_artist);
        candidate["albumName"] = json!(found_album);
        assert!(
            matching::rank(&request, &as_record(candidate)).is_some(),
            "{title} / {artist} / {album}"
        );
    }
}

#[test]
fn normalization_preserves_performance_and_feature_identity() {
    for (title, artist, found_title, found_artist) in [
        ("Song (Live)", "Artist", "Song", "Artist"),
        ("Song (Acoustic)", "Artist", "Song", "Artist"),
        ("Song (Remix)", "Artist", "Song", "Artist"),
        ("Song (Clean)", "Artist", "Song (Explicit)", "Artist"),
        (
            "Song (2007 Stereo Mix)",
            "Artist",
            "Song (1994 Remaster)",
            "Artist",
        ),
        (
            "Song (feat. Guest) (Live)",
            "Artist",
            "Song",
            "Artist feat Guest",
        ),
        (
            "Song (feat. Guest)",
            "Artist",
            "Song (feat. Someone Else)",
            "Artist",
        ),
        ("Song", "Different Artist", "Song", "Artist"),
    ] {
        let request = Request {
            title: title.into(),
            artist: artist.into(),
            ..request()
        };
        let mut candidate = record();
        candidate["trackName"] = json!(found_title);
        candidate["artistName"] = json!(found_artist);
        assert!(
            matching::rank(&request, &as_record(candidate)).is_none(),
            "{title} / {artist}"
        );
    }
}

#[test]
fn candidate_search_chooses_matching_album_over_first_result_and_rejects_ambiguity() {
    let mut other = record();
    other["id"] = json!(2);
    other["albumName"] = json!("Compilation");
    other["syncedLyrics"] = json!("[00:01]different words");
    let found = select(
        &request(),
        vec![as_record(other.clone()), as_record(record())],
    )
    .unwrap();
    assert_eq!(found.source_url, "https://lrclib.net/api/get/1");
    let mut conflicting = record();
    conflicting["id"] = json!(3);
    conflicting["syncedLyrics"] = json!("[00:01]conflicting lyrics");
    assert!(
        select(
            &request(),
            vec![as_record(conflicting), as_record(record())]
        )
        .unwrap_err()
        .starts_with("Multiple online")
    );
    let mut same = record();
    same["id"] = json!(4);
    same["albumName"] = json!("Reissue");
    assert!(select(&request(), vec![as_record(same), as_record(record())]).is_ok());
}

#[test]
fn lookup_uses_normalized_candidate_search_without_a_model_call_and_caches_misses() {
    let mut request = request();
    request.title = "Song (feat. Guest)".into();
    let (endpoint, server) = mock_replies(
        vec![("200 OK", json!([{"bad":"entry"},record()]).to_string())],
        None,
    );
    assert!(
        lookup_with_search(&request, &endpoint, &Cancellation::default(), || panic!(
            "unnecessary DeepSeek request"
        ))
        .is_ok()
    );
    let requests = server.join().unwrap();
    assert!(requests[0].contains("track_name=song&artist_name=artist"));
    let (endpoint, server) = mock_replies(
        vec![("200 OK", "[]".into()), ("404 Not Found", String::new())],
        None,
    );
    assert!(
        lookup_with_search(&request, &endpoint, &Cancellation::default(), || Err(
            "DeepSeek web search failed".into()
        ))
        .is_err()
    );
    server.join().unwrap();
    // The mock listener is closed: a second call must use the negative cache.
    assert_eq!(
        lookup_with_search(&request, &endpoint, &Cancellation::default(), || panic!(
            "repeated search"
        ))
        .unwrap_err(),
        "No matching online lyrics found"
    );
}

#[test]
fn lrclib_429_suppresses_followup_calls_across_tracks() {
    let (endpoint, server) = mock_replies(vec![("429 Too Many Requests", String::new())], None);
    let expected = "Online lyrics rate limit reached; try again later";
    assert_eq!(
        lookup_with_search(&request(), &endpoint, &Cancellation::default(), || panic!(
            "rate limited"
        ))
        .unwrap_err(),
        expected
    );
    server.join().unwrap();
    let mut another = request();
    another.title = "Another song".into();
    assert_eq!(
        lookup_with_search(&another, &endpoint, &Cancellation::default(), || panic!(
            "rate limited"
        ))
        .unwrap_err(),
        expected
    );
}
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

fn mock_replies(
    replies: Vec<(&'static str, String)>,
    cancel_after_reply: Option<Cancellation>,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    mock_replies_with_retry(replies, cancel_after_reply, 60)
}

fn mock_replies_with_retry(
    replies: Vec<(&'static str, String)>,
    cancel_after_reply: Option<Cancellation>,
    retry_seconds: u64,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    use std::{io::Write, net::TcpListener, thread};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/api/get", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in replies {
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
            assert!(headers.contains("user-agent: nanomusicplayer/"));
            assert!(!headers.contains("authorization:"));
            assert!(!headers.contains("x-api-key:"));
            requests.push(headers.lines().next().unwrap().to_owned());
            if let Some(cancellation) = &cancel_after_reply {
                cancellation.cancel();
            }
            let retry = if status.starts_with("429") {
                format!("Retry-After: {retry_seconds}\r\n")
            } else {
                String::new()
            };
            write!(socket, "HTTP/1.1 {status}\r\n{retry}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    (endpoint, server)
}

#[test]
fn a_short_provider_cooldown_allows_only_one_cancellable_retry() {
    let (endpoint, server) = mock_replies_with_retry(
        vec![
            ("429 Too Many Requests", String::new()),
            ("200 OK", json!([record()]).to_string()),
        ],
        None,
        1,
    );
    assert!(
        lookup_with_search(&request(), &endpoint, &Cancellation::default(), || panic!(
            "unnecessary model call"
        ))
        .is_ok()
    );
    assert_eq!(server.join().unwrap().len(), 2);
    let cancellation = Cancellation::default();
    let (endpoint, server) = mock_replies_with_retry(
        vec![("429 Too Many Requests", String::new())],
        Some(cancellation.clone()),
        1,
    );
    assert_eq!(
        lookup_with_search(&request(), &endpoint, &cancellation, || panic!("cancelled"))
            .unwrap_err(),
        "Online lyric search cancelled"
    );
    assert_eq!(server.join().unwrap().len(), 1);
    let (endpoint, server) = mock_replies_with_retry(
        vec![
            ("429 Too Many Requests", String::new()),
            ("429 Too Many Requests", String::new()),
        ],
        None,
        1,
    );
    assert_eq!(
        lookup_with_search(&request(), &endpoint, &Cancellation::default(), || panic!(
            "rate limited"
        ))
        .unwrap_err(),
        "Online lyrics rate limit reached; try again later"
    );
    assert_eq!(server.join().unwrap().len(), 2);
}

#[test]
fn deepseek_search_failure_does_not_block_verified_lrclib_lyrics() {
    let (endpoint, server) = mock_replies(vec![("200 OK", json!([record()]).to_string())], None);
    let found = lookup_with_search(&request(), &endpoint, &Cancellation::default(), || {
        panic!("a direct match must not call DeepSeek")
    })
    .unwrap();
    assert_eq!(found.source_url, "https://lrclib.net/api/get/1");
    assert_eq!(server.join().unwrap().len(), 1);

    // A failed search still respects cancellation before any LRCLIB request.
    let cancellation = Cancellation::default();
    let (endpoint, server) = mock_replies(
        vec![("200 OK", "[]".into()), ("404 Not Found", String::new())],
        None,
    );
    assert_eq!(
        lookup_with_search(&request(), &endpoint, &cancellation, || {
            cancellation.cancel();
            Err("DeepSeek web search failed".into())
        })
        .unwrap_err(),
        "Online lyric search cancelled"
    );
    assert_eq!(server.join().unwrap().len(), 2);
}

#[test]
fn wax_poetic_uses_candidate_search_after_missing_or_unrelated_result() {
    let request = Request {
        title: "Wax Poetic".into(),
        artist: "Capstan".into(),
        album: "Cultural Divide - EP".into(),
        duration: 222.0,
        ..request()
    };
    // Synthetic lyric text; only recording metadata mirrors the regression.
    let mut matching = record();
    matching["id"] = json!(10460169);
    matching["trackName"] = json!("Wax Poetic");
    matching["artistName"] = json!("Capstan");
    matching["albumName"] = json!("Cultural Divide");
    matching["duration"] = json!(222);
    for first in [
        ("404 Not Found", String::new()),
        ("200 OK", record().to_string()),
    ] {
        let (endpoint, server) = mock_replies(
            vec![first, ("200 OK", serde_json::json!([matching]).to_string())],
            None,
        );
        let sources = vec!["https://lrclib.net/api/get/999".into()];
        let found = retrieve(&request, &endpoint, &sources, &Cancellation::default()).unwrap();
        assert_eq!(found.source_url, "https://lrclib.net/api/get/10460169");
        assert_eq!(found.lyrics.timed[1].at_ms, 2000);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], "get /api/get/999 http/1.1");
        assert!(requests[1].starts_with("get /api/search?"));
        assert!(!requests[1].contains("album_name="));
        assert!(requests[1].contains("track_name=wax+poetic"));
        assert!(requests[1].contains("artist_name=capstan"));
    }
}

#[test]
fn album_fallback_still_rejects_different_versions_and_recordings() {
    for (field, value) in [
        ("trackName", json!("Song (Acoustic)")),
        ("artistName", json!("Another Artist")),
        ("albumName", json!("Album (Live)")),
        ("duration", json!(65)),
        ("instrumental", json!(true)),
        ("syncedLyrics", json!("[05:00]too late")),
    ] {
        let mut wrong = record();
        wrong[field] = value;
        let (endpoint, server) = mock_replies(
            vec![
                ("200 OK", serde_json::json!([wrong]).to_string()),
                ("404 Not Found", String::new()),
            ],
            None,
        );
        assert!(
            retrieve(&request(), &endpoint, &[], &Cancellation::default()).is_err(),
            "{field}"
        );
        assert_eq!(server.join().unwrap().len(), 2);
    }
    for album in [
        "Album",
        "Album - EP",
        "Album EP",
        "Album (EP)",
        "Album [EP]",
        "Album - Single",
        "Album Single",
        "Album (Single)",
        "Album [Single]",
    ] {
        let mut equivalent = record();
        equivalent["albumName"] = json!(album);
        assert!(
            validate_record(&request(), serde_json::from_value(equivalent).unwrap()).is_ok(),
            "{album}"
        );
    }
}

#[test]
fn retries_stop_on_cancellation_and_service_errors() {
    let cancellation = Cancellation::default();
    let (endpoint, server) = mock_replies(
        vec![("404 Not Found", String::new())],
        Some(cancellation.clone()),
    );
    assert_eq!(
        retrieve(&request(), &endpoint, &[], &cancellation).unwrap_err(),
        "Online lyric search cancelled"
    );
    assert_eq!(server.join().unwrap().len(), 1);
    for (status, body, error) in [
        (
            "429 Too Many Requests",
            "",
            "Online lyrics rate limit reached; try again later",
        ),
        (
            "503 Service Unavailable",
            "",
            "Online lyrics service is unavailable",
        ),
        (
            "200 OK",
            "invalid JSON",
            "Online lyrics returned an invalid response",
        ),
    ] {
        let (endpoint, server) = mock_replies(vec![(status, body.into())], None);
        assert_eq!(
            retrieve(&request(), &endpoint, &[], &Cancellation::default()).unwrap_err(),
            error
        );
        assert_eq!(server.join().unwrap().len(), 1);
    }
}

#[test]
fn http_lookup_identifies_client_bounds_responses_and_reports_missing_or_rate_limited() {
    for (status, body, ok) in [
        ("200 OK", record().to_string(), true),
        ("404 Not Found", "missing".to_owned(), false),
        ("429 Too Many Requests", "limited".to_owned(), false),
        ("200 OK", "x".repeat(RESPONSE_LIMIT as usize + 1), false),
    ] {
        let (endpoint, server) = mock_replies(vec![(status, body)], None);
        assert_eq!(
            fetch_record(
                &lyrics_client().unwrap(),
                &request(),
                &endpoint,
                Query::Signature
            )
            .and_then(|record| validate_record(&request(), record).map_err(FetchError::Candidate))
            .is_ok(),
            ok
        );
        let requests = server.join().unwrap();
        assert!(requests[0].contains("track_name=song"));
        assert!(requests[0].contains("artist_name=artist"));
        assert!(requests[0].contains("duration=60"));
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
    assert!(validate_record(&request(), serde_json::from_value(empty.clone()).unwrap()).is_err());
    empty["syncedLyrics"] = json!("[00:01]\n[00:02]");
    assert!(validate_record(&request(), serde_json::from_value(empty).unwrap()).is_err());
    assert_eq!(lrclib_id("https://lrclib.net/api/get/123"), Some(123));
    assert_eq!(lrclib_id("https://lrclib.net.evil.test/api/get/123"), None);
    assert_eq!(lrclib_id("http://127.0.0.1/api/get/123"), None);
}
