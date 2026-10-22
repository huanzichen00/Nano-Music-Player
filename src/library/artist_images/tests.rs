use super::*;
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct FixtureProvider {
    calls: Arc<AtomicUsize>,
    results: VecDeque<Result<Portrait, String>>,
}

impl Provider for FixtureProvider {
    fn find(&mut self, _: &ArtistRequest) -> Result<Portrait, String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.results
            .pop_front()
            .unwrap_or_else(|| Err("fixture offline".into()))
    }
}

fn credit() -> ImageCredit {
    ImageCredit {
        author: "Portrait Photographer".into(),
        license: "CC BY-SA 4.0".into(),
        license_url: "https://creativecommons.org/licenses/by-sa/4.0/".into(),
        description_url: "https://commons.wikimedia.org/wiki/File:Artist.jpg".into(),
    }
}

fn portrait() -> Portrait {
    Portrait {
        pixels: RgbaImage::from_pixel(20, 10, image::Rgba([1, 2, 3, 255])),
        credit: credit(),
    }
}

fn setup(
    directory: &Path,
    results: Vec<Result<Portrait, String>>,
) -> (Worker, Receiver<Event>, Arc<AtomicUsize>) {
    let (sender, receiver) = mpsc::sync_channel(100);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut worker = Worker::new(
        directory.to_owned(),
        Box::new(FixtureProvider {
            calls: calls.clone(),
            results: results.into(),
        }),
        sender,
    );
    worker.clock = Box::new(|| 100);
    (worker, receiver, calls)
}

fn artist(name: &str, albums: &[&str]) -> ArtistRequest {
    ArtistRequest {
        artist: name.into(),
        albums: albums.iter().map(|album| (*album).into()).collect(),
    }
}

#[test]
fn imported_album_jobs_deduplicate_names_without_merging_distinct_credits() {
    let directory = tempfile::tempdir().unwrap();
    let (mut worker, _, _) = setup(directory.path(), vec![]);
    worker.command(
        Command::Enqueue(vec![
            artist("  Björk ", &["Debut"]),
            artist("BJÖRK", &["Post"]),
            artist("Bjo\u{308}rk", &["Debut"]),
            artist("Bjork", &["Other"]),
            artist("Björk & Guest", &["Live"]),
        ]),
        100,
    );
    assert_eq!(worker.queue.jobs.len(), 3);
    assert_eq!(worker.queue.jobs["björk"].albums, ["Debut", "Post"]);
    assert!(worker.queue.jobs.contains_key("bjork"));
    let reloaded = load_queue(&directory.path().join("artist-images-v1/queue.json")).unwrap();
    assert_eq!(reloaded.jobs.len(), 3);
}

#[test]
fn success_is_cached_on_disk_and_does_not_repeat_search_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let (mut worker, events, calls) = setup(directory.path(), vec![Ok(portrait())]);
    worker.command(
        Command::Enqueue(vec![artist("Blackfield", &["Blackfield"])]),
        100,
    );
    worker.process("blackfield", 100);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let ready = &worker.queue.jobs["blackfield"];
    assert_eq!(ready.state, JobState::Ready);
    assert_eq!(ready.credit.as_ref(), Some(&credit()));
    assert!(
        events
            .try_iter()
            .any(|event| matches!(event, Event::Image { .. }))
    );
    let (mut restarted, events, calls) = setup(directory.path(), vec![]);
    assert!(!restarted.active);
    restarted.command(Command::Cached(vec!["BLACKFIELD".into()]), 100);
    assert!(
        events
            .try_iter()
            .any(|event| matches!(event, Event::Image { pixels, .. } if pixels.width() == 20))
    );
    restarted.command(
        Command::Enqueue(vec![artist("Blackfield", &["New Album"])]),
        100,
    );
    assert!(restarted.next(100).is_none());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn failed_requests_remain_durable_with_bounded_backoff_and_manual_retry() {
    let directory = tempfile::tempdir().unwrap();
    let (mut worker, _, calls) = setup(
        directory.path(),
        vec![Err("offline".into()), Ok(portrait())],
    );
    worker.command(Command::Enqueue(vec![artist("Blackfield", &[])]), 100);
    worker.process("blackfield", 100);
    let failed = &worker.queue.jobs["blackfield"];
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.error.as_deref(), Some("offline"));
    assert_eq!(failed.next_retry, 160);
    assert!(worker.next(159).is_none());
    assert_eq!(worker.next(160).as_deref(), Some("blackfield"));
    worker.command(Command::Retry, 101);
    worker.process("blackfield", 101);
    assert_eq!(worker.queue.jobs["blackfield"].state, JobState::Ready);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(
        (backoff(1), backoff(2), backoff(u32::MAX)),
        (60, 120, 86_400)
    );
}

#[test]
fn interrupted_job_resumes_pending_only_after_enqueue_activation() {
    let directory = tempfile::tempdir().unwrap();
    let (mut worker, _, _) = setup(directory.path(), vec![]);
    worker.command(Command::Enqueue(vec![artist("Blackfield", &[])]), 100);
    worker.queue.jobs.get_mut("blackfield").unwrap().state = JobState::Running;
    worker.dirty = true;
    assert!(worker.persist(100));
    let (mut restarted, _, _) = setup(directory.path(), vec![]);
    assert_eq!(restarted.queue.jobs["blackfield"].state, JobState::Pending);
    assert!(restarted.next(100).is_none());
    restarted.command(Command::Cached(vec![]), 100);
    assert!(restarted.next(100).is_none());
    restarted.command(Command::Enqueue(vec![]), 100);
    assert_eq!(restarted.next(100).as_deref(), Some("blackfield"));
}

#[test]
fn newly_imported_album_context_reopens_ambiguous_failure() {
    let directory = tempfile::tempdir().unwrap();
    let (mut worker, _, _) = setup(directory.path(), vec![Err("Ambiguous artist".into())]);
    worker.command(Command::Enqueue(vec![artist("Phoenix", &["Live"])]), 100);
    worker.process("phoenix", 100);
    worker.command(Command::Enqueue(vec![artist("Phoenix", &["Live"])]), 101);
    assert_eq!(worker.queue.jobs["phoenix"].state, JobState::Failed);
    worker.command(
        Command::Enqueue(vec![artist("Phoenix", &["Wolfgang Amadeus Phoenix"])]),
        101,
    );
    assert_eq!(worker.queue.jobs["phoenix"].state, JobState::Pending);
}

#[test]
fn atomic_queue_write_failure_keeps_job_in_memory_and_never_starts_network() {
    let directory = tempfile::tempdir().unwrap();
    let (mut worker, _, calls) = setup(directory.path(), vec![Ok(portrait())]);
    let blocker = directory.path().join("artist-images-v1");
    fs::write(&blocker, "not a directory").unwrap();
    worker.command(Command::Enqueue(vec![artist("Blackfield", &[])]), 100);
    assert!(worker.dirty);
    assert_eq!(worker.queue.jobs["blackfield"].state, JobState::Pending);
    assert!(worker.next(100).is_none());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    fs::remove_file(blocker).unwrap();
    assert!(worker.persist(131));
    worker.process("blackfield", 131);
    assert_eq!(worker.queue.jobs["blackfield"].state, JobState::Ready);
}

#[test]
fn corrupt_cached_image_is_requeued_without_search_from_cache_request() {
    let directory = tempfile::tempdir().unwrap();
    let (mut worker, _, _) = setup(directory.path(), vec![Ok(portrait())]);
    worker.command(Command::Enqueue(vec![artist("Blackfield", &[])]), 100);
    worker.process("blackfield", 100);
    fs::write(worker.root.join(asset_name("blackfield")), b"broken PNG").unwrap();
    let (mut restarted, events, calls) = setup(directory.path(), vec![]);
    restarted.command(Command::Cached(vec!["Blackfield".into()]), 100);
    assert_eq!(restarted.queue.jobs["blackfield"].state, JobState::Pending);
    assert!(
        !events
            .try_iter()
            .any(|event| matches!(event, Event::Image { .. }))
    );
    assert!(restarted.next(100).is_none());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn invalid_queue_is_backed_up_before_new_imports_can_replace_it() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("artist-images-v1");
    fs::create_dir_all(&root).unwrap();
    let original = b"{not a valid queue";
    fs::write(root.join("queue.json"), original).unwrap();
    let (mut worker, events, _) = setup(directory.path(), vec![]);
    assert!(
        events
            .try_iter()
            .any(|event| matches!(event, Event::Error(_)))
    );
    worker.command(Command::Enqueue(vec![artist("Blackfield", &[])]), 100);
    let backup = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .find(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
        .unwrap();
    assert_eq!(fs::read(backup.path()).unwrap(), original);
    assert_eq!(load_queue(&root.join("queue.json")).unwrap().jobs.len(), 1);
}

#[test]
fn no_license_means_no_saved_or_published_portrait() {
    let directory = tempfile::tempdir().unwrap();
    let mut unlicensed = portrait();
    unlicensed.credit.license = "All rights reserved".into();
    let (mut worker, events, _) = setup(directory.path(), vec![Ok(unlicensed)]);
    worker.command(Command::Enqueue(vec![artist("Blackfield", &[])]), 100);
    worker.process("blackfield", 100);
    assert_eq!(worker.queue.jobs["blackfield"].state, JobState::Failed);
    assert!(!worker.root.join(asset_name("blackfield")).exists());
    assert!(
        !events
            .try_iter()
            .any(|event| matches!(event, Event::Image { .. }))
    );
}

#[test]
fn image_decoder_rejects_size_bombs_and_accepts_a_real_png() {
    let mut bytes = Cursor::new(Vec::new());
    portrait()
        .pixels
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    assert_eq!(
        decode_image(bytes.get_ref()).unwrap().dimensions(),
        (20, 10)
    );
    assert!(decode_image(&vec![0; IMAGE_LIMIT as usize + 1]).is_err());
    assert!(decode_image(b"not an image").is_err());
    let mut bomb = vec![137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82];
    bomb.extend_from_slice(&10_000u32.to_be_bytes());
    bomb.extend_from_slice(&10_000u32.to_be_bytes());
    bomb.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut crc = !0u32;
    for byte in &bomb[12..] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320u32 & 0u32.wrapping_sub(crc & 1));
        }
    }
    bomb.extend_from_slice(&(!crc).to_be_bytes());
    bomb.extend_from_slice(&[0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130]);
    assert!(decode_image(&bomb).is_err());
}

#[test]
fn cache_filename_is_hash_only_even_for_path_like_artist_credits() {
    let filename = asset_name("../../Artists/Blackfield");
    assert_eq!(filename.len(), 68);
    assert!(!filename.contains('/'));
    assert!(filename.ends_with(".png"));
}

#[test]
fn slow_failure_backoff_starts_when_request_finishes_not_when_it_begins() {
    use std::sync::atomic::AtomicI64;
    struct SlowFailure(Arc<AtomicI64>);
    impl Provider for SlowFailure {
        fn find(&mut self, _: &ArtistRequest) -> Result<Portrait, String> {
            self.0.store(221, Ordering::Relaxed);
            Err("timed out".into())
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(AtomicI64::new(100));
    let (mut worker, _, _) = setup(directory.path(), vec![]);
    worker.provider = Box::new(SlowFailure(clock.clone()));
    worker.clock = Box::new(move || clock.load(Ordering::Relaxed));
    worker.command(Command::Enqueue(vec![artist("Blackfield", &[])]), 100);
    worker.process("blackfield", 100);
    assert_eq!(worker.queue.jobs["blackfield"].next_retry, 281);
    assert!(worker.next(221).is_none());
    assert_eq!(worker.next(281).as_deref(), Some("blackfield"));
}

#[test]
fn recovery_event_requires_successful_storage_and_does_not_repeat_for_normal_saves() {
    let directory = tempfile::tempdir().unwrap();
    let (mut worker, events, _) = setup(directory.path(), vec![]);
    let blocker = directory.path().join("artist-images-v1");
    fs::write(&blocker, "not a directory").unwrap();
    worker.command(Command::Enqueue(vec![artist("Blackfield", &[])]), 100);
    assert!(
        !events
            .try_iter()
            .any(|event| matches!(event, Event::Recovered))
    );
    fs::remove_file(blocker).unwrap();
    assert!(worker.persist(131));
    assert_eq!(
        events
            .try_iter()
            .filter(|event| matches!(event, Event::Recovered))
            .count(),
        1
    );
    worker.command(Command::Enqueue(vec![artist("Blackfield", &["New"])]), 131);
    assert!(
        !events
            .try_iter()
            .any(|event| matches!(event, Event::Recovered))
    );
}

#[test]
fn discarded_album_context_cannot_bypass_failed_job_backoff() {
    let directory = tempfile::tempdir().unwrap();
    let (mut worker, _, _) = setup(directory.path(), vec![Err("offline".into())]);
    let albums = (0..100)
        .map(|index| format!("Album {index:03}"))
        .collect::<Vec<_>>();
    worker.command(
        Command::Enqueue(vec![ArtistRequest {
            artist: "Blackfield".into(),
            albums,
        }]),
        100,
    );
    worker.process("blackfield", 100);
    worker.command(
        Command::Enqueue(vec![artist("Blackfield", &["ZZZ discarded 101st album"])]),
        101,
    );
    assert_eq!(worker.queue.jobs["blackfield"].state, JobState::Failed);
    assert_eq!(worker.queue.jobs["blackfield"].albums.len(), 100);
    assert_eq!(worker.queue.jobs["blackfield"].next_retry, 160);
}

#[test]
fn queue_capacity_rejects_excess_new_jobs_and_context_without_freezing_existing_work() {
    let directory = tempfile::tempdir().unwrap();
    let (mut worker, events, calls) = setup(directory.path(), vec![Ok(portrait())]);
    worker.command(
        Command::Enqueue(vec![artist("Blackfield", &["Blackfield"])]),
        100,
    );
    let encoded = serde_json::to_vec_pretty(&worker.queue).unwrap().len() as u64;
    worker.queue_limit = encoded + RESULT_RESERVE + 10;
    let previous = worker.queue.jobs["blackfield"].clone();
    worker.command(
        Command::Enqueue(vec![
            artist("Haken", &["The Mountain"]),
            artist(
                "Blackfield",
                &["An extended new context that no longer fits in the queue"],
            ),
        ]),
        101,
    );
    assert_eq!(worker.queue.jobs.len(), 1);
    assert_eq!(worker.queue.jobs["blackfield"], previous);
    assert!(!worker.dirty);
    assert_eq!(worker.next(101).as_deref(), Some("blackfield"));
    assert!(
        events
            .try_iter()
            .any(|event| matches!(event, Event::Error(error) if error.contains("full")))
    );
    worker.process("blackfield", 101);
    assert_eq!(worker.queue.jobs["blackfield"].state, JobState::Ready);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(!worker.dirty);
}

#[test]
fn capacity_warning_clears_after_full_enqueue_and_successful_save() {
    let directory = tempfile::tempdir().unwrap();
    let (mut worker, events, _) = setup(directory.path(), vec![]);
    worker.command(Command::Enqueue(vec![artist("Blackfield", &[])]), 100);
    let encoded = serde_json::to_vec_pretty(&worker.queue).unwrap().len() as u64;
    worker.queue_limit = encoded + RESULT_RESERVE + 10;
    worker.command(Command::Enqueue(vec![artist("Haken", &[])]), 100);
    assert!(worker.had_capacity_error);
    assert!(
        !events
            .try_iter()
            .any(|event| matches!(event, Event::Recovered))
    );
    worker.queue_limit = QUEUE_LIMIT;
    worker.command(Command::Enqueue(vec![artist("Haken", &[])]), 101);
    assert!(!worker.had_capacity_error);
    assert_eq!(
        events
            .try_iter()
            .filter(|event| matches!(event, Event::Recovered))
            .count(),
        1
    );
    worker.command(Command::Enqueue(vec![artist("Haken", &[])]), 102);
    assert!(
        !events
            .try_iter()
            .any(|event| matches!(event, Event::Recovered))
    );
}

#[test]
fn capacity_recovery_cannot_clear_a_storage_failure_before_data_is_saved() {
    let directory = tempfile::tempdir().unwrap();
    let (mut worker, events, _) = setup(directory.path(), vec![]);
    worker.command(Command::Enqueue(vec![artist("Blackfield", &[])]), 100);
    let encoded = serde_json::to_vec_pretty(&worker.queue).unwrap().len() as u64;
    worker.queue_limit = encoded + RESULT_RESERVE + 10;
    worker.command(Command::Enqueue(vec![artist("Haken", &[])]), 100);
    let _ = events.try_iter().collect::<Vec<_>>();
    worker.queue_limit = QUEUE_LIMIT;
    let queue_path = worker.root.join("queue.json");
    fs::remove_file(&queue_path).unwrap();
    fs::create_dir(&queue_path).unwrap();
    worker.command(Command::Enqueue(vec![artist("Haken", &[])]), 101);
    assert!(worker.had_storage_error && worker.had_capacity_error);
    assert!(
        !events
            .try_iter()
            .any(|event| matches!(event, Event::Recovered))
    );
    fs::remove_dir(queue_path).unwrap();
    assert!(!worker.persist(102));
    assert!(
        !events
            .try_iter()
            .any(|event| matches!(event, Event::Recovered))
    );
    assert!(worker.persist(131));
    assert!(!worker.had_storage_error && !worker.had_capacity_error);
    assert_eq!(
        events
            .try_iter()
            .filter(|event| matches!(event, Event::Recovered))
            .count(),
        1
    );
}

#[test]
fn storage_recovery_does_not_clear_an_unresolved_capacity_warning() {
    let directory = tempfile::tempdir().unwrap();
    let (mut worker, events, _) = setup(directory.path(), vec![]);
    worker.command(Command::Enqueue(vec![artist("Blackfield", &[])]), 100);
    let encoded = serde_json::to_vec_pretty(&worker.queue).unwrap().len() as u64;
    worker.queue_limit = encoded + RESULT_RESERVE + 10;
    worker.command(Command::Enqueue(vec![artist("Haken", &[])]), 100);
    let queue_path = worker.root.join("queue.json");
    fs::remove_file(&queue_path).unwrap();
    fs::create_dir(&queue_path).unwrap();
    worker.process("blackfield", 101);
    assert!(worker.had_storage_error && worker.had_capacity_error);
    let _ = events.try_iter().collect::<Vec<_>>();
    fs::remove_dir(queue_path).unwrap();
    assert!(worker.persist(131));
    assert!(!worker.had_storage_error && worker.had_capacity_error);
    assert!(
        !events
            .try_iter()
            .any(|event| matches!(event, Event::Recovered))
    );
    worker.queue_limit = QUEUE_LIMIT;
    worker.command(Command::Enqueue(vec![artist("Haken", &[])]), 132);
    assert_eq!(
        events
            .try_iter()
            .filter(|event| matches!(event, Event::Recovered))
            .count(),
        1
    );
}

#[test]
#[ignore = "uses public MusicBrainz, Wikidata and Wikimedia Commons services"]
fn live_blackfield_public_chain_downloads_a_licensed_portrait_without_deepseek() {
    let directory = tempfile::tempdir().unwrap();
    let (sender, events) = mpsc::sync_channel(100);
    let mut worker = Worker::new(
        directory.path().to_owned(),
        Box::new(provider::WebProvider::new(directory.path().to_owned())),
        sender,
    );
    worker.command(
        Command::Enqueue(vec![artist("Blackfield", &["Blackfield"])]),
        unix_now(),
    );
    worker.process("blackfield", unix_now());
    let job = &worker.queue.jobs["blackfield"];
    assert_eq!(
        job.state,
        JobState::Ready,
        "public image fetch failed: {:?}",
        job.error
    );
    let credit = job.credit.as_ref().unwrap();
    assert!(!credit.author.is_empty());
    eprintln!("Public portrait attribution: {credit:?}");
    assert!(provider::valid_credit(credit));
    assert!(events.try_iter().any(|event| matches!(event, Event::Image { pixels, .. } if pixels.width() > 0 && pixels.width() <= 640 && pixels.height() <= 640)));
    let (mut restarted, cached, calls) = setup(directory.path(), vec![]);
    restarted.command(Command::Cached(vec!["Blackfield".into()]), unix_now());
    assert!(
        cached
            .try_iter()
            .any(|event| matches!(event, Event::Image { .. }))
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}
