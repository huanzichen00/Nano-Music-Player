use super::{
    action::Action,
    i18n::Language,
    navigation::{Filter, Group, Navigation, Screen},
    typography::Font,
};
use crate::{
    audio::{PlaybackStatus, Repeat, engine::AudioCommand, queue::Queue},
    input::Surface,
    library::Track,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
};

#[derive(Clone)]
pub enum Target {
    None,
    RefreshStatistics,
    StatisticsRange(crate::library::statistics::StatisticsRange),
    Screen(Screen),
    Play(usize),
    PlayVideo(usize),
    Podcast(crate::podcast::Action),
    Action(Action),
    Shortcut(Action),
    Language(Language),
    Font(Font),
    GeniusPreview,
    DisplayMode(bool),
    Zoom(f32),
    ReplayGain(crate::audio::gain::GainMode),
    ReplayGainPreamp(f32),
    Translation(crate::translation::Command),
    FindLyrics(usize),
}

#[derive(Clone)]
pub struct Row {
    pub title: String,
    pub subtitle: String,
    pub artwork_id: Option<usize>,
    pub target: Target,
}

pub enum Effect {
    RefreshStatistics,
    Podcast(crate::podcast::Action),
    Audio(AudioCommand),
    Video(crate::video::engine::Command),
    ImportVideos,
    RefreshLibrary,
    Import,
    FocusSearch,
    FocusScreen,
    Resize,
    EditShortcut(Action),
    RestoreShortcuts,
    LanguageChanged,
    FontChanged,
    WindowPreferencesChanged,
    RomajiVisibilityChanged,
    ConfigureDeepSeek,
    Translation(crate::translation::Command),
    FindLyrics {
        id: usize,
    },
    GeniusPreview,
    NewPlaylist,
    EditPlaylist {
        id: i64,
    },
    GenerateRomaji {
        id: usize,
    },
    CancelRomaji,
    RemoveRomaji {
        id: usize,
    },
    EditLyrics {
        id: usize,
        command: crate::library::scanner::LyricsCommand,
    },
}

pub struct State {
    pub statistics: super::statistics_state::StatisticsState,
    pub podcasts: super::podcast_state::PodcastState,
    pub language: Language,
    pub font: Font,
    pub always_on_top: bool,
    pub smooth_slides: bool,
    pub navigation: Navigation,
    pub tracks: Vec<Track>,
    /// Paths temporarily hidden from library browsing; vector slots stay stable for queued IDs.
    pub unavailable_tracks: BTreeSet<PathBuf>,
    pub videos: Vec<crate::video::library::Video>,
    pub video_progress: crate::video::progress::WatchProgress,
    pub video_subtitles: crate::video::subtitles::SubtitleAssociations,
    pub video_active: bool,
    pub video_current: Option<usize>,
    pub video_generation: u64,
    pub video_position: f64,
    pub video_status: crate::video::Status,
    pub video_paused: bool,
    pub video_overlay: bool,
    pub video_importing: bool,
    pub playlists: Vec<crate::library::playlists::Playlist>,
    pub playlist_list_editing: bool,
    pub smart_playlists: HashMap<crate::library::statistics::SmartPlaylist, Vec<PathBuf>>,
    pub ratings: HashMap<PathBuf, u8>,
    pub ratings_ready: bool,
    /// The song whose rating is shown above its album track list.
    pub album_rating_path: Option<PathBuf>,
    pub gapless_playback: bool,
    pub replay_gain_mode: crate::audio::gain::GainMode,
    pub replay_gain_preamp_db: f32,
    pub replay_gain_prevent_clipping: bool,
    pub queue: Queue,
    pub current: Option<usize>,
    pub status: PlaybackStatus,
    pub paused: bool,
    pub position: f64,
    pub volume: f32,
    pub muted: bool,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub device_mode: bool,
    pub zoom: f32,
    pub home_page: usize,
    pub query: String,
    pub reference_open: bool,
    pub lyrics_visible: bool,
    pub overlay_visible: bool,
    pub romaji_visible: bool,
    pub translation_target: crate::translation::TargetLanguage,
    pub translation_visible: bool,
    pub lyrics_lookup_status: HashMap<PathBuf, String>,
    pub romaji: HashMap<PathBuf, crate::library::romaji::Romaji>,
    pub romaji_generating: Option<PathBuf>,
    pub importing: bool,
    pub library_refreshing: bool,
    pub notice: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            statistics: super::statistics_state::StatisticsState::default(),
            podcasts: super::podcast_state::PodcastState::default(),
            language: Language::English,
            font: Font::Classic,
            always_on_top: true,
            smooth_slides: true,
            navigation: Navigation::default(),
            tracks: Vec::new(),
            unavailable_tracks: BTreeSet::new(),
            videos: Vec::new(),
            video_progress: crate::video::progress::WatchProgress::default(),
            video_subtitles: crate::video::subtitles::SubtitleAssociations::default(),
            video_active: false,
            video_current: None,
            video_generation: 0,
            video_position: 0.0,
            video_status: crate::video::Status::Finished,
            video_paused: true,
            video_overlay: true,
            video_importing: false,
            playlists: Vec::new(),
            playlist_list_editing: false,
            smart_playlists: HashMap::new(),
            ratings: HashMap::new(),
            ratings_ready: false,
            album_rating_path: None,
            gapless_playback: true,
            replay_gain_mode: crate::audio::gain::GainMode::Off,
            replay_gain_preamp_db: 0.0,
            replay_gain_prevent_clipping: true,
            queue: Queue::default(),
            current: None,
            status: PlaybackStatus::Stopped,
            paused: false,
            position: 0.0,
            volume: 0.5,
            muted: false,
            shuffle: false,
            repeat: Repeat::Off,
            device_mode: true,
            zoom: 1.25,
            home_page: 0,
            query: String::new(),
            reference_open: false,
            lyrics_visible: false,
            overlay_visible: false,
            romaji_visible: true,
            translation_target: crate::translation::TargetLanguage::English,
            translation_visible: false,
            lyrics_lookup_status: HashMap::new(),
            romaji: HashMap::new(),
            romaji_generating: None,
            importing: false,
            library_refreshing: false,
            notice: "Import a music folder to begin".to_owned(),
        }
    }
}

impl State {
    pub fn surface(&self) -> Surface {
        match &self.navigation.current().screen {
            Screen::Home => Surface::Home,
            Screen::VideoPlaying if self.navigation.selected() == 3 => Surface::VideoPlayingVolume,
            Screen::VideoPlaying => Surface::VideoPlaying,
            Screen::NowPlaying | Screen::Podcasts(super::navigation::PodcastPage::Playing)
                if self.navigation.selected() == 3 =>
            {
                Surface::NowPlayingVolume
            }
            Screen::NowPlaying | Screen::Podcasts(super::navigation::PodcastPage::Playing) => {
                Surface::NowPlaying
            }
            Screen::Placeholder(_) => Surface::Placeholder,
            _ => Surface::Library,
        }
    }

    pub fn supports_list_shuffle(&self) -> bool {
        match self.navigation.current().screen {
            Screen::Songs(
                Filter::Album { .. }
                | Filter::Statistics {
                    album_order: true, ..
                },
            ) => false,
            Screen::Songs(_) | Screen::Artist(_) => true,
            _ => false,
        }
    }

    pub fn rows(&self) -> Vec<Row> {
        let mut rows = self.list_rows();
        if self.supports_list_shuffle() {
            rows.insert(
                0,
                Row {
                    title: "Shuffle".into(),
                    subtitle: String::new(),
                    artwork_id: None,
                    target: Target::Action(Action::ShuffleList),
                },
            );
        }
        if matches!(
            self.navigation.current().screen,
            Screen::Songs(Filter::Playlist { .. })
        ) {
            rows.insert(
                0,
                Row {
                    title: "Edit".into(),
                    subtitle: String::new(),
                    artwork_id: None,
                    target: Target::Action(Action::EditPlaylist),
                },
            );
        }
        rows
    }

    fn list_rows(&self) -> Vec<Row> {
        if let Screen::Statistics(page) = self.navigation.current().screen {
            return self.statistics_rows(page);
        }
        if let Some(rows) = self.podcast_rows() {
            return rows;
        }
        if let Some(rows) = self.settings_rows() {
            return rows;
        }
        let row = |title: &str, target| Row {
            title: title.to_owned(),
            subtitle: String::new(),
            artwork_id: None,
            target,
        };
        let to = |title: &str, screen| row(title, Target::Screen(screen));
        match &self.navigation.current().screen {
            Screen::Videos => {
                std::iter::once(row("Import Videos…", Target::Action(Action::ImportVideos)))
                    .chain(self.videos.iter().enumerate().map(|(id, video)| Row {
                        title: video.title.clone(),
                        subtitle: format!(
                            "{}:{:02} · {}",
                            video.duration as u64 / 60,
                            video.duration as u64 % 60,
                            video.video_codec
                        ),
                        artwork_id: None,
                        target: Target::PlayVideo(id),
                    }))
                    .collect()
            }
            Screen::Lyrics(id) => vec![
                Row {
                    title: "Import / Replace Lyrics…".into(),
                    subtitle: self
                        .tracks
                        .get(*id)
                        .map_or_else(String::new, |t| t.title.clone()),
                    artwork_id: None,
                    target: Target::Action(Action::ImportLyrics),
                },
                row("Remove Lyrics", Target::Action(Action::RemoveLyrics)),
                Row {
                    title: "Use Automatic Lyrics".into(),
                    subtitle: "From audio tags and nearby files".into(),
                    artwork_id: None,
                    target: Target::Action(Action::AutomaticLyrics),
                },
                row("Generate Romaji", Target::Action(Action::GenerateRomaji)),
                row(
                    "Cancel Romaji Generation",
                    Target::Action(Action::CancelRomaji),
                ),
                row("Remove Romaji", Target::Action(Action::RemoveRomaji)),
                row("Show / Hide Romaji", Target::Action(Action::ToggleRomaji)),
                to("Find Lyrics", Screen::LyricsSearch(*id)),
            ],
            Screen::LyricSongs => {
                let mut songs: Vec<_> = self
                    .tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, track)| self.track_available(track))
                    .collect();
                songs.sort_by_cached_key(|(_, track)| track_title_key(track));
                songs
                    .into_iter()
                    .map(|(id, track)| Row {
                        title: track.title.clone(),
                        subtitle: track.artist.clone(),
                        artwork_id: None,
                        target: Target::Screen(Screen::LyricsSearch(id)),
                    })
                    .collect()
            }
            Screen::LyricsSearch(id) => self
                .tracks
                .get(*id)
                .map(|track| {
                    vec![
                        Row {
                            title: track.title.clone(),
                            subtitle: track.artist.clone(),
                            artwork_id: None,
                            target: Target::Screen(Screen::LyricSongs),
                        },
                        Row {
                            title: "Find Lyrics Online".into(),
                            subtitle: self
                                .lyrics_lookup_status
                                .get(&track.path)
                                .cloned()
                                .unwrap_or_else(|| {
                                    if track.lyrics.is_empty() {
                                        "Search online sources"
                                    } else {
                                        "Lyrics are already available"
                                    }
                                    .into()
                                }),
                            artwork_id: None,
                            target: Target::FindLyrics(*id),
                        },
                        row(
                            "Cancel Lyric Search",
                            Target::Translation(crate::translation::Command::Cancel),
                        ),
                        Row {
                            title: "Lyrics for Track".into(),
                            subtitle: "Import, replace or choose file lyrics".into(),
                            artwork_id: None,
                            target: Target::Screen(Screen::Lyrics(*id)),
                        },
                        row("DeepSeek API…", Target::Action(Action::ConfigureDeepSeek)),
                    ]
                })
                .unwrap_or_default(),
            Screen::Shortcuts => Action::CUSTOMIZABLE
                .iter()
                .map(|action| Row {
                    title: action.title().into(),
                    subtitle: String::new(),
                    artwork_id: None,
                    target: Target::Shortcut(*action),
                })
                .chain(std::iter::once(row(
                    "Restore All Defaults",
                    Target::Action(Action::RestoreShortcuts),
                )))
                .collect(),
            Screen::Music => vec![
                Row {
                    title: "Now Playing".into(),
                    subtitle: if let Some(episode) = self.podcasts.current() {
                        episode.title.clone()
                    } else if self.video_active {
                        self.video_current
                            .and_then(|id| self.videos.get(id))
                            .map(|track| track.title.clone())
                            .unwrap_or_default()
                    } else {
                        self.current
                            .and_then(|id| self.tracks.get(id))
                            .map(|track| track.title.clone())
                            .unwrap_or_default()
                    },
                    artwork_id: None,
                    target: Target::Screen(Screen::NowPlaying),
                },
                row("Genius Mixes", Target::GeniusPreview),
                to("Playlists", Screen::Groups(Group::Playlists)),
                to("Artists", Screen::Groups(Group::Artists)),
                to("Albums", Screen::Groups(Group::Albums)),
                to("Songs", Screen::Songs(Filter::All)),
                to("Genres", Screen::Groups(Group::Genres)),
                row("Search", Target::Action(Action::Search)),
                row("Import Music…", Target::Action(Action::ImportFolder)),
            ],
            Screen::Groups(Group::Playlists) => {
                let mut playlists: Vec<_> = self.playlists.iter().collect();
                playlists.sort_by_cached_key(|p| (p.name.to_lowercase(), p.name.clone(), p.id));
                let mut rows = vec![
                    row("Add", Target::Action(Action::NewPlaylist)),
                    row("Edit", Target::Action(Action::EditPlaylist)),
                    row("Genius", Target::GeniusPreview),
                    Row {
                        title: "My Top Rated".into(),
                        subtitle: format!("{} songs", self.top_rated_ids().len()),
                        artwork_id: None,
                        target: Target::Screen(Screen::Songs(Filter::TopRated)),
                    },
                ];
                let smart_row = |kind: crate::library::statistics::SmartPlaylist| Row {
                    title: kind.label().into(),
                    subtitle: format!(
                        "{} songs",
                        self.smart_playlists.get(&kind).map_or(0, Vec::len)
                    ),
                    artwork_id: None,
                    target: Target::Screen(Screen::Songs(Filter::SmartPlaylist(kind))),
                };
                rows.push(smart_row(
                    crate::library::statistics::SmartPlaylist::RecentlyPlayed,
                ));
                rows.extend(playlists.into_iter().map(|playlist| Row {
                    title: playlist.name.clone(),
                    subtitle: format!(
                            "{} songs",
                            playlist
                                .tracks
                                .iter()
                                .filter(|path| self.tracks.iter().any(
                                    |track| &track.path == *path && self.track_available(track)
                                ))
                                .count()
                        ),
                    artwork_id: None,
                    target: Target::Screen(Screen::Songs(Filter::Playlist {
                        id: playlist.id,
                        name: playlist.name.clone(),
                    })),
                }));
                rows.extend(
                    crate::library::statistics::SmartPlaylist::ALL
                        .into_iter()
                        .filter(|kind| {
                            *kind != crate::library::statistics::SmartPlaylist::RecentlyPlayed
                        })
                        .map(smart_row),
                );
                rows
            }
            Screen::Groups(Group::Albums) => {
                let mut rows = self.group_rows(&Group::Albums, None);
                rows.insert(
                    0,
                    Row {
                        title: "All Songs".into(),
                        subtitle: format!(
                            "{} songs",
                            self.tracks
                                .iter()
                                .filter(|track| self.track_available(track))
                                .count()
                        ),
                        artwork_id: None,
                        target: Target::Screen(Screen::Songs(Filter::All)),
                    },
                );
                rows
            }
            Screen::Groups(group) => self.group_rows(group, None),
            Screen::Artist(artist) => self.group_rows(&Group::Albums, Some(artist)),
            Screen::Songs(Filter::TopRated) => self
                .top_rated_ids()
                .into_iter()
                .map(|id| Row {
                    title: self.tracks[id].title.clone(),
                    subtitle: self.tracks[id].artist.clone(),
                    artwork_id: None,
                    target: Target::Play(id),
                })
                .collect(),
            Screen::Songs(_) | Screen::Search => {
                if let Screen::Songs(Filter::Playlist { id, .. }) =
                    &self.navigation.current().screen
                {
                    let tracks_by_path: std::collections::HashMap<_, _> = self
                        .tracks
                        .iter()
                        .enumerate()
                        .filter(|(_, track)| self.track_available(track))
                        .map(|(id, track)| (&track.path, (id, track)))
                        .collect();
                    return self
                        .playlists
                        .iter()
                        .find(|playlist| playlist.id == *id)
                        .into_iter()
                        .flat_map(|playlist| &playlist.tracks)
                        .filter_map(|path| tracks_by_path.get(path))
                        .map(|(id, track)| Row {
                            title: track.title.clone(),
                            subtitle: track.artist.clone(),
                            artwork_id: None,
                            target: Target::Play(*id),
                        })
                        .collect();
                }
                if let Screen::Songs(Filter::SmartPlaylist(kind)) =
                    &self.navigation.current().screen
                {
                    let tracks_by_path: HashMap<_, _> = self
                        .tracks
                        .iter()
                        .enumerate()
                        .filter(|(_, track)| self.track_available(track))
                        .map(|(id, track)| (&track.path, (id, track)))
                        .collect();
                    return self
                        .smart_playlists
                        .get(kind)
                        .into_iter()
                        .flatten()
                        .filter_map(|path| tracks_by_path.get(path))
                        .map(|(id, track)| Row {
                            title: track.title.clone(),
                            subtitle: track.artist.clone(),
                            artwork_id: None,
                            target: Target::Play(*id),
                        })
                        .collect();
                }
                let is_album = matches!(
                    self.navigation.current().screen,
                    Screen::Songs(
                        Filter::Album { .. }
                            | Filter::Statistics {
                                album_order: true,
                                ..
                            }
                    )
                );
                let mut rows: Vec<Row> = self
                    .tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, track)| self.track_available(track))
                    .filter(|(_, track)| match &self.navigation.current().screen {
                        Screen::Search => track.matches(&self.query),
                        Screen::Songs(Filter::Album { name, artist }) => {
                            track.album == *name
                                && artist.as_ref().is_none_or(|a| &track.artist == a)
                        }
                        Screen::Songs(Filter::Genre(genre)) => &track.genre == genre,
                        Screen::Songs(Filter::Statistics { paths, .. }) => {
                            paths.contains(&track.path)
                        }
                        _ => true,
                    })
                    .map(|(id, track)| Row {
                        title: track.title.clone(),
                        subtitle: track.artist.clone(),
                        artwork_id: None,
                        target: Target::Play(id),
                    })
                    .collect();
                if is_album {
                    rows.sort_by(|a, b| {
                        let id = |row: &Row| match row.target {
                            Target::Play(id) => id,
                            _ => unreachable!(),
                        };
                        let a_id = id(a);
                        let b_id = id(b);
                        let a_track = &self.tracks[a_id];
                        let b_track = &self.tracks[b_id];
                        (
                            a_track.number == 0,
                            a_track.number,
                            a_track.title.to_lowercase(),
                            &a_track.title,
                            a_track.path.to_string_lossy(),
                        )
                            .cmp(&(
                                b_track.number == 0,
                                b_track.number,
                                b_track.title.to_lowercase(),
                                &b_track.title,
                                b_track.path.to_string_lossy(),
                            ))
                    });
                } else {
                    rows.sort_by(|a, b| {
                        let id = |row: &Row| match row.target {
                            Target::Play(id) => id,
                            _ => unreachable!(),
                        };
                        let a_track = &self.tracks[id(a)];
                        let b_track = &self.tracks[id(b)];
                        track_title_key(a_track).cmp(&track_title_key(b_track))
                    });
                }
                rows
            }
            _ => Vec::new(),
        }
    }

    fn group_rows(&self, group: &Group, artist: Option<&str>) -> Vec<Row> {
        let mut groups: BTreeMap<&str, GroupedTracks<'_>> = BTreeMap::new();
        for (id, track) in self.tracks.iter().enumerate() {
            if !self.track_available(track) {
                continue;
            }
            if artist.is_some_and(|artist| track.artist != artist) {
                continue;
            }
            let entry = groups.entry(group_name(track, group)).or_default();
            if *group == Group::Albums {
                entry.artists.insert(track.artist.as_str());
            }
            if matches!(group, Group::Artists | Group::Albums) && !track.artwork.is_empty() {
                let key = (
                    track.album.to_lowercase(),
                    track.album.as_str(),
                    track.title.to_lowercase(),
                    track.title.as_str(),
                    track.path.to_string_lossy(),
                );
                if entry
                    .artwork
                    .as_ref()
                    .is_none_or(|(_, current)| key < *current)
                {
                    entry.artwork = Some((id, key));
                }
            }
        }
        let mut rows: Vec<_> = groups
            .into_iter()
            .map(|(name, entry)| Row {
                title: name.to_owned(),
                subtitle: if *group == Group::Albums {
                    artist
                        .map(str::to_owned)
                        .unwrap_or_else(|| entry.artists.into_iter().collect::<Vec<_>>().join(", "))
                } else {
                    String::new()
                },
                artwork_id: entry.artwork.map(|(id, _)| id),
                target: Target::Screen(match group {
                    Group::Artists => Screen::Artist(name.to_owned()),
                    Group::Albums => Screen::Songs(Filter::Album {
                        name: name.to_owned(),
                        artist: artist.map(str::to_owned),
                    }),
                    _ => Screen::Songs(Filter::Genre(name.to_owned())),
                }),
            })
            .collect();
        sort_rows(&mut rows);
        rows
    }

    pub fn supports_letter_jump(&self) -> bool {
        matches!(
            self.navigation.current().screen,
            Screen::Groups(Group::Artists | Group::Albums | Group::Genres | Group::Playlists)
                | Screen::Artist(_)
                | Screen::Songs(_)
                | Screen::Search
        )
    }

    pub fn jump_to_letter(&mut self, letter: char) -> bool {
        if !letter.is_ascii_alphabetic() || !self.supports_letter_jump() {
            return false;
        }
        let rows = self.rows();
        if let Some(index) = rows.iter().position(|row| {
            !matches!(
                row.target,
                Target::Action(Action::ShuffleList | Action::EditPlaylist)
            ) && row
                .title
                .chars()
                .next()
                .is_some_and(|c| c.eq_ignore_ascii_case(&letter))
        }) {
            self.navigation.select(index);
            true
        } else {
            false
        }
    }

    fn start_shuffled(&mut self, ids: Vec<usize>) -> Vec<Effect> {
        use rand::seq::IndexedRandom;
        let Some(&start) = ids.choose(&mut rand::rng()) else {
            return Vec::new();
        };
        self.shuffle = true;
        self.queue.set(ids, start, true);
        self.navigation.open(Screen::NowPlaying);
        self.navigation.select(1);
        self.start(start)
    }

    fn top_rated_ids(&self) -> Vec<usize> {
        let mut ids: Vec<_> = self
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, track)| {
                self.track_available(track)
                    && self.ratings.get(&track.path).copied().unwrap_or(0) >= 4
            })
            .map(|(id, _)| id)
            .collect();
        ids.sort_by(|a, b| {
            self.ratings[&self.tracks[*b].path]
                .cmp(&self.ratings[&self.tracks[*a].path])
                .then_with(|| {
                    track_title_key(&self.tracks[*a]).cmp(&track_title_key(&self.tracks[*b]))
                })
        });
        ids
    }

    pub fn selection_count(&self) -> usize {
        match &self.navigation.current().screen {
            Screen::Home => {
                if self.home_page == 0 {
                    6
                } else {
                    3
                }
            }
            Screen::NowPlaying => 7,
            Screen::VideoPlaying => 5,
            Screen::Podcasts(super::navigation::PodcastPage::Playing) => 8,
            Screen::Groups(Group::Playlists) => {
                4 + crate::library::statistics::SmartPlaylist::ALL.len() + self.playlists.len()
            }
            Screen::Groups(group) => {
                self.tracks
                    .iter()
                    .filter(|track| self.track_available(track))
                    .map(|track| group_name(track, group))
                    .collect::<BTreeSet<_>>()
                    .len()
                    + usize::from(*group == Group::Albums)
            }
            Screen::Artist(artist) => {
                self.tracks
                    .iter()
                    .filter(|track| self.track_available(track))
                    .filter(|track| &track.artist == artist)
                    .map(|track| track.album.as_str())
                    .collect::<BTreeSet<_>>()
                    .len()
                    + 1
            }
            _ => self.rows().len(),
        }
    }

    pub fn merge_library(&mut self, tracks: Vec<Track>) {
        let selected_path = self.selected_track_path();
        // Existing indexes remain stable for in-flight audio events and the queue.
        for track in tracks {
            self.unavailable_tracks.remove(&track.path);
            if let Some(existing) = self.tracks.iter_mut().find(|t| t.path == track.path) {
                *existing = track;
            } else {
                self.tracks.push(track);
            }
        }
        self.restore_selection(selected_path);
    }

    pub fn track_available(&self, track: &Track) -> bool {
        !self.unavailable_tracks.contains(&track.path)
    }

    /// Startup refreshes must not revive deleted files or overwrite a newer
    /// import/manual lyric edit. Existing queue and playback indexes stay fixed.
    pub fn refresh_cached_track(&mut self, original: &Track, track: Track) -> bool {
        if original.path != track.path || !self.track_available(original) {
            return false;
        }
        if !self.tracks.iter().any(|current| current == original) {
            return false;
        }
        self.merge_library(vec![track]);
        true
    }

    pub fn mark_track_available(&mut self, path: &std::path::Path, available: bool) {
        let selected_path = self.selected_track_path();
        if available {
            self.unavailable_tracks.remove(path);
        } else {
            self.unavailable_tracks.insert(path.to_path_buf());
        }
        self.restore_selection(selected_path);
    }

    fn selected_track_path(&self) -> Option<PathBuf> {
        self.rows()
            .get(self.navigation.selected())
            .and_then(|row| match row.target {
                Target::Play(id) => self.tracks.get(id).map(|track| track.path.clone()),
                _ => None,
            })
    }

    fn restore_selection(&mut self, selected_path: Option<PathBuf>) {
        let selected = selected_path.and_then(|path| {
            self.rows().iter().position(|row| {
                matches!(row.target, Target::Play(id) if self.tracks.get(id).is_some_and(|track| track.path == path))
            })
        });
        let selected = selected.unwrap_or_else(|| {
            self.navigation
                .selected()
                .min(self.selection_count().saturating_sub(1))
        });
        self.navigation.select(selected);
    }

    /// Ratings are shown only in the album list opened through Now Playing.
    /// Pin the displayed song so a gapless handoff cannot redirect an in-flight edit.
    pub fn rating_target(&self) -> Option<&Track> {
        if self.video_active {
            return None;
        }
        let Screen::Songs(Filter::Album { name, artist }) = &self.navigation.current().screen
        else {
            return None;
        };
        let path = self.album_rating_path.as_ref()?;
        self.tracks.iter().find(|track| {
            &track.path == path
                && &track.album == name
                && artist.as_ref().is_none_or(|artist| &track.artist == artist)
                && self.track_available(track)
        })
    }

    pub fn lyrics_target(&self) -> Option<usize> {
        match self.navigation.current().screen {
            Screen::Lyrics(id) | Screen::LyricsSearch(id) => Some(id),
            Screen::Songs(_) | Screen::Search => self
                .rows()
                .get(self.navigation.selected())
                .and_then(|row| match row.target {
                    Target::Play(id) => Some(id),
                    _ => None,
                }),
            _ => self.current,
        }
    }

    pub fn effective_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.volume }
    }
    pub fn duration(&self) -> f64 {
        if let Some(e) = self.podcasts.current() {
            return e.duration;
        }
        if self.video_active {
            return self
                .video_current
                .and_then(|id| self.videos.get(id))
                .map_or(0.0, |v| v.duration);
        }
        self.current
            .and_then(|id| self.tracks.get(id))
            .map_or(0.0, |t| t.duration)
    }

    pub fn seek(&mut self, seconds: f64) -> Effect {
        if self.podcasts.active.is_some() {
            let seconds = if seconds.is_finite() {
                seconds.max(0.0)
            } else {
                0.0
            };
            self.podcasts.position = if self.duration() > 0.0 {
                seconds.min(self.duration())
            } else {
                seconds
            };
            if matches!(
                self.podcasts.status,
                PlaybackStatus::Stopped | PlaybackStatus::Error
            ) && let Some(path) = self.podcasts.current().and_then(|e| e.local_path.clone())
            {
                self.podcasts.paused = true;
                self.podcasts.status = PlaybackStatus::Loading;
                return Effect::Audio(AudioCommand::LoadAt {
                    id: crate::podcast::AUDIO_ID,
                    path,
                    volume: self.effective_volume(),
                    paused: true,
                    start: self.podcasts.position,
                    speed: self.podcasts.speed,
                });
            }
            return Effect::Audio(AudioCommand::Seek(self.podcasts.position));
        }
        if self.video_active {
            let seconds = if seconds.is_finite() { seconds } else { 0.0 };
            if matches!(
                self.video_status,
                crate::video::Status::Finished | crate::video::Status::Error
            ) && let Some(video) = self
                .video_current
                .and_then(|id| self.videos.get(id))
                .cloned()
            {
                self.video_generation += 1;
                self.video_paused = true;
                self.video_status = crate::video::Status::Loading;
                self.video_position = seconds.clamp(0.0, video.duration);
                self.video_progress
                    .record(&video.path, self.video_position, video.duration);
                return Effect::Video(crate::video::engine::Command::Load {
                    generation: self.video_generation,
                    video,
                    start: self.video_position,
                    volume: self.effective_volume(),
                    paused: true,
                });
            }
            self.video_position = seconds.clamp(0.0, self.duration().max(0.0));
            self.remember_video_progress();
            return Effect::Video(crate::video::engine::Command::Seek {
                generation: self.video_generation,
                seconds: self.video_position,
            });
        }
        let seconds = if seconds.is_finite() { seconds } else { 0.0 };
        self.position = seconds.clamp(0.0, self.duration().max(0.0));
        Effect::Audio(AudioCommand::Seek(self.position))
    }

    pub fn set_volume(&mut self, volume: f32) -> Effect {
        self.volume = volume.clamp(0.0, 1.0);
        self.muted = false;
        self.volume_effect()
    }

    /// Peek the next eligible queue slot without changing playback intent.
    /// Uses the queue cursor so repeated entries retain their distinct positions.
    pub fn gapless_successor(&self) -> Option<(usize, usize)> {
        let current = self.current?;
        if self.queue.current() != Some(current) {
            return None;
        }
        if self.repeat == crate::audio::Repeat::One {
            return self
                .tracks
                .get(current)
                .filter(|t| self.track_available(t))
                .map(|_| (self.queue.position, current));
        }
        let len = self.queue.order.len();
        for step in 1..=len {
            let raw = self.queue.position + step;
            if raw >= len && self.repeat != crate::audio::Repeat::All {
                break;
            }
            let position = raw % len;
            let id = self.queue.order[position];
            if self.tracks.get(id).is_some_and(|t| self.track_available(t)) {
                return Some((position, id));
            }
        }
        None
    }

    pub fn start(&mut self, id: usize) -> Vec<Effect> {
        self.remember_video_progress();
        let Some(track) = self.tracks.get(id) else {
            return Vec::new();
        };
        if !self.track_available(track) {
            return Vec::new();
        }
        self.video_active = false;
        self.current = Some(id);
        self.status = PlaybackStatus::Loading;
        self.paused = false;
        self.position = 0.0;
        self.notice.clear();
        self.lyrics_visible = !track.lyrics.is_empty();
        self.overlay_visible = self.lyrics_visible;
        vec![Effect::Audio(AudioCommand::Load {
            id,
            path: track.path.clone(),
            volume: self.effective_volume(),
            paused: false,
        })]
    }

    pub fn active_position(&self) -> f64 {
        if self.podcasts.active.is_some() {
            return self.podcasts.position;
        }
        if self.video_active {
            self.video_position
        } else {
            self.position
        }
    }

    pub fn landscape_video(&self) -> bool {
        self.video_active
            && self.navigation.current().screen == Screen::VideoPlaying
            && self
                .video_current
                .and_then(|id| self.videos.get(id))
                .is_some_and(|v| v.is_landscape())
    }

    pub fn merge_videos(&mut self, videos: Vec<crate::video::library::Video>) {
        // Keep indices stable for playback and UI events during asynchronous imports.
        for video in videos {
            if let Some(old) = self.videos.iter_mut().find(|v| v.path == video.path) {
                *old = video;
            } else {
                self.videos.push(video);
            }
        }
    }

    /// Remove one confirmed missing path, retaining progress and remapping the
    /// active index by path so deletion cannot retarget another video's session.
    pub fn remove_video(&mut self, path: &std::path::Path) {
        self.remember_video_progress();
        let current = self
            .video_current
            .and_then(|id| self.videos.get(id))
            .map(|v| v.path.clone());
        self.videos.retain(|video| video.path != path);
        self.video_current = current
            .as_ref()
            .and_then(|path| self.videos.iter().position(|v| &v.path == path));
        if self.video_active && self.video_current.is_none() {
            self.video_active = false;
            self.video_paused = true;
            self.video_status = crate::video::Status::Finished;
            self.video_generation += 1;
            self.video_position = 0.0;
            if self.navigation.current().screen == Screen::VideoPlaying {
                self.navigation.back();
                self.navigation.open(Screen::Videos);
            }
        }
        if self.navigation.current().screen == Screen::Videos {
            self.navigation
                .select(self.navigation.selected().min(self.videos.len()));
        }
    }

    fn volume_effect(&self) -> Effect {
        if self.video_active {
            Effect::Video(crate::video::engine::Command::Volume {
                generation: self.video_generation,
                volume: self.effective_volume(),
            })
        } else {
            Effect::Audio(AudioCommand::Volume(self.effective_volume()))
        }
    }

    fn video_pause(&mut self) -> Effect {
        if !matches!(
            self.video_status,
            crate::video::Status::Finished | crate::video::Status::Error
        ) {
            self.video_status = if self.video_paused {
                crate::video::Status::Paused
            } else {
                crate::video::Status::Playing
            };
        }
        Effect::Video(crate::video::engine::Command::Pause {
            generation: self.video_generation,
            paused: self.video_paused,
        })
    }

    pub fn remember_video_progress(&mut self) {
        if self.video_active
            && !matches!(
                self.video_status,
                crate::video::Status::Loading | crate::video::Status::Error
            )
            && let Some(video) = self.video_current.and_then(|id| self.videos.get(id))
        {
            let position = if self.video_status == crate::video::Status::Finished {
                video.duration
            } else {
                self.video_position
            };
            self.video_progress
                .record(&video.path, position, video.duration);
        }
    }

    pub fn start_video(&mut self, id: usize) -> Vec<Effect> {
        self.remember_video_progress();
        let Some(video) = self.videos.get(id).cloned() else {
            return Vec::new();
        };
        self.video_active = true;
        self.video_current = Some(id);
        self.video_generation = self
            .video_generation
            .checked_add(1)
            .expect("video session counter exhausted");
        self.video_position = self.video_progress.resume(&video.path, video.duration);
        self.video_paused = false;
        self.video_status = crate::video::Status::Loading;
        self.video_overlay = true;
        self.paused = true;
        if self.current.is_some() {
            self.status = PlaybackStatus::Paused;
        }
        self.notice.clear();
        if self.navigation.current().screen != Screen::VideoPlaying {
            self.navigation.open(Screen::Videos);
            self.navigation.open(Screen::VideoPlaying);
        }
        self.navigation.select(1);
        let mut effects = vec![Effect::Video(crate::video::engine::Command::Load {
            generation: self.video_generation,
            video: video.clone(),
            start: self.video_position,
            volume: self.effective_volume(),
            paused: false,
        })];
        effects.push(Effect::Video(crate::video::engine::Command::Subtitle {
            generation: self.video_generation,
            preference: self.video_subtitles.get(&video.path),
        }));
        effects
    }

    pub fn advance(&mut self, manual: bool) -> Vec<Effect> {
        for _ in 0..self.queue.order.len() {
            let Some(id) = self.queue.advance(self.repeat, manual) else {
                break;
            };
            if self
                .tracks
                .get(id)
                .is_some_and(|track| self.track_available(track))
            {
                return self.start(id);
            }
            if !manual
                && self.repeat == Repeat::Off
                && self.queue.position + 1 >= self.queue.order.len()
            {
                break;
            }
        }
        self.status = PlaybackStatus::Stopped;
        self.position = self.duration();
        Vec::new()
    }

    pub fn apply(&mut self, action: Action) -> Vec<Effect> {
        use Action::*;
        self.remember_video_progress();
        if let Some(effects) = self.apply_podcast(action) {
            return effects;
        }
        if matches!(action, Back | Home | Settings | Search | NowPlaying) {
            self.album_rating_path = None;
        }
        if self.video_active {
            match action {
                TogglePlay => {
                    if matches!(
                        self.video_status,
                        crate::video::Status::Finished | crate::video::Status::Error
                    ) {
                        return self.start_video(self.video_current.unwrap_or(0));
                    }
                    self.video_paused = !self.video_paused;
                    return vec![self.video_pause()];
                }
                NextTrack => {
                    if let Some(id) = self.video_current.filter(|id| id + 1 < self.videos.len()) {
                        return self.start_video(id + 1);
                    }
                    return Vec::new();
                }
                PreviousTrack => {
                    if self.video_position >= 5.0 {
                        let effect = self.seek(0.0);
                        return vec![effect];
                    }
                    if let Some(id) = self.video_current.filter(|id| *id > 0) {
                        return self.start_video(id - 1);
                    }
                    let effect = self.seek(0.0);
                    return vec![effect];
                }
                NowPlaying => {
                    self.navigation.open(Screen::VideoPlaying);
                    self.navigation.select(1);
                    return vec![Effect::FocusScreen];
                }
                ToggleOverlay => {
                    self.video_overlay = !self.video_overlay;
                    return Vec::new();
                }
                ToggleLyrics | ToggleShuffle | CycleRepeat | TrackList | ToggleRomaji => {
                    return Vec::new();
                }
                _ => {}
            }
        }
        match action {
            SelectPrevious | SelectNext => {
                let count = self.selection_count();
                if count > 0 {
                    let step = if self.surface() == Surface::Home {
                        2
                    } else {
                        1
                    };
                    let current = self.navigation.selected();
                    self.navigation.select(if action == SelectPrevious {
                        current.saturating_sub(step)
                    } else {
                        (current + step).min(count - 1)
                    });
                }
            }
            Left | Right => {
                let selected = self.navigation.selected();
                if action == Left {
                    if selected % 2 == 1 {
                        self.navigation.select(selected - 1);
                    } else if self.home_page > 0 {
                        self.home_page -= 1;
                        self.navigation.select(1);
                    }
                } else if selected.is_multiple_of(2) && selected + 1 < self.selection_count() {
                    self.navigation.select(selected + 1);
                } else if self.home_page == 0 {
                    self.home_page = 1;
                    self.navigation.select(0);
                }
            }
            NavigateLeft | NavigateRight => {
                return self.apply(if self.surface() == Surface::Home {
                    if action == NavigateLeft { Left } else { Right }
                } else if action == NavigateLeft {
                    Back
                } else {
                    Activate
                });
            }
            Activate => {
                let selected = self.navigation.selected();
                match self.navigation.current().screen.clone() {
                    Screen::Home => {
                        let name = if self.home_page == 0 {
                            ["Music", "Videos", "Fitness", "Podcasts", "Photos", "Radio"]
                                [selected.min(5)]
                        } else {
                            ["Clock", "Settings", "Statistics"][selected.min(2)]
                        };
                        self.navigation.open(if name == "Music" {
                            Screen::Music
                        } else if name == "Videos" {
                            Screen::Videos
                        } else if name == "Podcasts" {
                            self.notice.clear();
                            Screen::Podcasts(super::navigation::PodcastPage::Home)
                        } else if name == "Settings" {
                            Screen::Settings
                        } else if name == "Statistics" {
                            Screen::Statistics(super::navigation::StatisticsPage::Home)
                        } else {
                            Screen::Placeholder(name.to_owned())
                        });
                    }
                    Screen::VideoPlaying => {
                        return self.apply(
                            [
                                PreviousTrack,
                                TogglePlay,
                                NextTrack,
                                ToggleMute,
                                ToggleOverlay,
                            ][selected.min(4)],
                        );
                    }
                    Screen::NowPlaying => {
                        return self.apply(
                            [
                                PreviousTrack,
                                TogglePlay,
                                NextTrack,
                                ToggleMute,
                                ToggleShuffle,
                                CycleRepeat,
                                TrackList,
                            ][selected.min(6)],
                        );
                    }
                    _ => {
                        if let Some(row) = self.rows().get(selected) {
                            match row.target.clone() {
                                Target::None => {}
                                Target::RefreshStatistics => {
                                    return vec![Effect::RefreshStatistics];
                                }
                                Target::StatisticsRange(range) => {
                                    self.statistics.range = range;
                                    self.statistics.snapshot = None;
                                    self.statistics.failed = false;
                                    self.navigation.back();
                                    return Vec::new();
                                }
                                Target::Screen(screen) => {
                                    if self.playlist_list_editing
                                        && self.navigation.current().screen
                                            == Screen::Groups(Group::Playlists)
                                    {
                                        if let Screen::Songs(Filter::Playlist { id, .. }) = screen {
                                            return vec![Effect::EditPlaylist { id }];
                                        }
                                        return Vec::new();
                                    }
                                    let screen = if self.podcasts.active.is_some()
                                        && screen == Screen::NowPlaying
                                    {
                                        Screen::Podcasts(super::navigation::PodcastPage::Playing)
                                    } else if self.video_active && screen == Screen::NowPlaying {
                                        Screen::VideoPlaying
                                    } else {
                                        screen
                                    };
                                    let languages = screen == Screen::Languages;
                                    let fonts = screen == Screen::Fonts;
                                    self.navigation.open(screen);
                                    if self.is_settings_list()
                                        && !languages
                                        && !fonts
                                        && let Some(index) = self
                                            .rows()
                                            .iter()
                                            .position(|row| self.setting_selected(&row.target))
                                    {
                                        self.navigation.select(index);
                                    }
                                    if languages {
                                        self.navigation.select(self.language.index());
                                    }
                                    if fonts {
                                        self.navigation.select(self.font.index());
                                    }
                                }
                                Target::DisplayMode(device) => {
                                    return self.apply(if device {
                                        DeviceMode
                                    } else {
                                        ScreenMode
                                    });
                                }
                                Target::Zoom(zoom) => {
                                    self.zoom = zoom;
                                    return vec![Effect::Resize];
                                }
                                Target::ReplayGain(mode) => {
                                    self.replay_gain_mode = mode;
                                    return vec![Effect::WindowPreferencesChanged];
                                }
                                Target::ReplayGainPreamp(db) => {
                                    self.replay_gain_preamp_db = db;
                                    return vec![Effect::WindowPreferencesChanged];
                                }
                                Target::Language(language) => {
                                    self.language = language;
                                    return vec![Effect::LanguageChanged];
                                }
                                Target::Font(font) => {
                                    self.font = font;
                                    return vec![Effect::FontChanged];
                                }
                                Target::Action(action) => return self.apply(action),
                                Target::GeniusPreview => return vec![Effect::GeniusPreview],
                                Target::Translation(command) => {
                                    return vec![Effect::Translation(command)];
                                }
                                Target::FindLyrics(id) => return vec![Effect::FindLyrics { id }],
                                Target::Shortcut(action) => {
                                    return vec![Effect::EditShortcut(action)];
                                }
                                Target::Podcast(command) => return vec![Effect::Podcast(command)],
                                Target::PlayVideo(id) => {
                                    return self.start_video(id);
                                }
                                Target::Play(id) => {
                                    let ids = self
                                        .rows()
                                        .iter()
                                        .filter_map(|row| {
                                            if let Target::Play(id) = row.target {
                                                Some(id)
                                            } else {
                                                None
                                            }
                                        })
                                        .collect();
                                    self.queue.set(ids, id, self.shuffle);
                                    self.navigation.open(Screen::NowPlaying);
                                    self.navigation.select(1);
                                    return self.start(id);
                                }
                            }
                        }
                    }
                }
            }
            Back => {
                if self.playlist_list_editing
                    && self.navigation.current().screen == Screen::Groups(Group::Playlists)
                {
                    self.playlist_list_editing = false;
                    return vec![Effect::FocusScreen];
                }
                let video = self.navigation.current().screen == Screen::VideoPlaying;
                self.navigation.back();
                if video {
                    if self.navigation.current().screen != Screen::Videos {
                        self.navigation.open(Screen::Videos);
                    }
                    self.video_paused = true;
                    return vec![self.video_pause(), Effect::FocusScreen];
                }
                return vec![Effect::FocusScreen];
            }
            Home => {
                self.playlist_list_editing = false;
                self.reference_open = false;
                self.navigation.home();
                self.home_page = 0;
                return vec![Effect::FocusScreen];
            }
            TogglePlay => {
                if self.current.is_none()
                    || matches!(self.status, PlaybackStatus::Stopped | PlaybackStatus::Error)
                {
                    let id = self.current.unwrap_or(0);
                    if self.tracks.is_empty() {
                        self.notice = "Import a music folder to play music".to_owned();
                    } else {
                        if self.queue.current().is_none() {
                            self.queue
                                .set((0..self.tracks.len()).collect(), id, self.shuffle);
                        }
                        return self.start(id);
                    }
                } else {
                    self.paused = !self.paused;
                    self.status = if self.paused {
                        PlaybackStatus::Paused
                    } else {
                        PlaybackStatus::Playing
                    };
                    return vec![Effect::Audio(AudioCommand::Pause(self.paused))];
                }
            }
            NextTrack => return self.advance(true),
            PreviousTrack => {
                if self.position > 3.0 {
                    let effect = self.seek(0.0);
                    return vec![effect];
                }
                if let Some(id) = self.queue.previous() {
                    return self.start(id);
                }
            }
            VolumeUp | VolumeDown => {
                let effect =
                    self.set_volume(self.volume + if action == VolumeUp { 0.05 } else { -0.05 });
                self.notice = format!("Volume: {:.0}%", self.volume * 100.0);
                return vec![effect];
            }
            ToggleMute => {
                self.muted = !self.muted;
                self.notice = if self.muted { "Muted" } else { "Unmuted" }.to_owned();
                return vec![self.volume_effect()];
            }
            ShuffleList => {
                if !self.supports_list_shuffle() {
                    return Vec::new();
                }
                let ids: Vec<_> = if matches!(self.navigation.current().screen, Screen::Artist(_)) {
                    // Album collections use only the tracks represented by their album rows.
                    self.list_rows()
                        .iter()
                        .flat_map(|row| {
                            let mut tracks: Vec<_> = self
                                .tracks
                                .iter()
                                .enumerate()
                                .filter(|(_, track)| self.track_available(track))
                                .filter(|(_, track)| match &row.target {
                                    Target::Screen(Screen::Songs(Filter::Album {
                                        name,
                                        artist,
                                    })) => {
                                        &track.album == name
                                            && artist.as_ref().is_none_or(|a| &track.artist == a)
                                    }
                                    _ => false,
                                })
                                .collect();
                            tracks.sort_by_cached_key(|(_, track)| {
                                (
                                    track.number == 0,
                                    track.number,
                                    track.title.to_lowercase(),
                                    track.title.clone(),
                                    track.path.clone(),
                                )
                            });
                            tracks.into_iter().map(|(id, _)| id).collect::<Vec<_>>()
                        })
                        .collect()
                } else {
                    self.list_rows()
                        .iter()
                        .filter_map(|row| match row.target {
                            Target::Play(id) => Some(id),
                            _ => None,
                        })
                        .collect()
                };
                return self.start_shuffled(ids);
            }
            ShuffleAll => {
                let mut ids: Vec<_> = self
                    .tracks
                    .iter()
                    .enumerate()
                    .filter(|(_, track)| self.track_available(track))
                    .map(|(id, _)| id)
                    .collect();
                ids.sort_by(|a, b| {
                    track_title_key(&self.tracks[*a]).cmp(&track_title_key(&self.tracks[*b]))
                });
                return self.start_shuffled(ids);
            }
            ToggleShuffle => {
                self.shuffle = !self.shuffle;
                if self.shuffle {
                    self.queue.shuffle();
                } else {
                    self.queue.unshuffle();
                }
                self.notice = format!("Shuffle: {}", if self.shuffle { "On" } else { "Off" });
            }
            CycleRepeat => {
                self.repeat = self.repeat.cycle();
                self.notice = format!("Repeat: {}", self.repeat.label());
            }
            NowPlaying => {
                self.navigation.open(Screen::NowPlaying);
                self.navigation.select(1);
            }
            ImportVideos => {
                if !self.video_importing {
                    self.video_importing = true;
                    self.notice = "Choose local videos…".into();
                    return vec![Effect::ImportVideos];
                }
            }
            RefreshLibrary => {
                if !self.importing && !self.library_refreshing {
                    self.library_refreshing = true;
                    self.notice = "Refreshing library…".into();
                    return vec![Effect::RefreshLibrary];
                }
            }
            ImportFolder => {
                if matches!(
                    self.navigation.current().screen,
                    Screen::Videos | Screen::VideoPlaying
                ) {
                    return self.apply(ImportVideos);
                }
                if !self.importing {
                    self.importing = true;
                    self.notice = "Choose a music folder…".to_owned();
                    return vec![Effect::Import];
                }
            }
            JumpToLetter => {
                if self.supports_letter_jump() {
                    self.notice = "Press A–Z to jump".into();
                }
            }
            Search => {
                self.reference_open = false;
                self.navigation.open(Screen::Search);
                return vec![Effect::FocusSearch];
            }
            DeviceMode | ScreenMode => {
                self.device_mode = action == DeviceMode;
                return vec![Effect::Resize];
            }
            ToggleAlwaysOnTop => {
                self.always_on_top = !self.always_on_top;
                return vec![Effect::WindowPreferencesChanged];
            }
            ToggleSmoothSlides => {
                self.smooth_slides = !self.smooth_slides;
                return vec![Effect::WindowPreferencesChanged];
            }
            NewPlaylist => return vec![Effect::NewPlaylist],
            CycleReplayGain => {
                self.replay_gain_mode = match self.replay_gain_mode {
                    crate::audio::gain::GainMode::Off => crate::audio::gain::GainMode::Track,
                    crate::audio::gain::GainMode::Track => crate::audio::gain::GainMode::Album,
                    crate::audio::gain::GainMode::Album => crate::audio::gain::GainMode::Off,
                };
                return vec![Effect::WindowPreferencesChanged];
            }
            CycleReplayGainPreamp => {
                self.replay_gain_preamp_db = if self.replay_gain_preamp_db >= 6.0 {
                    -6.0
                } else {
                    self.replay_gain_preamp_db + 3.0
                };
                return vec![Effect::WindowPreferencesChanged];
            }
            ToggleReplayGainClipping => {
                self.replay_gain_prevent_clipping = !self.replay_gain_prevent_clipping;
                return vec![Effect::WindowPreferencesChanged];
            }
            ToggleGaplessPlayback => {
                self.gapless_playback = !self.gapless_playback;
                return vec![Effect::WindowPreferencesChanged];
            }
            EditPlaylist => {
                if self.navigation.current().screen == Screen::Groups(Group::Playlists) {
                    self.playlist_list_editing = !self.playlist_list_editing;
                }
                if let Screen::Songs(Filter::Playlist { id, .. }) = self.navigation.current().screen
                {
                    return vec![Effect::EditPlaylist { id }];
                }
            }
            ZoomIn | ZoomOut => {
                self.zoom =
                    (self.zoom + if action == ZoomIn { 0.25 } else { -0.25 }).clamp(0.75, 3.0);
                return vec![Effect::Resize];
            }
            ShowShortcuts => {
                self.reference_open = true;
                return vec![Effect::FocusScreen];
            }
            CloseShortcuts => {
                self.reference_open = false;
                return vec![Effect::FocusScreen];
            }
            ToggleLyrics => {
                self.lyrics_visible = !self.lyrics_visible;
                self.overlay_visible = true;
                self.navigation.open(Screen::NowPlaying);
            }
            ConfigureDeepSeek => return vec![Effect::ConfigureDeepSeek],
            FindOnlineLyrics => {
                if self.navigation.current().screen == Screen::NowPlaying
                    && self
                        .current
                        .and_then(|id| self.tracks.get(id))
                        .is_some_and(|track| track.lyrics.timed.is_empty())
                {
                    self.lyrics_visible = true;
                    self.overlay_visible = true;
                }
                return vec![Effect::Translation(crate::translation::Command::FindLyrics)];
            }
            ToggleTranslation => {
                return vec![Effect::Translation(crate::translation::Command::Toggle)];
            }
            ToggleRomaji => {
                self.romaji_visible = !self.romaji_visible;
                return vec![Effect::RomajiVisibilityChanged];
            }
            CancelRomaji => return vec![Effect::CancelRomaji],
            GenerateRomaji | RemoveRomaji => {
                if let Some(id) = self.lyrics_target() {
                    return vec![if action == GenerateRomaji {
                        Effect::GenerateRomaji { id }
                    } else {
                        Effect::RemoveRomaji { id }
                    }];
                }
                self.notice = "Choose a song first".into();
            }
            ToggleOverlay => {
                self.overlay_visible = !self.overlay_visible;
                self.lyrics_visible = self.overlay_visible
                    && self
                        .current
                        .and_then(|id| self.tracks.get(id))
                        .is_some_and(|track| !track.lyrics.is_empty());
                self.navigation.open(Screen::NowPlaying);
            }
            Settings => {
                self.reference_open = false;
                self.navigation.open(Screen::Settings);
                return vec![Effect::FocusScreen];
            }
            Shortcuts => self.navigation.open(Screen::Shortcuts),
            RestoreShortcuts => return vec![Effect::RestoreShortcuts],
            ManageLyrics => {
                if let Some(id) = self.lyrics_target() {
                    self.navigation.open(Screen::Lyrics(id));
                } else {
                    self.notice = "Choose a song first".into();
                }
            }
            ImportLyrics | RemoveLyrics | AutomaticLyrics => {
                if let Some(id) = self.lyrics_target() {
                    use crate::library::scanner::LyricsCommand;
                    return vec![Effect::EditLyrics {
                        id,
                        command: match action {
                            ImportLyrics => LyricsCommand::Import,
                            RemoveLyrics => LyricsCommand::Remove,
                            _ => LyricsCommand::Automatic,
                        },
                    }];
                }
                self.notice = "Choose a song first".into();
            }
            TrackList => {
                if let Some(track) = self.current.and_then(|id| self.tracks.get(id)) {
                    self.album_rating_path = Some(track.path.clone());
                    self.navigation.open(Screen::Songs(Filter::Album {
                        name: track.album.clone(),
                        artist: Some(track.artist.clone()),
                    }));
                }
            }
            FinishSearchEditing => {
                return vec![Effect::FocusScreen];
            }
            // The desktop controller owns the subtitle menu.
            ToggleSubtitles => {}
            SeekBackward | SeekForward => {
                let has_media = if self.video_active {
                    self.video_current.is_some()
                } else {
                    self.current.is_some()
                };
                if has_media {
                    let effect = self.seek(
                        self.active_position() + if action == SeekBackward { -5.0 } else { 5.0 },
                    );
                    return vec![effect];
                }
            }
        }
        Vec::new()
    }
}

fn track_title_key(track: &Track) -> (String, &str, String) {
    (
        track.title.to_lowercase(),
        &track.title,
        track.path.to_string_lossy().into_owned(),
    )
}

fn sort_rows(rows: &mut [Row]) {
    rows.sort_by_cached_key(|row| (row.title.to_lowercase(), row.title.clone()));
}

type ArtworkKey<'a> = (String, &'a str, String, &'a str, std::borrow::Cow<'a, str>);

#[derive(Default)]
struct GroupedTracks<'a> {
    artists: BTreeSet<&'a str>,
    artwork: Option<(usize, ArtworkKey<'a>)>,
}

fn group_name<'a>(track: &'a Track, group: &Group) -> &'a str {
    match group {
        Group::Artists => &track.artist,
        Group::Albums => &track.album,
        _ => &track.genre,
    }
}
