//! Desktop artist portraits. A single detached worker owns disk and network I/O.
//!
//! Creating the service or asking for cached images never starts a web search.
//! Enqueueing imported artists or explicitly retrying activates the durable queue.
mod provider;

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender, SyncSender},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use unicode_normalization::UnicodeNormalization;

const QUEUE_LIMIT: u64 = 8 * 1024 * 1024;
// Leave enough room for the bounded credit, filename and error of every job.
const RESULT_RESERVE: u64 = 8192;
const IMAGE_LIMIT: u64 = 8 * 1024 * 1024;
const THUMBNAIL_SIZE: u32 = 640;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistRequest {
    pub artist: String,
    pub albums: Vec<String>,
}

/// Case, canonical Unicode and redundant whitespace are equivalent. Punctuation,
/// accents and collaboration credits are deliberately preserved.
pub fn normalized_artist_key(name: &str) -> String {
    name.nfc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageCredit {
    pub author: String,
    pub license: String,
    pub license_url: String,
    pub description_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Pending,
    Running,
    Failed,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSnapshot {
    pub key: String,
    pub artist: String,
    pub albums: Vec<String>,
    pub state: JobState,
    pub attempts: u32,
    pub next_retry: i64,
    pub error: Option<String>,
    pub credit: Option<ImageCredit>,
    /// A hash-based, relative filename, never a provider-supplied filesystem path.
    pub asset: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueSnapshot {
    pub jobs: Vec<JobSnapshot>,
    pub pending: usize,
    pub running: usize,
    pub failed: usize,
    pub ready: usize,
}

pub enum Event {
    Queue(QueueSnapshot),
    Image {
        key: String,
        pixels: RgbaImage,
        credit: ImageCredit,
    },
    Error(String),
    /// Previously reported queue problems were recovered and saved successfully.
    Recovered,
}

enum Command {
    Enqueue(Vec<ArtistRequest>),
    Retry,
    Cached(Vec<String>),
}

pub struct ArtistImages {
    commands: Sender<Command>,
    events: Receiver<Event>,
}

impl ArtistImages {
    pub fn new(directory: PathBuf) -> Self {
        Self::with_provider(directory.clone(), move || {
            Box::new(provider::WebProvider::new(directory))
        })
    }

    fn with_provider(
        directory: PathBuf,
        provider: impl FnOnce() -> Box<dyn Provider> + Send + 'static,
    ) -> Self {
        let (commands, receive) = mpsc::channel();
        let (send, events) = mpsc::sync_channel(16);
        // No JoinHandle: shutdown and playback never wait for network timeouts.
        thread::spawn(move || Worker::new(directory, provider(), send).run(receive));
        Self { commands, events }
    }

    pub fn enqueue(&self, artists: Vec<ArtistRequest>) {
        let _ = self.commands.send(Command::Enqueue(artists));
    }

    pub fn retry_failed(&self) {
        let _ = self.commands.send(Command::Retry);
    }

    pub fn request_cached(&self, artists: Vec<String>) {
        let _ = self.commands.send(Command::Cached(artists));
    }

    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

struct Portrait {
    pixels: RgbaImage,
    credit: ImageCredit,
}

trait Provider: Send {
    fn find(&mut self, request: &ArtistRequest) -> Result<Portrait, String>;
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueFile {
    version: u32,
    jobs: BTreeMap<String, JobSnapshot>,
}

struct Worker {
    root: PathBuf,
    queue: QueueFile,
    provider: Box<dyn Provider>,
    events: SyncSender<Event>,
    active: bool,
    dirty: bool,
    persistence_retry: i64,
    preserve_invalid_queue: bool,
    had_storage_error: bool,
    had_capacity_error: bool,
    capacity_recovery_pending: bool,
    clock: Box<dyn Fn() -> i64 + Send>,
    queue_limit: u64,
}

impl Worker {
    fn new(directory: PathBuf, provider: Box<dyn Provider>, events: SyncSender<Event>) -> Self {
        let root = directory.join("artist-images-v1");
        let loaded = load_queue(&root.join("queue.json"));
        let preserve_invalid_queue = loaded.is_err();
        let mut queue = match loaded {
            Ok(queue) => queue,
            Err(error) => {
                let _ = events.send(Event::Error(error));
                QueueFile {
                    version: 1,
                    ..Default::default()
                }
            }
        };
        let mut dirty = false;
        for job in queue.jobs.values_mut() {
            if job.state == JobState::Running {
                job.state = JobState::Pending;
                job.next_retry = 0;
                dirty = true;
            }
        }
        Self {
            root,
            queue,
            provider,
            events,
            active: false,
            dirty,
            persistence_retry: 0,
            preserve_invalid_queue,
            had_storage_error: preserve_invalid_queue,
            had_capacity_error: false,
            capacity_recovery_pending: false,
            clock: Box::new(unix_now),
            queue_limit: QUEUE_LIMIT,
        }
    }

    fn emit_queue(&self) {
        let _ = self.events.send(Event::Queue(snapshot(&self.queue.jobs)));
    }

    fn persist(&mut self, now: i64) -> bool {
        if !self.dirty {
            return true;
        }
        if now < self.persistence_retry {
            return false;
        }
        let result = serde_json::to_vec_pretty(&self.queue)
            .map_err(|_| "Cannot encode artist image queue".into())
            .and_then(|bytes| {
                if bytes.len() as u64 > self.queue_limit {
                    return Err("Artist image queue exceeds storage limit".into());
                }
                if self.preserve_invalid_queue {
                    let backup = self
                        .root
                        .join(format!("queue.corrupt-{}.json", rand::random::<u64>()));
                    fs::rename(self.root.join("queue.json"), backup)
                        .map_err(|_| "Cannot preserve invalid artist image queue")?;
                    self.preserve_invalid_queue = false;
                }
                atomic_write(&self.root.join("queue.json"), &bytes)
            });
        match result {
            Ok(()) => {
                self.dirty = false;
                self.persistence_retry = 0;
                let had_error = self.had_storage_error || self.had_capacity_error;
                self.had_storage_error = false;
                if self.capacity_recovery_pending {
                    self.had_capacity_error = false;
                    self.capacity_recovery_pending = false;
                }
                if had_error && !self.had_capacity_error {
                    let _ = self.events.send(Event::Recovered);
                }
                true
            }
            Err(error) => {
                self.persistence_retry = now.saturating_add(30);
                self.had_storage_error = true;
                let _ = self.events.send(Event::Error(error));
                false
            }
        }
    }

    fn command(&mut self, command: Command, now: i64) {
        match command {
            Command::Enqueue(requests) => {
                self.active = true;
                let mut rejected = false;
                for request in requests {
                    if !valid_artist(&request.artist) {
                        continue;
                    }
                    let key = normalized_artist_key(&request.artist);
                    let previous = self.queue.jobs.get(&key).cloned();
                    let was_dirty = self.dirty;
                    let albums = clean_albums(request.albums);
                    match self.queue.jobs.get_mut(&key) {
                        Some(job) => {
                            let mut combined = job.albums.iter().cloned().collect::<BTreeSet<_>>();
                            combined.extend(albums);
                            let combined = combined.into_iter().take(100).collect::<Vec<_>>();
                            if job.albums != combined {
                                job.albums = combined;
                                // New context may resolve an ambiguous identity immediately.
                                if job.state == JobState::Failed {
                                    job.state = JobState::Pending;
                                    job.next_retry = 0;
                                    job.error = None;
                                }
                                self.dirty = true;
                            }
                        }
                        None => {
                            self.queue.jobs.insert(
                                key.clone(),
                                JobSnapshot {
                                    key: key.clone(),
                                    artist: request.artist.trim().to_owned(),
                                    albums,
                                    state: JobState::Pending,
                                    attempts: 0,
                                    next_retry: 0,
                                    error: None,
                                    credit: None,
                                    asset: None,
                                },
                            );
                            self.dirty = true;
                        }
                    }
                    if !self.within_capacity() {
                        if let Some(previous) = previous {
                            self.queue.jobs.insert(key, previous);
                        } else {
                            self.queue.jobs.remove(&key);
                        }
                        self.dirty = was_dirty;
                        rejected = true;
                    }
                }
                if rejected {
                    self.had_capacity_error = true;
                    self.capacity_recovery_pending = false;
                    let _ = self.events.send(Event::Error(
                        "Artist image queue is full; existing tasks will continue".into(),
                    ));
                } else if self.had_capacity_error {
                    // An unchanged but now fully accepted request must still
                    // reach disk before the UI clears its capacity warning.
                    self.capacity_recovery_pending = true;
                    self.dirty = true;
                }
            }
            Command::Retry => {
                self.active = true;
                self.persistence_retry = 0;
                for job in self.queue.jobs.values_mut() {
                    if job.state == JobState::Failed {
                        job.state = JobState::Pending;
                        job.next_retry = 0;
                        job.error = None;
                        self.dirty = true;
                    }
                }
            }
            Command::Cached(artists) => {
                let keys = artists
                    .into_iter()
                    .map(|artist| normalized_artist_key(&artist))
                    .collect::<BTreeSet<_>>();
                for key in keys {
                    self.cached(&key);
                }
            }
        }
        self.persist(now);
        self.emit_queue();
    }

    fn within_capacity(&self) -> bool {
        serde_json::to_vec_pretty(&self.queue).is_ok_and(|bytes| {
            let unfinished = self
                .queue
                .jobs
                .values()
                .filter(|job| job.state != JobState::Ready)
                .count() as u64;
            (bytes.len() as u64).saturating_add(unfinished.saturating_mul(RESULT_RESERVE))
                <= self.queue_limit
        })
    }

    fn cached(&mut self, key: &str) {
        let Some(job) = self
            .queue
            .jobs
            .get(key)
            .filter(|job| job.state == JobState::Ready)
        else {
            return;
        };
        let Some(credit) = job.credit.clone().filter(provider::valid_credit) else {
            self.invalidate_asset(key);
            return;
        };
        let Some(asset) = job
            .asset
            .as_deref()
            .filter(|asset| **asset == asset_name(key))
        else {
            self.invalidate_asset(key);
            return;
        };
        let pixels = read_limited(&self.root.join(asset), IMAGE_LIMIT)
            .and_then(|bytes| decode_image(&bytes));
        match pixels {
            Ok(pixels) => {
                let _ = self.events.send(Event::Image {
                    key: key.into(),
                    pixels,
                    credit,
                });
            }
            Err(_) => self.invalidate_asset(key),
        }
    }

    fn invalidate_asset(&mut self, key: &str) {
        if let Some(job) = self.queue.jobs.get_mut(key) {
            job.state = JobState::Pending;
            job.asset = None;
            job.credit = None;
            job.error = Some("Cached artist image is missing or invalid".into());
            job.next_retry = 0;
            self.dirty = true;
        }
    }

    fn next(&self, now: i64) -> Option<String> {
        if !self.active || self.dirty {
            return None;
        }
        self.queue
            .jobs
            .iter()
            .find(|(_, job)| {
                job.state == JobState::Pending
                    || (job.state == JobState::Failed && job.next_retry <= now)
            })
            .map(|(key, _)| key.clone())
    }

    fn process(&mut self, key: &str, now: i64) {
        let request = {
            let job = self.queue.jobs.get_mut(key).unwrap();
            job.state = JobState::Running;
            job.attempts = job.attempts.saturating_add(1);
            ArtistRequest {
                artist: job.artist.clone(),
                albums: job.albums.clone(),
            }
        };
        self.dirty = true;
        if !self.persist(now) {
            self.queue.jobs.get_mut(key).unwrap().state = JobState::Pending;
            self.emit_queue();
            return;
        }
        self.emit_queue();
        let result = self.provider.find(&request).and_then(|portrait| {
            if !provider::valid_credit(&portrait.credit) {
                return Err("Artist image has no reusable license or attribution".into());
            }
            let pixels = thumbnail(image::DynamicImage::ImageRgba8(portrait.pixels));
            let mut encoded = Cursor::new(Vec::new());
            pixels
                .write_to(&mut encoded, image::ImageFormat::Png)
                .map_err(|_| "Cannot encode artist image")?;
            let asset = asset_name(key);
            atomic_write(&self.root.join(&asset), encoded.get_ref())?;
            Ok((pixels, portrait.credit, asset))
        });
        let job = self.queue.jobs.get_mut(key).unwrap();
        match result {
            Ok((pixels, credit, asset)) => {
                job.state = JobState::Ready;
                job.next_retry = 0;
                job.error = None;
                job.asset = Some(asset);
                job.credit = Some(credit.clone());
                let _ = self.events.send(Event::Image {
                    key: key.into(),
                    pixels,
                    credit,
                });
            }
            Err(error) => {
                job.state = JobState::Failed;
                let completed_at = (self.clock)().max(now);
                job.next_retry = completed_at.saturating_add(backoff(job.attempts));
                job.error = Some(error.chars().take(300).collect());
            }
        }
        self.dirty = true;
        self.persist((self.clock)().max(now));
        self.emit_queue();
    }

    fn run(mut self, commands: Receiver<Command>) {
        self.emit_queue();
        loop {
            match commands.recv_timeout(Duration::from_millis(250)) {
                Ok(command) => self.command(command, unix_now()),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            // Drain imports before resolving, so album context is complete.
            while let Ok(command) = commands.try_recv() {
                self.command(command, unix_now());
            }
            let now = unix_now();
            self.persist(now);
            if let Some(key) = self.next(now) {
                self.process(&key, now);
            }
        }
    }
}

fn valid_artist(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 512 && !name.chars().any(char::is_control)
}

fn clean_albums(albums: Vec<String>) -> Vec<String> {
    albums
        .into_iter()
        .filter(|album| valid_artist(album))
        .map(|album| album.trim().to_owned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(100)
        .collect()
}

fn snapshot(jobs: &BTreeMap<String, JobSnapshot>) -> QueueSnapshot {
    let mut result = QueueSnapshot {
        jobs: jobs.values().cloned().collect(),
        ..Default::default()
    };
    for job in jobs.values() {
        match job.state {
            JobState::Pending => result.pending += 1,
            JobState::Running => result.running += 1,
            JobState::Failed => result.failed += 1,
            JobState::Ready => result.ready += 1,
        }
    }
    result
}

fn load_queue(path: &Path) -> Result<QueueFile, String> {
    if !path.exists() {
        return Ok(QueueFile {
            version: 1,
            ..Default::default()
        });
    }
    let queue: QueueFile = serde_json::from_slice(&read_limited(path, QUEUE_LIMIT)?)
        .map_err(|_| "Artist image queue is invalid; original file retained")?;
    if queue.version != 1
        || queue.jobs.iter().any(|(key, job)| {
            key != &job.key
                || *key != normalized_artist_key(&job.artist)
                || !valid_artist(&job.artist)
        })
    {
        return Err("Artist image queue is invalid; original file retained".into());
    }
    Ok(queue)
}

fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "Cannot read artist image cache")?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read artist image cache")?;
    if bytes.len() as u64 > limit {
        return Err("Artist image cache exceeds size limit".into());
    }
    Ok(bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid artist image cache path")?;
    fs::create_dir_all(parent).map_err(|_| "Cannot create artist image cache directory")?;
    let temporary = path.with_extension(format!("{}.tmp", rand::random::<u64>()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "Cannot write artist image cache")?;
        file.write_all(bytes)
            .map_err(|_| "Cannot write artist image cache")?;
        file.sync_all()
            .map_err(|_| "Cannot save artist image cache")?;
        fs::rename(&temporary, path).map_err(|_| "Cannot replace artist image cache")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn asset_name(key: &str) -> String {
    format!("{:x}.png", Sha256::digest(key.as_bytes()))
}

fn backoff(attempts: u32) -> i64 {
    (60_i64.saturating_mul(
        1_i64
            .checked_shl(attempts.saturating_sub(1).min(20))
            .unwrap_or(i64::MAX),
    ))
    .min(86_400)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

fn decode_image(bytes: &[u8]) -> Result<RgbaImage, String> {
    if bytes.len() as u64 > IMAGE_LIMIT {
        return Err("Artist image exceeds download size limit".into());
    }
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "Artist image is not a supported image")?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Jpeg | image::ImageFormat::Png)
    ) {
        return Err("Artist image is not PNG or JPEG".into());
    }
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| "Cannot read artist image dimensions")?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 24_000_000 {
        return Err("Artist image exceeds pixel limit".into());
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "Cannot read artist image")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(10_000);
    limits.max_image_height = Some(10_000);
    limits.max_alloc = Some(100 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map(thumbnail)
        .map_err(|_| "Cannot decode artist image".into())
}

fn thumbnail(image: image::DynamicImage) -> RgbaImage {
    image
        .thumbnail(
            THUMBNAIL_SIZE.min(image.width()),
            THUMBNAIL_SIZE.min(image.height()),
        )
        .to_rgba8()
}

#[cfg(test)]
mod tests;
