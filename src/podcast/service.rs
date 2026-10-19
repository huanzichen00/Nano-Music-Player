//! One storage coordinator; at most two blocking network workers. UI never joins HTTP.
use super::{Command, Episode, Event, Feed, ParsedFeed, feed, store::Store};
use reqwest::{blocking::Client, redirect::Policy};
use rodio::Source;
use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct Service {
    commands: Sender<Command>,
    pub events: Receiver<Event>,
    worker: Option<JoinHandle<()>>,
}
impl Service {
    pub fn new(directory: PathBuf) -> Self {
        let (commands, input) = mpsc::channel();
        let (events, output) = mpsc::channel();
        let worker = thread::spawn(move || run(directory, input, events));
        Self {
            commands,
            events: output,
            worker: Some(worker),
        }
    }
    pub fn send(&self, command: Command) -> Result<(), String> {
        self.commands
            .send(command)
            .map_err(|_| "Podcast service is unavailable".into())
    }
    pub fn shutdown(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = self.send(Command::Shutdown);
            let _ = worker.join();
        }
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Clone)]
enum Job {
    Preview(String, u64),
    Refresh(Feed),
    Download(Episode, PathBuf),
}
impl Job {
    fn key(&self) -> String {
        match self {
            Self::Preview(..) => "preview".into(),
            Self::Refresh(f) => format!("feed:{}", f.id),
            Self::Download(e, _) => format!("episode:{}", e.id),
        }
    }
}
enum ResultMessage {
    Done(u64, Result<Outcome, String>),
    Progress(u64, i64, u64, Option<u64>),
}
enum Outcome {
    Feed(Option<Box<ParsedFeed>>),
    Download(PathBuf, f64),
}
struct Task {
    job: Job,
    canceled: Arc<AtomicBool>,
}

fn snapshot(store: &Store, events: &Sender<Event>) {
    match store.library() {
        Ok(library) => {
            let _ = events.send(Event::Library(library));
        }
        Err(e) => {
            let _ = events.send(Event::Error(e));
        }
    }
}

fn run(directory: PathBuf, commands: Receiver<Command>, events: Sender<Event>) {
    let mut store = match Store::open(&directory) {
        Ok(s) => s,
        Err(e) => {
            let _ = events.send(Event::Error(e));
            return;
        }
    };
    snapshot(&store, &events);
    let (results, output) = mpsc::channel();
    let mut tasks: HashMap<u64, Task> = HashMap::new();
    let mut queue = VecDeque::new();
    let mut serial = 0u64;
    loop {
        match commands.recv_timeout(Duration::from_millis(30)) {
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(command) => {
                let mut job = None;
                let result = (|| -> Result<(), String> {
                    match command {
                        Command::Preview { url, request } => job = Some(Job::Preview(url, request)),
                        Command::Subscribe(parsed) => {
                            store.merge(*parsed, None)?;
                            snapshot(&store, &events);
                        }
                        Command::Refresh(id) => {
                            if let Some(feed) = store
                                .library()?
                                .feeds
                                .into_iter()
                                .find(|f| f.id == id && f.subscribed)
                            {
                                job = Some(Job::Refresh(feed));
                            }
                        }
                        Command::Unsubscribe(id) => {
                            let key = format!("feed:{id}");
                            queue.retain(|j: &Job| j.key() != key);
                            for task in tasks.values().filter(|t| t.job.key() == key) {
                                task.canceled.store(true, Ordering::Release);
                            }
                            store.unsubscribe(id)?;
                            snapshot(&store, &events);
                        }
                        Command::Download(id) => {
                            if let Some(e) = store.library()?.episodes.into_iter().find(|e| {
                                e.id == id && e.local_path.as_ref().is_none_or(|p| !p.is_file())
                            }) {
                                if e.media_url.is_none() {
                                    return Err("This episode has no playable audio".into());
                                }
                                job = Some(Job::Download(e, store.path(id)));
                            }
                        }
                        Command::CancelDownload(id) => {
                            let key = format!("episode:{id}");
                            queue.retain(|j: &Job| j.key() != key);
                            for task in tasks.values().filter(|t| t.job.key() == key) {
                                task.canceled.store(true, Ordering::Release);
                            }
                            let _ = events.send(Event::DownloadFinished {
                                id,
                                result: Err("Download canceled".into()),
                            });
                        }
                        Command::DeleteDownload(id) => {
                            store.delete_download(id)?;
                            snapshot(&store, &events);
                        }
                        Command::Progress {
                            id,
                            position,
                            completed,
                        } => {
                            store.progress(id, position, completed)?;
                            if completed.is_some() {
                                snapshot(&store, &events);
                            }
                        }
                        Command::Shutdown => unreachable!(),
                    }
                    Ok(())
                })();
                if let Err(e) = result {
                    let _ = events.send(Event::Error(e));
                }
                if let Some(job) = job {
                    let key = job.key();
                    if key == "preview" || key.starts_with("feed:") {
                        for task in tasks.values().filter(|t| t.job.key() == key) {
                            task.canceled.store(true, Ordering::Release);
                        }
                        queue.retain(|j: &Job| j.key() != key);
                    } else if tasks
                        .values()
                        .any(|t| t.job.key() == key && !t.canceled.load(Ordering::Acquire))
                        || queue.iter().any(|j: &Job| j.key() == key)
                    {
                        continue;
                    }
                    if queue.len() < 128 {
                        queue.push_back(job);
                    } else {
                        let _ = events.send(Event::Error("Podcast task queue is full".into()));
                    }
                }
            }
        }
        while let Ok(result) = output.try_recv() {
            match result {
                ResultMessage::Progress(token, id, bytes, total) => {
                    if tasks
                        .get(&token)
                        .is_some_and(|t| !t.canceled.load(Ordering::Acquire))
                    {
                        let _ = events.send(Event::DownloadProgress { id, bytes, total });
                    }
                }
                ResultMessage::Done(token, result) => {
                    let Some(task) = tasks.remove(&token) else {
                        continue;
                    };
                    if task.canceled.load(Ordering::Acquire) {
                        if let Ok(Outcome::Download(path, _)) = result {
                            let _ = std::fs::remove_file(path);
                        }
                        continue;
                    }
                    match task.job {
                        Job::Preview(_, request) => {
                            let result = result.and_then(|o| match o {
                                Outcome::Feed(Some(parsed)) => Ok(parsed),
                                _ => Err("No RSS feed returned".into()),
                            });
                            let _ = events.send(Event::Preview { request, result });
                        }
                        Job::Refresh(feed) => {
                            let result = result.and_then(|o| match o {
                                Outcome::Feed(Some(mut parsed)) => {
                                    parsed.feed.subscribed = feed.subscribed;
                                    store.merge(*parsed, Some(feed.id)).map(|_| ())
                                }
                                Outcome::Feed(None) => Ok(()),
                                _ => unreachable!(),
                            });
                            match result {
                                Ok(()) => {
                                    snapshot(&store, &events);
                                    let _ = events.send(Event::Refreshed(feed.id));
                                }
                                Err(e) => {
                                    let _ = events.send(Event::Error(e));
                                }
                            }
                        }
                        Job::Download(e, path) => {
                            let result = result.and_then(|o| match o {
                                Outcome::Download(temp, duration) => {
                                    std::fs::rename(&temp, &path).map_err(|e| e.to_string())?;
                                    store.downloaded(e.id, duration)
                                }
                                _ => unreachable!(),
                            });
                            snapshot(&store, &events);
                            let _ = events.send(Event::DownloadFinished { id: e.id, result });
                        }
                    }
                }
            }
        }
        while tasks.len() < 2 {
            let Some(job) = queue.pop_front() else {
                break;
            };
            serial += 1;
            let token = serial;
            let canceled = Arc::new(AtomicBool::new(false));
            tasks.insert(
                token,
                Task {
                    job: job.clone(),
                    canceled: canceled.clone(),
                },
            );
            let result = results.clone();
            let directory = directory.clone();
            thread::spawn(move || {
                let outcome = match &job {
                    Job::Preview(url, _) => fetch(url, None, &directory, &canceled)
                        .map(|parsed| Outcome::Feed(parsed.map(Box::new))),
                    Job::Refresh(feed) => fetch(&feed.url, Some(feed), &directory, &canceled)
                        .map(|parsed| Outcome::Feed(parsed.map(Box::new))),
                    Job::Download(e, path) => download(e, path, token, &canceled, &result),
                };
                let _ = result.send(ResultMessage::Done(token, outcome));
            });
        }
    }
    for task in tasks.values() {
        task.canceled.store(true, Ordering::Release);
    }
}

fn client(timeout: Duration) -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .user_agent(concat!(
            "Nano-Music-Player/",
            env!("CARGO_PKG_VERSION"),
            " (desktop podcast client)"
        ))
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !matches!(attempt.url().scheme(), "http" | "https")
            {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|e| e.without_url().to_string())
}
fn read_limited(mut response: reqwest::blocking::Response, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read podcast response".to_owned())?;
    if bytes.len() as u64 > limit {
        return Err("Podcast response is too large".into());
    }
    Ok(bytes)
}
fn fetch(
    url: &str,
    cached: Option<&Feed>,
    directory: &Path,
    canceled: &AtomicBool,
) -> Result<Option<ParsedFeed>, String> {
    let client = client(Duration::from_secs(30))?;
    let mut request = client.get(feed::url(url)?);
    if let Some(feed) = cached {
        if let Some(etag) = &feed.etag {
            request = request.header("If-None-Match", etag);
        }
        if let Some(modified) = &feed.modified {
            request = request.header("If-Modified-Since", modified);
        }
    }
    let response = request.send().map_err(|e| e.without_url().to_string())?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(None);
    }
    let response = response
        .error_for_status()
        .map_err(|e| e.without_url().to_string())?;
    let resolved = response.url().to_string();
    let etag = response
        .headers()
        .get("ETag")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let modified = response
        .headers()
        .get("Last-Modified")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = read_limited(response, 8 * 1024 * 1024)?;
    if canceled.load(Ordering::Acquire) {
        return Err("Canceled".into());
    }
    let mut parsed = feed::parse(&bytes, &resolved)?;
    parsed.aliases.push(url.to_owned());
    parsed.feed.url = url.to_owned();
    parsed.feed.etag = etag;
    parsed.feed.modified = modified;
    if let Some(url) = &parsed.feed.image_url {
        parsed.feed.artwork_path = cover(&client, url, directory).ok();
    }
    Ok(Some(parsed))
}
fn cover(client: &Client, url: &str, directory: &Path) -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    let root = directory.join("podcasts/artwork");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let path = root.join(format!("{:x}.png", Sha256::digest(url.as_bytes())));
    if path.is_file() {
        return Ok(path);
    }
    let response = client
        .get(feed::url(url)?)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.without_url().to_string())?;
    let bytes = read_limited(response, 4 * 1024 * 1024)?;
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_dimensions()
        .map_err(|e| e.to_string())?;
    if w > 4096 || h > 4096 {
        return Err("Artwork dimensions are too large".into());
    }
    let image = image::load_from_memory(&bytes)
        .map_err(|e| e.to_string())?
        .thumbnail(320, 320);
    // Artwork is a cache, and concurrent requests may share a URL.
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let temp = path.with_extension(format!(
        "{}.tmp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(&temp, png.into_inner()).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&temp, &path) {
        let _ = std::fs::remove_file(&temp);
        if !path.is_file() {
            return Err(e.to_string());
        }
    }
    Ok(path)
}
fn download(
    episode: &Episode,
    path: &Path,
    token: u64,
    canceled: &AtomicBool,
    events: &Sender<ResultMessage>,
) -> Result<Outcome, String> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = path.with_extension(format!("{nonce}.{token}.part"));
    let result = (|| {
        let mut response = client(Duration::from_secs(30))?
            .get(feed::url(
                episode.media_url.as_deref().ok_or("No episode audio")?,
            )?)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.without_url().to_string())?;
        let total = response.content_length();
        let mut file = File::create(&temp).map_err(|e| e.to_string())?;
        let mut bytes = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        let mut update = Instant::now() - Duration::from_secs(1);
        loop {
            if canceled.load(Ordering::Acquire) {
                return Err("Download canceled".into());
            }
            let count = response
                .read(&mut buffer)
                .map_err(|_| "Podcast download was interrupted".to_owned())?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
            if bytes > 4 * 1024 * 1024 * 1024 {
                return Err("Episode exceeds the 4 GB download limit".into());
            }
            file.write_all(&buffer[..count])
                .map_err(|e| e.to_string())?;
            if update.elapsed() >= Duration::from_millis(250) {
                let _ = events.send(ResultMessage::Progress(token, episode.id, bytes, total));
                update = Instant::now();
            }
        }
        if total.is_some_and(|total| total != bytes) {
            return Err("Podcast download is incomplete".into());
        }
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        if canceled.load(Ordering::Acquire) {
            return Err("Download canceled".into());
        }
        let mut decoder = rodio::Decoder::try_from(File::open(&temp).map_err(|e| e.to_string())?)
            .map_err(|_| "Downloaded file is not supported audio".to_owned())?;
        let duration = decoder.total_duration().map_or(0.0, |d| d.as_secs_f64());
        if decoder.next().is_none() {
            return Err("Downloaded audio is empty".into());
        }
        Ok(Outcome::Download(temp.clone(), duration))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn wait(service: &Service, predicate: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let event = service
                .events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("podcast worker did not complete");
            if predicate(&event) {
                return event;
            }
        }
    }

    #[test]
    fn rss_preview_download_validation_refresh_and_resume_are_durable() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server_base = base.clone();
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap();
                requests.push(request.clone());
                let (status, kind, body, extra) = if request.starts_with("GET /audio") {
                    (
                        "200 OK",
                        "audio/wav",
                        include_bytes!("../../tests/fixtures/quiet.wav").to_vec(),
                        "",
                    )
                } else if request.starts_with("GET /bad") {
                    (
                        "200 OK",
                        "text/html",
                        b"<html>not audio</html>".to_vec(),
                        "",
                    )
                } else if request
                    .to_ascii_lowercase()
                    .contains("if-none-match: \"v1\"")
                {
                    ("304 Not Modified", "application/rss+xml", Vec::new(), "")
                } else {
                    let xml = format!(
                        "<rss version=\"2.0\"><channel><title>Test Show</title><link>{server_base}</link><description>Example</description><item><title>Good</title><guid>one</guid><enclosure url=\"{server_base}/audio\" type=\"audio/wav\" length=\"0\"/></item><item><title>Bad</title><guid>two</guid><enclosure url=\"{server_base}/bad\" type=\"audio/mpeg\" length=\"0\"/></item></channel></rss>"
                    );
                    (
                        "200 OK",
                        "application/rss+xml",
                        xml.into_bytes(),
                        "ETag: \"v1\"\r\n",
                    )
                };
                write!(stream,"HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",body.len()).unwrap();
                stream.write_all(&body).unwrap();
            }
            requests
        });
        let directory = tempfile::tempdir().unwrap();
        let mut service = Service::new(directory.path().into());
        wait(&service, |e| matches!(e, Event::Library(_)));
        service
            .send(Command::Preview {
                url: format!("{base}/rss?token=private"),
                request: 42,
            })
            .unwrap();
        let Event::Preview {
            result: Ok(parsed), ..
        } = wait(&service, |e| {
            matches!(e, Event::Preview { request: 42, .. })
        })
        else {
            panic!("preview failed")
        };
        assert_eq!(parsed.feed.title, "Test Show");
        assert!(parsed.feed.url.ends_with("token=private"));
        // Preview does not create a subscription or download audio.
        assert!(
            Store::open(directory.path())
                .unwrap()
                .library()
                .unwrap()
                .feeds
                .is_empty()
        );
        service.send(Command::Subscribe(parsed)).unwrap();
        let Event::Library(library) = wait(
            &service,
            |e| matches!(e,Event::Library(l) if l.feeds.len()==1),
        ) else {
            unreachable!()
        };
        let feed = library.feeds[0].id;
        let good = library
            .episodes
            .iter()
            .find(|e| e.guid.as_deref() == Some("one"))
            .unwrap()
            .id;
        let bad = library
            .episodes
            .iter()
            .find(|e| e.guid.as_deref() == Some("two"))
            .unwrap()
            .id;
        service.send(Command::Download(good)).unwrap();
        assert!(matches!(
            wait(
                &service,
                |e| matches!(e,Event::DownloadFinished{id,..} if *id==good)
            ),
            Event::DownloadFinished { result: Ok(()), .. }
        ));
        service
            .send(Command::Progress {
                id: good,
                position: 0.125,
                completed: Some(false),
            })
            .unwrap();
        service.send(Command::Refresh(feed)).unwrap();
        wait(&service, |e| matches!(e, Event::Refreshed(_)));
        service.send(Command::Download(bad)).unwrap();
        assert!(matches!(
            wait(
                &service,
                |e| matches!(e,Event::DownloadFinished{id,..} if *id==bad)
            ),
            Event::DownloadFinished { result: Err(_), .. }
        ));
        service.shutdown();
        let library = Store::open(directory.path()).unwrap().library().unwrap();
        let episode = library.episodes.iter().find(|e| e.id == good).unwrap();
        assert_eq!(episode.position, 0.125);
        assert!(episode.local_path.as_ref().unwrap().is_file());
        assert!(
            library
                .episodes
                .iter()
                .find(|e| e.id == bad)
                .unwrap()
                .local_path
                .is_none()
        );
        let requests = server.join().unwrap();
        assert!(
            requests
                .iter()
                .any(|r| r.to_ascii_lowercase().contains("if-none-match: \"v1\""))
        );
        assert!(!directory.path().join("library.sqlite3").exists());
    }

    #[test]
    fn cancellation_never_publishes_partial_audio_and_shutdown_drains_progress() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/slow?token=private",
            listener.local_addr().unwrap()
        );
        let (started, ready) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request).unwrap();
            let bytes = include_bytes!("../../tests/fixtures/quiet.wav");
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .unwrap();
            socket.write_all(&bytes[..32]).unwrap();
            started.send(()).unwrap();
            thread::sleep(Duration::from_millis(300));
            let _ = socket.write_all(&bytes[32..]);
        });
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store
            .merge(
                ParsedFeed {
                    feed: Feed {
                        url: "https://example.org/rss".into(),
                        subscribed: true,
                        ..Feed::default()
                    },
                    episodes: vec![Episode {
                        guid: Some("one".into()),
                        media_url: Some(url),
                        ..Episode::default()
                    }],
                    aliases: vec![],
                },
                None,
            )
            .unwrap();
        let id = store.library().unwrap().episodes[0].id;
        drop(store);
        let mut service = Service::new(dir.path().into());
        wait(&service, |e| matches!(e, Event::Library(_)));
        service.send(Command::Download(id)).unwrap();
        ready.recv_timeout(Duration::from_secs(3)).unwrap();
        service.send(Command::CancelDownload(id)).unwrap();
        assert!(matches!(
            wait(&service, |e| matches!(e, Event::DownloadFinished { .. })),
            Event::DownloadFinished { result: Err(_), .. }
        ));
        service
            .send(Command::Progress {
                id,
                position: 17.0,
                completed: Some(false),
            })
            .unwrap();
        service.shutdown();
        server.join().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let library = store.library().unwrap();
        assert_eq!(library.episodes[0].position, 17.0);
        assert!(library.episodes[0].local_path.is_none());
        assert!(!store.path(id).exists());
    }
}
