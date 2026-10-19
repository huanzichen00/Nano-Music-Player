#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Group {
    Artists,
    Albums,
    Genres,
    Playlists,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    All,
    TopRated,
    Album {
        name: String,
        artist: Option<String>,
    },
    Genre(String),
    Playlist {
        id: i64,
        name: String,
    },
    SmartPlaylist(crate::library::statistics::SmartPlaylist),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Display,
    DisplayMode,
    WindowSize,
    Playback,
    ReplayGain,
    ReplayGainPreamp,
    Lyrics,
    Translation,
    TranslationLanguage,
}

#[cfg(not(any(target_os = "ios", feature = "mobile")))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PodcastPage {
    Home,
    Feed(i64),
    Latest,
    Downloads,
    Continue,
    Episode(i64),
    Playing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    #[cfg(not(any(target_os = "ios", feature = "mobile")))]
    Podcasts(PodcastPage),
    Home,
    Music,
    Videos,
    VideoPlaying,
    Groups(Group),
    Artist(String),
    Songs(Filter),
    Search,
    NowPlaying,
    Settings,
    SettingsSection(SettingsSection),
    Shortcuts,
    Lyrics(usize),
    LyricSongs,
    LyricsSearch(usize),
    Languages,
    Fonts,
    Placeholder(String),
}

impl Screen {
    pub fn title(&self) -> &str {
        match self {
            Self::Home => "iPod",
            Self::Music => "Music",
            #[cfg(not(any(target_os = "ios", feature = "mobile")))]
            Self::Podcasts(page) => match page {
                PodcastPage::Home | PodcastPage::Feed(_) => "Podcasts",
                PodcastPage::Latest => "Latest Episodes",
                PodcastPage::Downloads => "Downloaded Episodes",
                PodcastPage::Continue => "Continue Listening",
                PodcastPage::Episode(_) => "Episode Details",
                PodcastPage::Playing => "Now Playing",
            },
            Self::Videos => "Videos",
            Self::VideoPlaying => "Now Playing",
            Self::Groups(Group::Artists) => "Artists",
            Self::Groups(Group::Albums) => "Albums",
            Self::Groups(Group::Genres) => "Genres",
            Self::Groups(Group::Playlists) => "Playlists",
            Self::Artist(name) | Self::Placeholder(name) => name,
            Self::Songs(Filter::All) => "Songs",
            Self::Songs(Filter::TopRated) => "My Top Rated",
            Self::Songs(Filter::Album { name, .. }) => name,
            Self::Songs(Filter::Genre(name)) => name,
            Self::Songs(Filter::Playlist { name, .. }) => name,
            Self::Songs(Filter::SmartPlaylist(kind)) => kind.label(),
            Self::Search => "Search",
            Self::NowPlaying => "Now Playing",
            Self::Settings => "Settings",
            Self::SettingsSection(section) => match section {
                SettingsSection::Display => "Appearance",
                SettingsSection::DisplayMode => "Window Style",
                SettingsSection::WindowSize => "Window Size",
                SettingsSection::Playback => "Playback",
                SettingsSection::ReplayGain => "Volume Leveling",
                SettingsSection::ReplayGainPreamp => "Volume Boost",
                SettingsSection::Lyrics => "Lyrics",
                SettingsSection::Translation => "Song Translation",
                SettingsSection::TranslationLanguage => "Translate To",
            },
            Self::Shortcuts => "Shortcuts",
            Self::Lyrics(_) => "Lyrics for Track",
            Self::LyricSongs => "Choose Song",
            Self::LyricsSearch(_) => "Find Lyrics",
            Self::Languages => "Language",
            Self::Fonts => "Font",
        }
    }
}

#[derive(Debug)]
pub struct Entry {
    pub screen: Screen,
    pub selected: usize,
}

#[derive(Debug)]
pub struct Navigation {
    stack: Vec<Entry>,
}

impl Default for Navigation {
    fn default() -> Self {
        Self {
            stack: vec![Entry {
                screen: Screen::Home,
                selected: 0,
            }],
        }
    }
}

impl Navigation {
    pub fn depth(&self) -> usize {
        self.stack.len()
    }
    pub fn current(&self) -> &Entry {
        &self.stack[self.stack.len() - 1]
    }
    pub fn selected(&self) -> usize {
        self.current().selected
    }
    pub fn select(&mut self, selected: usize) {
        if let Some(entry) = self.stack.last_mut() {
            entry.selected = selected;
        }
    }
    pub fn open(&mut self, screen: Screen) {
        if self.current().screen != screen {
            self.stack.push(Entry {
                screen,
                selected: 0,
            });
        }
    }
    pub fn rename_playlist(&mut self, id: i64, name: &str) {
        for entry in &mut self.stack {
            if let Screen::Songs(Filter::Playlist {
                id: entry_id,
                name: entry_name,
            }) = &mut entry.screen
                && *entry_id == id
            {
                *entry_name = name.to_owned();
            }
        }
    }
    pub fn back(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
    }
    pub fn home(&mut self) {
        self.stack.truncate(1);
        self.select(0);
    }
}
