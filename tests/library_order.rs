use nano_music_player::{
    app::{
        action::Action,
        navigation::{Filter, Group, Screen},
        state::{State, Target},
    },
    library::{Track, lyrics::Lyrics},
};
use std::path::PathBuf;

fn track(title: &str, artist: &str, album: &str, number: u32, path: &str, artwork: bool) -> Track {
    Track {
        path: PathBuf::from(path),
        title: title.into(),
        artist: artist.into(),
        album: album.into(),
        genre: "Rock".into(),
        number,
        duration: 60.0,
        artwork: if artwork { vec![1] } else { vec![] },
        lyrics: Lyrics::default(),
    }
}
fn state(tracks: Vec<Track>, screen: Screen) -> State {
    let mut s = State {
        tracks,
        ..State::default()
    };
    s.navigation.open(screen);
    s
}
fn titles(s: &State) -> Vec<String> {
    s.rows()
        .into_iter()
        .filter(|r| !matches!(r.target, Target::Action(_)))
        .map(|r| r.title)
        .collect()
}

#[test]
fn group_and_song_rows_are_case_folded_stable_and_keep_track_indexes() {
    let tracks = vec![
        track("zulu", "zArtist", "zAlbum", 1, "/z", false),
        track("beta", "Alpha", "Shared", 2, "/b", false),
        track("Alpha", "alpha", "shared", 1, "/a", false),
        track("ALPHA", "Alpha", "Shared", 1, "/A", false),
    ];
    let artists = state(tracks.clone(), Screen::Groups(Group::Artists));
    assert_eq!(titles(&artists), ["Alpha", "alpha", "zArtist"]);
    let songs = state(tracks, Screen::Songs(Filter::All));
    let rows = songs.rows().into_iter().skip(1).collect::<Vec<_>>();
    assert_eq!(
        rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["ALPHA", "Alpha", "beta", "zulu"]
    );
    assert!(matches!(rows[0].target, Target::Play(3)));
    assert!(matches!(rows[1].target, Target::Play(2)));
}

#[test]
fn album_tracks_order_known_numbers_then_unknown_and_deterministic_ties() {
    let s = state(
        vec![
            track("unknown", "A", "Album", 0, "/u", false),
            track("zulu", "A", "Album", 2, "/z", false),
            track("beta", "A", "Album", 1, "/b", false),
            track("Alpha", "A", "Album", 1, "/a", false),
            track("ALPHA", "A", "Album", 1, "/A", false),
        ],
        Screen::Songs(Filter::Album {
            name: "Album".into(),
            artist: Some("A".into()),
        }),
    );
    let rows = s.rows();
    assert_eq!(
        rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["ALPHA", "Alpha", "beta", "zulu", "unknown"]
    );
    assert!(matches!(rows[0].target, Target::Play(4)));
}

#[test]
fn artist_album_and_now_playing_include_guest_tracks_in_album_order() {
    let mut s = state(
        vec![
            track("Finale", "Band", "Compilation", 3, "/finale", false),
            track("Guest", "Band, Guest", "Compilation", 2, "/guest", false),
            track("Opening", "Band", "Compilation", 1, "/opening", false),
            track("Other", "Guest", "Other Album", 1, "/other", false),
            track("Missing", "Guest", "Compilation", 4, "/missing", false),
        ],
        Screen::Artist("Band".into()),
    );
    s.mark_track_available(&PathBuf::from("/missing"), false);
    assert_eq!(titles(&s), ["Compilation"]);
    s.navigation.select(1); // Skip the artist shuffle control.
    s.apply(Action::Activate);
    assert_eq!(titles(&s), ["Opening", "Guest", "Finale"]);
    assert_eq!(s.selection_count(), 3);
    s.apply(Action::Activate);
    assert_eq!(s.queue.order, [2, 1, 0]);
    assert_eq!(s.current, Some(2));
    s.apply(Action::NextTrack);
    assert_eq!(s.current, Some(1));
    s.apply(Action::TrackList);
    assert_eq!(titles(&s), ["Opening", "Guest", "Finale"]);
    assert_eq!(s.rating_target().map(|t| t.title.as_str()), Some("Guest"));
}

#[test]
fn artist_shuffle_stays_with_the_artist_when_album_navigation_includes_guests() {
    let mut s = state(
        vec![
            track("Finale", "Band", "Compilation", 3, "/finale", false),
            track("Guest", "Band, Guest", "Compilation", 2, "/guest", false),
            track("Opening", "Band", "Compilation", 1, "/opening", false),
            track("Solo", "Band", "Solo Album", 1, "/solo", false),
        ],
        Screen::Artist("Band".into()),
    );
    s.apply(Action::ShuffleList);
    assert!(s.shuffle);
    s.queue.unshuffle();
    assert_eq!(s.queue.order, [2, 0, 3]);
}

#[test]
fn album_and_artist_rows_choose_deterministic_artwork_and_jump_preserves_track_indexes() {
    let tracks = vec![
        track("Later", "Artist", "album", 2, "/later", true),
        track("first", "Artist", "album", 1, "/first", false),
        track("Other", "Artist", "Album", 1, "/other", true),
        track("song", "Beta", "Bee", 1, "/bee", true),
    ];
    let albums = state(tracks.clone(), Screen::Groups(Group::Albums));
    let rows = albums.rows().into_iter().skip(1).collect::<Vec<_>>();
    assert_eq!(
        rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["Album", "album", "Bee"]
    );
    assert_eq!(rows[0].artwork_id, Some(2));
    assert_eq!(rows[1].artwork_id, Some(0));
    let artist = state(tracks.clone(), Screen::Artist("Artist".into()));
    let albums = artist.rows().into_iter().skip(1).collect::<Vec<_>>();
    assert_eq!(albums[0].artwork_id, Some(2));
    assert_eq!(albums[1].artwork_id, Some(0));

    let mut songs = state(tracks, Screen::Songs(Filter::All));
    assert!(songs.supports_letter_jump());
    assert!(songs.jump_to_letter('L'));
    assert_eq!(songs.navigation.selected(), 2);
    assert!(!songs.jump_to_letter('Q'));
    assert!(matches!(
        songs.rows()[songs.navigation.selected()].target,
        Target::Play(0)
    ));
    songs.navigation.open(Screen::Settings);
    assert!(!songs.supports_letter_jump());
    assert!(!songs.jump_to_letter('A'));
}

#[test]
fn reverse_insertion_keeps_display_order_and_path_ties() {
    let tracks = vec![
        track("Same", "A", "Album", 1, "/z", true),
        track("Same", "A", "Album", 1, "/a", true),
        track("beta", "B", "Bee", 2, "/b", false),
    ];
    let original = state(tracks.clone(), Screen::Songs(Filter::All));
    let mut reversed = tracks;
    reversed.reverse();
    let reverse = state(reversed, Screen::Songs(Filter::All));
    let paths = |s: &State| {
        s.rows()
            .into_iter()
            .filter(|r| !matches!(r.target, Target::Action(_)))
            .map(|r| match r.target {
                Target::Play(id) => s.tracks[id].path.clone(),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(paths(&original), paths(&reverse));
    assert_eq!(paths(&original)[1], PathBuf::from("/a"));
    let artist = state(original.tracks.clone(), Screen::Groups(Group::Artists));
    assert_eq!(artist.rows()[0].artwork_id, Some(1));
    assert_eq!(artist.rows()[1].artwork_id, None);
}
