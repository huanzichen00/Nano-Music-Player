use super::*;
use crate::{
    app::navigation::PodcastPage,
    podcast::{self, Command, Event, ParsedFeed},
};
use std::time::{Duration, Instant};

pub(super) const AUDIO_ID: usize = podcast::AUDIO_ID;
pub(super) struct Podcasts {
    service: podcast::service::Service,
    draft: Option<ParsedFeed>,
    request: u64,
    autoplay: Option<(i64, bool)>,
    deleting: Option<(u64, i64)>,
    saved: Instant,
    artwork: Option<PathBuf>,
    seek_to_end: bool,
    advance: Option<(i64, bool)>,
    covers: HashMap<PathBuf, slint::Image>,
}
impl Podcasts {
    pub(super) fn new(directory: PathBuf) -> Self {
        Self {
            service: podcast::service::Service::new(directory),
            draft: None,
            request: 0,
            autoplay: None,
            deleting: None,
            saved: Instant::now(),
            artwork: None,
            seek_to_end: false,
            advance: None,
            covers: HashMap::new(),
        }
    }
}

impl Controller {
    fn podcast_send(&mut self, command: Command) {
        if let Err(e) = self.podcasts.service.send(command) {
            self.state.notice = e;
        }
    }
    pub(super) fn podcast_action(&mut self, ui: &AppWindow, action: podcast::Action) {
        use podcast::Action as P;
        match action {
            P::Add => {
                self.prepare_portrait_dialog(ui);
                self.podcasts.draft = None;
                self.podcasts.request += 1;
                ui.set_podcast_subscribe_open(true);
                ui.set_podcast_busy(false);
                ui.set_podcast_preview_ready(false);
                ui.set_podcast_preview_title("".into());
                ui.set_podcast_preview_description("".into());
                ui.set_podcast_preview_artwork(slint::Image::default());
                ui.set_podcast_message("".into());
                ui.invoke_focus_screen();
            }
            P::Refresh(id) => {
                self.podcast_send(Command::Refresh(id));
                self.state.notice = self.text("Refreshing episodes…").to_string();
            }
            P::Unsubscribe(id) => {
                self.podcast_send(Command::Unsubscribe(id));
                self.state.navigation.back();
                self.rows_dirty = true;
            }
            P::Download(id) => {
                self.state.podcasts.downloads.entry(id).or_insert((0, None));
                self.podcast_send(Command::Download(id));
            }
            P::CancelDownload(id) => {
                if self
                    .podcasts
                    .autoplay
                    .is_some_and(|(current, _)| current == id)
                {
                    self.podcasts.autoplay = None;
                }
                if self.state.podcasts.downloads.remove(&id).is_some() {
                    self.podcast_send(Command::CancelDownload(id));
                }
            }
            P::DeleteDownload(id) => {
                self.podcast_action(ui, P::CancelDownload(id));
                if self.state.podcasts.active == Some(id) {
                    self.leave_podcast();
                    self.pending_media = None;
                    self.pending_seek = None;
                    self.handoff += 1;
                    self.podcasts.deleting = Some((self.handoff, id));
                    if let Err(e) = self.engine.send(AudioCommand::Stop {
                        handoff: self.handoff,
                    }) {
                        self.state.notice = e;
                    }
                } else {
                    self.podcast_send(Command::DeleteDownload(id));
                }
            }
            P::Mark(id, completed) => {
                if self.state.podcasts.active == Some(id) {
                    self.save_podcast_progress();
                }
                if let Some(e) = self
                    .state
                    .podcasts
                    .library
                    .episodes
                    .iter_mut()
                    .find(|e| e.id == id)
                {
                    e.completed = completed;
                    let position = e.position;
                    self.podcast_send(Command::Progress {
                        id,
                        position,
                        completed: Some(completed),
                    });
                }
            }
            P::Play(id, restart) => {
                self.podcasts.advance = None;
                self.podcasts.autoplay = None;
                let Some(episode) = self
                    .state
                    .podcasts
                    .library
                    .episodes
                    .iter()
                    .find(|e| e.id == id)
                    .cloned()
                else {
                    return;
                };
                let Some(path) = episode.local_path.filter(|p| p.is_file()) else {
                    self.podcasts.autoplay = Some((id, restart));
                    self.podcast_action(ui, P::Download(id));
                    self.state.notice = self.text("Downloading before playback…").to_string();
                    self.rows_dirty = true;
                    return;
                };
                self.save_podcast_progress();
                self.cancel_gapless_prefetch();
                self.playback_session = None;
                self.media_marker = None;
                self.state.video_active = false;
                self.state.paused = true;
                if self.state.current.is_some() {
                    self.state.status = PlaybackStatus::Paused;
                }
                self.state.podcasts.active = Some(id);
                self.state.podcasts.position = if restart
                    || (episode.duration > 0.0 && episode.position >= episode.duration)
                {
                    0.0
                } else {
                    episode.position
                };
                self.state.podcasts.paused = false;
                self.state.podcasts.status = PlaybackStatus::Loading;
                self.podcasts.seek_to_end = false;
                if let Some(e) = self
                    .state
                    .podcasts
                    .library
                    .episodes
                    .iter_mut()
                    .find(|e| e.id == id)
                {
                    e.completed = false;
                    e.last_played = chrono::Utc::now().timestamp();
                }
                self.podcast_send(Command::Progress {
                    id,
                    position: self.state.podcasts.position,
                    completed: Some(false),
                });
                self.state
                    .navigation
                    .open(Screen::Podcasts(PodcastPage::Playing));
                self.state.navigation.select(1);
                self.state.notice.clear();
                self.send_music(AudioCommand::LoadAt {
                    id: AUDIO_ID,
                    path,
                    volume: self.state.effective_volume(),
                    paused: false,
                    start: self.state.podcasts.position,
                    speed: self.state.podcasts.speed,
                });
            }
        }
        self.rows_dirty = true;
    }

    pub(super) fn podcast_command(&mut self, ui: &AppWindow, command: &str) {
        match command {
            "changed" => {
                self.podcasts.draft = None;
                self.podcasts.request += 1;
                ui.set_podcast_preview_ready(false);
                ui.set_podcast_busy(false);
            }
            "close" => {
                self.podcasts.request += 1;
                self.podcasts.draft = None;
                ui.set_podcast_subscribe_open(false);
                ui.set_podcast_busy(false);
                ui.invoke_focus_screen();
            }
            "preview" if !ui.get_podcast_busy() => {
                match podcast::feed::url(ui.get_podcast_url().as_str()) {
                    Ok(url) => {
                        self.podcasts.request += 1;
                        self.podcasts.draft = None;
                        ui.set_podcast_preview_ready(false);
                        ui.set_podcast_busy(true);
                        ui.set_podcast_message(self.text("Loading RSS feed…"));
                        self.podcast_send(Command::Preview {
                            url: url.to_string(),
                            request: self.podcasts.request,
                        });
                    }
                    Err(e) => ui.set_podcast_message(self.text(&e)),
                }
            }
            "subscribe" if !ui.get_podcast_busy() => {
                if let Some(parsed) = self.podcasts.draft.take() {
                    self.podcast_send(Command::Subscribe(Box::new(parsed)));
                    self.podcast_command(ui, "close");
                }
            }
            _ => {}
        }
        self.sync(ui);
    }

    pub(super) fn poll_podcasts(&mut self, ui: &AppWindow) {
        while let Ok(event) = self.podcasts.service.events.try_recv() {
            match event {
                Event::Library(mut library) => {
                    for feed in &library.feeds {
                        if let Some(path) = &feed.artwork_path {
                            self.podcasts.covers.entry(path.clone()).or_insert_with(|| {
                                slint::Image::load_from_path(path).unwrap_or_default()
                            });
                        }
                    }
                    if let Some(id) = self.state.podcasts.active
                        && let Some(e) = library.episodes.iter_mut().find(|e| e.id == id)
                    {
                        e.position = self.state.podcasts.position;
                        if let Some(current) = self.state.podcasts.current() {
                            e.completed = current.completed;
                        }
                    }
                    self.state.podcasts.library = library;
                    self.rows_dirty = true;
                }
                Event::Preview { request, result }
                    if request == self.podcasts.request && ui.get_podcast_subscribe_open() =>
                {
                    ui.set_podcast_busy(false);
                    match result {
                        Ok(parsed) => {
                            ui.set_podcast_preview_artwork(
                                parsed
                                    .feed
                                    .artwork_path
                                    .as_ref()
                                    .and_then(|p| slint::Image::load_from_path(p).ok())
                                    .unwrap_or_default(),
                            );
                            ui.set_podcast_preview_title(parsed.feed.title.clone().into());
                            ui.set_podcast_preview_description(
                                format!("{}\n{}", parsed.feed.author, parsed.feed.description)
                                    .into(),
                            );
                            ui.set_podcast_message(
                                format!(
                                    "{} {}",
                                    parsed.episodes.len(),
                                    self.text("episodes · audio downloads on demand")
                                )
                                .into(),
                            );
                            ui.set_podcast_preview_ready(true);
                            self.podcasts.draft = Some(*parsed);
                        }
                        Err(e) => {
                            ui.set_podcast_message(self.text(&e));
                            ui.set_podcast_preview_ready(false);
                        }
                    }
                }
                Event::Preview { .. } => {}
                Event::DownloadProgress { id, bytes, total } => {
                    self.state.podcasts.downloads.insert(id, (bytes, total));
                    self.rows_dirty = true;
                }
                Event::DownloadFinished { id, result } => {
                    self.state.podcasts.downloads.remove(&id);
                    self.rows_dirty = true;
                    match result {
                        Ok(()) => {
                            if let Some((expected, restart)) = self
                                .podcasts
                                .autoplay
                                .filter(|(expected, _)| *expected == id)
                            {
                                self.podcasts.autoplay = None;
                                self.podcast_action(ui, podcast::Action::Play(expected, restart));
                            }
                        }
                        Err(e) => {
                            if self
                                .podcasts
                                .autoplay
                                .is_some_and(|(expected, _)| expected == id)
                            {
                                self.podcasts.autoplay = None;
                            }
                            self.state.notice = self.text(&e).to_string();
                        }
                    }
                }
                Event::Refreshed(_) => {
                    self.state.notice = self.text("Episodes refreshed").to_string()
                }
                Event::Error(e) => {
                    self.state.notice = self.text(&e).to_string();
                    if ui.get_podcast_subscribe_open() {
                        ui.set_podcast_busy(false);
                        ui.set_podcast_message(self.text(&e));
                    }
                }
            }
        }
        if let Some((id, restart)) = self.podcasts.advance.take() {
            self.podcast_action(ui, podcast::Action::Play(id, restart));
        }
        if self.state.podcasts.active.is_some()
            && self.state.podcasts.status == PlaybackStatus::Playing
            && self.podcasts.saved.elapsed() >= Duration::from_secs(5)
        {
            self.save_podcast_progress();
        }
    }

    pub(super) fn save_podcast_progress(&mut self) {
        if let Some(id) = self.state.podcasts.active {
            let position = self.state.podcasts.position;
            if let Some(e) = self
                .state
                .podcasts
                .library
                .episodes
                .iter_mut()
                .find(|e| e.id == id)
            {
                e.position = position;
                e.last_played = chrono::Utc::now().timestamp();
            }
            self.podcast_send(Command::Progress {
                id,
                position,
                completed: None,
            });
            self.podcasts.saved = Instant::now();
        }
    }
    pub(super) fn leave_podcast(&mut self) {
        self.save_podcast_progress();
        self.state.podcasts.active = None;
        self.podcasts.autoplay = None;
        self.podcasts.advance = None;
        self.media_marker = None;
    }
    pub(super) fn podcast_deletion_pending(&self) -> bool {
        self.podcasts.deleting.is_some()
    }
    pub(super) fn podcast_audio_event(&mut self, event: &AudioEvent) -> bool {
        if let AudioEvent::Stopped { handoff } = event {
            if let Some((expected, id)) = self
                .podcasts
                .deleting
                .filter(|(expected, _)| expected == handoff)
            {
                let _ = expected;
                self.podcasts.deleting = None;
                // A new play of the same episode supersedes the deletion.
                if self.state.podcasts.active != Some(id) {
                    self.podcast_send(Command::DeleteDownload(id));
                }
                // Loads wait for this release barrier, so Stop cannot be invalidated
                // by a newer load before the old file is actually closed on Windows.
                match &self.pending_media {
                    Some(PendingMedia::Music { handoff, .. }) => {
                        if let Err(e) = self
                            .video_engine
                            .send(crate::video::engine::Command::Stop { handoff: *handoff })
                        {
                            self.media_error(e);
                        }
                    }
                    Some(PendingMedia::Video { handoff, .. }) => {
                        if let Err(e) = self.engine.send(AudioCommand::Stop { handoff: *handoff }) {
                            self.media_error(e);
                        }
                    }
                    None => {}
                }
            }
            return false;
        }
        if self.state.podcasts.active.is_none() {
            return false;
        }
        if self.pending_media.is_some() {
            return true;
        }
        match event {
            AudioEvent::Progress {
                id,
                status,
                position,
                ..
            } if *id == AUDIO_ID => {
                self.state.podcasts.position = *position;
                self.state.podcasts.status = if self.state.podcasts.paused {
                    PlaybackStatus::Paused
                } else {
                    *status
                };
            }
            AudioEvent::Finished(id) if *id == AUDIO_ID => {
                self.state.podcasts.status = PlaybackStatus::Stopped;
                self.state.podcasts.paused = true;
                let id = self.state.podcasts.active.unwrap();
                let natural = !self.podcasts.seek_to_end;
                let completed =
                    natural || self.state.podcasts.current().is_some_and(|e| e.completed);
                if let Some(e) = self
                    .state
                    .podcasts
                    .library
                    .episodes
                    .iter_mut()
                    .find(|e| e.id == id)
                {
                    if completed {
                        e.completed = true;
                    }
                    if e.duration > 0.0 {
                        self.state.podcasts.position = e.duration;
                    }
                }
                self.save_podcast_progress();
                self.podcast_send(Command::Progress {
                    id,
                    position: self.state.podcasts.position,
                    completed: Some(completed),
                });
                if natural {
                    self.podcasts.advance = if self.state.podcasts.repeat_episode {
                        Some((id, true))
                    } else {
                        self.state.podcasts.current().and_then(|current| {
                            let mut episodes: Vec<_> = self
                                .state
                                .podcasts
                                .library
                                .episodes
                                .iter()
                                .filter(|e| {
                                    e.feed_id == current.feed_id
                                        && (e.local_path.is_some() || e.media_url.is_some())
                                })
                                .collect();
                            episodes.sort_by_key(|e| (e.published.unwrap_or(0), e.id));
                            let index = episodes.iter().position(|e| e.id == id)?;
                            episodes
                                .into_iter()
                                .skip(index + 1)
                                .find(|e| !e.completed)
                                .map(|e| (e.id, false))
                        })
                    };
                }
                self.rows_dirty = true;
            }
            AudioEvent::Error { id, message } if id.is_none() || *id == Some(AUDIO_ID) => {
                self.state.podcasts.status = PlaybackStatus::Error;
                self.state.notice = format!("Podcast playback error: {message}");
            }
            _ => {}
        }
        true
    }
    pub(super) fn podcast_transport(&mut self, command: &AudioCommand) {
        if self.state.podcasts.active.is_some() {
            match command {
                AudioCommand::Pause(_) => self.save_podcast_progress(),
                AudioCommand::Seek(seconds) | AudioCommand::LoadAt { start: seconds, .. } => {
                    self.podcasts.seek_to_end =
                        self.state.duration() > 0.0 && *seconds >= self.state.duration();
                    self.save_podcast_progress();
                }
                _ => {}
            }
        }
    }
    pub(super) fn podcast_row_icon(&self, target: &Target) -> Option<i32> {
        let unplayed = match target {
            Target::Screen(Screen::Podcasts(PodcastPage::Feed(id))) => self
                .state
                .podcasts
                .library
                .episodes
                .iter()
                .any(|e| e.feed_id == *id && !e.completed),
            Target::Podcast(podcast::Action::Play(id, _)) => self
                .state
                .podcasts
                .library
                .episodes
                .iter()
                .any(|e| e.id == *id && !e.completed),
            _ => return None,
        };
        Some(if unplayed { 11 } else { 10 })
    }
    pub(super) fn podcast_row_artwork(&self, target: &Target) -> Option<slint::Image> {
        let Target::Screen(Screen::Podcasts(PodcastPage::Feed(id))) = target else {
            return None;
        };
        let feed = self
            .state
            .podcasts
            .library
            .feeds
            .iter()
            .find(|f| f.id == *id)?;
        feed.artwork_path
            .as_ref()
            .and_then(|path| self.podcasts.covers.get(path))
            .cloned()
    }
    pub(super) fn podcast_details(&mut self, ui: &AppWindow, index: usize) {
        if let Some(Target::Podcast(podcast::Action::Play(id, _))) =
            self.state.rows().get(index).map(|row| row.target.clone())
        {
            self.state.navigation.select(index);
            self.state
                .navigation
                .open(Screen::Podcasts(PodcastPage::Episode(id)));
            self.rows_dirty = true;
            self.sync(ui);
        }
    }
    pub(super) fn sync_podcasts(&mut self, ui: &AppWindow) {
        let page = match &self.state.navigation.current().screen {
            Screen::Podcasts(page) => Some(page),
            _ => None,
        };
        let detail = if let Some(PodcastPage::Episode(id)) = page {
            self.state
                .podcasts
                .library
                .episodes
                .iter()
                .find(|e| e.id == *id)
        } else {
            None
        };
        let current = detail.or_else(|| self.state.podcasts.current());
        let feed = current.and_then(|e| {
            self.state
                .podcasts
                .library
                .feeds
                .iter()
                .find(|f| f.id == e.feed_id)
        });
        ui.set_podcast_detail(detail.is_some());
        ui.set_podcast_list(matches!(
            page,
            Some(
                PodcastPage::Home
                    | PodcastPage::Feed(_)
                    | PodcastPage::Latest
                    | PodcastPage::Downloads
                    | PodcastPage::Continue
            )
        ));
        ui.set_podcast_overlay(self.state.podcasts.overlay);
        ui.set_podcast_repeat(self.state.podcasts.repeat_episode);
        ui.set_podcast_speed(
            if self.state.podcasts.speed == 0.5 {
                "½X"
            } else if self.state.podcasts.speed == 2.0 {
                "2X"
            } else {
                "1X"
            }
            .into(),
        );
        ui.set_podcast_author(feed.map_or("", |f| f.author.as_str()).into());
        ui.set_podcast_date(
            current
                .and_then(|e| e.published)
                .and_then(|v| chrono::DateTime::from_timestamp(v, 0))
                .map(|v| v.format("%Y-%m-%d").to_string())
                .unwrap_or_default()
                .into(),
        );
        ui.set_podcast_description(
            detail
                .map(|e| format!("{}\n\n{}", e.title, e.description))
                .unwrap_or_default()
                .into(),
        );
        ui.set_podcast_title(current.map_or("", |e| e.title.as_str()).into());
        ui.set_podcast_show(feed.map_or("", |f| f.title.as_str()).into());
        if let Some(PodcastPage::Feed(id)) = page
            && let Some(f) = self
                .state
                .podcasts
                .library
                .feeds
                .iter()
                .find(|f| f.id == *id)
        {
            ui.set_screen_title(f.title.clone().into());
        }
        let artwork = feed.and_then(|f| f.artwork_path.clone());
        if artwork != self.podcasts.artwork {
            ui.set_podcast_artwork(
                artwork
                    .as_ref()
                    .and_then(|p| slint::Image::load_from_path(p).ok())
                    .unwrap_or_default(),
            );
            self.podcasts.artwork = artwork;
        }
    }
    pub(super) fn publish_podcast_snapshot(&mut self) -> bool {
        let Some(e) = self.state.podcasts.current() else {
            return false;
        };
        let marker = (
            Some(AUDIO_ID),
            self.state.podcasts.status,
            self.state.podcasts.position.floor() as i64,
        );
        if self.media_marker == Some(marker) {
            return true;
        }
        self.media_marker = Some(marker);
        let feed = self
            .state
            .podcasts
            .library
            .feeds
            .iter()
            .find(|f| f.id == e.feed_id);
        self.media_bridge
            .publish(Some(crate::platform::macos_media::MediaSnapshot {
                title: e.title.clone(),
                artist: feed.map_or("", |f| f.author.as_str()).into(),
                album: feed.map_or("", |f| f.title.as_str()).into(),
                artwork: feed
                    .and_then(|f| f.artwork_path.as_ref())
                    .and_then(|p| std::fs::read(p).ok()),
                duration_seconds: (e.duration > 0.0).then_some(e.duration),
                elapsed_seconds: self.state.podcasts.position,
                playback_state: if self.state.podcasts.status == PlaybackStatus::Playing {
                    crate::platform::macos_media::PlaybackState::Playing
                } else {
                    crate::platform::macos_media::PlaybackState::Paused
                },
            }));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::podcast::{Episode, Feed, Library};

    #[test]
    fn podcast_events_do_not_update_music_or_cross_pending_handoffs() {
        let directory = tempfile::tempdir().unwrap();
        let mut c = Controller::new(directory.path().into());
        c.state.podcasts.library = Library {
            feeds: vec![Feed {
                id: 1,
                ..Feed::default()
            }],
            episodes: vec![Episode {
                id: 1,
                feed_id: 1,
                duration: 300.0,
                ..Episode::default()
            }],
        };
        c.state.current = Some(0);
        c.state.position = 42.0;
        c.state.podcasts.active = Some(1);
        c.state.podcasts.position = 50.0;
        let progress = |id, position| AudioEvent::Progress {
            id,
            status: PlaybackStatus::Playing,
            position,
            observed_at: Instant::now(),
        };
        assert!(c.podcast_audio_event(&progress(0, 99.0)));
        assert_eq!(c.state.podcasts.position, 50.0);
        assert!(c.podcast_audio_event(&progress(AUDIO_ID, 123.0)));
        assert_eq!(c.state.podcasts.position, 123.0);
        assert_eq!(c.state.position, 42.0);
        c.pending_media = Some(PendingMedia::Music {
            handoff: 9,
            command: AudioCommand::Pause(false),
        });
        c.podcast_audio_event(&progress(AUDIO_ID, 200.0));
        assert_eq!(c.state.podcasts.position, 123.0);
        c.pending_media = None;
        c.podcast_audio_event(&AudioEvent::Finished(AUDIO_ID));
        assert!(c.state.podcasts.current().unwrap().completed);
        c.state.podcasts.library.episodes[0].completed = false;
        c.podcasts.seek_to_end = true;
        c.podcast_audio_event(&AudioEvent::Finished(AUDIO_ID));
        assert!(!c.state.podcasts.current().unwrap().completed);
        c.podcasts.autoplay = Some((1, false));
        c.send_music(AudioCommand::Load {
            id: 0,
            path: directory.path().join("missing.wav"),
            volume: 0.5,
            paused: false,
        });
        assert!(c.state.podcasts.active.is_none());
        assert!(c.podcasts.autoplay.is_none());
    }

    #[test]
    fn native_continuation_skips_played_episodes_and_seek_to_end_does_not_continue() {
        let directory = tempfile::tempdir().unwrap();
        let mut c = Controller::new(directory.path().into());
        c.state.podcasts.library.episodes = (1..=3)
            .map(|id| Episode {
                id,
                feed_id: 1,
                published: Some(id * 100),
                completed: id == 2,
                position: if id == 3 { 10.0 } else { 0.0 },
                media_url: Some(format!("https://example.org/{id}.mp3")),
                ..Episode::default()
            })
            .collect();
        c.state.podcasts.active = Some(1);
        c.podcast_audio_event(&AudioEvent::Finished(AUDIO_ID));
        assert_eq!(c.podcasts.advance, Some((3, false)));
        c.podcasts.advance = None;
        c.state.podcasts.repeat_episode = true;
        c.podcast_audio_event(&AudioEvent::Finished(AUDIO_ID));
        assert_eq!(c.podcasts.advance, Some((1, true)));
        c.podcasts.advance = None;
        c.podcasts.seek_to_end = true;
        c.podcast_audio_event(&AudioEvent::Finished(AUDIO_ID));
        assert!(c.podcasts.advance.is_none());
        assert!(c.state.podcasts.current().unwrap().completed);
    }

    #[test]
    fn deleting_download_waits_for_release_before_starting_the_next_owner() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = podcast::store::Store::open(directory.path()).unwrap();
        store
            .merge(
                ParsedFeed {
                    feed: Feed {
                        url: "https://example.org/feed".into(),
                        ..Feed::default()
                    },
                    episodes: vec![Episode {
                        title: "Episode".into(),
                        ..Episode::default()
                    }],
                    aliases: vec![],
                },
                None,
            )
            .unwrap();
        let id = store.library().unwrap().episodes[0].id;
        std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quiet.wav"),
            store.path(id),
        )
        .unwrap();
        store.downloaded(id, 1.0).unwrap();
        let library = store.library().unwrap();
        let file = store.path(id);
        drop(store);
        let mut c = Controller::new(directory.path().into());
        c.state.podcasts.library = library;
        c.handoff = 7;
        c.podcasts.deleting = Some((7, id));
        c.send_music(AudioCommand::Load {
            id: 0,
            path: directory.path().join("missing.wav"),
            volume: 0.5,
            paused: false,
        });
        assert!(c.podcast_deletion_pending());
        assert!(matches!(
            c.pending_media,
            Some(PendingMedia::Music { handoff: 8, .. })
        ));
        c.podcast_audio_event(&AudioEvent::Stopped { handoff: 6 });
        assert!(c.podcast_deletion_pending());
        assert!(file.exists());
        c.podcast_audio_event(&AudioEvent::Stopped { handoff: 7 });
        assert!(!c.podcast_deletion_pending());
        c.podcasts.service.shutdown();
        assert!(!file.exists());
    }
}
