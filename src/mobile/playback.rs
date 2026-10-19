//! Playback intent separate from output acknowledgements and native interruptions.
use crate::audio::{PlaybackStatus, Repeat, queue::Queue};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Load {
    pub track: usize,
    pub session: usize,
    pub paused: bool,
}

#[derive(Default)]
pub(super) struct Playback {
    pub queue: Queue,
    pub current: Option<usize>,
    pub session: usize,
    pub status: PlaybackStatus,
    pub position: f64,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub wants_playing: bool,
    interruption: Option<(bool, u64)>,
    revision: u64,
    finished: bool,
    session_counter: usize,
    load_generation: u64,
}

impl Playback {
    pub fn select(&mut self, track: usize, order: Vec<usize>) -> Load {
        self.queue.set(order, track, self.shuffle);
        self.user_intent(true);
        self.load(track)
    }

    fn load(&mut self, track: usize) -> Load {
        // Audio IDs identify a load, never a library index. Repeat One and rapid
        // next/back therefore cannot accept an old Finished for the same song.
        self.load_generation = self.load_generation.wrapping_add(1);
        self.session = self.reserve_session();
        self.current = Some(track);
        self.position = 0.0;
        self.finished = false;
        self.status = PlaybackStatus::Loading;
        Load {
            track,
            session: self.session,
            paused: !self.should_play(),
        }
    }

    /// Reserve a source ID without changing the currently displayed song.
    /// A later manual load must never reuse a prefetched source's identity.
    pub fn reserve_session(&mut self) -> usize {
        self.session_counter = self.session_counter.wrapping_add(1);
        self.session_counter
    }

    pub fn load_generation(&self) -> u64 {
        self.load_generation
    }

    /// Peek inside the selected run (including search/album scopes), preserving
    /// duplicate entries by their queue slot rather than rebuilding a queue.
    pub fn successor(&self) -> Option<(usize, usize)> {
        if self.current.is_none() || self.queue.current() != self.current {
            return None;
        }
        let position = if self.repeat == Repeat::One {
            self.queue.position
        } else if self.queue.position + 1 < self.queue.order.len() {
            self.queue.position + 1
        } else if self.repeat == Repeat::All {
            0
        } else {
            return None;
        };
        self.queue
            .order
            .get(position)
            .copied()
            .map(|track| (position, track))
    }

    /// Adopt an engine source that already started on the same sink. This is
    /// an acknowledgement, so it preserves pause and interruption intent.
    pub fn transition(
        &mut self,
        from: usize,
        to: usize,
        track: usize,
        queue_position: usize,
    ) -> bool {
        if self.current.is_none() || self.session != from || from == to {
            return false;
        }
        let position = if self.queue.order.get(queue_position) == Some(&track) {
            Some(queue_position)
        } else {
            self.queue.order.iter().position(|id| *id == track)
        };
        let Some(position) = position else {
            return false;
        };
        self.session = to;
        self.current = Some(track);
        self.queue.position = position;
        self.position = 0.0;
        self.finished = false;
        self.status = if self.should_play() {
            PlaybackStatus::Playing
        } else {
            PlaybackStatus::Paused
        };
        true
    }

    pub fn advance(&mut self, manual: bool) -> Option<Load> {
        let track = self.queue.advance(self.repeat, manual)?;
        if manual {
            self.user_intent(true);
        }
        Some(self.load(track))
    }

    pub fn previous(&mut self) -> Option<Load> {
        let track = self.queue.previous()?;
        self.user_intent(true);
        Some(self.load(track))
    }

    pub fn restart(&mut self) -> Option<Load> {
        self.user_intent(true);
        self.current.map(|track| self.load(track))
    }

    /// Reconfigure audio without turning a paused or interrupted song into playback.
    pub fn reload(&mut self) -> Option<Load> {
        self.current.map(|track| self.load(track))
    }

    pub fn user_intent(&mut self, playing: bool) {
        self.revision = self.revision.wrapping_add(1);
        self.wants_playing = playing;
        if self.current.is_some() && self.status != PlaybackStatus::Loading {
            self.status = if self.should_play() {
                PlaybackStatus::Playing
            } else {
                PlaybackStatus::Paused
            };
        }
    }

    pub fn should_play(&self) -> bool {
        self.wants_playing && self.interruption.is_none()
    }

    pub fn interrupted(&self) -> bool {
        self.interruption.is_some()
    }

    /// Call only after AVAudioSession activation succeeds for an explicit Play
    /// or song selection. iOS does not guarantee a matching interruption Ended.
    pub fn session_reactivated(&mut self) {
        self.interruption = None;
    }

    pub fn progress(&mut self, session: usize, status: PlaybackStatus, position: f64) {
        if session != self.session || self.finished || self.current.is_none() {
            return;
        }
        if position.is_finite() {
            self.position = position.max(0.0);
        }
        // A progress acknowledgement queued before Pause must not undo the
        // user's intent (Pause does not invalidate the engine's generation).
        self.status = if status == PlaybackStatus::Playing && !self.should_play() {
            PlaybackStatus::Paused
        } else {
            status
        };
    }

    pub fn finish(&mut self, session: usize) -> Option<Load> {
        if session != self.session || self.finished || self.current.is_none() {
            return None;
        }
        self.finished = true;
        if self.should_play()
            && let Some(load) = self.advance(false)
        {
            return Some(load);
        }
        self.wants_playing = false;
        self.status = PlaybackStatus::Stopped;
        None
    }

    pub fn interruption_began(&mut self) {
        // Duplicate native notifications must retain the original resume intent.
        if self.interruption.is_none() {
            self.interruption = Some((self.wants_playing, self.revision));
            if self.current.is_some() {
                self.status = PlaybackStatus::Paused;
            }
        }
    }

    pub fn interruption_ended(&mut self, should_resume: bool) {
        let Some((was_playing, revision)) = self.interruption.take() else {
            return;
        };
        self.wants_playing =
            should_resume && self.wants_playing && (was_playing || self.revision != revision);
        if self.current.is_some() {
            self.status = if self.wants_playing {
                PlaybackStatus::Playing
            } else {
                PlaybackStatus::Paused
            };
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        if self.shuffle {
            self.queue.shuffle();
        } else {
            self.queue.unshuffle();
        }
    }

    pub fn clear(&mut self) {
        self.load_generation = self.load_generation.wrapping_add(1);
        self.session = self.reserve_session();
        self.current = None;
        self.queue = Queue::default();
        self.wants_playing = false;
        self.position = 0.0;
        self.status = PlaybackStatus::Stopped;
        self.finished = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_activation_recovers_a_missing_interruption_end_without_reviving_pause() {
        let mut p = Playback::default();
        p.select(0, vec![0, 1]);
        p.interruption_began();
        assert!(!p.should_play());
        // Browsing, settings reloads and a failed activation cannot clear it.
        assert!(p.reload().unwrap().paused);
        assert!(p.interrupted());
        let latest = p.select(1, vec![0, 1]);
        assert_eq!(latest.track, 1);
        assert!(p.interrupted());
        p.session_reactivated(); // Native activation succeeded for that tap.
        assert!(p.should_play());
        p.user_intent(false);
        p.interruption_ended(true); // A late old Ended cannot undo user Pause.
        assert!(!p.should_play());
        p.interruption_began();
        p.session_reactivated();
        assert!(
            !p.should_play(),
            "activation alone never changes user intent"
        );
    }

    #[test]
    fn preference_reload_keeps_pause_and_interruption_intent() {
        let mut playback = Playback::default();
        let old = playback.select(0, vec![0, 1]);
        playback.user_intent(false);
        let reloaded = playback.reload().unwrap();
        assert!(reloaded.paused);
        assert!(!playback.should_play());
        assert_ne!(old.session, reloaded.session);
        assert!(playback.finish(old.session).is_none());

        playback.user_intent(true);
        playback.interruption_began();
        assert!(playback.reload().unwrap().paused);
        assert!(!playback.should_play());
        playback.interruption_ended(true);
        assert!(playback.should_play());
    }

    #[test]
    fn natural_finish_plays_successor_once_and_stops_at_end() {
        let mut p = Playback::default();
        let a = p.select(0, vec![0, 1]);
        let b = p.finish(a.session).unwrap();
        assert_eq!(b.track, 1);
        assert!(p.finish(a.session).is_none());
        assert!(p.finish(b.session).is_none());
        assert_eq!(p.status, PlaybackStatus::Stopped);
        assert!(!p.should_play());
    }

    #[test]
    fn repeat_one_uses_new_identity_and_manual_next_advances() {
        let mut p = Playback::default();
        p.repeat = Repeat::One;
        let a = p.select(0, vec![0, 1]);
        let repeat = p.finish(a.session).unwrap();
        assert_eq!(repeat.track, 0);
        assert_ne!(a.session, repeat.session);
        assert!(p.finish(a.session).is_none());
        assert_eq!(p.advance(true).unwrap().track, 1);
    }

    #[test]
    fn stale_progress_does_not_replace_new_track_or_pause_intent() {
        let mut p = Playback::default();
        let a = p.select(0, vec![0, 1]);
        let b = p.advance(true).unwrap();
        p.progress(a.session, PlaybackStatus::Playing, 80.0);
        assert_eq!(p.position, 0.0);
        p.user_intent(false);
        p.progress(b.session, PlaybackStatus::Playing, 0.5);
        assert_eq!(p.status, PlaybackStatus::Paused);
        assert!(!p.should_play());
    }

    #[test]
    fn interruption_respects_user_pause_and_native_resume_permission() {
        let mut p = Playback::default();
        p.select(0, vec![0]);
        p.interruption_began();
        p.interruption_began();
        assert!(!p.should_play());
        p.interruption_ended(true);
        assert!(p.should_play());
        p.interruption_began();
        p.user_intent(false);
        p.interruption_ended(true);
        assert!(!p.should_play());
        p.user_intent(true);
        p.interruption_began();
        p.interruption_ended(false);
        assert!(!p.should_play());
    }

    #[test]
    fn output_disconnect_during_interruption_cancels_resume() {
        let mut p = Playback::default();
        p.select(0, vec![0]);
        p.interruption_began();
        p.user_intent(false); // Same path used for OutputDisconnected.
        p.interruption_ended(true);
        assert!(!p.should_play());
    }

    #[test]
    fn prefetched_transition_has_unique_identity_and_keeps_pause_intent() {
        let mut p = Playback::default();
        let first = p.select(2, vec![2, 7]);
        let next = p.reserve_session();
        let generation = p.load_generation();
        p.user_intent(false);
        assert!(p.transition(first.session, next, 7, 1));
        assert_eq!(p.current, Some(7));
        assert_eq!(p.queue.position, 1);
        assert_eq!(p.status, PlaybackStatus::Paused);
        assert_eq!(p.load_generation(), generation);
        assert!(p.finish(first.session).is_none());
        assert!(!p.transition(first.session, next, 7, 1));
        assert_ne!(p.restart().unwrap().session, next);
    }

    #[test]
    fn successor_respects_repeat_scoped_runs_and_duplicate_slots() {
        let mut p = Playback::default();
        p.select(4, vec![4, 9, 4]);
        assert_eq!(p.successor(), Some((1, 9)));
        p.queue.position = 2;
        assert_eq!(p.successor(), None);
        p.repeat = Repeat::All;
        assert_eq!(p.successor(), Some((0, 4)));
        p.repeat = Repeat::One;
        assert_eq!(p.successor(), Some((2, 4)));
        let next = p.reserve_session();
        assert!(p.transition(p.session, next, 4, 2));
        assert_eq!(p.queue.position, 2);
        p.clear();
        assert_eq!(p.successor(), None);
    }
}
