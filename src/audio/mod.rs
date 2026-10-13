pub mod engine;
pub mod gain;
pub mod queue;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    #[default]
    Stopped,
    Playing,
    Paused,
    Loading,
    Error,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::All => "All",
            Self::One => "One",
        }
    }
}
