use nano_music_player::{
    app::{
        action::Action,
        navigation::{Filter, Screen},
        state::State,
    },
    library::{
        database::Database,
        lyrics::Lyrics,
        scanner::{self, CachedLoadEvent},
    },
};
use std::time::Duration;

#[test]
fn source_sidecars_refresh_after_the_cached_snapshot_and_manual_removal_wins() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("song.mp3");
    std::fs::copy("tests/fixtures/quiet.mp3", &source).unwrap();
    let database = directory.path().join("library.sqlite3");
    let track = scanner::read_track(source.clone()).unwrap();
    Database::open(&database).unwrap().save(&track, 1).unwrap();
    std::fs::write(source.with_extension("lrc"), "[00:01.00]New sidecar").unwrap();
    let events = scanner::load_cached(database.clone());
    let CachedLoadEvent::Cached(cached) = events.recv_timeout(Duration::from_secs(2)).unwrap()
    else {
        panic!("cached event")
    };
    assert!(cached[0].lyrics.is_empty());
    let CachedLoadEvent::Refreshed { original, track } =
        events.recv_timeout(Duration::from_secs(2)).unwrap()
    else {
        panic!("sidecar update")
    };
    assert_eq!(*original, cached[0]);
    assert_eq!(track.lyrics.lines(), ["New sidecar"]);
    assert!(matches!(
        events.recv_timeout(Duration::from_secs(2)).unwrap(),
        CachedLoadEvent::Complete
    ));
    Database::open(&database)
        .unwrap()
        .remove_lyrics(&source)
        .unwrap();
    let events = scanner::load_cached(database);
    let CachedLoadEvent::Cached(cached) = events.recv_timeout(Duration::from_secs(2)).unwrap()
    else {
        panic!("cached removal")
    };
    assert!(cached[0].lyrics.is_empty());
    assert!(matches!(
        events.recv_timeout(Duration::from_secs(2)).unwrap(),
        CachedLoadEvent::Complete
    ));
}

#[test]
fn late_refresh_keeps_newer_lyrics_imports_and_deleted_tracks_with_stable_queue_ids() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("song.mp3");
    std::fs::copy("tests/fixtures/quiet.mp3", &source).unwrap();
    let original = scanner::read_track(source).unwrap();
    let mut refreshed = original.clone();
    refreshed.title = "Changed title".into();
    let mut state = State::default();
    state.merge_library(vec![original.clone()]);
    state.navigation.open(Screen::Songs(Filter::All));
    state.navigation.select(1);
    state.apply(Action::Activate);
    state.position = 23.0;
    let queue = state.queue.order.clone();
    assert!(state.refresh_cached_track(&original, refreshed.clone()));
    assert_eq!(state.current, Some(0));
    assert_eq!(state.queue.order, queue);
    assert_eq!(state.position, 23.0);
    assert_eq!(state.tracks[0].title, "Changed title");
    assert!(
        !state.refresh_cached_track(&original, original.clone()),
        "newer imported metadata wins"
    );

    state.tracks[0] = original.clone();
    state.tracks[0].lyrics = Lyrics::plain_text("Newer manual lyrics");
    assert!(!state.refresh_cached_track(&original, refreshed.clone()));
    assert_eq!(state.tracks[0].lyrics.lines(), ["Newer manual lyrics"]);

    state.tracks[0] = original.clone();
    state.mark_track_available(&original.path, false);
    assert!(!state.refresh_cached_track(&original, refreshed));
    assert!(!state.track_available(&original));
}
