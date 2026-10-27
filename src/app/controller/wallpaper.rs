use super::*;

impl Controller {
    pub(crate) fn start_wallpaper(&mut self) {
        if let Err(error) = self.wallpaper.start() {
            self.state.notice = format!("Wallpaper error: {error}");
        }
        self.wallpaper_marker = None;
    }

    pub(crate) fn stop_wallpaper(&mut self) {
        if let Err(error) = self.wallpaper.shutdown() {
            eprintln!("Cannot restore desktop wallpaper: {error}");
        }
    }

    pub(super) fn sync_wallpaper(&mut self) {
        let current = wallpaper_track(&self.state);
        let marker = (self.state.cover_wallpaper, current);
        if self.wallpaper_marker == Some(marker) && self.last_track == self.state.current {
            return;
        }
        self.wallpaper_marker = Some(marker);
        let artwork = current
            .filter(|_| self.state.cover_wallpaper)
            .and_then(|id| self.state.tracks.get(id))
            .map(|track| track.artwork.as_slice());
        if let Err(error) = self.wallpaper.update(artwork) {
            self.state.notice = format!("Wallpaper error: {error}");
        }
    }
}

fn wallpaper_track(state: &State) -> Option<usize> {
    if state.video_active
        || state.podcasts.active.is_some()
        || matches!(
            state.status,
            PlaybackStatus::Stopped | PlaybackStatus::Error
        )
    {
        None
    } else {
        state.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_music_keeps_its_wallpaper_and_other_media_or_stopped_music_does_not() {
        let mut state = State {
            current: Some(3),
            status: PlaybackStatus::Playing,
            ..Default::default()
        };
        assert_eq!(wallpaper_track(&state), Some(3));
        state.status = PlaybackStatus::Paused;
        state.paused = true;
        assert_eq!(wallpaper_track(&state), Some(3));
        state.video_active = true;
        assert_eq!(wallpaper_track(&state), None);
        state.video_active = false;
        state.podcasts.active = Some(1);
        assert_eq!(wallpaper_track(&state), None);
        state.podcasts.active = None;
        state.status = PlaybackStatus::Stopped;
        assert_eq!(wallpaper_track(&state), None);
        state.status = PlaybackStatus::Error;
        assert_eq!(wallpaper_track(&state), None);
    }
}
