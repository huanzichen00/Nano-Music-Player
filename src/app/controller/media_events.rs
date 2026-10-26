//! Drain engine events and publish system-media state at the existing poll points.
//! The controller retains worker ownership and task scheduling.

use super::Controller;
use crate::{
    audio::{PlaybackStatus, engine::AudioEvent},
    ui::AppWindow,
};
use slint::{ModelRc, VecModel};

impl Controller {
    pub(super) fn poll_audio_events(&mut self, ui: &AppWindow) {
        while let Ok(event) = self.engine.events.try_recv() {
            self.handle_audio_event(ui, event);
        }
    }

    pub(super) fn handle_audio_event(&mut self, ui: &AppWindow, event: AudioEvent) {
        if self.podcast_audio_event(&event) {
            return;
        }
        match event {
            AudioEvent::Stopped { handoff } => self.finish_video_handoff(handoff),
            AudioEvent::Progress {
                id,
                status,
                position,
                observed_at,
            } if !self.state.video_active
                && self.pending_media.is_none()
                && self.state.current == Some(id) =>
            {
                self.update_playback_session(id, status, position, observed_at);
                self.practice_progress(id, status, position, std::time::Instant::now());
                self.state.position = position;
                self.state.status = if self.state.paused {
                    PlaybackStatus::Paused
                } else {
                    status
                };
            }
            AudioEvent::Transitioned { from, to, token: _ }
                if !self.state.video_active
                    && self.pending_media.is_none()
                    && self.state.current == Some(from) =>
            {
                let Some(has_lyrics) = self.state.tracks.get(to).map(|t| !t.lyrics.is_empty())
                else {
                    return;
                };
                self.complete_playback_session();
                self.prefetched = None;
                // The source may have started just before repeat/shuffle/gain intent
                // changed. Worker generation filtering makes this authoritative.
                let position = self
                    .state
                    .gapless_successor()
                    .filter(|(_, next)| *next == to)
                    .map(|(position, _)| position)
                    .or_else(|| self.state.queue.order.iter().position(|id| *id == to));
                if let Some(position) = position {
                    self.state.queue.position = position;
                }
                self.state.current = Some(to);
                self.state.position = 0.0;
                self.state.status = if self.state.paused {
                    PlaybackStatus::Paused
                } else {
                    PlaybackStatus::Playing
                };
                self.state.lyrics_visible = has_lyrics;
                self.state.overlay_visible = self.state.lyrics_visible;
                self.last_track = None;
                self.lyrics_dirty = true;
                self.begin_playback_session(to);
                self.rows_dirty = true;
                self.schedule_gapless_prefetch();
            }
            AudioEvent::PrefetchFailed { id, token, message }
                if self
                    .prefetched
                    .is_some_and(|(_, to, expected)| to == id && expected == token) =>
            {
                if let Some((from, to, _)) = self.prefetched.take() {
                    self.failed_prefetch = Some((from, to));
                }
                self.state.notice = format!("Could not prepare next track: {message}");
            }
            AudioEvent::Finished(id)
                if !self.state.video_active
                    && self.pending_media.is_none()
                    && self.state.current == Some(id) =>
            {
                self.complete_playback_session();
                if self.practice.enabled() {
                    self.practice.cancel();
                    self.state.paused = true;
                    self.state.status = PlaybackStatus::Stopped;
                    self.state.position = self.state.duration();
                    return;
                }
                let effects = self.state.advance(false);
                self.effects(ui, effects);
            }
            AudioEvent::Error { id, message }
                if !self.state.video_active
                    && self.pending_media.is_none()
                    && (id.is_none() || id == self.state.current) =>
            {
                self.practice.cancel();
                self.state.status = PlaybackStatus::Error;
                self.state.notice = format!("Playback error: {message}");
            }
            _ => {}
        }
    }

    pub(super) fn poll_video_events(&mut self, ui: &AppWindow) {
        while let Ok(event) = self.video_engine.events.try_recv() {
            match event {
                crate::video::engine::Event::MissingFile { generation, path }
                    if self.state.video_active && generation == self.state.video_generation =>
                {
                    self.state.video_status = crate::video::Status::Error;
                    self.state.video_paused = true;
                    ui.set_video_frame(slint::Image::default());
                    self.video_tasks.remove_missing(path);
                }
                crate::video::engine::Event::SubtitleTracks { generation, tracks }
                    if self.state.video_active && generation == self.state.video_generation =>
                {
                    self.subtitle_tracks = tracks;
                    ui.set_subtitle_choices(ModelRc::new(VecModel::from(self.subtitle_choices())));
                }
                crate::video::engine::Event::SubtitleError {
                    generation,
                    message,
                } if self.state.video_active && generation == self.state.video_generation => {
                    self.state.notice = format!("Subtitle error: {message}");
                }
                crate::video::engine::Event::Stopped { handoff } => {
                    self.finish_music_handoff(handoff)
                }
                crate::video::engine::Event::Finished(generation)
                    if self.state.video_active && generation == self.state.video_generation =>
                {
                    self.state.video_status = crate::video::Status::Finished;
                    self.state.video_paused = true;
                    self.state.video_position = self.state.duration();
                    self.state.video_overlay = true;
                }
                crate::video::engine::Event::Error {
                    generation,
                    message,
                } if self.state.video_active && generation == self.state.video_generation => {
                    self.state.video_status = crate::video::Status::Error;
                    self.state.video_paused = true;
                    self.state.notice = format!("Playback error: {message}");
                    ui.set_video_frame(slint::Image::default());
                }
                _ => {}
            }
        }
        if let Some(progress) = self.video_engine.take_progress()
            && self.state.video_active
            && self.pending_media.is_none()
            && progress.generation == self.state.video_generation
        {
            self.state.video_position = progress.position;
            self.state.video_status = match progress.status {
                crate::video::Status::Playing | crate::video::Status::Paused
                    if self.state.video_paused =>
                {
                    crate::video::Status::Paused
                }
                status => status,
            };
        }
    }

    pub(super) fn poll_media_commands(&mut self, ui: &AppWindow) {
        while let Some(command) = self.media_bridge.poll_command() {
            self.handle_media_command(ui, command);
        }
    }

    pub(super) fn publish_media_snapshot(&mut self) {
        if self.publish_podcast_snapshot() {
            return;
        }
        let current = self.state.current;
        let position = self.state.position.floor().max(0.0) as i64;
        let marker = (current, self.state.status, position);
        if self.media_marker == Some(marker) {
            return;
        }
        self.media_marker = Some(marker);
        let snapshot = current
            .filter(|_| {
                !self.state.video_active
                    && matches!(
                        self.state.status,
                        PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Loading
                    )
            })
            .and_then(|id| self.state.tracks.get(id))
            .map(|track| crate::platform::macos_media::MediaSnapshot {
                title: track.title.clone(),
                artist: track.artist.clone(),
                album: track.album.clone(),
                artwork: (!track.artwork.is_empty()).then(|| track.artwork.clone()),
                duration_seconds: (track.duration.is_finite() && track.duration > 0.0)
                    .then_some(track.duration),
                elapsed_seconds: position as f64,
                playback_state: match self.state.status {
                    PlaybackStatus::Playing => crate::platform::macos_media::PlaybackState::Playing,
                    PlaybackStatus::Paused | PlaybackStatus::Loading => {
                        crate::platform::macos_media::PlaybackState::Paused
                    }
                    _ => crate::platform::macos_media::PlaybackState::Stopped,
                },
            });
        self.media_bridge.publish(snapshot);
    }
}

#[cfg(test)]
#[path = "media_events_tests.rs"]
mod tests;
