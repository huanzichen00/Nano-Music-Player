//! Mobile presentation of the same nano navigation used by the desktop app.
//!
//! The shared navigation owns history and selection. This adapter only builds
//! rows for features available on iOS and supplies their playback scope.

use crate::{
    library::{Track, playlists::Playlist},
    shared::{
        i18n::{Language, translate},
        navigation::Navigation as SharedNavigation,
        settings::{self, Target as Setting},
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

pub use crate::shared::navigation::{Filter, Group, Screen as Page, SettingsSection as Section};

const TRANSFER_PAGE: &str = "Computer Transfer";

#[derive(Debug, Clone, PartialEq)]
pub enum Selection {
    Open(Page),
    Play {
        track: usize,
        order: Rc<[usize]>,
        shuffle: bool,
    },
    Command(&'static str),
    Setting(Setting),
    EditPlaylist(i64),
    DeletePlaylist(i64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub title: String,
    pub subtitle: String,
    pub icon: i32,
    pub chevron: bool,
    pub show_artwork: bool,
    /// A playable song's index into the original library; menu rows use None.
    pub track: Option<usize>,
    pub action: Selection,
}

#[derive(Debug)]
pub struct Navigation {
    inner: SharedNavigation,
    language: Language,
    playlist_editing: bool,
}

impl Default for Navigation {
    fn default() -> Self {
        Self {
            inner: SharedNavigation::default(),
            language: Language::English,
            playlist_editing: false,
        }
    }
}

impl Navigation {
    pub fn set_language(&mut self, language: Language) {
        self.language = language;
    }

    pub fn playlist_editing(&self) -> bool {
        self.playlist_editing
    }

    pub fn toggle_playlist_editing(&mut self) {
        self.playlist_editing = !self.playlist_editing;
    }

    pub fn rename_playlist(&mut self, id: i64, name: &str) {
        self.inner.rename_playlist(id, name);
    }

    pub fn is_settings_list(&self) -> bool {
        matches!(
            self.current(),
            Page::Settings | Page::SettingsSection(_) | Page::Languages
        )
    }
    pub fn current(&self) -> &Page {
        &self.inner.current().screen
    }

    pub fn selected(&self) -> usize {
        self.inner.selected()
    }

    pub fn select(&mut self, selected: usize) {
        self.inner.select(selected);
    }

    pub fn screen(&self) -> i32 {
        match self.current() {
            Page::Home => 0,
            Page::Songs(_) | Page::Artist(_) => 1,
            Page::NowPlaying => 2,
            Page::Search => 3,
            Page::LyricSongs | Page::Lyrics(_) | Page::LyricsSearch(_) => 9,
            Page::Placeholder(name) if name == TRANSFER_PAGE => 6,
            Page::Settings | Page::SettingsSection(_) | Page::Languages => 5,
            Page::Music => 7,
            Page::Groups(_) => 8,
            _ => 4,
        }
    }

    pub fn title(&self) -> String {
        match self.current() {
            // Runtime metadata must remain untouched even if it happens to be
            // identical to an interface translation key such as "Music".
            Page::Artist(name)
            | Page::Songs(
                Filter::Album { name, .. } | Filter::Genre(name) | Filter::Playlist { name, .. },
            ) => name.clone(),
            _ => translate(self.language, self.current().title()),
        }
    }

    pub fn home(&mut self) {
        self.inner.home();
        self.playlist_editing = false;
    }

    pub fn back(&mut self) {
        self.inner.back();
    }

    pub fn open(&mut self, page: Page) {
        self.inner.open(page);
    }

    pub fn open_transfer(&mut self) {
        self.open(Page::Placeholder(TRANSFER_PAGE.into()));
    }

    /// Route the original two HomeView icon pages without exposing unfinished
    /// desktop features as working mobile controls.
    pub fn open_home(&mut self, page: i32, index: i32) {
        let target = match (page, index) {
            (0, 0) => Page::Music,
            (0, 1) => Page::Placeholder("Videos".into()),
            (0, 2) => Page::Placeholder("Fitness".into()),
            (0, 3) => Page::Placeholder("Podcasts".into()),
            (0, 4) => Page::Placeholder("Photos".into()),
            (0, 5) => Page::Placeholder("Radio".into()),
            (1, 0) => Page::Placeholder("Clock".into()),
            (1, 1) => Page::Settings,
            _ => return,
        };
        self.open(target);
    }

    pub fn rows(&self, tracks: &[Track], query: &str, playlists: &[Playlist]) -> Vec<Entry> {
        let mut rows = match self.current() {
            Page::Music => music_rows(),
            Page::Settings | Page::SettingsSection(_) | Page::Languages => {
                settings_rows(self.current())
            }
            Page::Groups(group @ (Group::Artists | Group::Albums | Group::Genres)) => {
                group_rows(tracks, group)
            }
            Page::Groups(Group::Playlists) => {
                playlist_rows(playlists, self.playlist_editing, self.language)
            }
            Page::Songs(Filter::Playlist { id, .. }) => playlist_songs(tracks, playlists, *id),
            Page::Songs(Filter::All | Filter::Album { .. } | Filter::Genre(_))
            | Page::Artist(_)
            | Page::Search => song_rows(tracks, self.current(), query),
            _ => Vec::new(),
        };
        if !matches!(
            self.current(),
            Page::Groups(Group::Artists | Group::Albums | Group::Genres)
        ) {
            for (index, row) in rows.iter_mut().enumerate() {
                if matches!(self.current(), Page::Groups(Group::Playlists)) && index >= 2 {
                    continue;
                }
                if row.track.is_none() {
                    row.title = translate(self.language, &row.title);
                    row.subtitle = translate(self.language, &row.subtitle);
                }
            }
        }
        rows
    }
}

fn row(title: &str, icon: i32, action: Selection) -> Entry {
    Entry {
        title: title.into(),
        subtitle: String::new(),
        icon,
        chevron: matches!(action, Selection::Open(_)),
        show_artwork: false,
        track: None,
        action,
    }
}

fn music_rows() -> Vec<Entry> {
    vec![
        row("Now Playing", 8, Selection::Open(Page::NowPlaying)),
        row(
            "Playlists",
            2,
            Selection::Open(Page::Groups(Group::Playlists)),
        ),
        row("Artists", 3, Selection::Open(Page::Groups(Group::Artists))),
        row("Albums", 4, Selection::Open(Page::Groups(Group::Albums))),
        row("Songs", 5, Selection::Open(Page::Songs(Filter::All))),
        row("Genres", 6, Selection::Open(Page::Groups(Group::Genres))),
        row("Genius Mixes", 1, Selection::Command("genius")),
        row("Search", 7, Selection::Open(Page::Search)),
        row("Import Music…", 0, Selection::Command("import")),
        row(TRANSFER_PAGE, 0, Selection::Command("computer-transfer")),
    ]
}

fn settings_rows(page: &Page) -> Vec<Entry> {
    use crate::shared::action::Action;
    let mut entries = settings::rows(page, None)
        .unwrap_or_default()
        .into_iter()
        // Keep the desktop menu definitions, exposing only wired mobile features.
        .filter(|row| {
            matches!(
                row.target,
                Setting::Screen(
                    Page::SettingsSection(
                        Section::Display
                            | Section::Playback
                            | Section::Lyrics
                            | Section::DisplayMode
                            | Section::ReplayGain
                            | Section::ReplayGainPreamp
                            | Section::Translation
                            | Section::TranslationLanguage
                    ) | Page::Languages
                        | Page::LyricSongs
                ) | Setting::Action(Action::ToggleSmoothSlides | Action::ToggleReplayGainClipping)
                    | Setting::Action(
                        Action::ToggleGaplessPlayback
                            | Action::ToggleTranslation
                            | Action::ToggleRomaji
                            | Action::ConfigureDeepSeek
                    )
                    | Setting::Language(_)
                    | Setting::DisplayMode(_)
                    | Setting::ReplayGain(_)
                    | Setting::ReplayGainPreamp(_)
                    | Setting::Translation(_)
            )
        })
        .map(|item| {
            let mut entry = row(&item.title, 0, Selection::Setting(item.target.clone()));
            entry.subtitle = match item.target {
                Setting::Screen(Page::SettingsSection(Section::Display)) => {
                    "Show the iPod body or screen only".into()
                }
                _ => item.subtitle,
            };
            entry.chevron = matches!(item.target, Setting::Screen(_));
            entry
        })
        .collect::<Vec<_>>();
    if *page == Page::SettingsSection(Section::Lyrics) {
        let mut lyrics = row(
            "Show Lyrics",
            0,
            Selection::Setting(Setting::Action(Action::ToggleLyrics)),
        );
        lyrics.subtitle = "Show or hide song lyrics".into();
        entries.push(lyrics);
    }
    if *page == Page::Settings {
        entries.extend([
            row("Import Music…", 0, Selection::Command("import")),
            row(TRANSFER_PAGE, 0, Selection::Command("computer-transfer")),
            row("Refresh Library", 0, Selection::Command("refresh")),
        ]);
    }
    entries
}

fn playlist_rows(playlists: &[Playlist], editing: bool, language: Language) -> Vec<Entry> {
    let mut rows = vec![
        row("Add Playlist", 0, Selection::Command("new-playlist")),
        row(
            if editing { "Done" } else { "Edit" },
            0,
            Selection::Command("toggle-playlist-editing"),
        ),
    ];
    let mut saved: Vec<_> = playlists.iter().collect();
    saved.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });
    rows.extend(saved.into_iter().map(|playlist| Entry {
        title: playlist.name.clone(),
        subtitle: format!("{}{}", playlist.tracks.len(), translate(language, " songs")),
        icon: 0,
        chevron: true,
        show_artwork: false,
        track: None,
        action: if editing {
            Selection::EditPlaylist(playlist.id)
        } else {
            Selection::Open(Page::Songs(Filter::Playlist {
                id: playlist.id,
                name: playlist.name.clone(),
            }))
        },
    }));
    rows
}

fn playlist_songs(tracks: &[Track], playlists: &[Playlist], id: i64) -> Vec<Entry> {
    let Some(playlist) = playlists.iter().find(|playlist| playlist.id == id) else {
        return Vec::new();
    };
    // Resolve paths on every rebuild because imports may have changed the
    // library indices; playlist order is independent from title sorting.
    let indices: BTreeMap<_, _> = tracks
        .iter()
        .enumerate()
        .map(|(id, track)| (&track.path, id))
        .collect();
    let order: Vec<_> = playlist
        .tracks
        .iter()
        .filter_map(|path| indices.get(path).copied())
        .collect();
    let order: Rc<[usize]> = order.into();
    let mut rows = vec![row("Edit Playlist", 0, Selection::EditPlaylist(id))];
    rows.push(row(
        "Shuffle",
        0,
        match order.first() {
            Some(&track) => Selection::Play {
                track,
                order: order.clone(),
                shuffle: true,
            },
            None => Selection::Command("playlist-empty"),
        },
    ));
    rows.extend(order.iter().map(|&id| Entry {
        title: tracks[id].title.clone(),
        subtitle: tracks[id].artist.clone(),
        icon: 0,
        chevron: false,
        show_artwork: false,
        track: Some(id),
        action: Selection::Play {
            track: id,
            order: order.clone(),
            shuffle: false,
        },
    }));
    rows.push(row("Delete Playlist", 0, Selection::DeletePlaylist(id)));
    rows
}

fn song_rows(tracks: &[Track], page: &Page, query: &str) -> Vec<Entry> {
    let order: Vec<_> = tracks
        .iter()
        .enumerate()
        .filter(|(_, track)| match page {
            Page::Search => track.matches(query),
            Page::Artist(name) => track.artist == *name,
            Page::Songs(Filter::Album { name, artist }) => {
                track.album == *name && artist.as_ref().is_none_or(|artist| track.artist == *artist)
            }
            Page::Songs(Filter::Genre(genre)) => track.genre == *genre,
            _ => true,
        })
        .map(|(index, _)| index)
        .collect();
    let Some(&first) = order.first() else {
        return Vec::new();
    };
    let order: Rc<[usize]> = order.into();
    let mut rows = Vec::with_capacity(order.len() + 1);
    rows.push(row(
        "Shuffle",
        0,
        Selection::Play {
            track: first,
            order: order.clone(),
            shuffle: true,
        },
    ));
    rows.extend(order.iter().map(|&index| Entry {
        title: tracks[index].title.clone(),
        subtitle: tracks[index].artist.clone(),
        icon: 0,
        chevron: false,
        show_artwork: false,
        track: Some(index),
        action: Selection::Play {
            track: index,
            order: order.clone(),
            shuffle: false,
        },
    }));
    rows
}

fn group_rows(tracks: &[Track], group: &Group) -> Vec<Entry> {
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, track) in tracks.iter().enumerate() {
        let name = match group {
            Group::Artists => track.artist.as_str(),
            Group::Albums => track.album.as_str(),
            Group::Genres => track.genre.as_str(),
            Group::Playlists => return Vec::new(),
        };
        groups.entry(name).or_default().push(index);
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by(|(left, _), (right, _)| {
        left.to_lowercase()
            .cmp(&right.to_lowercase())
            .then_with(|| left.cmp(right))
    });
    groups
        .into_iter()
        .map(|(name, indices)| {
            let page = match group {
                Group::Artists => Page::Artist(name.into()),
                Group::Albums => Page::Songs(Filter::Album {
                    name: name.into(),
                    artist: None,
                }),
                Group::Genres => Page::Songs(Filter::Genre(name.into())),
                Group::Playlists => unreachable!(),
            };
            let show_artwork = matches!(group, Group::Artists | Group::Albums);
            let subtitle = if *group == Group::Albums {
                indices
                    .iter()
                    .map(|&index| tracks[index].artist.as_str())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                String::new()
            };
            Entry {
                title: name.into(),
                subtitle,
                icon: 0,
                chevron: true,
                show_artwork,
                track: None,
                action: Selection::Open(page),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::lyrics::Lyrics;
    use std::path::PathBuf;

    fn track(title: &str, artist: &str, album: &str, genre: &str) -> Track {
        Track {
            path: PathBuf::from(format!("/{artist}/{title}.wav")),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            genre: genre.into(),
            number: 0,
            duration: 1.0,
            artwork: Vec::new(),
            lyrics: Lyrics::default(),
        }
    }

    #[test]
    fn song_rows_share_one_queue_scope_instead_of_copying_it_per_row() {
        let tracks = (0..1024)
            .map(|id| track(&format!("Song {id}"), "Artist", "Album", ""))
            .collect::<Vec<_>>();
        let rows = song_rows(&tracks, &Page::Songs(Filter::All), "");
        let Selection::Play { order: first, .. } = &rows[0].action else {
            panic!("shuffle row")
        };
        for row in &rows[1..] {
            let Selection::Play { order, .. } = &row.action else {
                panic!("song row")
            };
            assert!(Rc::ptr_eq(first, order));
        }
    }

    #[test]
    fn home_icons_open_music_settings_and_honest_feature_placeholders() {
        let mut navigation = Navigation::default();
        navigation.open_home(0, 0);
        assert_eq!(navigation.current(), &Page::Music);
        navigation.home();
        navigation.open_home(1, 1);
        assert_eq!(navigation.current(), &Page::Settings);
        navigation.home();
        navigation.open_home(0, 1);
        assert_eq!(navigation.current(), &Page::Placeholder("Videos".into()));
        assert_eq!(navigation.screen(), 4);
        navigation.open_home(7, -1);
        assert_eq!(navigation.current(), &Page::Placeholder("Videos".into()));
        navigation.open_transfer();
        assert_eq!(navigation.screen(), 6);
    }

    #[test]
    fn shared_history_keeps_selection_avoids_duplicate_pages_and_resets_at_home() {
        let mut navigation = Navigation::default();
        navigation.open(Page::Music);
        navigation.select(3);
        navigation.open(Page::Groups(Group::Albums));
        navigation.open(Page::Groups(Group::Albums));
        navigation.back();
        assert_eq!(navigation.current(), &Page::Music);
        assert_eq!(navigation.selected(), 3);
        navigation.open(Page::NowPlaying);
        navigation.home();
        navigation.back();
        assert_eq!(navigation.current(), &Page::Home);
        assert_eq!(navigation.selected(), 0);
    }

    #[test]
    fn grouped_albums_preserve_real_track_indices_and_include_each_artist() {
        let tracks = vec![
            track("Third", "Zebra", "z album", "Rock"),
            track("First", "Beta", "alpha", "Jazz"),
            track("Second", "Alpha", "alpha", "Rock"),
        ];
        let mut navigation = Navigation::default();
        navigation.open(Page::Groups(Group::Albums));
        let groups = navigation.rows(&tracks, "ignored query", &[]);
        assert_eq!(
            groups
                .iter()
                .map(|entry| entry.title.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "z album"]
        );
        assert_eq!(groups[0].subtitle, "Alpha, Beta");
        assert!(groups[0].show_artwork);
        assert!(groups.iter().all(|entry| entry.track.is_none()));
        let Selection::Open(album) = &groups[0].action else {
            panic!("album must open its song list")
        };
        navigation.open(album.clone());
        let songs = navigation.rows(&tracks, "Third", &[]);
        assert_eq!(
            songs.iter().filter_map(|row| row.track).collect::<Vec<_>>(),
            [1, 2]
        );
        navigation.open(Page::Artist("Alpha".into()));
        assert_eq!(
            navigation
                .rows(&tracks, "", &[])
                .iter()
                .filter_map(|row| row.track)
                .collect::<Vec<_>>(),
            [2]
        );
        navigation.open(Page::Songs(Filter::Genre("Rock".into())));
        assert_eq!(
            navigation
                .rows(&tracks, "", &[])
                .iter()
                .filter_map(|row| row.track)
                .collect::<Vec<_>>(),
            [0, 2]
        );
    }

    #[test]
    fn search_and_shuffle_use_only_visible_tracks_in_their_original_order() {
        let tracks = vec![
            track("Outside", "Elsewhere", "Other", "Rock"),
            track("Match two", "Artist", "Album", "Jazz"),
            track("Unrelated", "Elsewhere", "Other", "Rock"),
            track("Match one", "Artist", "Album", "Jazz"),
        ];
        let mut navigation = Navigation::default();
        navigation.open(Page::Search);
        let rows = navigation.rows(&tracks, "  MATCH  ", &[]);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].track, None);
        assert_eq!(
            rows[0].action,
            Selection::Play {
                track: 1,
                order: vec![1, 3].into(),
                shuffle: true
            }
        );
        assert_eq!(
            rows[2].action,
            Selection::Play {
                track: 3,
                order: vec![1, 3].into(),
                shuffle: false
            }
        );
        assert!(navigation.rows(&tracks, "no results", &[]).is_empty());
        navigation.open(Page::Songs(Filter::Album {
            name: "Music".into(),
            artist: None,
        }));
        assert_eq!(navigation.title(), "Music");
    }

    #[test]
    fn playlist_scope_uses_saved_order_after_library_reindex_and_empty_lists_stay_editable() {
        let tracks = vec![
            track("A", "Artist", "Album", "Genre"),
            track("B", "Artist", "Album", "Genre"),
        ];
        let mut playlists = vec![Playlist {
            id: 17,
            name: "Music".into(),
            tracks: vec![
                tracks[1].path.clone(),
                PathBuf::from("/missing.wav"),
                tracks[0].path.clone(),
            ],
        }];
        let mut navigation = Navigation::default();
        navigation.set_language(Language::SimplifiedChinese);
        navigation.open(Page::Groups(Group::Playlists));
        let groups = navigation.rows(&tracks, "", &playlists);
        assert_eq!(
            groups[2].title, "Music",
            "playlist names are metadata, not translation keys"
        );
        assert_eq!(groups[2].track, None);
        navigation.open(Page::Songs(Filter::Playlist {
            id: 17,
            name: "Music".into(),
        }));
        assert_eq!(navigation.title(), "Music");
        let rows = navigation.rows(&tracks, "", &playlists);
        assert_eq!(rows[0].action, Selection::EditPlaylist(17));
        assert_eq!(
            rows.iter().filter_map(|row| row.track).collect::<Vec<_>>(),
            [1, 0]
        );
        assert_eq!(
            rows[2].action,
            Selection::Play {
                track: 1,
                order: vec![1, 0].into(),
                shuffle: false
            }
        );
        assert_eq!(rows.last().unwrap().action, Selection::DeletePlaylist(17));
        playlists[0].tracks.clear();
        let empty = navigation.rows(&tracks, "", &playlists);
        assert!(empty.iter().all(|row| row.track.is_none()));
        assert_eq!(empty[0].action, Selection::EditPlaylist(17));
        navigation.back();
        navigation.toggle_playlist_editing();
        assert_eq!(
            navigation.rows(&tracks, "", &playlists)[2].action,
            Selection::EditPlaylist(17)
        );
    }
}
