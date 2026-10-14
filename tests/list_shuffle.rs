use nano_music_player::{
    app::{
        action::Action,
        navigation::{Filter, Screen},
        state::{State, Target},
    },
    library::{Track, lyrics::Lyrics, playlists::Playlist},
};
use std::path::PathBuf;

fn fixture(screen: Screen) -> State {
    let mut state = State {
        tracks: [
            ("Zulu", "A", "One", 2),
            ("Shuffle Song", "A", "One", 1),
            ("Alpha", "B", "Two", 1),
        ]
        .into_iter()
        .enumerate()
        .map(|(id, (title, artist, album, number))| Track {
            path: PathBuf::from(format!("/{id}.wav")),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            number,
            genre: if id == 2 { "Jazz" } else { "Rock" }.into(),
            duration: 60.0,
            artwork: vec![],
            lyrics: Lyrics::default(),
        })
        .collect(),
        ..State::default()
    };
    state.playlists = vec![Playlist {
        id: 4,
        name: "Mix".into(),
        tracks: vec![
            state.tracks[2].path.clone(),
            PathBuf::from("/missing.wav"),
            state.tracks[0].path.clone(),
        ],
    }];
    state.navigation.open(screen);
    state
}

#[test]
fn shuffle_entry_plays_only_the_current_list_and_keeps_canonical_order() {
    for (screen, canonical) in [
        (Screen::Songs(Filter::All), vec![2, 1, 0]),
        (
            Screen::Songs(Filter::Album {
                name: "One".into(),
                artist: Some("A".into()),
            }),
            vec![1, 0],
        ),
        (Screen::Songs(Filter::Genre("Rock".into())), vec![1, 0]),
        (
            Screen::Songs(Filter::Playlist {
                id: 4,
                name: "Mix".into(),
            }),
            vec![2, 0],
        ),
        (Screen::Artist("A".into()), vec![1, 0]),
    ] {
        let mut state = fixture(screen.clone());
        state.query = "Shuffle Song".into();
        let shuffle_index = usize::from(matches!(screen, Screen::Songs(Filter::Playlist { .. })));
        assert!(matches!(
            state.rows()[shuffle_index].target,
            Target::Action(Action::ShuffleList)
        ));
        state.navigation.select(shuffle_index);
        let effects = state.apply(Action::Activate);
        assert!(!effects.is_empty());
        assert_eq!(state.navigation.current().screen, Screen::NowPlaying);
        assert!(state.shuffle);
        let mut actual = state.queue.order.clone();
        actual.sort_unstable();
        let mut expected = canonical.clone();
        expected.sort_unstable();
        assert_eq!(actual, expected, "scope {screen:?}");
        assert_eq!(state.queue.position, 0);
        assert_eq!(state.current, state.queue.current());
        state.apply(Action::ToggleShuffle);
        assert_eq!(state.queue.order, canonical, "unshuffle {screen:?}");
    }
}

#[test]
fn letter_jump_skips_shuffle_control_and_empty_shuffle_is_harmless() {
    let mut state = fixture(Screen::Songs(Filter::All));
    assert!(state.jump_to_letter('S'));
    assert_eq!(state.navigation.selected(), 2);
    assert!(matches!(state.rows()[2].target, Target::Play(1)));
    state.navigation.open(Screen::Songs(Filter::Playlist {
        id: 99,
        name: "Empty".into(),
    }));
    let previous_queue = state.queue.order.clone();
    assert_eq!(state.rows().len(), 2);
    assert!(state.apply(Action::ShuffleList).is_empty());
    assert!(!state.shuffle);
    assert_eq!(state.queue.order, previous_queue);
    assert!(matches!(
        state.navigation.current().screen,
        Screen::Songs(_)
    ));
    state.navigation.open(Screen::Settings);
    assert!(!state.supports_list_shuffle());
    assert!(state.apply(Action::ShuffleList).is_empty());
}
