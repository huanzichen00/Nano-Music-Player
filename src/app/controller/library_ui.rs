//! Apply background library results to stable state slots and presentation caches.
//!
//! LibraryTasks remains the sole task/channel owner. The poll loop retains its
//! one-startup-event budget, import draining, and existing ordering with lyrics,
//! statistics, and playback; no scanning or new I/O is added to UI projection.
use super::*;

impl Controller {
    pub(super) fn poll_library_loading(&mut self) {
        if let Some(event) = self.library_tasks.poll_load() {
            match event {
                LibraryLoadEvent::Cached(mut prepared) => {
                    // An import can finish before startup publication. Keep its
                    // newer tracks instead of replacing them with cached rows.
                    prepared.value.retain(|track| {
                        !self
                            .state
                            .tracks
                            .iter()
                            .any(|current| current.path == track.path)
                    });
                    prepared.previews.retain_tracks(&prepared.value);
                    self.state.merge_library(prepared.value);
                    self.start_library_refresh();
                    self.update_smart_playlist_library();
                    self.lyrics_dirty = true;
                    self.thumbnails.clear();
                    self.thumbnails.seed(&self.state.tracks, prepared.previews);
                    self.artwork.clear();
                    self.last_track = None;
                    self.rows_dirty = true;
                    self.playlist_candidates_dirty = true;
                }
                LibraryLoadEvent::Refreshed { original, prepared } => {
                    let Prepared { value, previews } = *prepared;
                    let artwork_changed = value.artwork != original.artwork;
                    let playlist_metadata_changed =
                        value.title != original.title || value.artist != original.artist;
                    if self.state.refresh_cached_track(&original, value) {
                        self.playlist_candidates_dirty |= playlist_metadata_changed;
                        self.statistics.invalidate();
                        self.media_marker = None;
                        self.lyrics_dirty = true;
                        if artwork_changed {
                            self.thumbnails.clear();
                            self.thumbnails.seed(&self.state.tracks, previews);
                            self.artwork.clear();
                            self.last_track = None;
                        }
                        self.rows_dirty = true;
                    }
                }
                LibraryLoadEvent::Error(message) => {
                    self.state.notice = message;
                }
                LibraryLoadEvent::Complete { missing_cached } => {
                    self.enqueue_library_artist_images();
                    if missing_cached {
                        self.state.notice =
                            "Library loading stopped before cached tracks arrived".into();
                    }
                }
            }
        }
    }

    pub(super) fn poll_library_imports(&mut self, ui: &AppWindow) {
        while let Some(update) = self.library_tasks.poll_import() {
            if update.picker_closed {
                self.dialog_open = false;
                ui.invoke_focus_screen();
            }
            let thumbnails::ImportEvent::Completed(result) = update.event else {
                self.state.notice = "Scanning music folders…".to_owned();
                continue;
            };
            self.state.importing = false;
            match result {
                Ok(Some(prepared)) => {
                    let result = prepared.value;
                    for root in result.roots {
                        if !self.settings.library_roots.contains(&root) {
                            self.settings.library_roots.push(root);
                        }
                    }
                    let _ = self.settings.save(&self.directory.join("settings.json"));
                    self.state.merge_library(result.tracks);
                    self.playlist_candidates_dirty = true;

                    self.update_smart_playlist_library();
                    self.lyrics_dirty = true;
                    self.thumbnails.clear();
                    self.thumbnails.seed(&self.state.tracks, prepared.previews);
                    self.artwork.clear();
                    self.rows_dirty = true;
                    self.last_track = None;
                    self.state.notice = format!(
                        "{} songs in library{}",
                        self.state.tracks.len(),
                        if result.warnings.is_empty() {
                            String::new()
                        } else {
                            format!(" · {} files skipped", result.warnings.len())
                        }
                    );
                }
                Ok(None) => self.state.notice = "Import cancelled".to_owned(),
                Err(message) => self.state.notice = format!("Import failed: {message}"),
            }
        }
    }

    pub(super) fn update_smart_playlist_library(&self) {
        let paths = self
            .state
            .tracks
            .iter()
            .filter(|track| self.state.track_available(track))
            .map(|track| track.path.clone())
            .collect();
        self.statistics.set_library(paths);
        self.enqueue_library_artist_images();
    }

    pub(super) fn start_library_refresh(&mut self) {
        self.library_tasks
            .start_refresh(self.settings.library_roots.clone());
        self.state.library_refreshing = true;
        if self.state.notice == "Import a music folder to begin" {
            self.state.notice = "Refreshing library…".into();
        }
    }

    pub(super) fn poll_library_refresh(&mut self) {
        let Some(result) = self.library_tasks.poll_refresh() else {
            return;
        };
        self.state.library_refreshing = false;
        match result {
            Ok(prepared) => self.apply_library_refresh(prepared),
            Err(error) => self.state.notice = format!("Library refresh failed: {error}"),
        }
    }

    pub(super) fn apply_library_refresh(
        &mut self,
        mut prepared: Prepared<crate::library::refresh::Snapshot>,
    ) {
        let snapshot = &mut prepared.value;
        // User lyric edits may finish during a refresh. Preserve newer values;
        // stable vector slots keep queued and currently playing IDs unchanged.
        let current: HashMap<_, _> = self
            .state
            .tracks
            .iter()
            .map(|track| (&track.path, track))
            .collect();
        let original: HashMap<_, _> = snapshot
            .original
            .iter()
            .map(|track| (&track.path, track))
            .collect();
        snapshot.tracks.retain(|track| {
            let previous = current.get(&track.path).copied();
            let was_missing = self.state.unavailable_tracks.contains(&track.path);
            if previous == Some(track) && !was_missing {
                return false;
            }
            previous.is_none()
                || previous == original.get(&track.path).copied()
                || (!original.contains_key(&track.path) && was_missing)
        });
        prepared.previews.retain_tracks(&snapshot.tracks);
        let changed = !snapshot.tracks.is_empty()
            || snapshot.missing.iter().any(|path| {
                self.state
                    .tracks
                    .iter()
                    .any(|track| &track.path == path && self.state.track_available(track))
            });
        if changed {
            let artwork_changed = snapshot.tracks.iter().any(|track| {
                self.state
                    .tracks
                    .iter()
                    .find(|old| old.path == track.path)
                    .is_none_or(|old| old.artwork != track.artwork)
            });
            self.state
                .merge_library(std::mem::take(&mut snapshot.tracks));
            for path in &snapshot.missing {
                self.mark_library_missing(path);
            }
            if artwork_changed {
                self.thumbnails.clear();
                self.artwork.clear();
                self.last_track = None;
            }
            self.thumbnails.seed(&self.state.tracks, prepared.previews);
            self.update_smart_playlist_library();
            self.lyrics_dirty = true;
            self.media_marker = None;
            self.rows_dirty = true;
            self.playlist_candidates_dirty = true;
        }
        let count = self
            .state
            .tracks
            .iter()
            .filter(|track| self.state.track_available(track))
            .count();
        if self.state.notice == "Refreshing library…" {
            self.state.notice = format!(
                "{count} songs in library{}",
                if snapshot.warnings.is_empty() {
                    String::new()
                } else {
                    format!(" · {} files skipped", snapshot.warnings.len())
                }
            );
        }
    }

    fn mark_library_missing(&mut self, path: &std::path::Path) {
        let Some(id) = self
            .state
            .tracks
            .iter()
            .position(|track| track.path == path)
        else {
            return;
        };
        self.state.mark_track_available(path, false);
        self.playlist_candidates_dirty = true;
        self.cancel_gapless_prefetch();
        if self.state.current == Some(id) {
            if !self.state.video_active && self.state.podcasts.active.is_none() {
                self.handoff += 1;
                if matches!(self.pending_media, Some(PendingMedia::Music { .. })) {
                    self.pending_media = None;
                    self.pending_seek = None;
                }
                let _ = self.engine.send(AudioCommand::Stop {
                    handoff: self.handoff,
                });
            }
            self.state.current = None;
            self.state.status = PlaybackStatus::Stopped;
            self.state.position = 0.0;
            self.playback_session = None;
            self.last_track = None;
        }
    }
}
