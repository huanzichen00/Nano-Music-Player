//! Audio/video commands, listening sessions, and gapless handoff coordination.
//! Worker ownership and polling order stay with the controller.

use super::Controller;
use crate::audio::{PlaybackStatus, engine::AudioCommand};

pub(super) enum PendingMedia {
    Music {
        handoff: u64,
        command: AudioCommand,
    },
    Video {
        handoff: u64,
        command: crate::video::engine::Command,
    },
}

impl Controller {
    pub(super) fn media_error(&mut self, message: String) {
        self.practice.cancel();
        self.pending_media = None;
        self.pending_seek = None;
        if self.state.video_active {
            self.state.video_status = crate::video::Status::Error;
        } else if self.state.podcasts.active.is_some() {
            self.state.podcasts.status = PlaybackStatus::Error;
        } else {
            self.state.status = PlaybackStatus::Error;
        }
        self.state.notice = format!("Playback error: {message}");
    }

    pub(super) fn send_music(&mut self, mut command: AudioCommand) {
        self.prepare_practice_load(&mut command);
        self.save_video_progress(true);
        if matches!(
            command,
            AudioCommand::Load { .. } | AudioCommand::LoadWithGain { .. }
        ) {
            self.leave_podcast();
        }
        self.podcast_transport(&command);
        let command = match command {
            AudioCommand::Load {
                id,
                path,
                volume,
                paused,
            } => {
                self.begin_playback_session(id);
                AudioCommand::LoadWithGain {
                    id,
                    path,
                    volume,
                    paused,
                    gain: self.gain_config(),
                }
            }
            AudioCommand::LoadWithGain {
                id,
                path,
                volume,
                paused,
                ..
            } => {
                self.begin_playback_session(id);
                AudioCommand::LoadWithGain {
                    id,
                    path,
                    volume,
                    paused,
                    gain: self.gain_config(),
                }
            }
            AudioCommand::Seek(seconds) => {
                self.media_marker = None;
                self.failed_prefetch = None;
                self.prefetched = None;
                if let Some(session) = &mut self.playback_session {
                    session.seek_reset(self.listening_clock.elapsed());
                }
                AudioCommand::Seek(seconds)
            }
            AudioCommand::Pause(paused) => {
                if let Some(session) = &mut self.playback_session {
                    session.set_paused(self.listening_clock.elapsed(), paused);
                }
                AudioCommand::Pause(paused)
            }
            other => other,
        };
        if matches!(
            command,
            AudioCommand::LoadWithGain { .. }
                | AudioCommand::Load { .. }
                | AudioCommand::LoadAt { .. }
        ) {
            self.handoff += 1;
            self.pending_seek = None;
            self.cancel_gapless_prefetch();
            self.pending_media = Some(PendingMedia::Music {
                handoff: self.handoff,
                command,
            });
            if self.podcast_deletion_pending() {
                return;
            }
            if let Err(error) = self.video_engine.send(crate::video::engine::Command::Stop {
                handoff: self.handoff,
            }) {
                self.media_error(error);
            }
        } else if self.pending_media.is_some() {
            if let AudioCommand::Seek(seconds) = command {
                self.pending_seek = Some(seconds);
            }
            // Pause and volume are read from authoritative reducer state at the acknowledgement.
        } else if let Err(error) = self.engine.send(command) {
            self.media_error(error);
        }
    }

    pub(super) fn gain_config(&self) -> crate::audio::gain::GainConfig {
        crate::audio::gain::GainConfig {
            mode: self.state.replay_gain_mode,
            preamp_db: self.state.replay_gain_preamp_db,
            prevent_clipping: self.state.replay_gain_prevent_clipping,
        }
    }

    pub(super) fn begin_playback_session(&mut self, id: usize) {
        let Some(track) = self.state.tracks.get(id) else {
            self.playback_session = None;
            return;
        };
        let duration = (track.duration.is_finite() && track.duration > 0.0)
            .then(|| std::time::Duration::from_secs_f64(track.duration));
        let mut session =
            crate::library::statistics::ListeningSession::new(track.path.clone(), duration);
        session.set_paused(self.listening_clock.elapsed(), self.state.paused);
        self.playback_session = Some(session);
        self.failed_prefetch = None;
    }

    pub(super) fn update_playback_session(
        &mut self,
        id: usize,
        status: PlaybackStatus,
        position: f64,
        observed_at: std::time::Instant,
    ) {
        if !position.is_finite() || position < 0.0 {
            return;
        }
        let Some(track) = self.state.tracks.get(id) else {
            return;
        };
        if self
            .playback_session
            .as_ref()
            .is_none_or(|s| s.path() != track.path)
        {
            self.begin_playback_session(id);
        }
        let timestamp = observed_at.saturating_duration_since(self.listening_clock);
        let play = self.playback_session.as_mut().and_then(|session| {
            session.observe(
                timestamp,
                std::time::Duration::from_secs_f64(position),
                status == PlaybackStatus::Playing && !self.state.paused,
            )
        });
        self.record_qualified_play(play);
    }

    pub(super) fn complete_playback_session(&mut self) {
        let play = self
            .playback_session
            .as_mut()
            .and_then(|session| session.complete(self.listening_clock.elapsed()));
        self.record_qualified_play(play);
    }

    fn record_qualified_play(&self, play: Option<crate::library::statistics::QualifiedPlay>) {
        if let Some(play) = play {
            self.statistics.record_play(play);
        }
    }

    pub(super) fn schedule_gapless_prefetch(&mut self) {
        if self.podcast_deletion_pending()
            || self.state.podcasts.active.is_some()
            || self.pending_media.is_some()
            || self.state.video_active
            || self.practice.enabled()
            || !self.state.gapless_playback
            || matches!(
                self.state.status,
                PlaybackStatus::Stopped | PlaybackStatus::Error
            )
        {
            self.cancel_gapless_prefetch();
            return;
        }
        let Some(current) = self.state.current else {
            self.cancel_gapless_prefetch();
            return;
        };
        let Some((_, next)) = self.state.gapless_successor() else {
            self.cancel_gapless_prefetch();
            return;
        };
        if self.failed_prefetch == Some((current, next)) {
            self.cancel_gapless_prefetch();
            return;
        }
        if self
            .prefetched
            .is_some_and(|(from, to, _)| from == current && to == next)
        {
            return;
        }
        self.prefetch_token = self.prefetch_token.wrapping_add(1);
        let token = self.prefetch_token;
        let Some(track) = self.state.tracks.get(next) else {
            return;
        };
        if self
            .engine
            .send(AudioCommand::AppendNext {
                token,
                id: next,
                path: track.path.clone(),
                gain: self.gain_config(),
            })
            .is_ok()
        {
            self.prefetched = Some((current, next, token));
        }
    }

    pub(super) fn cancel_gapless_prefetch(&mut self) {
        if let Some((_, _, token)) = self.prefetched.take() {
            let _ = self.engine.send(AudioCommand::CancelPrefetch { token });
        }
    }

    pub(super) fn send_video(&mut self, command: crate::video::engine::Command) {
        use crate::video::engine::Command;
        if matches!(
            command,
            Command::Load { .. } | Command::Pause { .. } | Command::Seek { .. }
        ) {
            self.save_video_progress(true);
        }
        if let Command::Load { video, .. } = &command {
            self.practice.set_enabled(false);
            self.leave_podcast();
            self.playback_session = None;
            self.media_marker = None;
            self.media_bridge.publish(None);
            self.pending_subtitle = Some(self.state.video_subtitles.get(&video.path));
            self.subtitle_tracks.clear();
            self.handoff += 1;
            self.cancel_gapless_prefetch();
            self.pending_seek = None;
            self.pending_media = Some(PendingMedia::Video {
                handoff: self.handoff,
                command,
            });
            if self.podcast_deletion_pending() {
                return;
            }
            if let Err(error) = self.engine.send(AudioCommand::Stop {
                handoff: self.handoff,
            }) {
                self.media_error(error);
            }
        } else if self.pending_media.is_some() {
            match command {
                Command::Seek { seconds, .. } => self.pending_seek = Some(seconds),
                Command::Subtitle { preference, .. } => {
                    self.pending_subtitle = Some(preference);
                }
                _ => {}
            }
        } else if let Err(error) = self.video_engine.send(command) {
            self.media_error(error);
        }
    }

    pub(super) fn finish_video_handoff(&mut self, handoff: u64) {
        if !matches!(&self.pending_media, Some(PendingMedia::Video { handoff: expected, .. }) if *expected == handoff)
        {
            return;
        }
        let Some(PendingMedia::Video { mut command, .. }) = self.pending_media.take() else {
            return;
        };
        if let crate::video::engine::Command::Load {
            volume,
            paused,
            start,
            ..
        } = &mut command
        {
            *volume = self.state.effective_volume();
            *paused = self.state.video_paused;
            if let Some(seconds) = self.pending_seek.take() {
                *start = seconds;
            }
        }
        if let Err(error) = self.video_engine.send(command) {
            self.media_error(error);
            return;
        }
        if let Some(preference) = self.pending_subtitle.take()
            && let Err(error) = self
                .video_engine
                .send(crate::video::engine::Command::Subtitle {
                    generation: self.state.video_generation,
                    preference,
                })
        {
            self.media_error(error);
        }
    }

    pub(super) fn finish_music_handoff(&mut self, handoff: u64) {
        if !matches!(&self.pending_media, Some(PendingMedia::Music { handoff: expected, .. }) if *expected == handoff)
        {
            return;
        }
        let Some(PendingMedia::Music { mut command, .. }) = self.pending_media.take() else {
            return;
        };
        if let AudioCommand::Load { volume, paused, .. }
        | AudioCommand::LoadWithGain { volume, paused, .. }
        | AudioCommand::LoadAt { volume, paused, .. } = &mut command
        {
            *volume = self.state.effective_volume();
            *paused = if self.state.podcasts.active.is_some() {
                self.state.podcasts.paused
            } else {
                self.state.paused
            };
        }
        if let AudioCommand::LoadAt { start, speed, .. } = &mut command {
            *speed = self.state.podcasts.speed;
            if let Some(seconds) = self.pending_seek.take() {
                *start = seconds;
            }
        }
        if let Err(error) = self.engine.send(command) {
            self.media_error(error);
            return;
        }
        if let Some(seconds) = self.pending_seek.take() {
            self.send_music(AudioCommand::Seek(seconds));
        }
        self.schedule_gapless_prefetch();
    }
}
