#![cfg(not(any(target_os = "ios", feature = "mobile")))]

use chrono::TimeZone;
use nano_music_player::{
    app::{
        action::Action,
        navigation::{Filter, Screen, StatisticsDetailPage, StatisticsGroup, StatisticsPage},
        state::{Effect, Row, State, Target},
    },
    audio::{PlaybackStatus, Repeat, engine::AudioCommand},
    library::{
        Track,
        statistics::{StatisticsRange, StatisticsStore, StatisticsTrack, statistics_identity},
    },
};
use std::{collections::BTreeSet, path::PathBuf};

fn track(path: &str, title: &str, artist: &str, album: &str, number: u32) -> Track {
    Track {
        path: PathBuf::from(path),
        title: title.into(),
        artist: artist.into(),
        album: album.into(),
        genre: String::new(),
        number,
        duration: 120.0,
        artwork: vec![number as u8],
        lyrics: Default::default(),
    }
}

fn fixture() -> State {
    let tracks = vec![
        track(
            "/music/z-first.flac",
            "Z Opening",
            " Artist A ",
            "Shared",
            1,
        ),
        track(
            "/music/a-second.flac",
            "A Second",
            "artist a",
            " shared ",
            2,
        ),
        track(
            "/music/m-zero.flac",
            "Never Played",
            "Artist A",
            "Unplayed",
            1,
        ),
        track("/music/b-other.flac", "Other Hit", "Artist B", "Shared", 1),
        track(
            "/music/c-other.flac",
            "Other Two",
            "Artist B",
            "Other Album",
            2,
        ),
        track("/music/u-third.flac", "M Third", "Artist A", "Shared", 3),
    ];
    let metadata: Vec<_> = tracks
        .iter()
        .map(|track| StatisticsTrack {
            path: track.path.clone(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
        })
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let store = StatisticsStore::open(&directory.path().join("statistics.sqlite")).unwrap();
    let now = chrono::Utc
        .with_ymd_and_hms(2026, 10, 22, 12, 0, 0)
        .unwrap();
    store
        .register_library_paths(
            &tracks
                .iter()
                .map(|track| track.path.clone())
                .collect::<Vec<_>>(),
            now.timestamp(),
        )
        .unwrap();
    for (id, count) in [(0, 5), (1, 2), (3, 4), (4, 1), (5, 3)] {
        for _ in 0..count {
            store
                .record_play(&tracks[id].path, now.timestamp(), 30)
                .unwrap();
        }
    }
    // Removed songs retain their cumulative counts but do not acquire a current
    // metadata row or leak into the available artist/album membership.
    for _ in 0..4 {
        store
            .record_play(&PathBuf::from("/removed/old.flac"), now.timestamp(), 30)
            .unwrap();
    }
    let snapshot = store
        .listening_statistics(&metadata, StatisticsRange::AllTime, now)
        .unwrap();
    let mut state = State {
        tracks,
        ..State::default()
    };
    state.statistics.snapshot = Some(snapshot);
    state
}

fn activate_matching(state: &mut State, matches: impl Fn(&Row) -> bool) -> Vec<Effect> {
    let selected = state.rows().iter().position(matches).expect("matching row");
    state.navigation.select(selected);
    state.apply(Action::Activate)
}

fn activate_title(state: &mut State, title: &str) -> Vec<Effect> {
    activate_matching(state, |row| row.title == title)
}

fn detail(state: &State) -> (StatisticsGroup, StatisticsDetailPage) {
    match &state.navigation.current().screen {
        Screen::StatisticsDetail { group, page } => (group.clone(), *page),
        other => panic!("expected statistics detail, got {other:?}"),
    }
}

fn open_artist(state: &mut State, artist: &str) -> StatisticsGroup {
    state
        .navigation
        .open(Screen::Statistics(StatisticsPage::Artists));
    let effects = activate_matching(state, |row| {
        matches!(&row.target, Target::Screen(Screen::StatisticsDetail { group, .. })
            if group.artist.is_none() && statistics_identity(&group.name) == statistics_identity(artist))
    });
    assert_no_audio(&effects);
    let (group, page) = detail(state);
    assert_eq!(page, StatisticsDetailPage::Summary);
    group
}

fn open_album(state: &mut State, album: &str, artist: &str) -> StatisticsGroup {
    state
        .navigation
        .open(Screen::Statistics(StatisticsPage::Albums));
    let effects = activate_matching(state, |row| {
        matches!(&row.target, Target::Screen(Screen::StatisticsDetail { group, .. })
            if statistics_identity(&group.name) == statistics_identity(album)
                && group.artist.as_ref().is_some_and(|name| statistics_identity(name) == statistics_identity(artist)))
    });
    assert_no_audio(&effects);
    let (group, page) = detail(state);
    assert_eq!(page, StatisticsDetailPage::Summary);
    group
}

fn assert_no_audio(effects: &[Effect]) {
    assert!(
        effects
            .iter()
            .all(|effect| !matches!(effect, Effect::Audio(_)))
    );
}

fn play_ids(state: &State) -> Vec<usize> {
    state
        .rows()
        .iter()
        .filter_map(|row| match row.target {
            Target::Play(id) => Some(id),
            _ => None,
        })
        .collect()
}

fn play_paths(state: &State) -> BTreeSet<PathBuf> {
    play_ids(state)
        .into_iter()
        .map(|id| state.tracks[id].path.clone())
        .collect()
}

fn assert_loaded(effects: &[Effect], expected: usize) {
    assert!(matches!(effects, [Effect::Audio(AudioCommand::Load { id, .. })] if *id == expected));
}

#[test]
fn top_artist_and_album_open_details_without_touching_active_playback() {
    let mut state = fixture();
    state.queue.set(vec![3, 4], 3, false);
    state.current = Some(3);
    state.status = PlaybackStatus::Playing;
    state.paused = false;
    state.position = 17.0;
    state.repeat = Repeat::One;
    let artist = open_artist(&mut state, "Artist A");
    assert_eq!(artist.paths.len(), 4);
    assert!(artist.artist.is_none());
    assert_eq!(
        state.statistics_detail_info().subtitle,
        "10 plays · 4 tracks"
    );
    let album = open_album(&mut state, "Shared", "Artist A");
    assert_eq!(album.paths.len(), 3);
    assert!(album.artist.is_some());
    assert!(
        state
            .statistics_detail_info()
            .subtitle
            .ends_with("10 plays · 3 tracks")
    );
    assert_eq!(state.queue.order, [3, 4]);
    assert_eq!(state.queue.position, 0);
    assert_eq!(state.current, Some(3));
    assert_eq!(state.status, PlaybackStatus::Playing);
    assert!(!state.paused);
    assert_eq!(state.position, 17.0);
    assert_eq!(state.repeat, Repeat::One);
}

#[test]
fn artist_albums_include_unplayed_albums_and_keep_same_named_other_artists_separate() {
    let mut state = fixture();
    open_artist(&mut state, "Artist A");
    activate_title(&mut state, "Albums");
    assert_eq!(detail(&state).1, StatisticsDetailPage::Albums);
    let albums = state.statistics_entries();
    assert_eq!(albums.len(), 2);
    assert_eq!(statistics_identity(&albums[0].row.title), "shared");
    assert!(albums[0].row.subtitle.starts_with("10 plays"));
    assert_eq!(statistics_identity(&albums[1].row.title), "unplayed");
    assert!(albums[1].row.subtitle.starts_with("0 plays"));
    let selected = albums[1].row.title.clone();
    activate_title(&mut state, &selected);
    let (unplayed, page) = detail(&state);
    assert_eq!(page, StatisticsDetailPage::Summary);
    assert_eq!(
        unplayed.paths.as_ref(),
        &BTreeSet::from([state.tracks[2].path.clone()])
    );
    assert!(matches!(
        state.rows()[0].target,
        Target::PlayStatisticsGroup(_)
    ));
    assert!(
        state
            .statistics_detail_info()
            .subtitle
            .ends_with("0 plays · 1 tracks")
    );
    assert_no_audio(&activate_title(&mut state, "See All Tracks"));
    assert_eq!(play_ids(&state), [2]);
    assert!(!state.supports_list_shuffle());

    let artist_a = open_album(&mut state, "Shared", "Artist A");
    let artist_b = open_album(&mut state, "Shared", "Artist B");
    assert!(artist_a.paths.is_disjoint(&artist_b.paths));
    assert_eq!(
        artist_b.paths.as_ref(),
        &BTreeSet::from([state.tracks[3].path.clone()])
    );
    assert!(
        state
            .statistics_detail_info()
            .subtitle
            .ends_with("4 plays · 1 tracks")
    );
}

#[test]
fn artist_album_identity_uses_current_artist_metadata_even_before_snapshot_refresh() {
    let mut state = fixture();
    open_artist(&mut state, "Artist A");
    // A tag refresh may precede the asynchronous statistics refresh. Exact path
    // membership must not merge two current artists' same-named albums.
    state.tracks[5].artist = "Artist B".into();
    activate_title(&mut state, "Albums");
    let entries = state.statistics_entries();
    let shared: Vec<_> = entries
        .iter()
        .filter(|entry| statistics_identity(&entry.row.title) == "shared")
        .collect();
    assert_eq!(shared.len(), 2);
    assert!(shared[0].row.subtitle.starts_with("7 plays"));
    assert!(shared[1].row.subtitle.starts_with("3 plays"));
    let group_of = |row: &Row| match &row.target {
        Target::Screen(Screen::StatisticsDetail { group, .. }) => group.clone(),
        _ => panic!("album detail target"),
    };
    let a = group_of(&shared[0].row);
    let b = group_of(&shared[1].row);
    assert_eq!(statistics_identity(a.artist.as_ref().unwrap()), "artist a");
    assert_eq!(statistics_identity(b.artist.as_ref().unwrap()), "artist b");
    assert_eq!(
        a.paths.as_ref(),
        &BTreeSet::from([state.tracks[0].path.clone(), state.tracks[1].path.clone()])
    );
    assert_eq!(
        b.paths.as_ref(),
        &BTreeSet::from([state.tracks[5].path.clone()])
    );
    assert!(a.paths.is_disjoint(&b.paths));
}

#[test]
fn see_all_tracks_uses_exact_group_paths_and_existing_album_order() {
    let mut state = fixture();
    state.queue.set(vec![3, 4], 4, false);
    let artist = open_artist(&mut state, "Artist A");
    assert_no_audio(&activate_title(&mut state, "See All Tracks"));
    assert!(matches!(&state.navigation.current().screen,
        Screen::Songs(Filter::Statistics { paths, album_order: false, .. }) if **paths == *artist.paths));
    assert!(state.supports_list_shuffle());
    assert_eq!(play_ids(&state), [1, 5, 2, 0]);
    assert_eq!(play_paths(&state), *artist.paths);
    assert_eq!(state.queue.order, [3, 4]);
    assert_eq!(state.queue.position, 1);

    let album = open_album(&mut state, "Shared", "Artist A");
    assert_no_audio(&activate_title(&mut state, "See All Tracks"));
    assert!(matches!(&state.navigation.current().screen,
        Screen::Songs(Filter::Statistics { paths, album_order: true, .. }) if **paths == *album.paths));
    assert!(!state.supports_list_shuffle());
    assert_eq!(play_ids(&state), [0, 1, 5]);
    assert_eq!(play_paths(&state), *album.paths);
    assert_eq!(state.queue.order, [3, 4]);
}

#[test]
fn play_artist_reuses_song_activation_and_keeps_the_existing_shuffle_policy() {
    for shuffle in [false, true] {
        let mut state = fixture();
        state.shuffle = shuffle;
        state.repeat = Repeat::One;
        open_artist(&mut state, "Artist A");
        let effects = activate_title(&mut state, "Play Artist");
        assert_loaded(&effects, 1);
        assert_eq!(state.navigation.current().screen, Screen::NowPlaying);
        assert_eq!(state.current, Some(1));
        assert_eq!(state.queue.current(), Some(1));
        assert_eq!(state.shuffle, shuffle);
        assert_eq!(state.repeat, Repeat::One);
        if shuffle {
            let mut members = state.queue.order.clone();
            members.sort_unstable();
            assert_eq!(members, [0, 1, 2, 5]);
            assert_eq!(state.queue.position, 0);
            state.queue.unshuffle();
        }
        assert_eq!(state.queue.order, [1, 5, 2, 0]);
        state.apply(Action::Back);
        assert!(matches!(
            state.navigation.current().screen,
            Screen::Songs(Filter::Statistics {
                album_order: false,
                ..
            })
        ));
        assert!(state.supports_list_shuffle());
    }
}

#[test]
fn play_album_preserves_number_order_and_respects_existing_global_shuffle() {
    for shuffle in [false, true] {
        let mut state = fixture();
        state.shuffle = shuffle;
        let group = open_album(&mut state, "Shared", "Artist A");
        let effects = activate_title(&mut state, "Play Album");
        assert_loaded(&effects, 0);
        assert_eq!(state.navigation.current().screen, Screen::NowPlaying);
        assert_eq!(state.current, Some(0));
        assert_eq!(state.shuffle, shuffle);
        assert_eq!(state.queue.current(), Some(0));
        if shuffle {
            let mut members = state.queue.order.clone();
            members.sort_unstable();
            assert_eq!(members, [0, 1, 5]);
            state.queue.unshuffle();
        }
        assert_eq!(state.queue.order, [0, 1, 5]);
        state.apply(Action::Back);
        assert_eq!(
            state.navigation.current().screen,
            Screen::Songs(group.filter())
        );
        assert!(!state.supports_list_shuffle());
    }
}

#[test]
fn top_tracks_are_ranked_within_exact_artist_or_album_membership() {
    let mut state = fixture();
    open_artist(&mut state, "Artist A");
    activate_title(&mut state, "Top Tracks");
    assert_eq!(detail(&state).1, StatisticsDetailPage::Tracks);
    assert_eq!(play_ids(&state), [0, 5, 1]);
    let entries = state.statistics_entries();
    assert_eq!(
        entries.iter().map(|entry| entry.rank).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.value.as_str())
            .collect::<Vec<_>>(),
        ["5 plays", "3 plays", "2 plays"]
    );
    let effects = activate_title(&mut state, "M Third");
    assert_loaded(&effects, 5);
    assert_eq!(state.queue.order, [0, 5, 1]);

    open_album(&mut state, "Shared", "Artist B");
    activate_title(&mut state, "Top Tracks");
    assert_eq!(play_ids(&state), [3]);
    assert_eq!(state.statistics_entries()[0].value, "4 plays");
}

#[test]
fn stale_snapshots_cannot_play_missing_or_unavailable_members() {
    let mut state = fixture();
    let artist = open_artist(&mut state, "Artist A");
    state.mark_track_available(&PathBuf::from("/music/z-first.flac"), false);
    state
        .tracks
        .retain(|track| track.path != std::path::Path::new("/music/a-second.flac"));
    let expected = BTreeSet::from([
        PathBuf::from("/music/m-zero.flac"),
        PathBuf::from("/music/u-third.flac"),
    ]);
    assert_eq!(
        state.statistics_detail_info().subtitle,
        "3 plays · 2 tracks"
    );
    activate_title(&mut state, "Top Tracks");
    for title in ["Z Opening", "A Second"] {
        let before = state.queue.order.clone();
        let effects = activate_title(&mut state, title);
        assert_no_audio(&effects);
        assert_eq!(state.queue.order, before);
        assert_eq!(detail(&state).1, StatisticsDetailPage::Tracks);
        assert!(matches!(
            state.rows()[state.navigation.selected()].target,
            Target::None
        ));
    }
    state.apply(Action::Back);
    let effects = activate_title(&mut state, "Play Artist");
    assert!(
        matches!(effects.as_slice(), [Effect::Audio(AudioCommand::Load { path, .. })] if path == &PathBuf::from("/music/u-third.flac"))
    );
    let queued_paths: BTreeSet<_> = state
        .queue
        .order
        .iter()
        .map(|id| state.tracks[*id].path.clone())
        .collect();
    assert_eq!(queued_paths, expected);

    state.navigation.open(Screen::StatisticsDetail {
        group: artist.clone(),
        page: StatisticsDetailPage::Summary,
    });
    for path in artist.paths.iter() {
        state.mark_track_available(path, false);
    }
    assert_eq!(
        state.statistics_detail_info().subtitle,
        "0 plays · 0 tracks"
    );
    assert!(matches!(state.rows()[0].target, Target::None));
    let queue = state.queue.order.clone();
    assert_no_audio(&activate_title(&mut state, "Play Artist"));
    assert_eq!(state.queue.order, queue);
    assert_eq!(detail(&state).1, StatisticsDetailPage::Summary);
}

#[test]
fn detail_navigation_restores_page_selection_and_keeps_different_group_contexts() {
    let mut state = fixture();
    let artist = open_artist(&mut state, "Artist A");
    activate_title(&mut state, "Albums");
    let album_name = state.rows()[0].title.clone();
    activate_title(&mut state, &album_name);
    let (album, _) = detail(&state);
    activate_title(&mut state, "Top Tracks");
    state.navigation.select(1);
    state.apply(Action::Back);
    assert_eq!(
        detail(&state),
        (album.clone(), StatisticsDetailPage::Summary)
    );
    assert_eq!(state.navigation.selected(), 1);
    state.apply(Action::Back);
    assert_eq!(
        detail(&state),
        (artist.clone(), StatisticsDetailPage::Albums)
    );
    assert_eq!(state.navigation.selected(), 0);
    state.apply(Action::Back);
    assert_eq!(detail(&state), (artist, StatisticsDetailPage::Summary));
    assert_eq!(state.navigation.selected(), 1);

    let other = open_album(&mut state, "Shared", "Artist B");
    state.navigation.open(Screen::StatisticsDetail {
        group: album.clone(),
        page: StatisticsDetailPage::Tracks,
    });
    state.navigation.select(2);
    let depth = state.navigation.depth();
    state.navigation.open(Screen::StatisticsDetail {
        group: other.clone(),
        page: StatisticsDetailPage::Tracks,
    });
    assert_eq!(state.navigation.depth(), depth + 1);
    assert_eq!(detail(&state), (other, StatisticsDetailPage::Tracks));
    state.apply(Action::Back);
    assert_eq!(detail(&state), (album, StatisticsDetailPage::Tracks));
    assert_eq!(state.navigation.selected(), 2);
}

#[test]
fn counts_and_representative_artwork_are_stable_when_library_order_changes() {
    let mut state = fixture();
    assert_eq!(state.statistics_counts(), (19, 2, 3, 5));
    let artist = open_artist(&mut state, "Artist A");
    let before = state.statistics_detail_info();
    assert_eq!(before.subtitle, "10 plays · 4 tracks");
    assert_eq!(
        state.tracks[before.artwork_id.unwrap()].path,
        PathBuf::from("/music/a-second.flac")
    );
    state.tracks.reverse();
    let after = state.statistics_detail_info();
    assert_eq!(before.title, after.title);
    assert_eq!(before.subtitle, after.subtitle);
    assert_eq!(
        state.tracks[after.artwork_id.unwrap()].path,
        PathBuf::from("/music/a-second.flac")
    );
    let no_art = state
        .tracks
        .iter_mut()
        .find(|track| track.path == std::path::Path::new("/music/a-second.flac"))
        .unwrap();
    no_art.artwork.clear();
    let fallback = state.statistics_artwork(&artist.paths).unwrap();
    assert_eq!(
        state.tracks[fallback].path,
        PathBuf::from("/music/m-zero.flac")
    );
    state.mark_track_available(&PathBuf::from("/music/m-zero.flac"), false);
    let available = state.statistics_artwork(&artist.paths).unwrap();
    assert_eq!(
        state.tracks[available].path,
        PathBuf::from("/music/u-third.flac")
    );
    assert_eq!(
        state.statistics_detail_info().subtitle,
        "10 plays · 3 tracks"
    );
}
