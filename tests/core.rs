use nano_music_player::{
    app::{
        action::Action,
        config::Settings,
        navigation::{Filter, Group, Navigation, Screen},
        state::{Effect, State, Target},
    },
    audio::{PlaybackStatus, Repeat, engine::AudioCommand, queue::Queue},
    input::{Binding, Context, Dispatch, Dispatcher, Key, KeyPress, Keymap, Modifiers, Surface},
    library::{
        Track,
        database::Database,
        lyrics::{self, Line, Lyrics},
    },
};
use std::{fs, path::PathBuf};

fn primary() -> Modifiers {
    Modifiers {
        primary: true,
        ..Modifiers::default()
    }
}

fn context(surface: Surface) -> Context {
    Context {
        surface,
        text_input: false,
        reference_open: false,
        native_dialog: false,
    }
}

fn press(key: Key, modifiers: Modifiers, repeat: bool) -> KeyPress {
    KeyPress {
        key,
        modifiers,
        repeat,
    }
}

fn dispatch(key: Key, modifiers: Modifiers, repeat: bool, surface: Surface) -> Dispatch {
    Dispatcher::new(Keymap::default()).dispatch(&press(key, modifiers, repeat), context(surface))
}

fn track(id: usize, title: &str, artist: &str, album: &str, genre: &str) -> Track {
    Track {
        path: PathBuf::from(format!("/music/{id}.mp3")),
        title: title.to_owned(),
        artist: artist.to_owned(),
        album: album.to_owned(),
        genre: genre.to_owned(),
        number: (id + 1) as u32,
        duration: 120.0,
        artwork: Vec::new(),
        lyrics: Lyrics::default(),
    }
}

fn music_state() -> State {
    let mut state = State::default();
    state.merge_library(vec![
        track(0, "First", "Alice", "Shared", "Jazz"),
        track(1, "Second", "Alice", "Solo", "Rock"),
        track(2, "Third", "Bob", "Shared", "Jazz"),
    ]);
    state
}

fn assert_volume_effect(effects: &[Effect], expected: f32) {
    assert!(
        matches!(effects, [Effect::Audio(AudioCommand::Volume(value))] if (*value - expected).abs() < 0.00001)
    );
}

fn assert_seek_effect(effects: &[Effect], expected: f64) {
    assert!(
        matches!(effects, [Effect::Audio(AudioCommand::Seek(value))] if (*value - expected).abs() < 0.00001)
    );
}

#[test]
fn all_default_shortcuts_resolve_to_their_semantic_actions() {
    let plain = [
        (Key::Up, Action::SelectPrevious),
        (Key::Down, Action::SelectNext),
        (Key::Enter, Action::Activate),
        (Key::Escape, Action::Back),
        (Key::Backspace, Action::Back),
        (Key::Letter('h'), Action::Left),
        (Key::Left, Action::Left),
        (Key::Right, Action::Right),
        (Key::Space, Action::TogglePlay),
        (Key::Letter('m'), Action::ToggleMute),
        (Key::Letter('s'), Action::ToggleShuffle),
        (Key::Letter('r'), Action::CycleRepeat),
        (Key::Letter('n'), Action::NowPlaying),
        (Key::Letter('l'), Action::Right),
        (Key::Letter('y'), Action::ToggleLyrics),
    ];
    let modified = [
        (Key::Right, Action::NextTrack),
        (Key::Left, Action::PreviousTrack),
        (Key::Up, Action::VolumeUp),
        (Key::Down, Action::VolumeDown),
        (Key::Letter('o'), Action::ImportFolder),
        (Key::Letter('f'), Action::Search),
        (Key::Digit('1'), Action::DeviceMode),
        (Key::Digit('2'), Action::ScreenMode),
        (Key::Plus, Action::ZoomIn),
        (Key::Equals, Action::ZoomIn),
        (Key::Minus, Action::ZoomOut),
    ];
    for (key, action) in plain {
        assert_eq!(
            dispatch(key.clone(), Modifiers::default(), false, Surface::Home),
            Dispatch::Action(action),
            "{key:?}"
        );
    }
    for (key, action) in modified {
        assert_eq!(
            dispatch(key.clone(), primary(), false, Surface::Home),
            Dispatch::Action(action),
            "primary + {key:?}"
        );
    }
}

#[test]
fn slint_key_text_is_normalized_without_accepting_unknown_or_multiple_characters() {
    use slint::platform::Key as SlintKey;
    let special = [
        (SlintKey::UpArrow, Key::Up),
        (SlintKey::DownArrow, Key::Down),
        (SlintKey::LeftArrow, Key::Left),
        (SlintKey::RightArrow, Key::Right),
        (SlintKey::Return, Key::Enter),
        (SlintKey::Escape, Key::Escape),
        (SlintKey::Backspace, Key::Backspace),
    ];
    for (slint_key, expected) in special {
        assert_eq!(
            Key::from_slint(&char::from(slint_key).to_string()),
            Some(expected)
        );
    }
    for (text, expected) in [
        ("H", Key::Letter('h')),
        ("m", Key::Letter('m')),
        ("1", Key::Digit('1')),
        ("+", Key::Plus),
        ("=", Key::Equals),
        ("-", Key::Minus),
        (" ", Key::Space),
        ("\n", Key::Enter),
    ] {
        assert_eq!(Key::from_slint(text), Some(expected));
    }
    for text in ["", "Enter", "ab", "é", "你"] {
        assert_eq!(Key::from_slint(text), None, "{text:?}");
    }
}

#[test]
fn native_primary_modifier_and_other_modifiers_match_exactly() {
    let dispatcher = Dispatcher::new(Keymap::default());
    // Slint supplies the platform's Command/Control equivalent as `primary`.
    // Physical secondary, Alt and Shift must never accidentally trigger those bindings.
    for mask in 0..16 {
        let modifiers = Modifiers {
            primary: mask & 1 != 0,
            secondary: mask & 2 != 0,
            alt: mask & 4 != 0,
            shift: mask & 8 != 0,
        };
        assert_eq!(
            dispatcher.dispatch(
                &press(Key::Letter('m'), modifiers, false),
                context(Surface::Home)
            ),
            if mask == 0 {
                Dispatch::Action(Action::ToggleMute)
            } else {
                Dispatch::Ignored
            },
            "mute modifiers {modifiers:?}"
        );
        assert_eq!(
            dispatcher.dispatch(
                &press(Key::Letter('f'), modifiers, false),
                context(Surface::Home)
            ),
            if mask == 1 {
                Dispatch::Action(Action::Search)
            } else if mask == 0 || mask == 8 {
                Dispatch::Action(Action::JumpToLetter)
            } else {
                Dispatch::Ignored
            },
            "search modifiers {modifiers:?}"
        );
        assert_eq!(
            dispatcher.dispatch(&press(Key::Plus, modifiers, false), context(Surface::Home)),
            if mask == 1 || mask == 9 {
                Dispatch::Action(Action::ZoomIn)
            } else {
                Dispatch::Ignored
            },
            "plus modifiers {modifiers:?}"
        );
        assert_eq!(
            dispatcher.dispatch(
                &press(Key::Equals, modifiers, false),
                context(Surface::Home)
            ),
            if mask == 1 || mask == 9 {
                Dispatch::Action(Action::ZoomIn)
            } else {
                Dispatch::Ignored
            },
            "equals modifiers {modifiers:?}"
        );
    }
    assert_eq!(
        dispatch(
            Key::Minus,
            Modifiers {
                shift: true,
                ..primary()
            },
            false,
            Surface::Home
        ),
        Dispatch::Ignored
    );
}

#[test]
fn held_keys_repeat_movement_volume_and_seek_but_consume_toggle_and_activation_repeats() {
    for binding in Keymap::default().bindings {
        let expected = if binding.action.repeatable() {
            Dispatch::Action(match binding.action {
                Action::NavigateLeft => Action::Left,
                Action::NavigateRight => Action::Right,
                action => action,
            })
        } else {
            Dispatch::Consumed
        };
        assert_eq!(
            dispatch(binding.key.clone(), binding.modifiers, true, Surface::Home),
            expected,
            "repeat {:?}",
            binding.action
        );
    }
    for (surface, key, expected) in [
        (Surface::Library, Key::Right, Dispatch::Consumed),
        (Surface::Library, Key::Left, Dispatch::Consumed),
        (
            Surface::NowPlaying,
            Key::Left,
            Dispatch::Action(Action::SeekBackward),
        ),
        (
            Surface::NowPlaying,
            Key::Right,
            Dispatch::Action(Action::SeekForward),
        ),
        (
            Surface::NowPlayingVolume,
            Key::Left,
            Dispatch::Action(Action::VolumeDown),
        ),
        (
            Surface::NowPlayingVolume,
            Key::Right,
            Dispatch::Action(Action::VolumeUp),
        ),
    ] {
        assert_eq!(dispatch(key, Modifiers::default(), true, surface), expected);
    }
}

#[test]
fn left_and_right_follow_the_current_surface_without_remapping_track_shortcuts() {
    for (surface, left, right) in [
        (
            Surface::Home,
            Dispatch::Action(Action::Left),
            Dispatch::Action(Action::Right),
        ),
        (
            Surface::Library,
            Dispatch::Action(Action::Back),
            Dispatch::Action(Action::Activate),
        ),
        (
            Surface::Placeholder,
            Dispatch::Action(Action::Back),
            Dispatch::Consumed,
        ),
        (
            Surface::NowPlaying,
            Dispatch::Action(Action::SeekBackward),
            Dispatch::Action(Action::SeekForward),
        ),
        (
            Surface::NowPlayingVolume,
            Dispatch::Action(Action::VolumeDown),
            Dispatch::Action(Action::VolumeUp),
        ),
    ] {
        assert_eq!(
            dispatch(Key::Left, Modifiers::default(), false, surface),
            left
        );
        assert_eq!(
            dispatch(Key::Right, Modifiers::default(), false, surface),
            right
        );
        assert_eq!(
            dispatch(Key::Left, primary(), false, surface),
            Dispatch::Action(Action::PreviousTrack)
        );
        assert_eq!(
            dispatch(Key::Right, primary(), false, surface),
            Dispatch::Action(Action::NextTrack)
        );
    }
}

#[test]
fn text_editing_protects_all_shortcuts_and_only_plain_enter_escape_end_editing() {
    let dispatcher = Dispatcher::new(Keymap::default());
    for surface in [
        Surface::Home,
        Surface::Library,
        Surface::NowPlaying,
        Surface::NowPlayingVolume,
        Surface::Placeholder,
    ] {
        let editing = Context {
            text_input: true,
            reference_open: true,
            ..context(surface)
        };
        for binding in &dispatcher.keymap.bindings {
            for repeat in [false, true] {
                let expected = if binding.modifiers == Modifiers::default()
                    && matches!(binding.key, Key::Enter | Key::Escape)
                {
                    if repeat {
                        Dispatch::Consumed
                    } else {
                        Dispatch::Action(Action::FinishSearchEditing)
                    }
                } else {
                    Dispatch::Ignored
                };
                assert_eq!(
                    dispatcher.dispatch(
                        &press(binding.key.clone(), binding.modifiers, repeat),
                        editing
                    ),
                    expected,
                    "editing {:?}, repeat {repeat}, surface {surface:?}",
                    binding.action
                );
            }
        }
        for key in [Key::Enter, Key::Escape] {
            for modifiers in [
                primary(),
                Modifiers {
                    secondary: true,
                    ..Modifiers::default()
                },
                Modifiers {
                    shift: true,
                    ..Modifiers::default()
                },
                Modifiers {
                    alt: true,
                    ..Modifiers::default()
                },
            ] {
                assert_eq!(
                    dispatcher.dispatch(&press(key.clone(), modifiers, false), editing),
                    Dispatch::Ignored
                );
            }
        }
    }
    let unbound = Dispatcher::new(Keymap {
        version: 1,
        bindings: Vec::new(),
    });
    assert_eq!(
        unbound.dispatch(
            &press(Key::Enter, Modifiers::default(), false),
            Context {
                text_input: true,
                ..context(Surface::Library)
            }
        ),
        Dispatch::Action(Action::FinishSearchEditing)
    );
}

#[test]
fn native_dialogs_take_precedence_over_text_and_shortcut_reference() {
    let dispatcher = Dispatcher::new(Keymap::default());
    let modal = Context {
        text_input: true,
        reference_open: true,
        native_dialog: true,
        ..context(Surface::Library)
    };
    for binding in &dispatcher.keymap.bindings {
        for repeat in [false, true] {
            assert_eq!(
                dispatcher.dispatch(
                    &press(binding.key.clone(), binding.modifiers, repeat),
                    modal
                ),
                Dispatch::Ignored
            );
        }
    }
}

#[test]
fn shortcut_reference_closes_before_navigation_and_retains_global_controls() {
    let dispatcher = Dispatcher::new(Keymap::default());
    let reference = Context {
        reference_open: true,
        ..context(Surface::NowPlaying)
    };
    assert_eq!(
        dispatcher.dispatch(&press(Key::Escape, Modifiers::default(), false), reference),
        Dispatch::Action(Action::CloseShortcuts)
    );
    assert_eq!(
        dispatcher.dispatch(&press(Key::Escape, Modifiers::default(), true), reference),
        Dispatch::Consumed
    );
    for key in [
        Key::Up,
        Key::Down,
        Key::Enter,
        Key::Left,
        Key::Right,
        Key::Backspace,
        Key::Letter('h'),
    ] {
        assert_eq!(
            dispatcher.dispatch(&press(key, Modifiers::default(), false), reference),
            Dispatch::Consumed
        );
    }
    for (key, modifiers, action) in [
        (Key::Space, Modifiers::default(), Action::TogglePlay),
        (Key::Up, primary(), Action::VolumeUp),
        (Key::Right, primary(), Action::NextTrack),
        (Key::Letter('o'), primary(), Action::ImportFolder),
        (Key::Letter('f'), primary(), Action::Search),
    ] {
        assert_eq!(
            dispatcher.dispatch(&press(key, modifiers, false), reference),
            Dispatch::Action(action)
        );
    }
    let unbound = Dispatcher::new(Keymap {
        version: 1,
        bindings: Vec::new(),
    });
    assert_eq!(
        unbound.dispatch(&press(Key::Escape, Modifiers::default(), false), reference),
        Dispatch::Action(Action::CloseShortcuts)
    );
}

#[test]
fn configurable_keymaps_round_trip_and_replace_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config/shortcuts.json");
    let created = Keymap::load_or_create(&path).unwrap();
    assert_eq!(created.bindings, Keymap::default().bindings);
    assert_eq!(
        Keymap::load_or_create(&path).unwrap().bindings,
        created.bindings
    );
    let custom = Keymap {
        version: 1,
        bindings: vec![Binding {
            action: Action::TogglePlay,
            key: Key::Letter('p'),
            modifiers: Modifiers {
                alt: true,
                ..Modifiers::default()
            },
            optional_shift: false,
        }],
    };
    fs::write(&path, serde_json::to_vec_pretty(&custom).unwrap()).unwrap();
    let loaded = Keymap::load_or_create(&path).unwrap();
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.bindings, custom.bindings);
    let dispatcher = Dispatcher::new(loaded);
    assert_eq!(
        dispatcher.dispatch(
            &press(
                Key::Letter('p'),
                Modifiers {
                    alt: true,
                    ..Modifiers::default()
                },
                false
            ),
            context(Surface::Home)
        ),
        Dispatch::Action(Action::TogglePlay)
    );
    assert_eq!(
        dispatcher.dispatch(
            &press(Key::Space, Modifiers::default(), false),
            context(Surface::Home)
        ),
        Dispatch::Ignored
    );
}

#[test]
fn keymap_validation_rejects_ambiguous_shift_and_accepts_distinct_modifiers() {
    let plain = Binding {
        action: Action::TogglePlay,
        key: Key::Letter('p'),
        modifiers: Modifiers::default(),
        optional_shift: false,
    };
    let mut shifted = plain.clone();
    shifted.action = Action::ToggleMute;
    shifted.modifiers.shift = true;
    let valid = Keymap {
        version: 1,
        bindings: vec![plain.clone(), shifted.clone()],
    };
    assert!(valid.validate().is_ok());
    assert_eq!(
        Dispatcher::new(valid).dispatch(
            &press(Key::Letter('p'), shifted.modifiers, false),
            context(Surface::Home)
        ),
        Dispatch::Action(Action::ToggleMute)
    );
    for optional_first in [true, false] {
        let mut wildcard = plain.clone();
        wildcard.optional_shift = true;
        let bindings = if optional_first {
            vec![wildcard, shifted.clone()]
        } else {
            vec![shifted.clone(), wildcard]
        };
        assert!(
            Keymap {
                version: 1,
                bindings
            }
            .validate()
            .unwrap_err()
            .contains("Conflicting shortcut")
        );
    }
    assert!(
        Keymap {
            version: 1,
            bindings: vec![plain.clone(), plain.clone()]
        }
        .validate()
        .is_err()
    );
    let distinct = Binding {
        modifiers: primary(),
        ..plain.clone()
    };
    assert!(
        Keymap {
            version: 1,
            bindings: vec![plain, distinct]
        }
        .validate()
        .is_ok()
    );
}

#[test]
fn invalid_shortcut_configuration_is_reported_without_overwriting_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("shortcuts.json");
    for text in [
        "not json",
        r#"{"version":99,"bindings":[]}"#,
        r#"{"version":1,"bindings":[],"typo":true}"#,
        r#"{"version":1,"bindings":[{"action":"toggle_play","key":{"letter":"P"}}]}"#,
        r#"{"version":1,"bindings":[{"action":"toggle_play","key":{"digit":"x"}}]}"#,
        r#"{"version":1,"bindings":[{"action":"unknown_action","key":"space"}]}"#,
        r#"{"version":1,"bindings":[{"action":"toggle_play","key":"space","modifiers":{"ctrl":true}}]}"#,
    ] {
        fs::write(&path, text).unwrap();
        assert!(Keymap::load_or_create(&path).is_err(), "accepted {text}");
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }
}

#[test]
fn shortcut_labels_explain_both_platforms_and_action_names_round_trip() {
    let binding = Binding {
        action: Action::NextTrack,
        key: Key::Right,
        modifiers: Modifiers {
            primary: true,
            secondary: true,
            alt: true,
            shift: true,
        },
        optional_shift: false,
    };
    assert_eq!(binding.label(true), "⌘ + Ctrl + Option + Shift + →");
    assert_eq!(binding.label(false), "Ctrl + Super + Alt + Shift + →");
    for binding in Keymap::default().bindings {
        let serialized = serde_json::to_value(binding.action).unwrap();
        assert_eq!(
            Action::parse(serialized.as_str().unwrap()),
            Some(binding.action)
        );
        assert!(!binding.action.title().is_empty());
    }
    assert_eq!(Action::parse("not_an_action"), None);
}

#[test]
fn navigation_restores_each_parent_selection_and_never_pops_home() {
    let mut navigation = Navigation::default();
    navigation.select(4);
    navigation.open(Screen::Music);
    navigation.select(2);
    navigation.open(Screen::Groups(Group::Artists));
    navigation.select(1);
    navigation.open(Screen::Groups(Group::Artists));
    assert_eq!(
        navigation.selected(),
        1,
        "opening the current screen preserves its selection"
    );
    navigation.back();
    assert_eq!(navigation.current().screen, Screen::Music);
    assert_eq!(navigation.selected(), 2);
    navigation.back();
    assert_eq!(navigation.current().screen, Screen::Home);
    assert_eq!(navigation.selected(), 4);
    navigation.back();
    assert_eq!(navigation.current().screen, Screen::Home);
    assert_eq!(navigation.selected(), 4);
    navigation.open(Screen::NowPlaying);
    navigation.home();
    assert_eq!(navigation.current().screen, Screen::Home);
    assert_eq!(navigation.selected(), 0);
}

#[test]
fn empty_lists_and_a_library_refresh_keep_selection_valid() {
    let mut state = State::default();
    state.navigation.open(Screen::Songs(Filter::All));
    assert_eq!(state.selection_count(), 0);
    for action in [Action::SelectPrevious, Action::SelectNext, Action::Activate] {
        assert!(state.apply(action).is_empty());
        assert_eq!(state.navigation.selected(), 0);
    }
    assert_eq!(state.current, None);
    state = music_state();
    state.navigation.open(Screen::Search);
    state.navigation.select(2);
    state.query = "First".to_owned();
    state.merge_library(Vec::new());
    assert_eq!(state.navigation.selected(), 0);
    assert_eq!(state.selection_count(), 1);
    state.query = "nothing matches".to_owned();
    state.merge_library(Vec::new());
    assert_eq!(state.selection_count(), 0);
    assert_eq!(state.navigation.selected(), 0);
    assert!(state.apply(Action::Activate).is_empty());
}

#[test]
fn library_merge_updates_metadata_without_invalidating_current_or_queue_indexes() {
    let mut state = music_state();
    state.queue.set(vec![2, 0, 1], 2, false);
    state.start(2);
    let mut changed = state.tracks[2].clone();
    changed.title = "Updated".to_owned();
    state.merge_library(vec![track(3, "Added", "Carol", "New", "Folk"), changed]);
    assert_eq!(state.tracks.len(), 4);
    assert_eq!(state.current, Some(2));
    assert_eq!(state.queue.current(), Some(2));
    assert_eq!(state.tracks[2].title, "Updated");
    assert_eq!(state.queue.order, [2, 0, 1]);
}

#[test]
fn library_hierarchy_filters_same_named_albums_by_artist_and_searches_metadata() {
    let mut state = music_state();
    state.navigation.open(Screen::Groups(Group::Artists));
    assert_eq!(
        state
            .rows()
            .iter()
            .map(|row| row.title.as_str())
            .collect::<Vec<_>>(),
        ["Alice", "Bob"]
    );
    state.apply(Action::Activate);
    assert_eq!(
        state.navigation.current().screen,
        Screen::Artist("Alice".to_owned())
    );
    assert_eq!(
        state
            .rows()
            .iter()
            .map(|row| row.title.as_str())
            .collect::<Vec<_>>(),
        ["Shared", "Solo"]
    );
    state.apply(Action::Activate);
    assert_eq!(state.rows().len(), 1);
    assert!(matches!(state.rows()[0].target, Target::Play(0)));
    state.navigation.open(Screen::Songs(Filter::Album {
        name: "Shared".to_owned(),
        artist: None,
    }));
    assert_eq!(state.rows().len(), 2);
    state
        .navigation
        .open(Screen::Songs(Filter::Genre("Jazz".to_owned())));
    assert_eq!(state.rows().len(), 2);
    state.navigation.open(Screen::Search);
    for (query, count) in [
        (" ALICE ", 2),
        ("shared", 2),
        ("JAZZ", 2),
        ("third", 1),
        ("missing", 0),
        ("", 3),
    ] {
        state.query = query.to_owned();
        assert_eq!(state.rows().len(), count, "query {query:?}");
    }
}

#[test]
fn activation_builds_a_queue_from_visible_results_and_back_restores_selection() {
    let mut state = music_state();
    state
        .navigation
        .open(Screen::Songs(Filter::Genre("Jazz".to_owned())));
    state.navigation.select(1);
    let effects = state.apply(Action::Activate);
    assert_eq!(state.current, Some(2));
    assert_eq!(state.queue.order, [0, 2]);
    assert_eq!(state.queue.current(), Some(2));
    assert_eq!(state.navigation.current().screen, Screen::NowPlaying);
    assert_eq!(state.navigation.selected(), 1);
    assert_eq!(state.status, PlaybackStatus::Loading);
    assert!(matches!(
        effects.as_slice(),
        [Effect::Audio(AudioCommand::Load {
            id: 2,
            paused: false,
            ..
        })]
    ));
    assert!(matches!(
        state.apply(Action::Back).as_slice(),
        [Effect::FocusScreen]
    ));
    assert_eq!(
        state.navigation.current().screen,
        Screen::Songs(Filter::Genre("Jazz".to_owned()))
    );
    assert_eq!(state.navigation.selected(), 1);
}

#[test]
fn context_shortcuts_apply_to_seek_and_volume_while_preserving_navigation() {
    let mut state = music_state();
    state.start(0);
    state.apply(Action::NowPlaying);
    state.position = 10.0;
    let keymap = Dispatcher::new(Keymap::default());
    for (key, expected_action, expected_position) in [
        (Key::Right, Action::SeekForward, 15.0),
        (Key::Left, Action::SeekBackward, 10.0),
    ] {
        let result = keymap.dispatch(
            &press(key, Modifiers::default(), false),
            context(state.surface()),
        );
        assert_eq!(result, Dispatch::Action(expected_action));
        assert_seek_effect(&state.apply(expected_action), expected_position);
    }
    state.navigation.select(3);
    assert_eq!(state.surface(), Surface::NowPlayingVolume);
    assert_eq!(
        keymap.dispatch(
            &press(Key::Right, Modifiers::default(), true),
            context(state.surface())
        ),
        Dispatch::Action(Action::VolumeUp)
    );
    assert_volume_effect(&state.apply(Action::VolumeUp), 0.55);
    assert_eq!(state.position, 10.0);
    assert_eq!(state.navigation.current().screen, Screen::NowPlaying);
}

#[test]
fn home_grid_directional_navigation_and_home_reset_keep_valid_selection() {
    let mut state = State::default();
    state.apply(Action::Right);
    assert_eq!(state.navigation.selected(), 1);
    state.apply(Action::SelectNext);
    assert_eq!(state.navigation.selected(), 3);
    state.apply(Action::SelectNext);
    state.apply(Action::SelectNext);
    assert_eq!(state.navigation.selected(), 5);
    state.apply(Action::Right);
    assert_eq!(state.home_page, 1);
    assert_eq!(state.navigation.selected(), 0);
    state.apply(Action::Left);
    assert_eq!(state.home_page, 0);
    assert_eq!(state.navigation.selected(), 1);
    state.apply(Action::Right);
    state.apply(Action::Home);
    assert_eq!(state.home_page, 0);
    assert_eq!(state.navigation.selected(), 0);
}

#[test]
fn volume_is_bounded_and_mute_preserves_the_level_until_adjustment() {
    let mut state = State::default();
    assert_volume_effect(&state.apply(Action::ToggleMute), 0.0);
    assert_eq!(state.volume, 0.5);
    assert!(state.muted);
    assert_volume_effect(&state.apply(Action::ToggleMute), 0.5);
    state.apply(Action::ToggleMute);
    assert_volume_effect(&state.apply(Action::VolumeUp), 0.55);
    assert!(!state.muted);
    for _ in 0..40 {
        state.apply(Action::VolumeUp);
    }
    assert_eq!(state.volume, 1.0);
    assert_volume_effect(&state.apply(Action::VolumeUp), 1.0);
    for _ in 0..40 {
        state.apply(Action::VolumeDown);
    }
    assert_eq!(state.volume, 0.0);
    assert_volume_effect(&state.apply(Action::VolumeDown), 0.0);
    state.set_volume(2.0);
    assert_eq!(state.volume, 1.0);
    state.set_volume(-1.0);
    assert_eq!(state.volume, 0.0);
}

#[test]
fn repeat_cycles_off_all_one_and_play_pause_does_not_require_an_audio_device() {
    let mut state = music_state();
    for expected in [Repeat::All, Repeat::One, Repeat::Off] {
        state.apply(Action::CycleRepeat);
        assert_eq!(state.repeat, expected);
    }
    let effects = state.apply(Action::TogglePlay);
    assert!(matches!(
        effects.as_slice(),
        [Effect::Audio(AudioCommand::Load {
            id: 0,
            paused: false,
            ..
        })]
    ));
    assert_eq!(state.queue.order, [0, 1, 2]);
    assert!(matches!(
        state.apply(Action::TogglePlay).as_slice(),
        [Effect::Audio(AudioCommand::Pause(true))]
    ));
    assert_eq!(state.status, PlaybackStatus::Paused);
    assert!(matches!(
        state.apply(Action::TogglePlay).as_slice(),
        [Effect::Audio(AudioCommand::Pause(false))]
    ));
    assert_eq!(state.status, PlaybackStatus::Playing);
    let mut empty = State::default();
    assert!(empty.apply(Action::TogglePlay).is_empty());
    assert_eq!(empty.current, None);
    assert!(empty.notice.contains("Import"));
}

#[test]
fn seek_and_previous_track_respect_duration_and_restart_threshold() {
    let mut state = music_state();
    state.queue.set(vec![0, 1, 2], 1, false);
    state.start(1);
    assert_seek_effect(&[state.seek(-10.0)], 0.0);
    assert_seek_effect(&[state.seek(999.0)], 120.0);
    assert_seek_effect(&state.apply(Action::SeekForward), 120.0);
    assert_seek_effect(&state.apply(Action::PreviousTrack), 0.0);
    assert_eq!(state.current, Some(1));
    state.position = 3.0;
    assert!(matches!(
        state.apply(Action::PreviousTrack).as_slice(),
        [Effect::Audio(AudioCommand::Load { id: 0, .. })]
    ));
    state.position = 1.0;
    assert_seek_effect(&state.apply(Action::SeekBackward), 0.0);
    let mut empty = State::default();
    assert!(empty.apply(Action::SeekForward).is_empty());
    assert_seek_effect(&[empty.seek(20.0)], 0.0);
}

#[test]
fn application_actions_emit_focus_resize_and_single_import_effects() {
    let mut state = State::default();
    assert!(matches!(
        state.apply(Action::ImportFolder).as_slice(),
        [Effect::Import]
    ));
    assert!(state.importing);
    assert!(state.apply(Action::ImportFolder).is_empty());
    assert!(matches!(
        state.apply(Action::ShowShortcuts).as_slice(),
        [Effect::FocusScreen]
    ));
    assert!(state.reference_open);
    assert!(matches!(
        state.apply(Action::Search).as_slice(),
        [Effect::FocusSearch]
    ));
    assert!(!state.reference_open);
    assert_eq!(state.navigation.current().screen, Screen::Search);
    assert!(matches!(
        state.apply(Action::FinishSearchEditing).as_slice(),
        [Effect::FocusScreen]
    ));
    assert!(matches!(
        state.apply(Action::ScreenMode).as_slice(),
        [Effect::Resize]
    ));
    assert!(!state.device_mode);
    assert!(matches!(
        state.apply(Action::DeviceMode).as_slice(),
        [Effect::Resize]
    ));
    assert!(state.device_mode);
    for _ in 0..30 {
        state.apply(Action::ZoomIn);
    }
    assert_eq!(state.zoom, 3.0);
    for _ in 0..30 {
        state.apply(Action::ZoomOut);
    }
    assert_eq!(state.zoom, 0.75);
    state.apply(Action::ToggleLyrics);
    assert!(state.lyrics_visible);
    assert_eq!(state.navigation.current().screen, Screen::NowPlaying);
    state.apply(Action::ToggleLyrics);
    assert!(!state.lyrics_visible);
}

#[test]
fn automatic_queue_end_stops_but_manual_next_and_repeat_all_wrap() {
    let mut queue = Queue::default();
    queue.set(vec![7, 4, 9], 7, false);
    assert_eq!(queue.advance(Repeat::Off, false), Some(4));
    assert_eq!(queue.advance(Repeat::Off, false), Some(9));
    assert_eq!(queue.advance(Repeat::Off, false), None);
    assert_eq!(queue.current(), Some(9));
    assert_eq!(queue.advance(Repeat::Off, true), Some(7));
    queue.set(vec![7, 4, 9], 9, false);
    assert_eq!(queue.advance(Repeat::All, false), Some(7));
    assert_eq!(queue.previous(), Some(9));
    assert_eq!(queue.previous(), Some(4));
}

#[test]
fn repeat_one_only_replays_automatic_completion_and_empty_queues_are_safe() {
    let mut queue = Queue::default();
    queue.set(vec![7, 4], 7, false);
    assert_eq!(queue.advance(Repeat::One, false), Some(7));
    assert_eq!(queue.advance(Repeat::One, true), Some(4));
    assert_eq!(queue.advance(Repeat::One, false), Some(4));
    assert_eq!(queue.advance(Repeat::One, true), Some(7));
    queue.set(vec![7], 7, false);
    assert_eq!(queue.advance(Repeat::Off, false), None);
    assert_eq!(queue.advance(Repeat::One, false), Some(7));
    assert_eq!(queue.advance(Repeat::Off, true), Some(7));
    queue.set(Vec::new(), 42, true);
    for repeat in [Repeat::Off, Repeat::All, Repeat::One] {
        assert_eq!(queue.advance(repeat, false), None);
        assert_eq!(queue.advance(repeat, true), None);
    }
    assert_eq!(queue.previous(), None);
    queue.unshuffle();
    assert_eq!(queue.current(), None);
}

#[test]
fn shuffle_and_unshuffle_preserve_the_playing_track_and_membership() {
    let mut queue = Queue::default();
    queue.set(vec![3, 0, 4, 1, 2], 4, true);
    for _ in 0..8 {
        assert_eq!(queue.current(), Some(4));
        let mut membership = queue.order.clone();
        membership.sort_unstable();
        assert_eq!(membership, [0, 1, 2, 3, 4]);
        queue.shuffle();
    }
    queue.unshuffle();
    assert_eq!(queue.order, [0, 1, 2, 3, 4]);
    assert_eq!(queue.current(), Some(4));
}

#[test]
fn shuffled_run_visits_every_track_with_repeat_off() {
    for _ in 0..16 {
        let mut queue = Queue::default();
        queue.set(vec![0, 1, 2, 3, 4], 3, true);
        let mut played = vec![queue.current().unwrap()];
        while let Some(track) = queue.advance(Repeat::Off, false) {
            played.push(track);
        }
        assert_eq!(played[0], 3);
        played.sort_unstable();
        assert_eq!(played, [0, 1, 2, 3, 4]);
    }
}

#[test]
fn state_completion_stops_at_duration_and_repeat_one_reloads_from_zero() {
    let mut state = music_state();
    state.queue.set(vec![0, 1], 1, false);
    state.start(1);
    state.position = 119.0;
    assert!(state.advance(false).is_empty());
    assert_eq!(state.status, PlaybackStatus::Stopped);
    assert_eq!(state.position, state.duration());
    assert_eq!(state.current, Some(1));
    assert!(matches!(
        state.advance(true).as_slice(),
        [Effect::Audio(AudioCommand::Load { id: 0, .. })]
    ));
    state.repeat = Repeat::One;
    state.position = 120.0;
    assert!(matches!(
        state.advance(false).as_slice(),
        [Effect::Audio(AudioCommand::Load { id: 0, .. })]
    ));
    assert_eq!(state.position, 0.0);
    assert!(matches!(
        state.advance(true).as_slice(),
        [Effect::Audio(AudioCommand::Load { id: 1, .. })]
    ));
}

#[test]
fn lyrics_parse_multiple_unsorted_timestamps_and_decimal_fraction_precisions() {
    let lyrics = Lyrics::parse(
        "\u{feff}[ar:Artist]\n[00:20.25][00:05.50]Repeated\n[00:03.5]Tenths\n[00:04.250]Milliseconds\n[01:00]Minute",
    );
    assert_eq!(
        lyrics.timed,
        [
            Line {
                at_ms: 3500,
                text: "Tenths".to_owned()
            },
            Line {
                at_ms: 4250,
                text: "Milliseconds".to_owned()
            },
            Line {
                at_ms: 5500,
                text: "Repeated".to_owned()
            },
            Line {
                at_ms: 20250,
                text: "Repeated".to_owned()
            },
            Line {
                at_ms: 60000,
                text: "Minute".to_owned()
            },
        ]
    );
    assert_eq!(
        lyrics.lines(),
        ["Tenths", "Milliseconds", "Repeated", "Repeated", "Minute"]
    );
    assert!(!lyrics.is_empty());
}

#[test]
fn lyrics_offsets_advance_or_delay_lines_and_clamp_before_track_start() {
    let early = Lyrics::parse("[offset:1500]\n[00:01.00]Start\n[00:02.00]Next");
    assert_eq!(
        early
            .timed
            .iter()
            .map(|line| line.at_ms)
            .collect::<Vec<_>>(),
        [0, 500]
    );
    assert_eq!(early.active(0.0), Some(0));
    assert_eq!(early.active(0.5), Some(1));
    let late = Lyrics::parse("[00:01.00]Delayed\n[offset:-250]");
    assert_eq!(late.timed[0].at_ms, 1250);
    assert_eq!(late.active(1.249), None);
    assert_eq!(late.active(1.250), Some(0));
}

#[test]
fn lyrics_strip_enhanced_word_timestamps_but_preserve_literal_angle_text() {
    let lyrics = Lyrics::parse(
        "[00:01.00]<00:01.00>Hello <00:01.50>world <stage>!\n[00:02.00]Literal <unfinished",
    );
    assert_eq!(
        lyrics.lines(),
        ["Hello world <stage>!", "Literal <unfinished"]
    );
}

#[test]
fn plain_empty_metadata_only_and_partially_malformed_lyrics_remain_usable() {
    for text in [
        "",
        "\u{feff}",
        " \n\t\n",
        "[ar:Artist]\n[ti:Title]\n[offset:250]",
    ] {
        let lyrics = Lyrics::parse(text);
        assert!(
            lyrics.is_empty(),
            "expected no lyrics for {text:?}: {lyrics:?}"
        );
        assert_eq!(lyrics.active(90.0), None);
    }
    let plain = Lyrics::parse(" First line \n\n Second line ");
    assert_eq!(plain.lines(), ["First line", "Second line"]);
    assert_eq!(plain.active(10.0), None);
    let partial = Lyrics::parse("[00:01.00]Good\n[00:xx]Malformed\n[00:02.00]Still good");
    assert_eq!(partial.lines(), ["Good", "Still good"]);
}

#[test]
fn lyric_highlight_handles_exact_boundaries_duplicate_timestamps_and_backward_seeks() {
    let lyrics = Lyrics {
        timed: vec![
            Line {
                at_ms: 1000,
                text: "First".to_owned(),
            },
            Line {
                at_ms: 2000,
                text: "Second".to_owned(),
            },
            Line {
                at_ms: 2000,
                text: "Same time".to_owned(),
            },
            Line {
                at_ms: 10000,
                text: "Last".to_owned(),
            },
        ],
        plain: Vec::new(),
    };
    for (position, expected) in [
        (-1.0, None),
        (0.0, None),
        (0.999, None),
        (1.0, Some(0)),
        (1.999, Some(0)),
        (2.0, Some(2)),
        (9.999, Some(2)),
        (10.0, Some(3)),
        (999.0, Some(3)),
        (1.5, Some(0)),
        (0.0, None),
    ] {
        assert_eq!(lyrics.active(position), expected, "position {position}");
    }
}

#[test]
fn sidecar_lyrics_support_utf8_uppercase_extension_and_missing_or_empty_files() {
    let directory = tempfile::tempdir().unwrap();
    let audio = directory.path().join("song.mp3");
    assert!(lyrics::sidecar(&audio).is_none());
    fs::write(audio.with_extension("LRC"), "\u{feff}[00:01.00]你好").unwrap();
    assert_eq!(lyrics::sidecar(&audio).unwrap().lines(), ["你好"]);
    fs::remove_file(audio.with_extension("LRC")).unwrap();
    fs::write(audio.with_extension("lrc"), " \n\n ").unwrap();
    assert!(lyrics::sidecar(&audio).is_none());
}

#[test]
fn sidecar_lyrics_decode_utf16_and_gbk_without_corrupting_text() {
    let directory = tempfile::tempdir().unwrap();
    let audio = directory.path().join("song.flac");
    let text = "[00:01.00]中文歌词";
    for little_endian in [true, false] {
        let mut bytes = if little_endian {
            vec![0xff, 0xfe]
        } else {
            vec![0xfe, 0xff]
        };
        for unit in text.encode_utf16() {
            bytes.extend(if little_endian {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        fs::write(audio.with_extension("lrc"), bytes).unwrap();
        assert_eq!(lyrics::sidecar(&audio).unwrap().lines(), ["中文歌词"]);
    }
    let (bytes, _, had_errors) = encoding_rs::GBK.encode(text);
    assert!(!had_errors);
    fs::write(audio.with_extension("lrc"), bytes).unwrap();
    assert_eq!(lyrics::sidecar(&audio).unwrap().lines(), ["中文歌词"]);
}

#[test]
fn cached_track_metadata_and_lyrics_round_trip_with_stamp_invalidation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache/library.sqlite");
    let database = Database::open(&path).unwrap();
    let mut track = track(0, "Title", "Artist", "Album", "Genre");
    track.lyrics = Lyrics {
        timed: vec![Line {
            at_ms: 1250,
            text: "Line".to_owned(),
        }],
        plain: Vec::new(),
    };
    database.save(&track, 17).unwrap();
    let cached = database.cached(&track.path, 17).unwrap();
    assert_eq!(cached.title, "Title");
    assert_eq!(cached.lyrics.timed, track.lyrics.timed);
    assert!(database.cached(&track.path, 18).is_none());
    track.title = "Updated".to_owned();
    database.save(&track, 18).unwrap();
    assert!(database.cached(&track.path, 17).is_none());
    drop(database);
    let reopened = Database::open(&path).unwrap();
    let loaded = reopened.load().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].title, "Updated");
    assert_eq!(loaded[0].lyrics.timed, track.lyrics.timed);
    let mut legacy = serde_json::to_value(&track).unwrap();
    legacy.as_object_mut().unwrap().remove("lyrics");
    assert!(
        serde_json::from_value::<Track>(legacy)
            .unwrap()
            .lyrics
            .is_empty()
    );
}

#[test]
fn settings_default_partial_and_atomic_saved_configuration_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config/settings.json");
    let default = Settings::load(&path).unwrap();
    assert_eq!(
        default.font,
        nano_music_player::app::typography::Font::Classic
    );
    assert_eq!(default.zoom, 1.25);
    assert!(default.device_mode);
    assert_eq!(default.window_width, None);
    let saved = Settings {
        language: nano_music_player::app::i18n::Language::English,
        font: nano_music_player::app::typography::Font::Classic,
        always_on_top: true,
        romaji_visible: false,
        zoom: 2.0,
        device_mode: false,
        window_width: Some(750.0),
        window_height: Some(900.0),
    };
    saved.save(&path).unwrap();
    let loaded = Settings::load(&path).unwrap();
    assert_eq!(loaded.zoom, 2.0);
    assert!(!loaded.romaji_visible);
    assert!(!loaded.device_mode);
    assert_eq!(loaded.window_width, Some(750.0));
    assert_eq!(loaded.window_height, Some(900.0));
    assert!(!path.with_extension("json.tmp").exists());
    fs::write(&path, r#"{"device_mode":false}"#).unwrap();
    let partial = Settings::load(&path).unwrap();
    assert_eq!(partial.zoom, 1.25);
    assert!(partial.romaji_visible);
    assert!(!partial.device_mode);
    assert_eq!(partial.window_height, None);
}

#[test]
fn invalid_settings_reject_unknown_fields_zoom_and_window_dimensions() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    for value in [
        serde_json::json!({"zoom": 0.5}),
        serde_json::json!({"zoom": 3.25}),
        serde_json::json!({"window_width": 179.0}),
        serde_json::json!({"window_width": 3001.0}),
        serde_json::json!({"window_height": 323.0}),
        serde_json::json!({"window_height": 3001.0}),
        serde_json::json!({"zom": 1.25}),
    ] {
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(Settings::load(&path).is_err(), "accepted {value}");
    }
    for value in [
        serde_json::json!({"zoom": 0.75, "window_width": 240.0, "window_height": 460.0}),
        serde_json::json!({"zoom": 3.0, "window_width": 3000.0, "window_height": 3000.0}),
    ] {
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(Settings::load(&path).is_ok());
    }
}

#[test]
fn yazi_letters_keep_navigation_meaning_across_every_surface() {
    for surface in [
        Surface::Home,
        Surface::Library,
        Surface::NowPlaying,
        Surface::NowPlayingVolume,
        Surface::Placeholder,
    ] {
        for (key, action) in [
            ('h', Action::Back),
            ('j', Action::SelectNext),
            ('k', Action::SelectPrevious),
            ('l', Action::Activate),
        ] {
            if surface == Surface::Placeholder && key == 'l' {
                assert_eq!(
                    dispatch(Key::Letter(key), Modifiers::default(), false, surface),
                    Dispatch::Consumed
                );
                continue;
            }
            assert_eq!(
                dispatch(Key::Letter(key), Modifiers::default(), false, surface),
                Dispatch::Action(if surface == Surface::Home {
                    match key {
                        'h' => Action::Left,
                        'l' => Action::Right,
                        _ => action,
                    }
                } else {
                    action
                })
            );
        }
    }
}

#[test]
fn shortcut_edits_validate_before_saving_and_restore_actions_without_stealing_keys() {
    let map = Keymap::default();
    let binding = Binding {
        action: Action::ToggleMute,
        key: Key::Function(8),
        modifiers: Modifiers::default(),
        optional_shift: false,
    };
    let edited = map.edit(Action::ToggleMute, Some(binding), true).unwrap();
    assert_eq!(edited.labels(Action::ToggleMute), "F8");
    assert_eq!(map.labels(Action::ToggleMute), "M");
    let stolen = Binding {
        action: Action::TogglePlay,
        key: Key::Letter('m'),
        modifiers: Modifiers::default(),
        optional_shift: false,
    };
    let changed = edited.edit(Action::TogglePlay, Some(stolen), true).unwrap();
    assert!(changed.restore_action(Action::ToggleMute).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shortcuts.json");
    edited.save(&path).unwrap();
    assert_eq!(
        Keymap::load_or_create(&path).unwrap().bindings,
        edited.bindings
    );
    assert!(!path.with_extension("json.tmp").exists());
    let mut invalid = edited.clone();
    invalid.bindings.push(invalid.bindings[0].clone());
    assert!(invalid.save(&path).is_err());
    assert_eq!(
        Keymap::load_or_create(&path).unwrap().bindings,
        edited.bindings
    );
}

#[test]
fn untouched_legacy_defaults_upgrade_but_custom_v1_maps_are_preserved() {
    let mut legacy = Keymap {
        version: 1,
        ..Keymap::default()
    };
    legacy.bindings.retain(|b| {
        b.action != Action::Settings
            && b.action != Action::JumpToLetter
            && !(b.action == Action::Home && b.modifiers.shift)
            && b.key != Key::Letter('y')
    });
    for binding in &mut legacy.bindings {
        if binding.key == Key::Letter('h') {
            binding.action = Action::Home;
        }
        if binding.key == Key::Letter('l') {
            binding.action = Action::ToggleLyrics;
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shortcuts.json");
    legacy.save(&path).unwrap();
    assert_eq!(
        Keymap::load_or_create(&path).unwrap().bindings,
        Keymap::default().bindings
    );
    legacy.bindings.pop();
    legacy.save(&path).unwrap();
    assert_eq!(
        Keymap::load_or_create(&path).unwrap().bindings,
        legacy.bindings
    );
}

#[test]
fn settings_and_album_track_list_are_real_navigation_screens() {
    let mut state = music_state();
    state.home_page = 1;
    state.navigation.select(1);
    state.apply(Action::Activate);
    assert_eq!(state.navigation.current().screen, Screen::Settings);
    state.apply(Action::Activate);
    assert_eq!(state.navigation.current().screen, Screen::Shortcuts);
    assert!(matches!(
        state.apply(Action::Activate).as_slice(),
        [Effect::EditShortcut(Action::SelectPrevious)]
    ));
    state.apply(Action::Back);
    assert_eq!(state.navigation.current().screen, Screen::Settings);
    state.start(0);
    state.apply(Action::NowPlaying);
    state.apply(Action::TrackList);
    assert_eq!(
        state.rows().len(),
        1,
        "same-named albums remain scoped to their artist"
    );
    state.apply(Action::Back);
    assert_eq!(state.navigation.current().screen, Screen::NowPlaying);
}

#[test]
fn old_settings_default_to_english_and_language_survives_restart() {
    use nano_music_player::app::{config::Settings, i18n::Language};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    std::fs::write(&path, r#"{"zoom":1.5,"device_mode":false}"#).unwrap();
    let mut settings = Settings::load(&path).unwrap();
    assert_eq!(settings.language, Language::English);
    for language in Language::ALL {
        settings.language = language;
        settings.save(&path).unwrap();
        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded.language, language);
        assert_eq!(loaded.zoom, 1.5);
        assert!(!loaded.device_mode);
    }
}

#[test]
fn music_genius_entry_opens_preview_without_changing_playback() {
    let mut state = music_state();
    state.navigation.open(Screen::Music);
    let entry = state
        .rows()
        .iter()
        .position(|row| matches!(row.target, Target::GeniusPreview))
        .unwrap();
    for current in [None, Some(0)] {
        state.current = current;
        state.status = PlaybackStatus::Paused;
        state.position = 23.5;
        state.navigation.select(entry);
        let queue = state.queue.current();
        assert!(matches!(
            state.apply(Action::Activate).as_slice(),
            [Effect::GeniusPreview]
        ));
        assert_eq!(state.navigation.current().screen, Screen::Music);
        assert_eq!(state.current, current);
        assert_eq!(state.status, PlaybackStatus::Paused);
        assert_eq!(state.position, 23.5);
        assert_eq!(state.queue.current(), queue);
    }
}

#[test]
fn font_choices_migrate_persist_and_keep_settings_navigation() {
    use nano_music_player::app::typography::Font;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    fs::write(&path, r#"{"zoom":1.5,"device_mode":true}"#).unwrap();
    let mut settings = Settings::load(&path).unwrap();
    assert_eq!(settings.font, Font::Classic);
    let mut state = music_state();
    state.navigation.open(Screen::Settings);
    let font_row = state
        .rows()
        .iter()
        .position(|row| matches!(row.target, Target::Screen(Screen::Fonts)))
        .unwrap();
    state.navigation.select(font_row);
    state.apply(Action::Activate);
    assert_eq!(state.navigation.current().screen, Screen::Fonts);
    for font in Font::ALL {
        state.navigation.select(font.index());
        assert!(matches!(
            state.apply(Action::Activate).as_slice(),
            [Effect::FontChanged]
        ));
        assert_eq!(state.font, font);
        settings.font = font;
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap().font, font);
    }
    state.apply(Action::Back);
    assert_eq!(state.navigation.current().screen, Screen::Settings);
    assert_eq!(state.navigation.selected(), font_row);
    state.apply(Action::Activate);
    assert_eq!(state.navigation.selected(), Font::JetBrainsMono.index());
    fs::write(&path, r#"{"font":"missing_font"}"#).unwrap();
    assert!(Settings::load(&path).is_err());
}

#[test]
fn always_on_top_defaults_migrates_and_toggles_without_changing_playback() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    assert!(Settings::load(&path).unwrap().always_on_top);
    fs::write(&path, r#"{"device_mode":false}"#).unwrap();
    let mut settings = Settings::load(&path).unwrap();
    assert!(
        settings.always_on_top,
        "old settings inherit the pinned default"
    );
    let mut state = music_state();
    state.start(1);
    state.status = PlaybackStatus::Paused;
    state.position = 23.5;
    state.navigation.open(Screen::Settings);
    let row = state
        .rows()
        .iter()
        .position(|row| matches!(row.target, Target::Action(Action::ToggleAlwaysOnTop)))
        .unwrap();
    state.navigation.select(row);
    let queue = state.queue.current();
    let mode = state.device_mode;
    for expected in [false, true] {
        assert!(matches!(
            state.apply(Action::Activate).as_slice(),
            [Effect::WindowPreferencesChanged]
        ));
        assert_eq!(state.always_on_top, expected);
        assert_eq!(state.current, Some(1));
        assert_eq!(state.status, PlaybackStatus::Paused);
        assert_eq!(state.position, 23.5);
        assert_eq!(state.queue.current(), queue);
        assert_eq!(state.device_mode, mode);
        assert_eq!(state.navigation.selected(), row);
        settings.always_on_top = expected;
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap().always_on_top, expected);
    }
}
