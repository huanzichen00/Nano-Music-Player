use nano_music_player::{
    app::{
        action::Action,
        navigation::{Filter, Group, Screen},
        state::{State, Target},
    },
    library::{Track, lyrics::Lyrics, playlists::Playlist, statistics::SmartPlaylist},
};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc, time::Instant};

fn track(id: usize) -> Track {
    Track {
        path: PathBuf::from(format!("/library/{id:05}.mp3")),
        title: format!("Song {id:05}"),
        artist: format!("Artist {}", id % 5),
        album: format!("Album {}", id % 10),
        genre: if id.is_multiple_of(2) { "Rock" } else { "Jazz" }.into(),
        number: id as u32,
        duration: 60.0,
        artwork: Vec::new(),
        lyrics: Lyrics::default(),
    }
}

#[test]
fn large_merge_keeps_queue_slots_and_selection_while_updating_and_deduplicating_paths() {
    let mut state = State::default();
    state.merge_library((0..5_000).map(track).collect());
    state.navigation.open(Screen::Songs(Filter::All));
    state.navigation.select(1_235); // Shuffle precedes song 1234.
    state.current = Some(1_234);
    state.queue.set(vec![1_234, 3_000, 1_234], 1_234, false);
    let queue = state.queue.order.clone();
    let selected_path = state.tracks[1_234].path.clone();
    state
        .unavailable_tracks
        .insert(state.tracks[3_000].path.clone());

    let mut changed = track(1_234);
    changed.title = "Updated selected song".into();
    let mut duplicate_new = track(5_001);
    duplicate_new.title = "Last incoming value wins".into();
    let mut incoming: Vec<_> = (2_500..5_002).map(track).collect();
    incoming.extend([changed, duplicate_new]);
    state.merge_library(incoming);

    assert_eq!(state.tracks.len(), 5_002);
    assert_eq!(state.current, Some(1_234));
    assert_eq!(state.queue.order, queue);
    assert_eq!(state.tracks[1_234].path, selected_path);
    assert_eq!(state.tracks[1_234].title, "Updated selected song");
    assert_eq!(state.tracks[5_001].title, "Last incoming value wins");
    assert!(state.track_available(&state.tracks[3_000]));
    assert!(matches!(
        state.rows()[state.navigation.selected()].target,
        Target::Play(1_234)
    ));
}

#[test]
fn direct_public_track_edits_cannot_leave_a_stale_merge_index() {
    let mut state = State {
        tracks: vec![track(0), track(1)],
        ..State::default()
    };
    state.tracks[0].path = "/renamed.mp3".into();
    let mut renamed = track(8);
    renamed.path = "/renamed.mp3".into();
    state.merge_library(vec![renamed.clone(), track(0)]);
    assert_eq!(state.tracks.len(), 3);
    assert_eq!(state.tracks[0], renamed);
    assert_eq!(state.tracks[2].path, track(0).path);

    // Match the existing first-slot rule even for malformed public input.
    state.tracks.push(renamed.clone());
    renamed.title = "Only first duplicate is replaced".into();
    state.merge_library(vec![renamed.clone()]);
    assert_eq!(state.tracks[0].title, renamed.title);
    assert_ne!(state.tracks[3].title, renamed.title);
}

#[test]
fn lightweight_counts_match_rendered_rows_after_direct_state_changes() {
    let mut state = State {
        tracks: (0..40).map(track).collect(),
        ..State::default()
    };
    state.playlists = vec![Playlist {
        id: 7,
        name: "Saved".into(),
        tracks: vec![track(3).path, track(1).path, track(999).path, track(3).path],
    }];
    state.smart_playlists.insert(
        SmartPlaylist::RecentlyPlayed,
        vec![track(6).path, track(8).path, track(999).path],
    );
    state.ratings.insert(track(1).path, 4);
    state.ratings.insert(track(3).path, 5);
    let screens = vec![
        Screen::Songs(Filter::All),
        Screen::Songs(Filter::Album {
            name: "Album 1".into(),
            artist: None,
        }),
        Screen::Songs(Filter::Album {
            name: "Album 1".into(),
            artist: Some("Artist 1".into()),
        }),
        Screen::Songs(Filter::Genre("Rock".into())),
        Screen::Songs(Filter::TopRated),
        Screen::Songs(Filter::Playlist {
            id: 7,
            name: "Saved".into(),
        }),
        Screen::Songs(Filter::Playlist {
            id: 8,
            name: "Missing".into(),
        }),
        Screen::Songs(Filter::SmartPlaylist(SmartPlaylist::RecentlyPlayed)),
        Screen::Songs(Filter::SmartPlaylist(SmartPlaylist::NeverPlayed)),
        Screen::Songs(Filter::Statistics {
            name: "Selected".into(),
            album_order: false,
            paths: Arc::new(
                [track(1).path, track(6).path]
                    .into_iter()
                    .collect::<BTreeSet<_>>(),
            ),
        }),
        Screen::Songs(Filter::Statistics {
            name: "Album".into(),
            album_order: true,
            paths: Arc::new(
                [track(1).path, track(6).path]
                    .into_iter()
                    .collect::<BTreeSet<_>>(),
            ),
        }),
        Screen::Search,
        Screen::LyricSongs,
        Screen::Groups(Group::Artists),
        Screen::Groups(Group::Albums),
        Screen::Groups(Group::Genres),
        Screen::Groups(Group::Playlists),
        Screen::Artist("Artist 1".into()),
    ];
    for screen in screens {
        state.navigation.open(screen.clone());
        for query in ["", "  ARTIST 1  ", "jAzZ", "Song 00003", "missing"] {
            state.query = query.into();
            for changed in [false, true] {
                if changed {
                    state.unavailable_tracks.insert(track(3).path);
                    state.tracks[1].title = "Changed directly".into();
                    state.ratings.insert(track(2).path, 5);
                }
                assert_eq!(
                    state.selection_count(),
                    state.rows().len(),
                    "{screen:?}, query={query:?}"
                );
            }
        }
    }
}

#[test]
fn selection_moves_on_a_large_list_without_changing_the_projected_order() {
    let mut state = State {
        tracks: (0..5_000).rev().map(track).collect(),
        ..State::default()
    };
    state.navigation.open(Screen::Songs(Filter::All));
    for _ in 0..1_000 {
        assert!(state.apply(Action::SelectNext).is_empty());
    }
    assert_eq!(state.navigation.selected(), 1_000);
    let rows = state.rows();
    assert_eq!(rows[1_000].title, "Song 00999");
    assert!(matches!(rows[1_000].target, Target::Play(4_000)));
}

#[test]
#[ignore = "manual synthetic library merge and keyboard-navigation profile"]
fn profile_large_library_merge_and_navigation() {
    for count in [10_000, 20_000] {
        let mut state = State::default();
        let tracks = (0..count).map(track).collect();
        let start = Instant::now();
        state.merge_library(tracks);
        let merge = start.elapsed();
        state.navigation.open(Screen::Songs(Filter::All));
        let start = Instant::now();
        for _ in 0..100 {
            state.apply(Action::SelectNext);
        }
        let navigation = start.elapsed();
        let start = Instant::now();
        let rows = state.rows();
        eprintln!(
            "{count} tracks: merge={merge:?}, 100 selection moves={navigation:?}, sorted projection={:?}",
            start.elapsed()
        );
        assert_eq!(rows.len(), count + 1);
        assert_eq!(state.navigation.selected(), 100);
    }
}
