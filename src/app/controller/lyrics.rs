//! Source-bound lyric annotations and their asynchronous request lifetimes.
//!
//! The controller projects events into application state and owns lyric-edit
//! persistence. Workers here never hold a UI component or application state.
use crate::{
    library::{Track, online_lyrics, romaji},
    translation::{self, Cancellation, TargetLanguage},
};
use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread::JoinHandle,
};

type TranslationResult = (
    translation::Request,
    Result<translation::Translation, translation::Error>,
);
type LookupResult = (online_lyrics::Request, Result<online_lyrics::Found, String>);

pub(super) enum LookupEvent {
    /// Accepted source lyrics still need durable storage before UI publication.
    Found {
        request: online_lyrics::Request,
        found: online_lyrics::Found,
    },
    Failed {
        path: Option<PathBuf>,
        notice: String,
    },
}

pub(super) enum RomajiEvent {
    Loaded(HashMap<PathBuf, romaji::Romaji>),
    StorageFailed(String),
    Progress {
        done: usize,
        total: usize,
    },
    Generated {
        path: PathBuf,
        romaji: romaji::Romaji,
        storage: Result<(), String>,
    },
    Finished {
        notice: String,
    },
}

pub(super) struct RomajiRemoval {
    pub cancelled: bool,
    pub result: Result<(), String>,
}

enum CacheChange {
    Insert(translation::Translation),
    Remove(translation::CacheKey),
}

struct CacheWrite {
    id: u64,
    change: CacheChange,
}

struct CacheWritten {
    id: u64,
    result: Result<Arc<translation::Cache>, translation::Error>,
}

/// One FIFO writer owns the durable cache. Snapshot cloning, JSON encoding,
/// fsync and atomic replacement all stay off the UI thread.
struct TranslationWriter {
    commands: Option<mpsc::Sender<CacheWrite>>,
    events: Receiver<CacheWritten>,
    worker: Option<JoinHandle<()>>,
}

impl TranslationWriter {
    fn new(path: PathBuf, initial: Arc<translation::Cache>) -> Self {
        let (commands, receiver) = mpsc::channel::<CacheWrite>();
        let (sender, events) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut confirmed = initial;
            while let Ok(write) = receiver.recv() {
                let mut next = (*confirmed).clone();
                match write.change {
                    CacheChange::Insert(value) => next.insert(value),
                    CacheChange::Remove(key) => {
                        next.remove(&key);
                    }
                }
                let result = next.save(&path).map(|()| {
                    confirmed = Arc::new(next);
                    confirmed.clone()
                });
                // Continue draining accepted writes even after the UI closes.
                let _ = sender.send(CacheWritten {
                    id: write.id,
                    result,
                });
            }
        });
        Self {
            commands: Some(commands),
            events,
            worker: Some(worker),
        }
    }
}

impl Drop for TranslationWriter {
    fn drop(&mut self) {
        self.commands.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct PendingCacheWrite {
    id: u64,
    request: translation::Request,
    removal: bool,
    ui_generation: u64,
}

pub(super) struct LyricsService {
    directory: PathBuf,
    romaji_request: Option<romaji::Request>,
    romaji_store: romaji::Store,
    romaji_touched: HashSet<PathBuf>,
    translation_cache: Arc<translation::Cache>,
    translation_cache_revision: u64,
    translation_ui_generation: u64,
    translation_writer: Option<TranslationWriter>,
    translation_writes: VecDeque<PendingCacheWrite>,
    translation_write_id: u64,
    translation_request: Option<Receiver<TranslationResult>>,
    translation_request_id: u64,
    translation_active_request: Option<translation::Request>,
    translation_silenced_request: Option<u64>,
    translation_visible: bool,
    // A preference inherited by another song must not consume that song's first T.
    translation_shortcut_track: Option<PathBuf>,
    translation_cancel: Option<Cancellation>,
    online_lyrics_request: Option<Receiver<LookupResult>>,
    online_lyrics_cancel: Option<Cancellation>,
    translation_after_lookup: Option<PathBuf>,
    online_lyrics_path: Option<PathBuf>,
}

impl LyricsService {
    pub(super) fn new(directory: PathBuf, visible: bool) -> Self {
        Self {
            romaji_store: romaji::Store::new(directory.join("library.sqlite3")),
            translation_cache: Arc::new(
                translation::Cache::load(&directory.join("translations.json")).unwrap_or_default(),
            ),
            translation_writer: None,
            translation_cache_revision: 0,
            translation_ui_generation: 0,
            translation_writes: VecDeque::new(),
            translation_write_id: 0,
            directory,
            romaji_request: None,
            romaji_touched: HashSet::new(),
            translation_request: None,
            translation_request_id: 0,
            translation_active_request: None,
            translation_silenced_request: None,
            translation_visible: visible,
            translation_shortcut_track: None,
            translation_cancel: None,
            online_lyrics_request: None,
            online_lyrics_cancel: None,
            translation_after_lookup: None,
            online_lyrics_path: None,
        }
    }

    pub(super) fn visible(&self) -> bool {
        self.translation_visible
    }

    pub(super) fn set_visible(&mut self, visible: bool) {
        self.translation_ui_generation = self.translation_ui_generation.wrapping_add(1);
        self.translation_visible = visible;
    }

    pub(super) fn toggle_visible(&mut self) {
        self.set_visible(!self.translation_visible);
        if !self.translation_visible {
            self.translation_shortcut_track = None;
        }
    }

    pub(super) fn shortcut_should_translate(
        &self,
        current: Option<&Track>,
        target: TargetLanguage,
    ) -> bool {
        let path = current.map(|track| &track.path);
        let inherited_missing_translation = path.is_some()
            && self.translation_shortcut_track.as_ref() != path
            && !self.translation_busy()
            && !self.lookup_busy()
            && self.translation_after_lookup.is_none()
            && !current.is_some_and(|track| self.cached_translation(track, target).is_some());
        !self.translation_visible || inherited_missing_translation
    }

    pub(super) fn mark_shortcut(&mut self, current: Option<&Track>) {
        self.translation_shortcut_track = current.map(|track| track.path.clone());
    }

    pub(super) fn translation_busy(&self) -> bool {
        self.translation_request.is_some() || !self.translation_writes.is_empty()
    }

    /// Durable cache changes invalidate presentation even when a stale operation
    /// must stay silent and preserve a newer notice or display preference.
    pub(super) fn translation_cache_revision(&self) -> u64 {
        self.translation_cache_revision
    }

    pub(super) fn lookup_busy(&self) -> bool {
        self.online_lyrics_request.is_some()
    }

    pub(super) fn lookup_path(&self) -> Option<&Path> {
        self.online_lyrics_path.as_deref()
    }

    pub(super) fn source(track: &Track) -> Vec<translation::SourceLine> {
        if track.lyrics.timed.is_empty() {
            track
                .lyrics
                .plain
                .iter()
                .map(|text| translation::SourceLine {
                    at_ms: None,
                    text: text.clone(),
                })
                .collect()
        } else {
            track
                .lyrics
                .timed
                .iter()
                .map(|line| translation::SourceLine {
                    at_ms: Some(line.at_ms),
                    text: line.text.clone(),
                })
                .collect()
        }
    }

    fn provider_version(&self) -> String {
        let model = crate::ai::DeepSeekSettings::load(&self.directory.join("deepseek.json"))
            .map(|settings| settings.model)
            .unwrap_or_else(|_| crate::ai::DEFAULT_MODEL.into());
        format!("{}:{model}", translation::PROMPT_VERSION)
    }

    fn cache_request(&self, track: &Track, target: TargetLanguage) -> translation::Request {
        translation::Request {
            request_id: 0,
            track_id: track.path.clone(),
            target,
            provider_version: self.provider_version(),
            source: Self::source(track),
        }
    }

    pub(super) fn cached_translation(
        &self,
        track: &Track,
        target: TargetLanguage,
    ) -> Option<&translation::Translation> {
        self.translation_cache
            .get(&self.cache_request(track, target).cache_key())
    }

    /// The controller routes an empty source through online lookup first.
    pub(super) fn start_translation(
        &mut self,
        track: Option<&Track>,
        target: TargetLanguage,
    ) -> Result<(), String> {
        if self.translation_busy() {
            return Err("Translation is already running".into());
        }
        let track = track.ok_or("Choose a song first")?;
        let settings = crate::ai::DeepSeekSettings::load(&self.directory.join("deepseek.json"));
        let model = settings
            .as_ref()
            .map(|settings| settings.model.as_str())
            .unwrap_or(crate::ai::DEFAULT_MODEL);
        self.translation_request_id = self.translation_request_id.wrapping_add(1);
        let request = translation::Request {
            request_id: self.translation_request_id,
            track_id: track.path.clone(),
            target,
            provider_version: format!("{}:{model}", translation::PROMPT_VERSION),
            source: Self::source(track),
        };
        let active_request = request.clone();
        let cancellation = Cancellation::default();
        let worker_cancellation = cancellation.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = settings
                .map_err(|_| translation::Error::Provider)
                .and_then(|settings| {
                    crate::ai::DeepSeek::new(&settings).map_err(|_| translation::Error::Provider)
                })
                .and_then(|provider| {
                    translation::generate(&request, &provider, &worker_cancellation)
                });
            let _ = sender.send((request, result));
        });
        self.translation_request = Some(receiver);
        self.translation_active_request = Some(active_request);
        self.translation_cancel = Some(cancellation);
        Ok(())
    }

    pub(super) fn poll_translation(
        &mut self,
        current: Option<&Track>,
        target: TargetLanguage,
    ) -> Option<&'static str> {
        if let Some(notice) = self.poll_translation_writes(current, target) {
            return Some(notice);
        }
        let received = self.translation_request.as_ref()?.try_recv();
        let (request, result) = match received {
            Ok(value) => value,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                self.translation_request = None;
                self.translation_active_request = None;
                self.translation_cancel = None;
                return if self.translation_silenced_request.take().is_some()
                    || !self.translation_writes.is_empty()
                {
                    None
                } else {
                    Some("Translation failed; try again")
                };
            }
        };
        self.translation_request = None;
        self.translation_active_request = None;
        self.translation_cancel = None;
        let silenced = self.translation_silenced_request.take() == Some(request.request_id);
        let notice = self.apply_translation_result(request, result, current, target);
        if !silenced && self.translation_writes.is_empty() {
            notice
        } else {
            None
        }
    }

    pub(super) fn apply_translation_result(
        &mut self,
        request: translation::Request,
        result: Result<translation::Translation, translation::Error>,
        current: Option<&Track>,
        target: TargetLanguage,
    ) -> Option<&'static str> {
        let source = current.map(Self::source).unwrap_or_default();
        match result {
            Ok(value)
                if current.is_some_and(|track| {
                    request.accepts(
                        &value,
                        self.translation_request_id,
                        &track.path,
                        target,
                        &source,
                        &self.provider_version(),
                    )
                }) =>
            {
                if self
                    .queue_translation_write(request, CacheChange::Insert(value), false)
                    .is_err()
                {
                    return Some("Could not save translation cache");
                }
                None
            }
            Ok(_) => Some("Lyrics or target changed; translation discarded"),
            Err(translation::Error::Cancelled) => Some("Translation cancelled"),
            Err(_) => Some("Translation failed; try again"),
        }
    }

    fn queue_translation_write(
        &mut self,
        request: translation::Request,
        change: CacheChange,
        removal: bool,
    ) -> Result<(), String> {
        let writer = self.translation_writer.get_or_insert_with(|| {
            TranslationWriter::new(
                self.directory.join("translations.json"),
                self.translation_cache.clone(),
            )
        });
        self.translation_write_id = self.translation_write_id.wrapping_add(1);
        let id = self.translation_write_id;
        if writer
            .commands
            .as_ref()
            .is_none_or(|sender| sender.send(CacheWrite { id, change }).is_err())
        {
            self.translation_writer = None;
            self.translation_writes.clear();
            return Err("Could not save translation cache".into());
        }
        self.translation_writes.push_back(PendingCacheWrite {
            id,
            request,
            removal,
            ui_generation: self.translation_ui_generation,
        });
        Ok(())
    }

    fn poll_translation_writes(
        &mut self,
        current: Option<&Track>,
        target: TargetLanguage,
    ) -> Option<&'static str> {
        while !self.translation_writes.is_empty() {
            let received = self.translation_writer.as_ref()?.events.try_recv();
            let event = match received {
                Ok(event) => event,
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    self.translation_writes.clear();
                    self.translation_writer = None;
                    return Some("Could not save translation cache");
                }
            };
            let pending = self.translation_writes.pop_front()?;
            debug_assert_eq!(pending.id, event.id);
            let success = event.result.is_ok();
            if let Ok(cache) = event.result {
                self.translation_cache = cache;
                self.translation_cache_revision = self.translation_cache_revision.wrapping_add(1);
            }
            // Older writes can become durable, but cannot replace the notice or
            // visibility of a later remove, cancellation, song or target change.
            if pending.id != self.translation_write_id {
                continue;
            }
            if pending.ui_generation != self.translation_ui_generation {
                return None;
            }
            let context_matches = current.is_some_and(|track| {
                pending.request.track_id == track.path
                    && pending.request.target == target
                    && pending.request.source == Self::source(track)
                    && pending.request.provider_version == self.provider_version()
            });
            if !context_matches {
                return None;
            }
            if pending.removal {
                if success {
                    self.translation_visible = false;
                }
                return Some(if success {
                    "Translation removed"
                } else {
                    "Could not remove translation"
                });
            }
            if pending.request.request_id != self.translation_request_id {
                return None;
            }
            return Some(if success {
                "Translation ready"
            } else {
                "Could not save translation cache"
            });
        }
        None
    }

    pub(super) fn cancel_translation(&mut self) {
        if let Some(token) = self.translation_cancel.as_ref() {
            token.cancel();
        }
        self.translation_request_id = self.translation_request_id.wrapping_add(1);
        self.translation_ui_generation = self.translation_ui_generation.wrapping_add(1);
    }

    pub(super) fn change_target(
        &mut self,
        previous: TargetLanguage,
        target: TargetLanguage,
    ) -> Option<PathBuf> {
        let path = self.cancel_lookup();
        if previous != target {
            self.translation_shortcut_track = None;
            self.translation_request_id = self.translation_request_id.wrapping_add(1);
            self.translation_ui_generation = self.translation_ui_generation.wrapping_add(1);
        }
        if let Some(token) = self.translation_cancel.take() {
            token.cancel();
        }
        path
    }

    pub(super) fn remove_translation(
        &mut self,
        current: Option<&Track>,
        target: TargetLanguage,
    ) -> Result<(), String> {
        let track = current.ok_or_else(|| "Choose a song first".to_owned())?;
        let request = self.cache_request(track, target);
        if self
            .translation_active_request
            .as_ref()
            .is_some_and(|active| active.cache_key() == request.cache_key())
        {
            self.translation_silenced_request = self
                .translation_active_request
                .as_ref()
                .map(|active| active.request_id);
            self.cancel_translation();
            self.translation_active_request = None;
        }
        let key = request.cache_key();
        self.queue_translation_write(request, CacheChange::Remove(key), true)
    }

    /// External lyric-edit/import admission remains the controller's decision.
    pub(super) fn start_lookup(
        &mut self,
        track: Option<&Track>,
        translate_after: bool,
    ) -> Result<PathBuf, String> {
        if self.lookup_busy() || self.translation_busy() {
            return Err("A lyric request is already running".into());
        }
        let track = track.ok_or("Choose a song first")?;
        if !track.lyrics.timed.is_empty() {
            return Err("Lyrics are already available".into());
        }
        let request = online_lyrics::Request::new(track);
        let settings = crate::ai::DeepSeekSettings::load(&self.directory.join("deepseek.json"))?;
        let path = request.path.clone();
        self.translation_after_lookup = translate_after.then(|| path.clone());
        self.online_lyrics_path = Some(path.clone());
        let cancellation = Cancellation::default();
        let worker_cancellation = cancellation.clone();
        let (sender, receiver) = mpsc::channel();
        let database_path = self.directory.join("library.sqlite3");
        std::thread::spawn(move || {
            let result = (|| {
                if !request.previous_lyrics.is_empty() {
                    use crate::library::database::{Database, LyricsAssociation};
                    let association =
                        Database::open(&database_path)?.lyrics_association(&request.path)?;
                    if !matches!(association, LyricsAssociation::Manual { source, lyrics }
                        if online_lyrics::is_lrclib_source(&source) && lyrics == request.previous_lyrics)
                    {
                        return Err("Lyrics are already available".into());
                    }
                }
                let provider = crate::ai::DeepSeek::new(&settings).ok();
                let found =
                    online_lyrics::lookup(&request, provider.as_ref(), &worker_cancellation)?;
                if !request.previous_lyrics.is_empty() && found.lyrics.timed.is_empty() {
                    return Err(
                        "No matching synchronized lyrics found; existing lyrics kept".into(),
                    );
                }
                Ok(found)
            })();
            let _ = sender.send((request, result));
        });
        self.online_lyrics_request = Some(receiver);
        self.online_lyrics_cancel = Some(cancellation);
        Ok(path)
    }

    pub(super) fn cancel_lookup(&mut self) -> Option<PathBuf> {
        if let Some(token) = self.online_lyrics_cancel.as_ref() {
            token.cancel();
        }
        self.translation_after_lookup = None;
        // Keep the receiver until its HTTP worker exits; cancellation must not
        // admit another network request while the first is still executing.
        self.online_lyrics_path.clone()
    }

    pub(super) fn accepts_lookup(
        &self,
        request: &online_lyrics::Request,
        tracks: &[Track],
        unavailable: &BTreeSet<PathBuf>,
        editing: bool,
        current: Option<&Track>,
    ) -> bool {
        !editing
            && !unavailable.contains(&request.path)
            && !self
                .online_lyrics_cancel
                .as_ref()
                .is_some_and(Cancellation::is_cancelled)
            && tracks.iter().any(|track| request.matches(track))
            && (self.translation_after_lookup.is_none()
                || current.is_some_and(|track| track.path == request.path))
    }

    pub(super) fn poll_lookup(
        &mut self,
        tracks: &[Track],
        unavailable: &BTreeSet<PathBuf>,
        editing: bool,
        current: Option<&Track>,
    ) -> Option<LookupEvent> {
        let result = match self.online_lyrics_request.as_ref()?.try_recv() {
            Ok(value) => value,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                self.online_lyrics_request = None;
                self.online_lyrics_cancel = None;
                self.translation_after_lookup = None;
                return Some(LookupEvent::Failed {
                    path: self.online_lyrics_path.take(),
                    notice: "Online lyric search failed".into(),
                });
            }
        };
        self.online_lyrics_request = None;
        // The cancellation token still belongs to this response when checked.
        let accepted = self.accepts_lookup(&result.0, tracks, unavailable, editing, current);
        self.online_lyrics_cancel = None;
        match result {
            (request, Ok(found)) if accepted => Some(LookupEvent::Found { request, found }),
            (_, result) => {
                self.translation_after_lookup = None;
                // A concurrent manual edit still owns the lyric-edit completion.
                // Preserve its existing status handoff until that edit finishes.
                let path = if editing {
                    self.online_lyrics_path.clone()
                } else {
                    self.online_lyrics_path.take()
                };
                Some(LookupEvent::Failed {
                    path,
                    notice: match result {
                        Ok(_) => "Song or lyrics changed; online result discarded".into(),
                        Err(error) => error,
                    },
                })
            }
        }
    }

    /// Called only when the controller's lyric storage/edit receiver completes.
    pub(super) fn take_storage_context(&mut self) -> (Option<PathBuf>, Option<PathBuf>) {
        let online_path = if self.lookup_busy() {
            None
        } else {
            self.online_lyrics_path.take()
        };
        (online_path, self.translation_after_lookup.take())
    }

    pub(super) fn start_romaji(&mut self, track: Option<&Track>) -> Result<PathBuf, String> {
        if self.romaji_request.is_some() {
            return Err("Romaji generation is already running".into());
        }
        let track = track.ok_or("Choose a song first")?;
        let settings = crate::ai::DeepSeekSettings::load(&self.directory.join("deepseek.json"))?;
        let request = romaji::Request::start(
            track.path.clone(),
            track.lyrics.clone(),
            track.title.clone(),
            track.artist.clone(),
            settings,
        )?;
        let path = request.path.clone();
        self.romaji_request = Some(request);
        Ok(path)
    }

    pub(super) fn cancel_romaji(&mut self) -> bool {
        self.romaji_request.take().is_some()
    }

    pub(super) fn remove_romaji(&mut self, path: PathBuf) -> RomajiRemoval {
        let cancelled = self
            .romaji_request
            .as_ref()
            .is_some_and(|request| request.path == path)
            && self.cancel_romaji();
        self.romaji_touched.insert(path.clone());
        RomajiRemoval {
            cancelled,
            result: self.romaji_store.remove(path),
        }
    }

    pub(super) fn poll_romaji(&mut self, tracks: &[Track]) -> Vec<RomajiEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.romaji_store.events.try_recv() {
            match event {
                romaji::StoreEvent::Loaded(Ok(mut cache)) => {
                    cache.retain(|path, _| !self.romaji_touched.contains(path));
                    events.push(RomajiEvent::Loaded(cache));
                }
                romaji::StoreEvent::Loaded(Err(error))
                | romaji::StoreEvent::Written(Err(error)) => {
                    events.push(RomajiEvent::StorageFailed(error));
                }
                romaji::StoreEvent::Written(Ok(())) => {}
            }
        }
        while let Some(event) = self
            .romaji_request
            .as_ref()
            .and_then(|request| request.events.try_recv().ok())
        {
            match event {
                romaji::Event::Progress { done, total } => {
                    events.push(RomajiEvent::Progress { done, total });
                }
                romaji::Event::Completed(result) => {
                    let Some(request) = self.romaji_request.take() else {
                        break;
                    };
                    if !tracks
                        .iter()
                        .any(|track| track.path == request.path && track.lyrics == request.source)
                    {
                        events.push(RomajiEvent::Finished {
                            notice: "Lyrics changed; romaji result discarded".into(),
                        });
                        continue;
                    }
                    match result {
                        Ok(romaji) => {
                            self.romaji_touched.insert(request.path.clone());
                            let storage =
                                self.romaji_store.save(request.path.clone(), romaji.clone());
                            events.push(RomajiEvent::Generated {
                                path: request.path.clone(),
                                romaji,
                                storage,
                            });
                        }
                        Err(error) => events.push(RomajiEvent::Finished {
                            notice: format!("Romaji generation failed: {error}"),
                        }),
                    }
                }
            }
        }
        events
    }
}

impl Drop for LyricsService {
    fn drop(&mut self) {
        self.cancel_lookup();
        if let Some(token) = self.translation_cancel.as_ref() {
            token.cancel();
        }
        self.cancel_romaji();
        // The romaji store drains queued local writes on drop. HTTP workers
        // observe cancellation and remain detached, as they were in Controller.
    }
}

// Controller regressions can install offline completions without exposing
// mutable worker channels, tokens, or cache ownership to production callers.
#[cfg(test)]
impl LyricsService {
    pub(super) fn insert_cached_for_test(&mut self, value: translation::Translation) {
        Arc::make_mut(&mut self.translation_cache).insert(value);
    }

    pub(super) fn cached_for_test(
        &self,
        request: &translation::Request,
    ) -> Option<&translation::Translation> {
        self.translation_cache.get(&request.cache_key())
    }

    pub(super) fn cache_is_empty_for_test(&self) -> bool {
        self.translation_cache.is_empty()
    }

    pub(super) fn finish_translation_writes_for_test(
        &mut self,
        current: Option<&Track>,
        target: TargetLanguage,
    ) -> Option<&'static str> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut notice = None;
        while !self.translation_writes.is_empty() {
            if let Some(value) = self.poll_translation_writes(current, target) {
                notice = Some(value);
            }
            assert!(
                std::time::Instant::now() < deadline,
                "translation cache writer timed out"
            );
            std::thread::yield_now();
        }
        notice
    }

    pub(super) fn save_cache_for_test(&self) -> Result<(), translation::Error> {
        self.translation_cache
            .save(&self.directory.join("translations.json"))
    }

    pub(super) fn active_translation_for_test(&self) -> Option<&translation::Request> {
        self.translation_active_request.as_ref()
    }

    pub(super) fn translation_generation_for_test(&self) -> u64 {
        self.translation_request_id
    }

    pub(super) fn set_translation_generation_for_test(&mut self, generation: u64) {
        self.translation_request_id = generation;
    }

    pub(super) fn install_translation_completion(
        &mut self,
        request: translation::Request,
        result: Result<translation::Translation, translation::Error>,
    ) -> Cancellation {
        let cancellation = Cancellation::default();
        let (sender, receiver) = mpsc::channel();
        self.translation_request_id = request.request_id;
        self.translation_active_request = Some(request.clone());
        self.translation_cancel = Some(cancellation.clone());
        sender.send((request, result)).unwrap();
        self.translation_request = Some(receiver);
        cancellation
    }

    pub(super) fn take_translation_completion_for_test(&mut self) -> Option<TranslationResult> {
        self.translation_request.take()?.try_recv().ok()
    }

    pub(super) fn install_lookup_completion(
        &mut self,
        request: online_lyrics::Request,
        result: Result<online_lyrics::Found, String>,
        cancellation: Cancellation,
    ) {
        let (sender, receiver) = mpsc::channel();
        self.online_lyrics_path = Some(request.path.clone());
        sender.send((request, result)).unwrap();
        self.online_lyrics_request = Some(receiver);
        self.online_lyrics_cancel = Some(cancellation);
    }

    pub(super) fn set_lookup_continuation_for_test(&mut self, path: Option<PathBuf>) {
        self.translation_after_lookup = path;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{database::Database, lyrics::Lyrics};

    fn track(directory: &Path, lyrics: Lyrics) -> Track {
        Track {
            path: directory.join("song.wav"),
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            genre: "Indie".into(),
            number: 1,
            duration: 123.0,
            artwork: Vec::new(),
            lyrics,
        }
    }

    fn request(track: &Track, generation: u64) -> translation::Request {
        translation::Request {
            request_id: generation,
            track_id: track.path.clone(),
            target: TargetLanguage::English,
            provider_version: format!(
                "{}:{}",
                translation::PROMPT_VERSION,
                crate::ai::DEFAULT_MODEL
            ),
            source: LyricsService::source(track),
        }
    }

    fn translated(request: &translation::Request, text: &str) -> translation::Translation {
        request
            .validate_response(&serde_json::json!({"translations": [text]}).to_string())
            .unwrap()
    }

    fn finish_translation(service: &mut LyricsService, track: &Track) -> Option<&'static str> {
        service
            .poll_translation(Some(track), TargetLanguage::English)
            .or_else(|| {
                service.finish_translation_writes_for_test(Some(track), TargetLanguage::English)
            })
    }

    fn found() -> online_lyrics::Found {
        online_lyrics::Found {
            lyrics: Lyrics::plain_text("original"),
            source_url: "https://lrclib.net/api/get/1".into(),
        }
    }

    #[test]
    fn inherited_t_starts_once_and_late_completion_preserves_hidden_preference() {
        for initially_visible in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let track = track(directory.path(), Lyrics::plain_text("original"));
            let mut service = LyricsService::new(directory.path().into(), initially_visible);
            assert!(service.shortcut_should_translate(Some(&track), TargetLanguage::English));
            service.mark_shortcut(Some(&track));
            service.set_visible(true);
            let request = request(&track, 1);
            service
                .install_translation_completion(request.clone(), Ok(translated(&request, "new")));
            assert!(!service.shortcut_should_translate(Some(&track), TargetLanguage::English));
            service.toggle_visible();
            assert!(!service.visible());
            assert!(service.translation_busy());
            assert_eq!(service.translation_generation_for_test(), 1);
            assert_eq!(
                finish_translation(&mut service, &track),
                Some("Translation ready")
            );
            assert!(!service.visible());
            assert!(!service.translation_busy());
            assert!(
                service
                    .cached_translation(&track, TargetLanguage::English)
                    .is_some()
            );
        }
    }

    #[test]
    fn cancelled_translation_keeps_admission_until_completion_and_rejects_queued_success() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::plain_text("original"));
        let mut service = LyricsService::new(directory.path().into(), false);
        let request = request(&track, 11);
        let token = service
            .install_translation_completion(request.clone(), Ok(translated(&request, "new")));
        service.cancel_translation();
        assert!(token.is_cancelled());
        assert!(service.translation_busy());
        assert_eq!(
            service.start_translation(Some(&track), TargetLanguage::English),
            Err("Translation is already running".into())
        );
        assert_eq!(
            service.poll_translation(Some(&track), TargetLanguage::English),
            Some("Lyrics or target changed; translation discarded")
        );
        assert!(!service.translation_busy());
        assert!(service.translation_cache.is_empty());
    }

    #[test]
    fn in_flight_translation_cancellation_rejects_success_after_worker_exits() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::plain_text("original"));
        let request = request(&track, 12);
        let value = translated(&request, "late");
        let mut service = LyricsService::new(directory.path().into(), false);
        let (sender, receiver) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let token = Cancellation::default();
        service.translation_request = Some(receiver);
        service.translation_request_id = request.request_id;
        service.translation_active_request = Some(request.clone());
        service.translation_cancel = Some(token.clone());
        let worker = std::thread::spawn(move || {
            held.recv().unwrap();
            sender.send((request, Ok(value))).unwrap();
        });
        assert_eq!(
            service.poll_translation(Some(&track), TargetLanguage::English),
            None
        );
        service.cancel_translation();
        assert!(token.is_cancelled());
        assert!(service.translation_busy());
        release.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(
            service.poll_translation(Some(&track), TargetLanguage::English),
            Some("Lyrics or target changed; translation discarded")
        );
        assert!(!service.translation_busy());
        assert!(service.translation_cache.is_empty());
    }

    #[test]
    fn translation_completion_checks_track_target_exact_source_and_provider() {
        for changed in ["track", "target", "text", "time", "provider"] {
            let directory = tempfile::tempdir().unwrap();
            let track = track(directory.path(), Lyrics::parse("[00:01]original"));
            let request = request(&track, 1);
            let mut service = LyricsService::new(directory.path().into(), false);
            service
                .install_translation_completion(request.clone(), Ok(translated(&request, "new")));
            let mut current = track.clone();
            let mut target = TargetLanguage::English;
            match changed {
                "track" => current.path = directory.path().join("other.wav"),
                "target" => target = TargetLanguage::Japanese,
                "text" => current.lyrics = Lyrics::parse("[00:01]manual edit"),
                "time" => current.lyrics = Lyrics::parse("[00:02]original"),
                "provider" => std::fs::write(
                    directory.path().join("deepseek.json"),
                    r#"{"model":"different-model"}"#,
                )
                .unwrap(),
                _ => unreachable!(),
            }
            assert_eq!(
                service.poll_translation(Some(&current), target),
                Some("Lyrics or target changed; translation discarded"),
                "{changed}"
            );
            assert!(service.translation_cache.is_empty(), "{changed}");
        }
    }

    #[test]
    fn failed_cache_write_rolls_back_regeneration_and_removal() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::plain_text("original"));
        let request = request(&track, 1);
        let old = translated(&request, "old");
        let mut service = LyricsService::new(directory.path().into(), true);
        service.insert_cached_for_test(old.clone());
        service.install_translation_completion(request.clone(), Ok(translated(&request, "new")));
        std::fs::create_dir(directory.path().join("translations.json")).unwrap();
        assert_eq!(
            finish_translation(&mut service, &track),
            Some("Could not save translation cache")
        );
        assert_eq!(
            service.translation_cache.get(&request.cache_key()),
            Some(&old)
        );
        service
            .remove_translation(Some(&track), TargetLanguage::English)
            .unwrap();
        assert_eq!(
            service.finish_translation_writes_for_test(Some(&track), TargetLanguage::English),
            Some("Could not remove translation")
        );
        assert_eq!(
            service.translation_cache.get(&request.cache_key()),
            Some(&old)
        );
        assert!(service.visible());
    }

    #[test]
    fn removing_translation_invalidates_pending_success_and_persists_removal() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::plain_text("original"));
        let request = request(&track, 21);
        let value = translated(&request, "old");
        let mut service = LyricsService::new(directory.path().into(), true);
        service.insert_cached_for_test(value.clone());
        service.save_cache_for_test().unwrap();
        let token = service.install_translation_completion(request.clone(), Ok(value));
        service
            .remove_translation(Some(&track), TargetLanguage::English)
            .unwrap();
        assert!(token.is_cancelled());
        assert!(service.translation_busy());
        assert_eq!(
            service.finish_translation_writes_for_test(Some(&track), TargetLanguage::English),
            Some("Translation removed")
        );
        assert!(!service.visible());
        assert_eq!(
            service.poll_translation(Some(&track), TargetLanguage::English),
            None
        );
        let cache = translation::Cache::load(&directory.path().join("translations.json")).unwrap();
        assert!(cache.get(&request.cache_key()).is_none());
    }

    #[test]
    fn accepted_translation_publishes_only_after_durable_ack_and_removal_drains_on_exit() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::plain_text("original"));
        let request = request(&track, 1);
        let value = translated(&request, "saved");
        let mut service = LyricsService::new(directory.path().into(), true);
        service.set_translation_generation_for_test(1);
        assert_eq!(
            service.apply_translation_result(
                request.clone(),
                Ok(value),
                Some(&track),
                TargetLanguage::English
            ),
            None
        );
        assert!(service.translation_busy());
        assert!(
            service.translation_cache.is_empty(),
            "unacknowledged data is not published"
        );
        service
            .remove_translation(Some(&track), TargetLanguage::English)
            .unwrap();
        assert_eq!(
            service.finish_translation_writes_for_test(Some(&track), TargetLanguage::English),
            Some("Translation removed")
        );
        assert!(!service.visible());
        assert!(service.translation_cache.is_empty());
        assert!(
            translation::Cache::load(&directory.path().join("translations.json"))
                .unwrap()
                .is_empty()
        );

        // Explicit local writes survive closing the app before the next UI poll.
        service.set_translation_generation_for_test(1);
        let value = translated(&request, "second");
        service.apply_translation_result(
            request.clone(),
            Ok(value),
            Some(&track),
            TargetLanguage::English,
        );
        service
            .remove_translation(Some(&track), TargetLanguage::English)
            .unwrap();
        drop(service);
        assert!(
            translation::Cache::load(&directory.path().join("translations.json"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn storage_completion_cannot_change_a_new_target_or_cancelled_notice() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::plain_text("original"));
        let request = request(&track, 1);
        let mut service = LyricsService::new(directory.path().into(), true);
        service.set_translation_generation_for_test(1);
        service.apply_translation_result(
            request.clone(),
            Ok(translated(&request, "saved")),
            Some(&track),
            TargetLanguage::English,
        );
        service.change_target(TargetLanguage::English, TargetLanguage::Japanese);
        assert_eq!(
            service.finish_translation_writes_for_test(Some(&track), TargetLanguage::Japanese),
            None
        );
        assert!(
            service
                .cached_translation(&track, TargetLanguage::Japanese)
                .is_none()
        );
        service.apply_translation_result(
            request.clone(),
            Ok(translated(&request, "again")),
            Some(&track),
            TargetLanguage::English,
        );
        service.cancel_translation();
        assert_eq!(
            service.finish_translation_writes_for_test(Some(&track), TargetLanguage::English),
            None
        );
    }

    #[test]
    fn queued_success_is_rejected_after_a_target_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::plain_text("original"));
        let request = request(&track, 1);
        let mut service = LyricsService::new(directory.path().into(), true);
        let token = service
            .install_translation_completion(request.clone(), Ok(translated(&request, "obsolete")));
        service.change_target(TargetLanguage::English, TargetLanguage::Japanese);
        service.change_target(TargetLanguage::Japanese, TargetLanguage::English);
        assert!(token.is_cancelled());
        assert_eq!(
            service.poll_translation(Some(&track), TargetLanguage::English),
            Some("Lyrics or target changed; translation discarded")
        );
        assert!(!service.translation_busy());
        assert!(service.translation_cache.is_empty());
        assert!(!directory.path().join("translations.json").exists());
    }

    #[test]
    fn removal_ack_preserves_a_newer_cancel_or_display_choice() {
        for show_again in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let track = track(directory.path(), Lyrics::plain_text("original"));
            let request = request(&track, 1);
            let mut service = LyricsService::new(directory.path().into(), true);
            service.insert_cached_for_test(translated(&request, "old"));
            service
                .remove_translation(Some(&track), TargetLanguage::English)
                .unwrap();
            if show_again {
                service.set_visible(true);
            } else {
                service.cancel_translation();
            }
            let revision = service.translation_cache_revision();
            assert_eq!(
                service.finish_translation_writes_for_test(Some(&track), TargetLanguage::English),
                None
            );
            assert!(service.visible());
            assert_ne!(service.translation_cache_revision(), revision);
            assert!(service.translation_cache.is_empty());
            assert!(
                translation::Cache::load(&directory.path().join("translations.json"))
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn disconnected_translation_worker_clears_admission_and_reports_once() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::plain_text("original"));
        let mut service = LyricsService::new(directory.path().into(), false);
        let (sender, receiver) = mpsc::channel();
        service.translation_request = Some(receiver);
        assert_eq!(
            service.poll_translation(Some(&track), TargetLanguage::English),
            None
        );
        drop(sender);
        assert_eq!(
            service.poll_translation(Some(&track), TargetLanguage::English),
            Some("Translation failed; try again")
        );
        assert!(!service.translation_busy());
        assert_eq!(
            service.poll_translation(Some(&track), TargetLanguage::English),
            None
        );
    }

    #[test]
    fn cancelled_lookup_keeps_admission_and_cannot_publish_queued_result() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::default());
        let request = online_lyrics::Request::new(&track);
        let mut service = LyricsService::new(directory.path().into(), false);
        let token = Cancellation::default();
        service.set_lookup_continuation_for_test(Some(track.path.clone()));
        service.install_lookup_completion(request, Ok(found()), token.clone());
        assert_eq!(service.cancel_lookup(), Some(track.path.clone()));
        assert!(token.is_cancelled());
        assert!(service.lookup_busy());
        assert_eq!(
            service.start_lookup(Some(&track), false),
            Err("A lyric request is already running".into())
        );
        let event = service
            .poll_lookup(
                std::slice::from_ref(&track),
                &BTreeSet::new(),
                false,
                Some(&track),
            )
            .unwrap();
        assert!(
            matches!(event, LookupEvent::Failed { path: Some(path), notice }
            if path == track.path && notice == "Song or lyrics changed; online result discarded")
        );
        assert!(!service.lookup_busy());
        assert_eq!(service.take_storage_context(), (None, None));
    }

    #[test]
    fn manual_plain_lyrics_never_trigger_online_timestamp_discovery() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::plain_text("my words"));
        let db = Database::open(&directory.path().join("library.sqlite3")).unwrap();
        db.set_manual_lyrics(
            &track.path,
            &directory.path().join("manual.txt"),
            &track.lyrics,
        )
        .unwrap();
        std::fs::write(
            directory.path().join("deepseek.json"),
            r#"{"api_key":"offline-test-key","model":"deepseek-flash"}"#,
        )
        .unwrap();
        let mut service = LyricsService::new(directory.path().into(), false);
        service.start_lookup(Some(&track), false).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let event = loop {
            if let Some(event) = service.poll_lookup(
                std::slice::from_ref(&track),
                &BTreeSet::new(),
                false,
                Some(&track),
            ) {
                break event;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert!(
            matches!(event, LookupEvent::Failed { notice, .. } if notice == "Lyrics are already available")
        );
        assert!(!service.lookup_busy());
    }

    #[test]
    fn explicit_find_can_target_noncurrent_song_but_translation_continuation_cannot() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::default());
        let request = online_lyrics::Request::new(&track);
        let mut service = LyricsService::new(directory.path().into(), false);
        let tracks = std::slice::from_ref(&track);
        let unavailable = BTreeSet::new();
        assert!(service.accepts_lookup(&request, tracks, &unavailable, false, None));
        service.set_lookup_continuation_for_test(Some(track.path.clone()));
        assert!(!service.accepts_lookup(&request, tracks, &unavailable, false, None));
        assert!(service.accepts_lookup(&request, tracks, &unavailable, false, Some(&track)));
        assert!(!service.accepts_lookup(&request, tracks, &unavailable, true, Some(&track)));
        assert!(!service.accepts_lookup(
            &request,
            tracks,
            &BTreeSet::from([track.path.clone()]),
            false,
            Some(&track)
        ));
    }

    #[test]
    fn accepted_lookup_retains_storage_context_and_cancel_stops_only_continuation() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::default());
        let mut service = LyricsService::new(directory.path().into(), false);
        service.set_lookup_continuation_for_test(Some(track.path.clone()));
        service.install_lookup_completion(
            online_lyrics::Request::new(&track),
            Ok(found()),
            Cancellation::default(),
        );
        assert!(matches!(
            service.poll_lookup(
                std::slice::from_ref(&track),
                &BTreeSet::new(),
                false,
                Some(&track)
            ),
            Some(LookupEvent::Found { .. })
        ));
        assert!(!service.lookup_busy());
        assert_eq!(service.lookup_path(), Some(track.path.as_path()));
        service.cancel_lookup();
        assert_eq!(service.take_storage_context(), (Some(track.path), None));
    }

    #[test]
    fn disconnected_lookup_clears_admission_and_continuation() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::default());
        let mut service = LyricsService::new(directory.path().into(), false);
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        service.online_lyrics_request = Some(receiver);
        service.online_lyrics_path = Some(track.path.clone());
        service.translation_after_lookup = Some(track.path.clone());
        assert!(matches!(
            service.poll_lookup(std::slice::from_ref(&track), &BTreeSet::new(), false, Some(&track)),
            Some(LookupEvent::Failed { path: Some(path), notice })
                if path == track.path && notice == "Online lyric search failed"
        ));
        assert_eq!(service.take_storage_context(), (None, None));
        assert!(!service.lookup_busy());
    }

    #[test]
    fn rejected_lookup_during_manual_edit_preserves_existing_storage_status_handoff() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::default());
        let mut service = LyricsService::new(directory.path().into(), false);
        service.install_lookup_completion(
            online_lyrics::Request::new(&track),
            Ok(found()),
            Cancellation::default(),
        );
        assert!(matches!(
            service.poll_lookup(std::slice::from_ref(&track), &BTreeSet::new(), true, Some(&track)),
            Some(LookupEvent::Failed { path: Some(path), notice })
                if path == track.path && notice == "Song or lyrics changed; online result discarded"
        ));
        assert_eq!(service.lookup_path(), Some(track.path.as_path()));
        assert_eq!(service.take_storage_context(), (Some(track.path), None));
    }

    #[test]
    fn romaji_remove_before_initial_load_does_not_resurrect_cache_and_drains_write() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::plain_text("君"));
        let value =
            romaji::Romaji::new(track.lyrics.clone(), vec!["kimi".into()], "test".into()).unwrap();
        let database_path = directory.path().join("library.sqlite3");
        {
            let database = Database::open(&database_path).unwrap();
            database.save_romaji(&track.path, &value).unwrap();
        }
        let mut service = LyricsService::new(directory.path().into(), false);
        let removal = service.remove_romaji(track.path.clone());
        assert!(!removal.cancelled);
        removal.result.unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut loaded = false;
        while !loaded && std::time::Instant::now() < deadline {
            for event in service.poll_romaji(std::slice::from_ref(&track)) {
                if let RomajiEvent::Loaded(cache) = event {
                    assert!(!cache.contains_key(&track.path));
                    loaded = true;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(loaded, "initial cache must arrive");
        drop(service);
        assert!(
            Database::open(&database_path)
                .unwrap()
                .load_romaji()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn dropping_service_cancels_network_tokens_without_waiting_for_workers() {
        let directory = tempfile::tempdir().unwrap();
        let track = track(directory.path(), Lyrics::plain_text("original"));
        let mut service = LyricsService::new(directory.path().into(), false);
        let request = request(&track, 1);
        let translation =
            service.install_translation_completion(request, Err(translation::Error::Provider));
        let lookup = Cancellation::default();
        service.install_lookup_completion(
            online_lyrics::Request::new(&track),
            Err("failed".into()),
            lookup.clone(),
        );
        drop(service);
        assert!(translation.is_cancelled());
        assert!(lookup.is_cancelled());
    }
}
