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
    Screen(Screen),
    Play(usize),
    PlayVideo(usize),
    Action(Action),
    Shortcut(Action),
    Language(Language),
    Font(Font),
    GeniusPreview,
}

#[derive(Clone)]
pub struct Row {
    pub title: String,
    pub subtitle: String,
    pub artwork_id: Option<usize>,
    pub target: Target,
}

pub enum Effect {
    Audio(AudioCommand),
    Video(crate::video::engine::Command),
    ImportVideos,
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
    pub language: Language,
    pub font: Font,
    pub always_on_top: bool,
    pub smooth_slides: bool,
    pub navigation: Navigation,
    pub tracks: Vec<Track>,
    /// Paths temporarily hidden from library browsing; vector slots stay stable for queued IDs.
    pub unavailable_tracks: BTreeSet<PathBuf>,
    pub videos: Vec<crate::video::library::Video>,
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
    pub romaji: HashMap<PathBuf, crate::library::romaji::Romaji>,
    pub romaji_generating: Option<PathBuf>,
    pub importing: bool,
    pub notice: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            language: Language::English,
            font: Font::Classic,
            always_on_top: true,
            smooth_slides: true,
            navigation: Navigation::default(),
            tracks: Vec::new(),
            unavailable_tracks: BTreeSet::new(),
            videos: Vec::new(),
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
            romaji: HashMap::new(),
            romaji_generating: None,
            importing: false,
            notice: "Import a music folder to begin".to_owned(),
        }
    }
}

impl State {
    pub fn surface(&self) -> Surface {
        match &self.navigation.current().screen {
            Screen::Home => Surface::Home,
            Screen::NowPlaying | Screen::VideoPlaying if self.navigation.selected() == 3 => {
                Surface::NowPlayingVolume
            }
            Screen::NowPlaying | Screen::VideoPlaying => Surface::NowPlaying,
            Screen::Placeholder(_) => Surface::Placeholder,
            _ => Surface::Library,
        }
    }

    pub fn supports_list_shuffle(&self) -> bool {
        matches!(
            self.navigation.current().screen,
            Screen::Songs(_) | Screen::Search | Screen::Groups(Group::Albums) | Screen::Artist(_)
        )
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
            Screen::Settings => vec![
                Row {
                    title: "Keyboard Shortcuts".into(),
                    subtitle: "Customize keys and navigation".into(),
                    artwork_id: None,
                    target: Target::Screen(Screen::Shortcuts),
                },
                Row {
                    title: "Display Mode".into(),
                    subtitle: if self.device_mode { "Device" } else { "Screen" }.into(),
                    artwork_id: None,
                    target: Target::Action(if self.device_mode {
                        Action::ScreenMode
                    } else {
                        Action::DeviceMode
                    }),
                },
                row("Zoom In", Target::Action(Action::ZoomIn)),
                row("Zoom Out", Target::Action(Action::ZoomOut)),
                row("Lyrics for Track", Target::Action(Action::ManageLyrics)),
                Row {
                    title: "Language".into(),
                    subtitle: self.language.label().into(),
                    artwork_id: None,
                    target: Target::Screen(Screen::Languages),
                },
                Row {
                    title: "Show Romaji".into(),
                    subtitle: if self.romaji_visible { "On" } else { "Off" }.into(),
                    artwork_id: None,
                    target: Target::Action(Action::ToggleRomaji),
                },
                Row {
                    title: "Smooth Slides".into(),
                    subtitle: if self.smooth_slides {
                        "On · Screens slide in the navigation direction".into()
                    } else {
                        "Off · Screens slide in the navigation direction".into()
                    },
                    artwork_id: None,
                    target: Target::Action(Action::ToggleSmoothSlides),
                },
                row("DeepSeek API…", Target::Action(Action::ConfigureDeepSeek)),
                Row {
                    title: "Font".into(),
                    subtitle: self.font.label().into(),
                    artwork_id: None,
                    target: Target::Screen(Screen::Fonts),
                },
                Row {
                    title: "Always on Top".into(),
                    subtitle: if self.always_on_top { "On" } else { "Off" }.into(),
                    artwork_id: None,
                    target: Target::Action(Action::ToggleAlwaysOnTop),
                },
            ],
            Screen::Fonts => Font::ALL
                .into_iter()
                .map(|font| Row {
                    title: font.label().into(),
                    subtitle: format!(
                        "{}Aa Bb Cc Ss 0123 · 日本語 中文",
                        if self.font == font { "✓ · " } else { "" }
                    ),
                    artwork_id: None,
                    target: Target::Font(font),
                })
                .collect(),
            Screen::Languages => Language::ALL
                .into_iter()
                .map(|language| Row {
                    title: language.label().into(),
                    subtitle: if self.language == language {
                        "✓".into()
                    } else {
                        String::new()
                    },
                    artwork_id: None,
                    target: Target::Language(language),
                })
                .collect(),
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
                row(
                    "Use Automatic Lyrics",
                    Target::Action(Action::AutomaticLyrics),
                ),
                row("Generate Romaji", Target::Action(Action::GenerateRomaji)),
                row(
                    "Cancel Romaji Generation",
                    Target::Action(Action::CancelRomaji),
                ),
                row("Remove Romaji", Target::Action(Action::RemoveRomaji)),
                row("Show / Hide Romaji", Target::Action(Action::ToggleRomaji)),
            ],
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
                to("Now Playing", Screen::NowPlaying),
                to("Playlists", Screen::Groups(Group::Playlists)),
                to("Artists", Screen::Groups(Group::Artists)),
                to("Albums", Screen::Groups(Group::Albums)),
                to("Songs", Screen::Songs(Filter::All)),
                to("Genres", Screen::Groups(Group::Genres)),
                Row {
                    title: "Genius".into(),
                    subtitle: "Create from local songs".into(),
                    artwork_id: None,
                    target: Target::GeniusPreview,
                },
                row("Search", Target::Action(Action::Search)),
                row("Import Music…", Target::Action(Action::ImportFolder)),
            ],
            Screen::Groups(Group::Playlists) => {
                let mut playlists: Vec<_> = self.playlists.iter().collect();
                playlists.sort_by_cached_key(|p| (p.name.to_lowercase(), p.name.clone(), p.id));
                let mut rows = vec![
                    row("Add Playlist", Target::Action(Action::NewPlaylist)),
                    Row {
                        title: "All Songs".into(),
                        subtitle: format!(
                            "{} songs",
                            self.tracks
                                .iter()
                                .filter(|t| self.track_available(t))
                                .count()
                        ),
                        artwork_id: None,
                        target: Target::Screen(Screen::Songs(Filter::All)),
                    },
                ];
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
                rows
            }
            Screen::Groups(group) => self.group_rows(group, None),
            Screen::Artist(artist) => self.group_rows(&Group::Albums, Some(artist)),
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
                let is_album = matches!(
                    self.navigation.current().screen,
                    Screen::Songs(Filter::Album { .. })
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

    pub fn selection_count(&self) -> usize {
        match &self.navigation.current().screen {
            Screen::Home => {
                if self.home_page == 0 {
                    6
                } else {
                    2
                }
            }
            Screen::NowPlaying => 7,
            Screen::VideoPlaying => 5,
            Screen::Groups(Group::Playlists) => 2 + self.playlists.len(),
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

    pub fn lyrics_target(&self) -> Option<usize> {
        match self.navigation.current().screen {
            Screen::Lyrics(id) => Some(id),
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
                return Effect::Video(crate::video::engine::Command::Load {
                    generation: self.video_generation,
                    video,
                    start: self.video_position,
                    volume: self.effective_volume(),
                    paused: true,
                });
            }
            self.video_position = seconds.clamp(0.0, self.duration().max(0.0));
            return Effect::Video(crate::video::engine::Command::Seek {
                generation: self.video_generation,
                seconds: self.video_position,
            });
        }
        self.position = seconds.clamp(0.0, self.duration().max(0.0));
        Effect::Audio(AudioCommand::Seek(self.position))
    }

    pub fn set_volume(&mut self, volume: f32) -> Effect {
        self.volume = volume.clamp(0.0, 1.0);
        self.muted = false;
        self.volume_effect()
    }

    pub fn start(&mut self, id: usize) -> Vec<Effect> {
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

    pub fn start_video(&mut self, id: usize) -> Vec<Effect> {
        let Some(video) = self.videos.get(id).cloned() else {
            return Vec::new();
        };
        self.video_active = true;
        self.video_current = Some(id);
        self.video_generation = self
            .video_generation
            .checked_add(1)
            .expect("video session counter exhausted");
        self.video_position = 0.0;
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
            start: 0.0,
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
                            ["Clock", "Settings"][selected.min(1)]
                        };
                        self.navigation.open(if name == "Music" {
                            Screen::Music
                        } else if name == "Videos" {
                            Screen::Videos
                        } else if name == "Settings" {
                            Screen::Settings
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
                                Target::Screen(screen) => {
                                    let screen =
                                        if self.video_active && screen == Screen::NowPlaying {
                                            Screen::VideoPlaying
                                        } else {
                                            screen
                                        };
                                    let languages = screen == Screen::Languages;
                                    let fonts = screen == Screen::Fonts;
                                    self.navigation.open(screen);
                                    if languages {
                                        self.navigation.select(self.language.index());
                                    }
                                    if fonts {
                                        self.navigation.select(self.font.index());
                                    }
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
                                Target::Shortcut(action) => {
                                    return vec![Effect::EditShortcut(action)];
                                }
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
                let ids: Vec<_> = if matches!(
                    self.navigation.current().screen,
                    Screen::Groups(Group::Albums) | Screen::Artist(_)
                ) {
                    // Album collections use only the tracks represented by their album rows.
                    self.list_rows()
                        .iter()
                        .flat_map(|row| {
                            let mut tracks: Vec<_> = self
                                .tracks
                                .iter()
                                .enumerate()
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
                use rand::seq::IndexedRandom;
                let Some(&start) = ids.choose(&mut rand::rng()) else {
                    return Vec::new();
                };
                self.shuffle = true;
                self.queue.set(ids, start, true);
                self.navigation.open(Screen::NowPlaying);
                self.navigation.select(1);
                return self.start(start);
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
            EditPlaylist => {
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
                    self.navigation.open(Screen::Songs(Filter::Album {
                        name: track.album.clone(),
                        artist: Some(track.artist.clone()),
                    }));
                }
            }
            FinishSearchEditing => {
                return vec![Effect::FocusScreen];
            }
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
