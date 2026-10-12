use serde::{Deserialize, Serialize};

/// Semantic commands shared by keyboard, menu and pointer interactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    SelectPrevious,
    SelectNext,
    Activate,
    Back,
    Home,
    Left,
    Right,
    TogglePlay,
    NextTrack,
    PreviousTrack,
    VolumeUp,
    VolumeDown,
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    NowPlaying,
    ImportFolder,
    Search,
    DeviceMode,
    ScreenMode,
    ZoomIn,
    ZoomOut,
    ShowShortcuts,
    CloseShortcuts,
    FinishSearchEditing,
    SeekBackward,
    SeekForward,
    ToggleLyrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Playback,
    Navigation,
    Application,
}

impl Category {
    pub fn title(self) -> &'static str {
        match self {
            Self::Playback => "Global playback",
            Self::Navigation => "Screen navigation",
            Self::Application => "Application management",
        }
    }
}

impl Action {
    pub fn category(self) -> Category {
        match self {
            Self::TogglePlay
            | Self::NextTrack
            | Self::PreviousTrack
            | Self::VolumeUp
            | Self::VolumeDown
            | Self::ToggleMute
            | Self::ToggleShuffle
            | Self::CycleRepeat
            | Self::NowPlaying
            | Self::SeekBackward
            | Self::SeekForward
            | Self::ToggleLyrics => Category::Playback,
            Self::ImportFolder
            | Self::Search
            | Self::DeviceMode
            | Self::ScreenMode
            | Self::ZoomIn
            | Self::ZoomOut
            | Self::ShowShortcuts
            | Self::CloseShortcuts => Category::Application,
            _ => Category::Navigation,
        }
    }

    pub fn repeatable(self) -> bool {
        matches!(
            self,
            Self::SelectPrevious
                | Self::SelectNext
                | Self::Left
                | Self::Right
                | Self::VolumeUp
                | Self::VolumeDown
                | Self::SeekBackward
                | Self::SeekForward
                | Self::ZoomIn
                | Self::ZoomOut
        )
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::SelectPrevious => "Move selection up",
            Self::SelectNext => "Move selection down",
            Self::Activate => "Activate selected item",
            Self::Back => "Navigate back",
            Self::Home => "Physical Home button",
            Self::Left => "Left: grid / back / seek",
            Self::Right => "Right: grid / open / seek",
            Self::TogglePlay => "Play / Pause",
            Self::NextTrack => "Next track",
            Self::PreviousTrack => "Previous track",
            Self::VolumeUp => "Increase volume (hold to repeat)",
            Self::VolumeDown => "Decrease volume (hold to repeat)",
            Self::ToggleMute => "Toggle mute",
            Self::ToggleShuffle => "Toggle shuffle",
            Self::CycleRepeat => "Repeat: Off / All / One",
            Self::NowPlaying => "Open Now Playing",
            Self::ImportFolder => "Import music folder",
            Self::Search => "Search library",
            Self::DeviceMode => "Device Mode",
            Self::ScreenMode => "Screen Mode",
            Self::ZoomIn => "Zoom in",
            Self::ZoomOut => "Zoom out",
            Self::ShowShortcuts => "Keyboard shortcuts",
            Self::CloseShortcuts => "Close keyboard shortcuts",
            Self::FinishSearchEditing => "Browse search results",
            Self::SeekBackward => "Seek back 5 seconds",
            Self::SeekForward => "Seek forward 5 seconds",
            Self::ToggleLyrics => "Show / hide lyrics",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.to_owned())).ok()
    }
}
