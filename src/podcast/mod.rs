//! Desktop podcast subscriptions. Episodes never enter the music library.
pub mod feed;
pub mod service;
pub mod store;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type FeedId = i64;
pub type EpisodeId = i64;
/// An engine handle, never a music-library index. Generation filtering separates sessions.
pub(crate) const AUDIO_ID: usize = usize::MAX;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Feed {
    pub id: FeedId,
    pub url: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub image_url: Option<String>,
    #[serde(default)]
    pub artwork_path: Option<PathBuf>,
    pub subscribed: bool,
    pub etag: Option<String>,
    pub modified: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Episode {
    pub id: EpisodeId,
    pub feed_id: FeedId,
    pub guid: Option<String>,
    pub title: String,
    pub description: String,
    pub media_url: Option<String>,
    pub mime: String,
    pub published: Option<i64>,
    pub duration: f64,
    pub position: f64,
    pub completed: bool,
    pub last_played: i64,
    pub local_path: Option<PathBuf>,
    pub download_error: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Library {
    pub feeds: Vec<Feed>,
    pub episodes: Vec<Episode>,
}

#[derive(Clone, Debug)]
pub struct ParsedFeed {
    pub feed: Feed,
    pub episodes: Vec<Episode>,
    pub aliases: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum Command {
    Preview {
        url: String,
        request: u64,
    },
    Subscribe(Box<ParsedFeed>),
    Refresh(FeedId),
    Unsubscribe(FeedId),
    Download(EpisodeId),
    CancelDownload(EpisodeId),
    DeleteDownload(EpisodeId),
    Progress {
        id: EpisodeId,
        position: f64,
        completed: Option<bool>,
    },
    Shutdown,
}

#[derive(Clone, Debug)]
pub enum Event {
    Library(Library),
    Preview {
        request: u64,
        result: Result<Box<ParsedFeed>, String>,
    },
    DownloadProgress {
        id: EpisodeId,
        bytes: u64,
        total: Option<u64>,
    },
    DownloadFinished {
        id: EpisodeId,
        result: Result<(), String>,
    },
    Refreshed(FeedId),
    Error(String),
}

#[derive(Clone, Debug)]
pub enum Action {
    Add,
    Refresh(FeedId),
    Unsubscribe(FeedId),
    Download(EpisodeId),
    CancelDownload(EpisodeId),
    DeleteDownload(EpisodeId),
    Play(EpisodeId, bool),
    Mark(EpisodeId, bool),
}
