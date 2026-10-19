//! Prefetch source identities stay separate from library indices. Canceled
//! sources can still acknowledge an already-started transition on the engine.
use super::{Controller, playback::Playback};
use crate::{
    audio::{PlaybackStatus, engine::AudioCommand, gain::GainConfig},
    ui::MobileWindow,
};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
struct Candidate {
    position: usize,
    track: usize,
    path: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
struct Request {
    generation: u64,
    from: usize,
    next: Candidate,
    gain: GainConfig,
}

#[derive(Clone, Debug)]
struct Prepared {
    request: Request,
    session: usize,
    token: u64,
}

#[derive(Default)]
pub(super) struct Gapless {
    token: u64,
    generation: Option<u64>,
    pending: Option<Prepared>,
    // CancelPrefetch only prevents a source's first sample. If it already
    // started, Engine still reports that real transition, even after a new
    // repeat/shuffle/gapless preference. Keep its path/session until a Load
    // replaces the engine generation; stale event payloads cannot pick a song.
    canceled: Vec<Prepared>,
    failed: Option<Request>,
}

impl Gapless {
    fn synchronize(&mut self, playback: &Playback) {
        let generation = playback.load_generation();
        if self.generation != Some(generation) {
            self.generation = Some(generation);
            self.pending = None;
            self.canceled.clear();
            self.failed = None;
        }
    }

    fn cancel(&mut self) -> Option<u64> {
        self.failed = None;
        self.cancel_pending()
    }

    fn cancel_pending(&mut self) -> Option<u64> {
        let prepared = self.pending.take()?;
        let token = prepared.token;
        self.canceled.push(prepared);
        Some(token)
    }

    fn plan(
        &mut self,
        playback: &mut Playback,
        next: Option<Candidate>,
        gain: GainConfig,
    ) -> (Option<u64>, Option<Prepared>) {
        self.synchronize(playback);
        let Some(next) = next else {
            return (self.cancel_pending(), None);
        };
        let request = Request {
            generation: playback.load_generation(),
            from: playback.session,
            next,
            gain,
        };
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.request == request)
        {
            return (None, None);
        }
        let canceled = self.cancel_pending();
        if self.failed.as_ref() == Some(&request) {
            return (canceled, None);
        }
        self.token = self.token.wrapping_add(1);
        let prepared = Prepared {
            request,
            session: playback.reserve_session(),
            token: self.token,
        };
        self.pending = Some(prepared.clone());
        (canceled, Some(prepared))
    }

    fn transitioned(
        &mut self,
        playback: &Playback,
        from: usize,
        to: usize,
        token: u64,
    ) -> Option<Prepared> {
        self.synchronize(playback);
        if playback.current.is_none() || playback.session != from {
            return None;
        }
        let matches = |prepared: &Prepared| prepared.session == to && prepared.token == token;
        let prepared = if self.pending.as_ref().is_some_and(matches) {
            self.pending.take()
        } else {
            self.canceled
                .iter()
                .position(matches)
                .map(|index| self.canceled.remove(index))
        }?;
        self.failed = None;
        Some(prepared)
    }

    fn prefetch_failed(&mut self, playback: &Playback, id: usize, token: u64) -> bool {
        self.synchronize(playback);
        if !self
            .pending
            .as_ref()
            .is_some_and(|prepared| prepared.session == id && prepared.token == token)
        {
            return false;
        }
        self.failed = self.pending.take().map(|prepared| prepared.request);
        true
    }
}

impl Controller {
    pub(super) fn schedule_gapless(&mut self) {
        let next = (self.settings.gapless_playback
            && self.deferred_load.is_none()
            && !self.playback.interrupted()
            && !matches!(
                self.playback.status,
                PlaybackStatus::Stopped | PlaybackStatus::Error
            ))
        .then(|| self.playback.successor())
        .flatten()
        .and_then(|(position, track)| {
            self.tracks.get(track).map(|song| Candidate {
                position,
                track,
                path: song.path.clone(),
            })
        });
        let gain = self.audio_gain();
        let (cancel, prepared) = self.gapless.plan(&mut self.playback, next, gain);
        if let Some(token) = cancel {
            let _ = self.engine.send(AudioCommand::CancelPrefetch { token });
        }
        if let Some(prepared) = prepared {
            let command = AudioCommand::AppendNext {
                token: prepared.token,
                id: prepared.session,
                path: prepared.request.next.path,
                gain,
            };
            if let Err(message) = self.engine.send(command) {
                self.gapless_failed(prepared.session, prepared.token);
                self.notice = format!("Could not prepare next track: {message}");
            }
        }
    }

    pub(super) fn cancel_gapless(&mut self) {
        if let Some(token) = self.gapless.cancel() {
            let _ = self.engine.send(AudioCommand::CancelPrefetch { token });
        }
    }

    pub(super) fn gapless_failed(&mut self, id: usize, token: u64) -> bool {
        self.gapless.prefetch_failed(&self.playback, id, token)
    }

    pub(super) fn gapless_transition(
        &mut self,
        ui: &MobileWindow,
        from: usize,
        to: usize,
        token: u64,
    ) -> bool {
        let Some(prepared) = self.gapless.transitioned(&self.playback, from, to, token) else {
            return false;
        };
        self.complete_stats(from);
        // Import/refresh may have resorted track indices after prefetch. The
        // worker opened this path, not whichever song now occupies that index.
        let Some(track) = self
            .tracks
            .iter()
            .position(|track| track.path == prepared.request.next.path)
        else {
            self.cancel_gapless();
            self.fail("The next song is no longer in the library".into());
            return false;
        };
        if !self
            .playback
            .transition(from, to, track, prepared.request.next.position)
        {
            self.cancel_gapless();
            self.fail("The next song is no longer in the playback queue".into());
            return false;
        }
        if !self.playback.should_play() {
            let _ = self.engine.send(AudioCommand::Pause(true));
        }
        self.on_stats_track_change();
        self.deferred_seek = None;
        self.media_marker = None;
        self.media_artwork.clear();
        ui.set_artwork(slint::Image::default());
        self.artwork.request(to, self.tracks[track].artwork.clone());
        self.present_track(ui);
        self.rebuild_rows(ui);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::Repeat;

    fn candidate(playback: &Playback) -> Option<Candidate> {
        playback.successor().map(|(position, track)| Candidate {
            position,
            track,
            path: PathBuf::from(format!("{track}.wav")),
        })
    }

    fn prepare(gapless: &mut Gapless, playback: &mut Playback) -> Prepared {
        let next = candidate(playback);
        gapless
            .plan(playback, next, GainConfig::default())
            .1
            .unwrap()
    }

    #[test]
    fn natural_transition_advances_the_selected_run_once_without_load() {
        let mut playback = Playback::default();
        let first = playback.select(3, vec![3, 8, 11]);
        let mut gapless = Gapless::default();
        let next = prepare(&mut gapless, &mut playback);
        assert_eq!(next.request.next.track, 8);
        let generation = playback.load_generation();
        let accepted = gapless
            .transitioned(&playback, first.session, next.session, next.token)
            .unwrap();
        assert!(playback.transition(
            first.session,
            accepted.session,
            accepted.request.next.track,
            accepted.request.next.position
        ));
        assert_eq!(playback.queue.order, vec![3, 8, 11]);
        assert_eq!(playback.queue.position, 1);
        assert_eq!(playback.load_generation(), generation);
        assert_eq!(playback.status, PlaybackStatus::Playing);
        assert!(playback.finish(first.session).is_none());
        assert!(
            gapless
                .transitioned(&playback, first.session, next.session, next.token)
                .is_none()
        );
        assert_eq!(prepare(&mut gapless, &mut playback).request.next.track, 11);
    }

    #[test]
    fn manual_load_cannot_accept_an_old_prefetch_token() {
        let mut playback = Playback::default();
        let first = playback.select(0, vec![0, 1, 2]);
        let mut gapless = Gapless::default();
        let next = prepare(&mut gapless, &mut playback);
        assert!(
            gapless
                .transitioned(&playback, first.session, next.session, next.token + 1)
                .is_none()
        );
        let manual = playback.advance(true).unwrap();
        assert_ne!(manual.session, next.session);
        assert!(
            gapless
                .transitioned(&playback, first.session, next.session, next.token)
                .is_none()
        );
        assert_eq!(playback.current, Some(1));
        assert!(!gapless.prefetch_failed(&playback, next.session, next.token));
    }

    #[test]
    fn disabling_cancels_unstarted_prefetch_but_keeps_an_already_started_acknowledgement() {
        let mut playback = Playback::default();
        let first = playback.select(0, vec![0, 1]);
        let mut gapless = Gapless::default();
        let next = prepare(&mut gapless, &mut playback);
        assert_eq!(
            gapless.plan(&mut playback, None, GainConfig::default()).0,
            Some(next.token)
        );
        assert!(gapless.pending.is_none());
        let accepted = gapless
            .transitioned(&playback, first.session, next.session, next.token)
            .unwrap();
        playback.user_intent(false);
        assert!(playback.transition(first.session, accepted.session, 1, 1));
        assert_eq!(playback.status, PlaybackStatus::Paused);
        assert!(!playback.should_play());
    }

    #[test]
    fn canceled_repeat_or_shuffle_tokens_cannot_revive_pause_or_interruption() {
        let mut playback = Playback::default();
        let first = playback.select(0, vec![0, 1, 2]);
        let mut gapless = Gapless::default();
        let next = prepare(&mut gapless, &mut playback);
        playback.repeat = Repeat::One;
        let repeated = prepare(&mut gapless, &mut playback);
        assert_ne!(repeated.token, next.token);
        assert_eq!(repeated.request.next.track, 0);
        playback.interruption_began();
        gapless.cancel();
        let accepted = gapless
            .transitioned(&playback, first.session, next.session, next.token)
            .unwrap();
        assert!(playback.transition(first.session, accepted.session, 1, 1));
        assert_eq!(playback.status, PlaybackStatus::Paused);
        playback.user_intent(false);
        playback.interruption_ended(true);
        assert!(!playback.should_play());
    }

    #[test]
    fn failed_prefetch_is_not_retried_each_tick_and_user_change_can_retry() {
        let mut playback = Playback::default();
        playback.select(0, vec![0, 1]);
        let mut gapless = Gapless::default();
        let next = prepare(&mut gapless, &mut playback);
        assert!(!gapless.prefetch_failed(&playback, next.session, next.token + 1));
        assert!(gapless.prefetch_failed(&playback, next.session, next.token));
        let successor = candidate(&playback);
        assert!(
            gapless
                .plan(&mut playback, successor, GainConfig::default())
                .1
                .is_none()
        );
        gapless.cancel();
        assert_ne!(prepare(&mut gapless, &mut playback).token, next.token);
    }

    #[test]
    fn scoped_shuffle_and_repeat_all_prepare_only_their_queue_successor() {
        let mut playback = Playback::default();
        playback.select(9, vec![9, 3, 7]);
        playback.toggle_shuffle();
        let expected = playback.queue.order[1];
        let mut gapless = Gapless::default();
        assert_eq!(
            prepare(&mut gapless, &mut playback).request.next.track,
            expected
        );
        playback.repeat = Repeat::All;
        playback.queue.position = 2;
        playback.current = playback.queue.current();
        assert_eq!(
            prepare(&mut gapless, &mut playback).request.next.position,
            0
        );
    }

    #[test]
    fn seek_or_reenable_uses_new_ids_and_ignores_the_old_failure() {
        let mut playback = Playback::default();
        playback.select(0, vec![0, 1]);
        let mut gapless = Gapless::default();
        let old = prepare(&mut gapless, &mut playback);
        // Controller sends CancelPrefetch before Seek (which also invalidates
        // unstarted engine sources), then schedules again after polling events.
        assert_eq!(gapless.cancel(), Some(old.token));
        playback.position = 45.0;
        let next = prepare(&mut gapless, &mut playback);
        assert_ne!(next.session, old.session);
        assert_ne!(next.token, old.token);
        assert!(!gapless.prefetch_failed(&playback, old.session, old.token));
        assert!(
            gapless
                .transitioned(&playback, playback.session, next.session, old.token)
                .is_none()
        );
        assert_eq!(playback.position, 45.0);
        assert_eq!(gapless.pending.as_ref().unwrap().token, next.token);
        assert_eq!(
            gapless.plan(&mut playback, None, GainConfig::default()).0,
            Some(next.token)
        );
        let reenabled = prepare(&mut gapless, &mut playback);
        assert_ne!(reenabled.token, next.token);
    }

    #[test]
    fn short_sources_acknowledge_in_order_even_after_prefetch_was_replaced() {
        let mut playback = Playback::default();
        let first = playback.select(0, vec![0, 1, 2]);
        let mut gapless = Gapless::default();
        let started = prepare(&mut gapless, &mut playback);
        gapless.cancel();
        // Source 1 can already be playing when the UI changes queue intent.
        // The worker will report it before the newly appended source starts.
        playback.repeat = Repeat::One;
        let replacement = prepare(&mut gapless, &mut playback);
        assert!(
            gapless
                .transitioned(&playback, first.session, started.session, started.token)
                .is_some()
        );
        assert!(playback.transition(first.session, started.session, 1, 1));
        assert!(
            gapless
                .transitioned(
                    &playback,
                    started.session,
                    replacement.session,
                    replacement.token
                )
                .is_some()
        );
        assert!(playback.transition(started.session, replacement.session, 0, 0));
        assert_eq!(playback.current, Some(0));
        assert_eq!(playback.session, replacement.session);
    }
}
