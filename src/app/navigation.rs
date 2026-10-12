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
    Album {
        name: String,
        artist: Option<String>,
    },
    Genre(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Home,
    Music,
    Groups(Group),
    Artist(String),
    Songs(Filter),
    Search,
    NowPlaying,
    Settings,
    Shortcuts,
    Placeholder(String),
}

impl Screen {
    pub fn title(&self) -> &str {
        match self {
            Self::Home => "iPod",
            Self::Music => "Music",
            Self::Groups(Group::Artists) => "Artists",
            Self::Groups(Group::Albums) => "Albums",
            Self::Groups(Group::Genres) => "Genres",
            Self::Groups(Group::Playlists) => "Playlists",
            Self::Artist(name) | Self::Placeholder(name) => name,
            Self::Songs(Filter::All) => "Songs",
            Self::Songs(Filter::Album { name, .. }) => name,
            Self::Songs(Filter::Genre(name)) => name,
            Self::Search => "Search",
            Self::NowPlaying => "Now Playing",
            Self::Settings => "Settings",
            Self::Shortcuts => "Shortcuts",
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
