use nano_music_player::app::{
    action::Action,
    navigation::{Filter, Group, Screen, SettingsSection},
    state::{Effect, State, Target},
};
use nano_music_player::input::bindings::{Binding, Key, Keymap, Modifiers};
use nano_music_player::library::{Track, lyrics::Lyrics, playlists::Playlist};
fn fixture() -> State {
    State {
        tracks: (0..3)
            .map(|id| Track {
                path: format!("/{id}.wav").into(),
                title: format!("Song {id}"),
                artist: "Artist".into(),
                album: "Album".into(),
                genre: "Rock".into(),
                number: id + 1,
                duration: 60.,
                artwork: vec![],
                lyrics: Lyrics::default(),
            })
            .collect(),
        ..State::default()
    }
}
#[test]
fn settings_have_five_clear_roots_and_nested_playback_controls() {
    let mut state = fixture();
    state.start(1);
    let queue = state.queue.order.clone();
    state.navigation.open(Screen::Settings);
    assert_eq!(
        state
            .rows()
            .iter()
            .map(|r| r.title.as_str())
            .collect::<Vec<_>>(),
        [
            "Display",
            "Playback",
            "Lyrics",
            "Language",
            "Keyboard Shortcuts"
        ]
    );
    state.navigation.select(1);
    state.apply(Action::Activate);
    assert_eq!(
        state.navigation.current().screen,
        Screen::SettingsSection(SettingsSection::Playback)
    );
    assert_eq!(state.rows().len(), 2);
    state.navigation.select(1);
    state.apply(Action::Activate);
    assert_eq!(
        state.navigation.current().screen,
        Screen::SettingsSection(SettingsSection::ReplayGain)
    );
    assert_eq!(state.rows().len(), 3);
    state.apply(Action::Back);
    state.apply(Action::Back);
    assert_eq!(state.navigation.current().screen, Screen::Settings);
    assert_eq!(state.navigation.selected(), 1);
    assert_eq!(state.current, Some(1));
    assert_eq!(state.queue.order, queue);
}
#[test]
fn photo_navigation_uses_all_songs_and_live_now_playing_title() {
    let mut state = fixture();
    state.start(1);
    state.navigation.open(Screen::Music);
    assert_eq!(state.rows()[0].subtitle, "Song 1");
    assert_eq!(
        state
            .rows()
            .iter()
            .take(7)
            .map(|r| r.title.as_str())
            .collect::<Vec<_>>(),
        [
            "Now Playing",
            "Genius Mixes",
            "Playlists",
            "Artists",
            "Albums",
            "Songs",
            "Genres"
        ]
    );
    state
        .unavailable_tracks
        .insert(state.tracks[2].path.clone());
    state.navigation.open(Screen::Groups(Group::Albums));
    assert_eq!(state.rows()[0].title, "All Songs");
    assert_eq!(state.rows()[0].subtitle, "2 songs");
    assert!(!state.supports_list_shuffle());
    state.apply(Action::Activate);
    assert_eq!(
        state.navigation.current().screen,
        Screen::Songs(Filter::All)
    );
    assert_eq!(state.rows().len(), 3);
    for screen in [
        Screen::Music,
        Screen::Search,
        Screen::Groups(Group::Albums),
        Screen::Groups(Group::Artists),
        Screen::Groups(Group::Genres),
        Screen::Groups(Group::Playlists),
    ] {
        state.navigation.open(screen);
        assert!(!state.supports_list_shuffle());
        assert!(
            !state
                .rows()
                .iter()
                .any(|r| matches!(r.target, Target::Action(Action::ShuffleList)))
        );
    }
}
#[test]
fn top_rated_uses_real_ratings_and_playlist_edit_targets_saved_playlists() {
    let mut state = fixture();
    state.ratings.insert(state.tracks[0].path.clone(), 4);
    state.ratings.insert(state.tracks[1].path.clone(), 5);
    state.ratings.insert(state.tracks[2].path.clone(), 3);
    state.playlists.push(Playlist {
        id: 7,
        name: "Workout Music".into(),
        tracks: vec![state.tracks[0].path.clone()],
    });
    state.navigation.open(Screen::Groups(Group::Playlists));
    assert_eq!(state.rows()[3].title, "My Top Rated");
    assert_eq!(state.rows()[3].subtitle, "2 songs");
    state.navigation.select(3);
    state.apply(Action::Activate);
    assert!(matches!(state.rows()[1].target, Target::Play(1)));
    assert!(matches!(state.rows()[2].target, Target::Play(0)));
    state.navigation.open(Screen::Groups(Group::Playlists));
    state.apply(Action::EditPlaylist);
    state.navigation.select(3);
    assert!(state.apply(Action::Activate).is_empty());
    state.navigation.select(5);
    assert!(matches!(
        state.apply(Action::Activate).as_slice(),
        [Effect::EditPlaylist { id: 7 }]
    ));
}
#[test]
fn global_shuffle_plays_available_library_from_any_list_and_migration_preserves_custom_chords() {
    let mut state = fixture();
    state
        .unavailable_tracks
        .insert(state.tracks[1].path.clone());
    state.navigation.open(Screen::Search);
    state.query = "Nothing".into();
    assert!(!state.apply(Action::ShuffleAll).is_empty());
    let mut ids = state.queue.order.clone();
    ids.sort();
    assert_eq!(ids, [0, 2]);
    assert_eq!(state.current, state.queue.current());
    assert!(state.shuffle);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("shortcuts.json");
    let custom = Binding {
        action: Action::NextTrack,
        key: Key::Letter('s'),
        modifiers: Modifiers {
            shift: true,
            ..Default::default()
        },
        optional_shift: false,
    };
    Keymap {
        version: 4,
        bindings: vec![custom.clone()],
    }
    .save(&file)
    .unwrap();
    let migrated = Keymap::load_or_create(&file).unwrap();
    assert_eq!(migrated.version, 6);
    assert_eq!(migrated.bindings[0], custom);
    assert!(
        !migrated
            .bindings
            .iter()
            .any(|b| b.action == Action::ShuffleAll)
    );
    assert!(
        migrated
            .bindings
            .iter()
            .any(|b| b.action == Action::ToggleTranslation)
    );
    Keymap {
        version: 6,
        bindings: vec![],
    }
    .save(&file)
    .unwrap();
    assert!(Keymap::load_or_create(&file).unwrap().bindings.is_empty());
}

#[test]
fn translation_shortcut_migration_preserves_custom_t_and_explicit_unassignment() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shortcuts.json");
    let custom = Binding {
        action: Action::NextTrack,
        key: Key::Letter('t'),
        modifiers: Modifiers::default(),
        optional_shift: false,
    };
    Keymap {
        version: 5,
        bindings: vec![custom.clone()],
    }
    .save(&path)
    .unwrap();
    let migrated = Keymap::load_or_create(&path).unwrap();
    assert_eq!(migrated.version, 6);
    assert_eq!(migrated.bindings, [custom]);
    let unassigned = Keymap::default()
        .edit(Action::ToggleTranslation, None, true)
        .unwrap();
    assert_eq!(unassigned.version, 6);
    unassigned.save(&path).unwrap();
    assert!(
        !Keymap::load_or_create(&path)
            .unwrap()
            .bindings
            .iter()
            .any(|b| b.action == Action::ToggleTranslation)
    );
}
