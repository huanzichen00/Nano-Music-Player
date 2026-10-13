pub mod database;
pub mod deletion_monitor;
pub mod lyrics;
pub mod playlists;
pub mod romaji;
pub mod scanner;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub number: u32,
    pub duration: f64,
    pub artwork: Vec<u8>,
    #[serde(default)]
    pub lyrics: lyrics::Lyrics,
}

impl Track {
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        [&self.title, &self.artist, &self.album, &self.genre]
            .iter()
            .any(|text| text.to_lowercase().contains(&query))
    }
}
