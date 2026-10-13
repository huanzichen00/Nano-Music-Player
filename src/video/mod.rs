//! Local video playback. Native handles and decoding never enter the UI thread.
pub mod engine;
pub mod import;
pub mod library;
mod mpv;
pub mod orientation;
pub mod subtitles;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Loading,
    Playing,
    Paused,
    Finished,
    Error,
}

/// A session number, including when reopening the same file.
pub type Generation = u64;

#[cfg(test)]
mod tests;
