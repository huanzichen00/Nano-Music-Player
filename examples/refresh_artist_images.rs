//! Prepare artist photos from an existing library in a separate, new directory.
//! Never writes the live queue: merge only while the player is closed.

#[cfg(any(target_os = "ios", feature = "mobile"))]
fn main() {
    eprintln!("Artist photo refresh is available on desktop only.");
}

#[cfg(not(any(target_os = "ios", feature = "mobile")))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use nano_music_player::library::artist_images::{
        ArtistImages, ArtistRequest, Event, normalized_artist_key,
    };
    use std::{
        collections::{BTreeMap, BTreeSet},
        path::PathBuf,
        time::{Duration, Instant},
    };

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let resume = args.len() == 3 && args[2] == "--resume";
    if args.len() != 2 && !resume {
        return Err(
            "Usage: refresh_artist_images <source-config-dir> <output-dir> [--resume]".into(),
        );
    }
    let source = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let marker = output.join(".artist-photo-refresh-v1");
    if output.exists() && (!resume || !marker.is_file()) {
        return Err("Output directory must not exist; the live queue must never be shared".into());
    }
    // Read only metadata, without opening sources or loading audio/artwork blobs.
    let db = rusqlite::Connection::open_with_flags(
        source.join("library.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let mut statement = db.prepare(
        "SELECT json_extract(metadata, '$.artist'), json_extract(metadata, '$.album') FROM tracks",
    )?;
    let mut artists: BTreeMap<String, (String, BTreeSet<String>)> = BTreeMap::new();
    for row in statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (artist, album) = row?;
        let key = normalized_artist_key(&artist);
        if key.is_empty()
            || artist.len() > 512
            || artist.chars().any(char::is_control)
            || matches!(key.as_str(), "unknown artist" | "various artists")
        {
            continue;
        }
        let entry = artists.entry(key).or_insert((artist, BTreeSet::new()));
        if !album.trim().is_empty() {
            entry.1.insert(album);
        }
    }
    let requests: Vec<_> = artists
        .into_values()
        .map(|(artist, albums)| ArtistRequest {
            artist,
            albums: albums.into_iter().take(8).collect(),
        })
        .collect();
    if requests.is_empty() {
        return Err("No named artists found in the source library".into());
    }
    let expected = requests.len();
    std::fs::create_dir_all(&output)?;
    std::fs::write(
        marker,
        "Prepared by refresh_artist_images; never share a live queue.\n",
    )?;
    let service = ArtistImages::new(output.clone());
    let mut requests = Some(requests);
    let deadline = Instant::now() + Duration::from_secs(15 * 60);
    let mut previous = None;
    loop {
        for event in service.poll() {
            match event {
                Event::Queue(queue) => {
                    // Consume the startup snapshot before sending work, so an
                    // old all-failed snapshot cannot prematurely finish resume.
                    if let Some(requests) = requests.take() {
                        if resume {
                            service.retry_failed();
                        }
                        service.enqueue(requests);
                        continue;
                    }
                    let counts = (queue.pending, queue.running, queue.failed, queue.ready);
                    if previous != Some(counts) {
                        println!(
                            "pending={} running={} failed={} ready={}",
                            queue.pending, queue.running, queue.failed, queue.ready
                        );
                        previous = Some(counts);
                    }
                    if queue.jobs.len() >= expected && queue.pending == 0 && queue.running == 0 {
                        for job in &queue.jobs {
                            println!(
                                "{}: {:?} {}",
                                job.artist,
                                job.state,
                                job.error.as_deref().unwrap_or("")
                            );
                        }
                        println!("Prepared queue and images: {}", output.display());
                        return Ok(());
                    }
                }
                Event::Error(error) => return Err(error.into()),
                Event::Image { .. } | Event::Recovered => {}
            }
        }
        if Instant::now() >= deadline {
            return Err(
                "Timed out; completed photos and unfinished jobs remain in the output".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
