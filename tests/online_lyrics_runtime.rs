use nano_music_player::{
    ai::{DeepSeek, DeepSeekSettings},
    library::{Track, online_lyrics},
    translation::Cancellation,
};

/// Explicit opt-in: sends the supplied song metadata to DeepSeek and LRCLIB.
/// Never prints credentials or returned lyric text and never edits a library.
#[test]
#[ignore = "requires configured DeepSeek credentials and an explicit track metadata JSON"]
fn live_lookup_retrieves_matching_lyrics() {
    let settings_path = std::env::var_os("NANO_DEEPSEEK_SETTINGS").expect("NANO_DEEPSEEK_SETTINGS");
    let track_path =
        std::env::var_os("NANO_ONLINE_LYRICS_TRACK").expect("NANO_ONLINE_LYRICS_TRACK");
    let settings = DeepSeekSettings::load(std::path::Path::new(&settings_path)).unwrap();
    let track: Track = serde_json::from_slice(&std::fs::read(track_path).unwrap()).unwrap();
    let provider = DeepSeek::new(&settings).unwrap();
    let found = online_lyrics::lookup(
        &online_lyrics::Request::new(&track),
        Some(&provider),
        &Cancellation::default(),
    )
    .unwrap();
    assert!(!found.lyrics.is_empty());
    assert!(found.source_url.starts_with("https://lrclib.net/api/get/"));
    eprintln!(
        "Verified online lyric lookup and LRCLIB retrieval: {} lyric lines, source {}",
        found.lyrics.lines().len(),
        found.source_url
    );
}

/// Opt-in metadata-only smoke test. No model calls, playback or library writes.
#[test]
#[ignore = "requires an explicit JSON array of track metadata and network access"]
fn live_metadata_lookup_needs_no_deepseek_requests() {
    let path = std::env::var_os("NANO_ONLINE_LYRICS_TRACKS").expect("NANO_ONLINE_LYRICS_TRACKS");
    let tracks: Vec<Track> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!tracks.is_empty());
    for track in tracks {
        let found = online_lyrics::lookup(
            &online_lyrics::Request::new(&track),
            None,
            &Cancellation::default(),
        )
        .unwrap_or_else(|error| panic!("{} / {}: {error}", track.title, track.artist));
        eprintln!(
            "Verified {} / {}: {} lines, {}",
            track.title,
            track.artist,
            found.lyrics.lines().len(),
            found.source_url
        );
    }
}
