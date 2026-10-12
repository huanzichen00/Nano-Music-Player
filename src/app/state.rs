use super::{
    action::Action,
    navigation::{Filter, Group, Navigation, Screen},
};
use crate::{
    audio::{PlaybackStatus, Repeat, engine::AudioCommand, queue::Queue},
    input::Surface,
    library::Track,
};
use std::collections::BTreeSet;

#[derive(Clone)]
pub enum Target {
    Screen(Screen),
    Play(usize),
    Action(Action),
}

#[derive(Clone)]
pub struct Row {
    pub title: String,
    pub subtitle: String,
    pub target: Target,
}

pub enum Effect {
    Audio(AudioCommand),
    Import,
    FocusSearch,
    FocusScreen,
    Resize,
}

pub struct State {
    pub navigation: Navigation,
    pub tracks: Vec<Track>,
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
    pub importing: bool,
    pub notice: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            navigation: Navigation::default(),
            tracks: Vec::new(),
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
            importing: false,
            notice: "Import a music folder to begin".to_owned(),
        }
    }
}

impl State {
    pub fn surface(&self) -> Surface {
        match &self.navigation.current().screen {
            Screen::Home => Surface::Home,
            Screen::NowPlaying if self.navigation.selected() == 3 => Surface::NowPlayingVolume,
            Screen::NowPlaying => Surface::NowPlaying,
            Screen::Placeholder(_) => Surface::Placeholder,
            _ => Surface::Library,
        }
    }

    pub fn rows(&self) -> Vec<Row> {
        let row = |title: &str, target| Row {
            title: title.to_owned(),
            subtitle: String::new(),
            target,
        };
        let to = |title: &str, screen| row(title, Target::Screen(screen));
        match &self.navigation.current().screen {
            Screen::Music => vec![
                to("Now Playing", Screen::NowPlaying),
                to("Playlists", Screen::Groups(Group::Playlists)),
                to("Artists", Screen::Groups(Group::Artists)),
                to("Albums", Screen::Groups(Group::Albums)),
                to("Songs", Screen::Songs(Filter::All)),
                to("Genres", Screen::Groups(Group::Genres)),
                row("Search", Target::Action(Action::Search)),
                row("Import Music…", Target::Action(Action::ImportFolder)),
            ],
            Screen::Groups(Group::Playlists) => vec![to("All Songs", Screen::Songs(Filter::All))],
            Screen::Groups(group) => {
                let names: BTreeSet<&str> = self
                    .tracks
                    .iter()
                    .map(|t| match group {
                        Group::Artists => t.artist.as_str(),
                        Group::Albums => t.album.as_str(),
                        _ => t.genre.as_str(),
                    })
                    .collect();
                names
                    .into_iter()
                    .map(|name| {
                        to(
                            name,
                            match group {
                                Group::Artists => Screen::Artist(name.to_owned()),
                                Group::Albums => Screen::Songs(Filter::Album {
                                    name: name.to_owned(),
                                    artist: None,
                                }),
                                _ => Screen::Songs(Filter::Genre(name.to_owned())),
                            },
                        )
                    })
                    .collect()
            }
            Screen::Artist(artist) => self
                .tracks
                .iter()
                .filter(|t| &t.artist == artist)
                .map(|t| t.album.as_str())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|name| {
                    to(
                        name,
                        Screen::Songs(Filter::Album {
                            name: name.to_owned(),
                            artist: Some(artist.clone()),
                        }),
                    )
                })
                .collect(),
            Screen::Songs(_) | Screen::Search => self
                .tracks
                .iter()
                .enumerate()
                .filter(|(_, track)| match &self.navigation.current().screen {
                    Screen::Search => track.matches(&self.query),
                    Screen::Songs(Filter::Album { name, artist }) => {
                        track.album == *name && artist.as_ref().is_none_or(|a| &track.artist == a)
                    }
                    Screen::Songs(Filter::Genre(genre)) => &track.genre == genre,
                    _ => true,
                })
                .map(|(id, track)| Row {
                    title: track.title.clone(),
                    subtitle: track.artist.clone(),
                    target: Target::Play(id),
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn selection_count(&self) -> usize {
        match self.navigation.current().screen {
            Screen::Home => {
                if self.home_page == 0 {
                    6
                } else {
                    2
                }
            }
            Screen::NowPlaying => 6,
            _ => self.rows().len(),
        }
    }

    pub fn merge_library(&mut self, tracks: Vec<Track>) {
        // Existing indexes remain stable for in-flight audio events and the queue.
        for track in tracks {
            if let Some(existing) = self.tracks.iter_mut().find(|t| t.path == track.path) {
                *existing = track;
            } else {
                self.tracks.push(track);
            }
        }
        let max = self.selection_count().saturating_sub(1);
        self.navigation.select(self.navigation.selected().min(max));
    }

    pub fn effective_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.volume }
    }
    pub fn duration(&self) -> f64 {
        self.current
            .and_then(|id| self.tracks.get(id))
            .map_or(0.0, |t| t.duration)
    }

    pub fn seek(&mut self, seconds: f64) -> Effect {
        self.position = seconds.clamp(0.0, self.duration().max(0.0));
        Effect::Audio(AudioCommand::Seek(self.position))
    }

    pub fn set_volume(&mut self, volume: f32) -> Effect {
        self.volume = volume.clamp(0.0, 1.0);
        self.muted = false;
        Effect::Audio(AudioCommand::Volume(self.effective_volume()))
    }

    pub fn start(&mut self, id: usize) -> Vec<Effect> {
        let Some(track) = self.tracks.get(id) else {
            return Vec::new();
        };
        self.current = Some(id);
        self.status = PlaybackStatus::Loading;
        self.paused = false;
        self.position = 0.0;
        self.notice = format!("Playing {}", track.title);
        vec![Effect::Audio(AudioCommand::Load {
            id,
            path: track.path.clone(),
            volume: self.effective_volume(),
            paused: false,
        })]
    }

    pub fn advance(&mut self, manual: bool) -> Vec<Effect> {
        if let Some(id) = self.queue.advance(self.repeat, manual) {
            self.start(id)
        } else {
            self.status = PlaybackStatus::Stopped;
            self.position = self.duration();
            Vec::new()
        }
    }

    pub fn apply(&mut self, action: Action) -> Vec<Effect> {
        use Action::*;
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
                        } else {
                            Screen::Placeholder(name.to_owned())
                        });
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
                            ][selected.min(5)],
                        );
                    }
                    _ => {
                        if let Some(row) = self.rows().get(selected) {
                            match row.target.clone() {
                                Target::Screen(screen) => self.navigation.open(screen),
                                Target::Action(action) => return self.apply(action),
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
                self.navigation.back();
                return vec![Effect::FocusScreen];
            }
            Home => {
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
                return vec![Effect::Audio(AudioCommand::Volume(self.effective_volume()))];
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
            ImportFolder => {
                if !self.importing {
                    self.importing = true;
                    self.notice = "Choose a music folder…".to_owned();
                    return vec![Effect::Import];
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
                self.navigation.open(Screen::NowPlaying);
            }
            FinishSearchEditing => {
                return vec![Effect::FocusScreen];
            }
            SeekBackward | SeekForward => {
                if self.current.is_some() {
                    let effect =
                        self.seek(self.position + if action == SeekBackward { -5.0 } else { 5.0 });
                    return vec![effect];
                }
            }
        }
        Vec::new()
    }
}
