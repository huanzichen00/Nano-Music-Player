use super::{
    action::Action,
    i18n::translate,
    navigation::{PodcastPage, Screen},
    state::{Effect, Row, State, Target},
};
use crate::{
    audio::{PlaybackStatus, engine::AudioCommand},
    podcast::{self, Episode, Library},
};
use std::collections::HashMap;

pub struct PodcastState {
    pub library: Library,
    pub active: Option<i64>,
    pub position: f64,
    pub status: PlaybackStatus,
    pub paused: bool,
    pub overlay: bool,
    pub repeat_episode: bool,
    pub speed: f32,
    pub downloads: HashMap<i64, (u64, Option<u64>)>,
}
impl Default for PodcastState {
    fn default() -> Self {
        Self {
            library: Library::default(),
            active: None,
            position: 0.0,
            status: PlaybackStatus::Stopped,
            paused: true,
            overlay: false,
            repeat_episode: false,
            speed: 1.0,
            downloads: HashMap::new(),
        }
    }
}
impl PodcastState {
    pub fn current(&self) -> Option<&Episode> {
        self.active
            .and_then(|id| self.library.episodes.iter().find(|e| e.id == id))
    }
}

impl State {
    pub(super) fn podcast_rows(&self) -> Option<Vec<Row>> {
        use podcast::Action as P;
        let Screen::Podcasts(page) = &self.navigation.current().screen else {
            return None;
        };
        let row = |title: &str, subtitle: String, target| Row {
            title: translate(self.language, title),
            subtitle,
            artwork_id: None,
            target,
        };
        let to =
            |title: &str, page| row(title, String::new(), Target::Screen(Screen::Podcasts(page)));
        let command = |title: &str, action| row(title, String::new(), Target::Podcast(action));
        let mut rows = Vec::new();
        match page {
            PodcastPage::Home => {
                let mut feeds: Vec<_> = self
                    .podcasts
                    .library
                    .feeds
                    .iter()
                    .filter(|f| f.subscribed)
                    .collect();
                feeds.sort_by_key(|f| f.title.to_lowercase());
                for f in feeds {
                    rows.push(Row {
                        title: f.title.clone(),
                        subtitle: translate(self.language, "{0} Episodes").replace(
                            "{0}",
                            &self
                                .podcasts
                                .library
                                .episodes
                                .iter()
                                .filter(|e| e.feed_id == f.id)
                                .count()
                                .to_string(),
                        ),
                        artwork_id: None,
                        target: Target::Screen(Screen::Podcasts(PodcastPage::Feed(f.id))),
                    });
                }
                rows.push(command("Add Subscription…", P::Add));
                rows.push(to("Continue Listening", PodcastPage::Continue));
                rows.push(to("Latest Episodes", PodcastPage::Latest));
                rows.push(to("Downloaded Episodes", PodcastPage::Downloads));
            }
            PodcastPage::Episode(id) => {
                if let Some(e) = self.podcasts.library.episodes.iter().find(|e| e.id == *id) {
                    if e.media_url.is_some() || e.local_path.is_some() {
                        rows.push(command(
                            if e.local_path.is_none() {
                                "Download and Play"
                            } else if e.completed {
                                "Replay Episode"
                            } else if e.position > 0.0 {
                                "Continue Listening"
                            } else {
                                "Play Episode"
                            },
                            P::Play(*id, e.completed),
                        ));
                    }
                    if self.podcasts.downloads.contains_key(id) {
                        rows.push(command("Cancel Download", P::CancelDownload(*id)));
                    } else if e.local_path.is_some() {
                        rows.push(command("Delete Download", P::DeleteDownload(*id)));
                    } else if e.media_url.is_some() {
                        rows.push(command("Download Episode", P::Download(*id)));
                    }
                    rows.push(command(
                        if e.completed {
                            "Mark as Unplayed"
                        } else {
                            "Mark as Played"
                        },
                        P::Mark(*id, !e.completed),
                    ));
                    if e.position > 0.0 && e.local_path.is_some() {
                        rows.push(command("Play from Beginning", P::Play(*id, true)));
                    }
                }
            }
            PodcastPage::Playing => {}
            _ => {
                let mut episodes: Vec<_> = self
                    .podcasts
                    .library
                    .episodes
                    .iter()
                    .filter(|e| match page {
                        PodcastPage::Feed(id) => e.feed_id == *id,
                        PodcastPage::Downloads => e.local_path.is_some(),
                        PodcastPage::Continue => e.position > 0.0 && !e.completed,
                        PodcastPage::Latest => self
                            .podcasts
                            .library
                            .feeds
                            .iter()
                            .any(|f| f.id == e.feed_id && f.subscribed),
                        _ => false,
                    })
                    .collect();
                episodes.sort_by_key(|e| {
                    if *page == PodcastPage::Continue {
                        -e.last_played
                    } else if matches!(page, PodcastPage::Feed(_)) {
                        e.published.unwrap_or(0)
                    } else {
                        -e.published.unwrap_or(0)
                    }
                });
                for e in episodes {
                    let status = if let Some((bytes, total)) = self.podcasts.downloads.get(&e.id) {
                        total.filter(|t| *t > 0).map_or_else(
                            || format!("{} KB", bytes / 1024),
                            |t| format!("{}%", bytes.saturating_mul(100) / t),
                        )
                    } else {
                        translate(
                            self.language,
                            if e.completed {
                                "Played"
                            } else if e.local_path.is_some() {
                                "Downloaded"
                            } else {
                                "Not Downloaded"
                            },
                        )
                    };
                    let date = e
                        .published
                        .and_then(|v| chrono::DateTime::from_timestamp(v, 0))
                        .map(|v| v.format("%Y-%m-%d").to_string())
                        .unwrap_or_default();
                    rows.push(Row {
                        title: e.title.clone(),
                        subtitle: format!("{date} · {status}"),
                        artwork_id: None,
                        target: Target::Podcast(P::Play(e.id, false)),
                    });
                }
                if let PodcastPage::Feed(id) = page {
                    rows.push(command("Refresh Episodes", P::Refresh(*id)));
                    rows.push(command("Unsubscribe", P::Unsubscribe(*id)));
                }
            }
        }
        Some(rows)
    }

    pub(super) fn apply_podcast(&mut self, action: Action) -> Option<Vec<Effect>> {
        use Action::*;
        if self.navigation.current().screen == Screen::Podcasts(PodcastPage::Playing)
            && action == Activate
        {
            return Some(match self.navigation.selected() {
                4 => self.apply(CycleRepeat),
                5 => vec![self.seek(self.podcasts.position - 30.0)],
                6 => {
                    self.podcasts.speed = if self.podcasts.speed == 1.0 {
                        2.0
                    } else if self.podcasts.speed == 2.0 {
                        0.5
                    } else {
                        1.0
                    };
                    if matches!(
                        self.podcasts.status,
                        PlaybackStatus::Stopped | PlaybackStatus::Error
                    ) {
                        vec![self.seek(self.podcasts.position)]
                    } else {
                        vec![Effect::Audio(AudioCommand::Speed(self.podcasts.speed))]
                    }
                }
                7 => self.apply(TrackList),
                selected => {
                    self.apply([PreviousTrack, TogglePlay, NextTrack, ToggleMute][selected.min(3)])
                }
            });
        }
        let id = self.podcasts.active?;
        match action {
            TogglePlay => {
                if matches!(
                    self.podcasts.status,
                    PlaybackStatus::Stopped | PlaybackStatus::Error
                ) {
                    return Some(vec![Effect::Podcast(podcast::Action::Play(
                        id,
                        self.podcasts.current().is_some_and(|e| e.completed),
                    ))]);
                }
                self.podcasts.paused = !self.podcasts.paused;
                self.podcasts.status = if self.podcasts.paused {
                    PlaybackStatus::Paused
                } else {
                    PlaybackStatus::Playing
                };
                Some(vec![Effect::Audio(AudioCommand::Pause(
                    self.podcasts.paused,
                ))])
            }
            SeekBackward | SeekForward => {
                Some(vec![self.seek(
                    self.podcasts.position + if action == SeekBackward { -30.0 } else { 30.0 },
                )])
            }
            NowPlaying => {
                self.navigation.open(Screen::Podcasts(PodcastPage::Playing));
                self.navigation.select(1);
                Some(vec![Effect::FocusScreen])
            }
            NextTrack | PreviousTrack => {
                if action == PreviousTrack && self.podcasts.position > 3.0 {
                    return Some(vec![self.seek(0.0)]);
                }
                let feed = self.podcasts.current()?.feed_id;
                let mut episodes: Vec<_> = self
                    .podcasts
                    .library
                    .episodes
                    .iter()
                    .filter(|e| {
                        e.feed_id == feed && (e.local_path.is_some() || e.media_url.is_some())
                    })
                    .collect();
                episodes.sort_by_key(|e| e.published.unwrap_or(0));
                let index = episodes.iter().position(|e| e.id == id)?;
                let next = if action == NextTrack {
                    episodes.get(index + 1)
                } else {
                    index.checked_sub(1).and_then(|i| episodes.get(i))
                };
                Some(
                    next.map(|e| vec![Effect::Podcast(podcast::Action::Play(e.id, false))])
                        .unwrap_or_default(),
                )
            }
            ToggleOverlay => {
                self.podcasts.overlay = !self.podcasts.overlay;
                Some(Vec::new())
            }
            CycleRepeat => {
                self.podcasts.repeat_episode = !self.podcasts.repeat_episode;
                Some(Vec::new())
            }
            TrackList => {
                let feed = self.podcasts.current()?.feed_id;
                self.navigation
                    .open(Screen::Podcasts(PodcastPage::Feed(feed)));
                Some(vec![Effect::FocusScreen])
            }
            ToggleShuffle | ToggleRomaji | ToggleLyrics | ToggleTranslation | FindOnlineLyrics => {
                Some(Vec::new())
            }
            _ => None,
        }
    }
}
