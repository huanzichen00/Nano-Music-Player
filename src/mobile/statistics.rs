//! Ratings and smart-list projections around the shared persistence worker.
//! Listening credit comes only from the engine's timed position samples.
use super::{
    Controller,
    navigation::{Entry, Filter, Page, Selection},
    statistics_service::Event,
};
use crate::{
    audio::PlaybackStatus,
    library::{
        Track,
        statistics::{ListeningSession, QualifiedPlay, SmartPlaylist},
    },
    shared::i18n,
    ui::MobileWindow,
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, Instant},
};

fn top_rated(tracks: &[Track], ratings: &HashMap<PathBuf, u8>) -> Vec<usize> {
    let mut ids = tracks
        .iter()
        .enumerate()
        .filter(|(_, track)| ratings.get(&track.path).copied().unwrap_or(0) >= 4)
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    ids.sort_by(|&left, &right| {
        let left = &tracks[left];
        let right = &tracks[right];
        ratings[&right.path]
            .cmp(&ratings[&left.path])
            .then_with(|| {
                super::title(left)
                    .to_lowercase()
                    .cmp(&super::title(right).to_lowercase())
            })
            .then_with(|| super::title(left).cmp(super::title(right)))
            .then_with(|| left.path.cmp(&right.path))
    });
    ids
}

fn smart_order(tracks: &[Track], paths: &[PathBuf]) -> Vec<usize> {
    let available: HashMap<_, _> = tracks
        .iter()
        .enumerate()
        .map(|(id, track)| (&track.path, id))
        .collect();
    let mut included = HashSet::new();
    paths
        .iter()
        .filter_map(|path| available.get(path).copied())
        .filter(|id| included.insert(*id))
        .collect()
}

fn new_listening_session(track: &Track, timestamp: Duration, paused: bool) -> ListeningSession {
    let duration = Duration::try_from_secs_f64(track.duration)
        .ok()
        .filter(|duration| !duration.is_zero());
    let mut session = ListeningSession::new(track.path.clone(), duration);
    session.set_paused(timestamp, paused);
    session
}

impl Controller {
    pub(super) fn on_stats_library_loaded(&mut self) {
        self.statistics
            .set_library(self.tracks.iter().map(|track| track.path.clone()).collect());
        let current = self.current().map(|track| &track.path);
        if self.listening_id != Some(self.playback.session)
            || self.listening.as_ref().map(ListeningSession::path) != current.map(PathBuf::as_path)
        {
            self.on_stats_track_change();
        }
    }

    /// Explicit skips/reloads start a new tracker without completing the old
    /// song. Natural Finished/Transitioned callers complete before switching.
    pub(super) fn on_stats_track_change(&mut self) {
        self.listening = self.current().map(|track| {
            new_listening_session(
                track,
                self.listening_clock.elapsed(),
                !self.playback.should_play(),
            )
        });
        self.listening_id = self.listening.as_ref().map(|_| self.playback.session);
    }

    pub(super) fn observe_stats(
        &mut self,
        id: usize,
        status: PlaybackStatus,
        position: f64,
        observed_at: Instant,
    ) {
        if id != self.playback.session || self.playback.current.is_none() {
            return;
        }
        let Ok(position) = Duration::try_from_secs_f64(position) else {
            return;
        };
        if self.listening_id != Some(id) {
            self.on_stats_track_change();
        }
        let timestamp = observed_at.saturating_duration_since(self.listening_clock);
        let playing = status == PlaybackStatus::Playing && self.playback.should_play();
        let play = self
            .listening
            .as_mut()
            .and_then(|session| session.observe(timestamp, position, playing));
        self.record_stats(play);
    }

    pub(super) fn pause_stats(&mut self) {
        if let Some(session) = &mut self.listening {
            session.set_paused(self.listening_clock.elapsed(), !self.playback.should_play());
        }
    }

    pub(super) fn seek_stats(&mut self) {
        if let Some(session) = &mut self.listening {
            session.seek_reset(self.listening_clock.elapsed());
        }
    }

    pub(super) fn complete_stats(&mut self, id: usize) {
        if self.listening_id != Some(id) || self.playback.session != id {
            return;
        }
        let play = self
            .listening
            .as_mut()
            .and_then(|session| session.complete(self.listening_clock.elapsed()));
        self.record_stats(play);
    }

    fn record_stats(&self, play: Option<QualifiedPlay>) {
        if let Some(play) = play {
            self.statistics.record_play(play);
        }
    }

    pub(super) fn poll_statistics(&mut self, ui: &MobileWindow) {
        let mut changed = false;
        while let Some(event) = self.statistics.poll(&self.ratings) {
            match event {
                Event::RatingsLoaded(ratings) => {
                    self.ratings = ratings;
                    self.ratings_ready = true;
                    changed = true;
                }
                Event::RatingReverted { path, stars } => {
                    self.ratings.insert(path, stars);
                    self.notice = "Could not save rating".into();
                    changed = true;
                }
                Event::SmartPlaylists(playlists) => {
                    changed |= self.smart_playlists != playlists;
                    self.smart_playlists = playlists;
                }
                Event::Unavailable(message) => {
                    self.notice = format!("Playback statistics unavailable: {message}");
                }
            }
        }
        if changed {
            self.rebuild_rows(ui);
            self.sync_statistics_ui(ui);
        }
    }

    fn stats_rating_target(&self) -> Option<&Track> {
        match self.navigation.current() {
            Page::Lyrics(_) | Page::LyricsSearch(_) => self.annotation_track_ref(),
            Page::NowPlaying => self.current(),
            _ => None,
        }
    }

    pub(super) fn sync_statistics_ui(&self, ui: &MobileWindow) {
        let target = self.stats_rating_target();
        ui.set_rating_visible(self.ratings_ready && target.is_some());
        ui.set_rating_title(target.map(super::title).unwrap_or_default().into());
        ui.set_rating(
            target
                .and_then(|track| self.ratings.get(&track.path))
                .copied()
                .unwrap_or(0) as i32,
        );
    }

    pub(super) fn rate_track(&mut self, ui: &MobileWindow, stars: u8) {
        if stars > 5 || !self.ratings_ready {
            return;
        }
        let Some(path) = self.stats_rating_target().map(|track| track.path.clone()) else {
            return;
        };
        let old = self.ratings.get(&path).copied().unwrap_or(0);
        if old == stars {
            return;
        }
        self.ratings.insert(path.clone(), stars);
        if let Err(previous) = self.statistics.rate(&path, stars, old) {
            self.ratings.insert(path, previous);
            self.notice = "Could not save rating".into();
        }
        self.rebuild_rows(ui);
        self.sync_statistics_ui(ui);
        self.present(ui);
    }

    pub(super) fn stats_rows(&self) -> Option<Vec<Entry>> {
        let order = match self.navigation.current() {
            Page::Songs(Filter::TopRated) => top_rated(&self.tracks, &self.ratings),
            Page::Songs(Filter::SmartPlaylist(kind)) => smart_order(
                &self.tracks,
                self.smart_playlists
                    .get(kind)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
            ),
            _ => return None,
        };
        let order: std::rc::Rc<[usize]> = order.into();
        let mut rows = Vec::with_capacity(order.len() + 1);
        if let Some(&first) = order.first() {
            rows.push(Entry {
                title: i18n::translate(self.settings.language, "Shuffle"),
                subtitle: String::new(),
                icon: 0,
                chevron: false,
                show_artwork: false,
                track: None,
                action: Selection::Play {
                    track: first,
                    order: order.clone(),
                    shuffle: true,
                },
            });
        }
        rows.extend(order.iter().map(|&id| Entry {
            title: super::title(&self.tracks[id]).into(),
            subtitle: self.tracks[id].artist.clone(),
            icon: 0,
            chevron: false,
            show_artwork: false,
            track: Some(id),
            action: Selection::Play {
                track: id,
                order: order.clone(),
                shuffle: false,
            },
        }));
        Some(rows)
    }

    pub(super) fn merge_stats_menu(&self, rows: &mut Vec<Entry>) {
        if *self.navigation.current() != Page::Music {
            return;
        }
        let mut lists = vec![(
            "My Top Rated",
            Filter::TopRated,
            top_rated(&self.tracks, &self.ratings).len(),
        )];
        lists.extend(SmartPlaylist::ALL.into_iter().map(|kind| {
            let count = smart_order(
                &self.tracks,
                self.smart_playlists
                    .get(&kind)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
            )
            .len();
            (kind.label(), Filter::SmartPlaylist(kind), count)
        }));
        for (label, filter, count) in lists {
            let page = Page::Songs(filter);
            if rows
                .iter()
                .any(|row| row.action == Selection::Open(page.clone()))
            {
                continue;
            }
            rows.push(Entry {
                title: i18n::translate(self.settings.language, label),
                subtitle: format!(
                    "{count}{}",
                    i18n::translate(self.settings.language, " songs")
                ),
                icon: 0,
                chevron: true,
                show_artwork: false,
                track: None,
                action: Selection::Open(page),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(path: &str, title: &str, duration: f64) -> Track {
        Track {
            path: PathBuf::from(path),
            title: title.into(),
            artist: "Artist".into(),
            album: String::new(),
            genre: String::new(),
            number: 0,
            duration,
            artwork: Vec::new(),
            lyrics: Default::default(),
        }
    }

    #[test]
    fn rated_rankings_use_confirmed_paths_and_stable_ties() {
        let tracks = vec![
            track("/b.wav", "Same", 60.0),
            track("/a.wav", "Same", 60.0),
            track("/c.wav", "First", 60.0),
            track("/d.wav", "Low", 60.0),
        ];
        let ratings = HashMap::from([
            (PathBuf::from("/b.wav"), 4),
            (PathBuf::from("/a.wav"), 4),
            (PathBuf::from("/c.wav"), 5),
            (PathBuf::from("/d.wav"), 3),
            (PathBuf::from("/gone.wav"), 5),
        ]);
        assert_eq!(top_rated(&tracks, &ratings), vec![2, 1, 0]);
    }

    #[test]
    fn smart_rankings_survive_library_resort_without_adding_missing_or_unranked_songs() {
        let tracks = vec![
            track("/c.wav", "C", 60.0),
            track("/a.wav", "A", 60.0),
            track("/b.wav", "B", 60.0),
        ];
        let paths = ["/b.wav", "/gone.wav", "/a.wav", "/b.wav"].map(PathBuf::from);
        assert_eq!(smart_order(&tracks, &paths), vec![2, 1]);
        assert!(smart_order(&tracks, &[]).is_empty());
    }

    #[test]
    fn rating_presentation_borrows_the_correct_song_after_a_library_resort() {
        slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
            Default::default(),
        )))
        .unwrap();
        let ui = MobileWindow::new().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let storage = super::super::storage::Storage::new(directory.path().into()).unwrap();
        let mut controller = Controller::new(storage).unwrap();
        controller.tracks = vec![
            track("/playing.wav", "Playing", 60.0),
            track("/selected.wav", "Selected", 60.0),
        ];
        // UI metadata must borrow the track, including potentially large artwork.
        controller.tracks[0].artwork = vec![7; 1024 * 1024];
        controller.tracks[1].artwork = vec![9; 1024 * 1024];
        controller.playback.current = Some(0);
        controller.ratings_ready = true;
        controller.ratings.insert("/playing.wav".into(), 3);
        controller.ratings.insert("/selected.wav".into(), 5);
        controller.navigation.open(Page::NowPlaying);
        assert!(std::ptr::eq(
            controller.stats_rating_target().unwrap(),
            &controller.tracks[0]
        ));
        controller.sync_statistics_ui(&ui);
        assert_eq!(ui.get_rating_title(), "Playing");
        assert_eq!(ui.get_rating(), 3);

        controller.prepare_annotation_page(&Page::Lyrics(1));
        controller.navigation.open(Page::Lyrics(1));
        controller.tracks.swap(0, 1);
        assert!(std::ptr::eq(
            controller.stats_rating_target().unwrap(),
            &controller.tracks[0]
        ));
        controller.sync_statistics_ui(&ui);
        assert_eq!(ui.get_rating_title(), "Selected");
        assert_eq!(ui.get_rating(), 5);
        assert!(ui.get_rating_visible());

        controller.navigation.home();
        controller.sync_statistics_ui(&ui);
        assert!(!ui.get_rating_visible());
    }

    #[test]
    fn paused_loads_and_seek_jumps_do_not_create_play_counts() {
        let song = track("/song.wav", "Song", 10.0);
        let mut listening = new_listening_session(&song, Duration::ZERO, true);
        for second in 0..10 {
            assert!(
                listening
                    .observe(
                        Duration::from_secs(second),
                        Duration::from_secs(second),
                        true
                    )
                    .is_none()
            );
        }
        assert_eq!(listening.listened(), Duration::ZERO);
        listening.set_paused(Duration::from_secs(10), false);
        listening.seek_reset(Duration::from_secs(11));
        assert!(
            listening
                .observe(Duration::from_secs(11), Duration::from_secs(9), true)
                .is_none()
        );
        assert!(listening.complete(Duration::from_secs(12)).is_none());
    }

    #[test]
    fn actual_advancing_playback_qualifies_once_and_a_repeat_starts_fresh() {
        let song = track("/song.wav", "Song", 10.0);
        let mut listening = new_listening_session(&song, Duration::ZERO, false);
        let plays: Vec<_> = (0..=8)
            .filter_map(|second| {
                listening.observe(
                    Duration::from_secs(second),
                    Duration::from_secs(second),
                    true,
                )
            })
            .collect();
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].path, song.path);
        assert_eq!(plays[0].listened, Duration::from_secs(5));
        assert!(listening.complete(Duration::from_secs(9)).is_none());
        let repeated = new_listening_session(&song, Duration::from_secs(9), false);
        assert!(!repeated.counted());
        assert_eq!(repeated.listened(), Duration::ZERO);
    }
}
