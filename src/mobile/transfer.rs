//! A manually enabled, authenticated local HTTP endpoint for desktop imports.
//!
//! The deliberately small HTTP subset uses one request per connection and fixed
//! Content-Length bodies. Audio is streamed to app-owned temporary files and
//! passed through Storage, never accepted as an arbitrary filesystem path.

use super::storage::Storage;
use crate::library::{paths::is_audio, scanner::ScanResult};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    net::{Ipv4Addr, Shutdown, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_HEADERS: usize = 16 * 1024;
const MAX_UPLOAD: u64 = 512 * 1024 * 1024;
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
const BODY_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const IDLE_TIMEOUT: Duration = Duration::from_secs(15);
const SOCKET_TIMEOUT: Duration = Duration::from_secs(1);
const PAGE: &str = include_str!("../../web/transfer.html");

pub enum TransferEvent {
    LibraryChanged(ScanResult),
    Notice(String),
}

pub struct Transfer {
    urls: Vec<String>,
    stopped: Arc<StopState>,
    events: Receiver<TransferEvent>,
    worker: Option<JoinHandle<()>>,
}

struct StopState {
    flag: Arc<AtomicBool>,
    // A request is canceled across an expired background interval even if the
    // foreground resumes before its next socket timeout. The service itself
    // retains its listener, port and token while requests are paused.
    request_epoch: Option<u64>,
    access: TransferAccess,
}

type TransferAccess = Arc<dyn Fn() -> (bool, u64) + Send + Sync>;

impl Transfer {
    pub fn start(storage: Storage) -> Result<Self, String> {
        Self::start_with_access(
            storage,
            Arc::new(|| {
                (
                    crate::platform::ios::transfer_allowed(),
                    crate::platform::ios::background_epoch(),
                )
            }),
        )
    }

    fn start_with_access(storage: Storage, access: TransferAccess) -> Result<Self, String> {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))
            .map_err(|error| format!("无法开启电脑导入：{error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("无法配置电脑导入：{error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let token = random_token();
        let addresses = local_addresses();
        let urls = addresses
            .iter()
            .map(|address| format!("http://{address}:{port}/#{token}"))
            .collect();
        let staging = storage
            .music_dir()
            .parent()
            .ok_or_else(|| "音乐目录没有可用的应用路径".to_owned())?
            .join(".nano/transfers");
        fs::create_dir_all(&staging).map_err(|error| format!("无法建立上传暂存目录：{error}"))?;
        let stopped = Arc::new(StopState {
            flag: Arc::new(AtomicBool::new(false)),
            request_epoch: None,
            access,
        });
        let worker_stopped = Arc::clone(&stopped);
        let (sender, events) = mpsc::channel();
        if addresses == [Ipv4Addr::LOCALHOST] {
            let _ = sender.send(TransferEvent::Notice(
                "未发现局域网连接；当前地址仅供本机预览，请连接 Wi-Fi 后重新开启".into(),
            ));
        }
        let worker = thread::Builder::new()
            .name("nano-local-transfer".into())
            .spawn(move || {
                serve(listener, storage, staging, token, worker_stopped, sender);
            })
            .map_err(|error| format!("无法启动电脑导入：{error}"))?;
        Ok(Self {
            urls,
            stopped,
            events,
            worker: Some(worker),
        })
    }

    /// URLs include a per-session secret in the fragment, never in server logs.
    pub fn urls(&self) -> &[String] {
        &self.urls
    }

    pub fn poll_event(&self) -> Option<TransferEvent> {
        self.events.try_recv().ok()
    }

    pub fn is_running(&self) -> bool {
        !should_stop(&self.stopped)
    }

    pub fn is_paused(&self) -> bool {
        self.is_running() && !(self.stopped.access)().0
    }

    /// Cancellation is observed during accept and at least every socket timeout.
    /// Detach the worker so shutdown can never block the UI thread on disk I/O.
    pub fn stop(&mut self) {
        self.stopped.flag.store(true, Ordering::Release);
        self.worker.take();
    }
}

impl Drop for Transfer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn random_token() -> String {
    rand::random::<[u8; 16]>()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn should_stop(stopped: &StopState) -> bool {
    stopped.flag.load(Ordering::Acquire)
        || stopped.request_epoch.is_some_and(|epoch| {
            let (allowed, current_epoch) = (stopped.access)();
            !allowed || epoch != current_epoch
        })
}

fn serve(
    listener: TcpListener,
    storage: Storage,
    staging: PathBuf,
    token: String,
    stopped: Arc<StopState>,
    events: Sender<TransferEvent>,
) {
    let mut paused = false;
    while !should_stop(&stopped) {
        if !(stopped.access)().0 {
            if !paused {
                let _ = events.send(TransferEvent::Notice(
                    "Computer transfer paused after the background allowance expired. Return to the app to resume with the same address.".into(),
                ));
                paused = true;
            }
            thread::sleep(Duration::from_millis(40));
            continue;
        }
        if paused {
            paused = false;
            let _ = events.send(TransferEvent::Notice(
                "Computer transfer resumed with the same address. Keep the app open for large uploads.".into(),
            ));
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let request = StopState {
                    flag: Arc::clone(&stopped.flag),
                    request_epoch: Some((stopped.access)().1),
                    access: Arc::clone(&stopped.access),
                };
                let configured = stream.set_read_timeout(Some(SOCKET_TIMEOUT)).is_ok()
                    && stream.set_write_timeout(Some(SOCKET_TIMEOUT)).is_ok();
                if configured
                    && let Err(error) =
                        handle(&mut stream, &storage, &staging, &token, &request, &events)
                    && !should_stop(&request)
                {
                    let _ = respond_json(
                        &mut stream,
                        error.status,
                        &json!({ "ok": false, "error": error.message }),
                        &request,
                    );
                }
                let _ = stream.shutdown(Shutdown::Both);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(40));
            }
            Err(error) => {
                let _ = events.send(TransferEvent::Notice(format!("电脑导入连接错误：{error}")));
                break;
            }
        }
    }
    stopped.flag.store(true, Ordering::Release);
    let _ = events.send(TransferEvent::Notice("电脑导入已关闭，原地址已失效".into()));
}

struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body_start: Vec<u8>,
}

#[derive(Debug)]
struct RequestError {
    status: u16,
    message: String,
}

impl RequestError {
    fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

fn read_request(stream: &mut TcpStream, stopped: &StopState) -> Result<Request, RequestError> {
    let deadline = Instant::now() + HEADER_TIMEOUT;
    let mut last_data = Instant::now();
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut buffer = [0u8; 2048];
        let read = read_socket(stream, &mut buffer, stopped, deadline, &mut last_data)?;
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            if end + 4 > MAX_HEADERS {
                return Err(RequestError::new(431, "请求头过长"));
            }
            break end + 4;
        }
        if bytes.len() >= MAX_HEADERS {
            return Err(RequestError::new(431, "请求头过长"));
        }
    };
    let text = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| RequestError::new(400, "请求头编码无效"))?;
    let mut lines = text.split("\r\n");
    let parts: Vec<_> = lines.next().unwrap_or_default().split(' ').collect();
    if parts.len() != 3 || !matches!(parts[2], "HTTP/1.0" | "HTTP/1.1") {
        return Err(RequestError::new(400, "不支持的 HTTP 请求"));
    }
    let (method, path) = (parts[0].to_owned(), parts[1].to_owned());
    let mut headers = BTreeMap::new();
    for line in lines.take_while(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return Err(RequestError::new(400, "请求头格式无效"));
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() && byte != b'\t')
            || headers
                .insert(name.to_ascii_lowercase(), value.trim().to_owned())
                .is_some()
        {
            return Err(RequestError::new(400, "请求头无效或重复"));
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Err(RequestError::new(
            400,
            "请使用固定 Content-Length 上传，不支持分块编码",
        ));
    }
    if headers.contains_key("expect") {
        return Err(RequestError::new(417, "不支持 Expect 请求"));
    }
    Ok(Request {
        method,
        path,
        headers,
        body_start: bytes[header_end..].to_vec(),
    })
}

fn handle(
    stream: &mut TcpStream,
    storage: &Storage,
    staging: &Path,
    token: &str,
    stopped: &StopState,
    events: &Sender<TransferEvent>,
) -> Result<(), RequestError> {
    let request = read_request(stream, stopped)?;
    if request.method == "GET" && request.path == "/" {
        return respond(
            stream,
            200,
            "text/html; charset=utf-8",
            PAGE.as_bytes(),
            stopped,
        );
    }
    if !request.path.starts_with("/api/") {
        return Err(RequestError::new(404, "找不到此地址"));
    }
    if !request
        .headers
        .get("x-nano-token")
        .is_some_and(|value| token_matches(value, token))
    {
        return Err(RequestError::new(
            401,
            "导入地址已失效或缺少访问码，请重新打开手机上显示的完整地址",
        ));
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/library") => {
            let tracks = storage
                .load()
                .map_err(|error| RequestError::new(500, error))?;
            let rows: Vec<_> = tracks
                .iter()
                .map(|track| {
                    json!({
                        "title": track.title, "artist": track.artist,
                        "album": track.album, "duration": track.duration,
                    })
                })
                .collect();
            respond_json(
                stream,
                200,
                &json!({"ok": true, "tracks": rows, "count": tracks.len()}),
                stopped,
            )
        }
        ("POST", "/api/upload") => {
            let length = request
                .headers
                .get("content-length")
                .ok_or_else(|| RequestError::new(411, "上传需要 Content-Length"))?;
            if length.is_empty() || !length.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(RequestError::new(400, "文件长度无效"));
            }
            let length: u64 = length
                .parse()
                .map_err(|_| RequestError::new(413, "单个文件不能超过 512 MiB"))?;
            if length > MAX_UPLOAD {
                return Err(RequestError::new(413, "单个文件不能超过 512 MiB"));
            }
            if length == 0 || request.body_start.len() as u64 > length {
                return Err(RequestError::new(400, "文件长度与请求内容不符"));
            }
            let filename = decode_filename(
                request
                    .headers
                    .get("x-nano-filename")
                    .ok_or_else(|| RequestError::new(400, "上传缺少文件名"))?,
            )?;
            if !is_audio(Path::new(&filename)) {
                return Err(RequestError::new(
                    415,
                    "支持 MP3、FLAC、WAV、M4A、AAC、OGG 和 AIFF",
                ));
            }
            let (upload, mut file) = Upload::create(staging, &filename)?;
            file.write_all(&request.body_start)
                .map_err(|error| RequestError::new(500, format!("无法暂存音频：{error}")))?;
            let mut remaining = length - request.body_start.len() as u64;
            let deadline = Instant::now() + BODY_TIMEOUT;
            let mut last_data = Instant::now();
            let mut buffer = [0u8; 32 * 1024];
            while remaining > 0 {
                let capacity = (remaining as usize).min(buffer.len());
                let read = read_socket(
                    stream,
                    &mut buffer[..capacity],
                    stopped,
                    deadline,
                    &mut last_data,
                )?;
                file.write_all(&buffer[..read])
                    .map_err(|error| RequestError::new(500, format!("无法暂存音频：{error}")))?;
                remaining -= read as u64;
            }
            drop(file);
            if should_stop(stopped) {
                return Err(RequestError::new(503, "导入服务已关闭"));
            }
            let result = storage
                .import_files_if(std::slice::from_ref(&upload.file), || !should_stop(stopped))
                .map_err(|error| {
                    RequestError::new(if should_stop(stopped) { 503 } else { 500 }, error)
                })?;
            let prefix = format!("{}: ", upload.file.display());
            let failed = result
                .warnings
                .iter()
                .find_map(|warning| warning.strip_prefix(&prefix))
                .map(str::to_owned);
            let count = result.tracks.len();
            let warnings = result.warnings.clone();
            let _ = events.send(TransferEvent::LibraryChanged(result));
            if let Some(error) = failed {
                return Err(RequestError::new(
                    if should_stop(stopped) { 503 } else { 422 },
                    if error.starts_with("Cannot read audio metadata:") {
                        "无法识别音频内容，请检查文件是否完整、格式是否受支持".to_owned()
                    } else {
                        error
                    },
                ));
            }
            respond_json(
                stream,
                200,
                &json!({"ok": true, "filename": filename, "count": count, "warnings": warnings}),
                stopped,
            )
        }
        _ => Err(RequestError::new(404, "找不到此操作")),
    }
}

fn token_matches(value: &str, expected: &str) -> bool {
    if value.len() != expected.len() {
        return false;
    }
    value
        .bytes()
        .zip(expected.bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn decode_filename(encoded: &str) -> Result<String, RequestError> {
    let mut decoded = Vec::new();
    let mut bytes = encoded.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let (Some(high), Some(low)) = (bytes.next(), bytes.next()) else {
                return Err(RequestError::new(400, "文件名编码无效"));
            };
            let high = (high as char).to_digit(16);
            let low = (low as char).to_digit(16);
            let (Some(high), Some(low)) = (high, low) else {
                return Err(RequestError::new(400, "文件名编码无效"));
            };
            decoded.push((high * 16 + low) as u8);
        } else {
            decoded.push(byte);
        }
    }
    let filename =
        String::from_utf8(decoded).map_err(|_| RequestError::new(400, "文件名必须是 UTF-8"))?;
    if filename.is_empty()
        || filename.len() > 255
        || matches!(filename.as_str(), "." | "..")
        || filename
            .chars()
            .any(|character| character.is_control() || character == '/' || character == '\\')
        || Path::new(&filename)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(filename.as_str())
    {
        return Err(RequestError::new(
            400,
            "文件名不能包含目录或控制字符，且不能超过 255 字节",
        ));
    }
    Ok(filename)
}

fn read_socket(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    stopped: &StopState,
    deadline: Instant,
    last_data: &mut Instant,
) -> Result<usize, RequestError> {
    loop {
        if should_stop(stopped) {
            return Err(RequestError::new(503, "导入服务已关闭"));
        }
        if Instant::now() >= deadline || last_data.elapsed() >= IDLE_TIMEOUT {
            return Err(RequestError::new(408, "连接等待超时，请重试"));
        }
        match stream.read(buffer) {
            Ok(0) => return Err(RequestError::new(400, "上传连接已中断")),
            Ok(read) => {
                *last_data = Instant::now();
                return Ok(read);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(RequestError::new(400, format!("读取上传失败：{error}"))),
        }
    }
}

fn respond_json(
    stream: &mut TcpStream,
    status: u16,
    value: &Value,
    stopped: &StopState,
) -> Result<(), RequestError> {
    let body =
        serde_json::to_vec(value).map_err(|error| RequestError::new(500, error.to_string()))?;
    respond(
        stream,
        status,
        "application/json; charset=utf-8",
        &body,
        stopped,
    )
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    stopped: &StopState,
) -> Result<(), RequestError> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        417 => "Expectation Failed",
        422 => "Unprocessable Content",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'\r\n\r\n",
        body.len()
    );
    let deadline = Instant::now() + HEADER_TIMEOUT;
    for mut bytes in [header.as_bytes(), body] {
        while !bytes.is_empty() {
            if should_stop(stopped) || Instant::now() >= deadline {
                return Err(RequestError::new(503, "导入服务已关闭或连接超时"));
            }
            match stream.write(bytes) {
                Ok(0) => return Err(RequestError::new(400, "连接已关闭")),
                Ok(written) => bytes = &bytes[written..],
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(RequestError::new(400, error.to_string())),
            }
        }
    }
    Ok(())
}

struct Upload {
    directory: PathBuf,
    file: PathBuf,
}

impl Upload {
    fn create(staging: &Path, filename: &str) -> Result<(Self, File), RequestError> {
        loop {
            let directory = staging.join(random_token());
            match fs::create_dir(&directory) {
                Ok(()) => {
                    let upload = Self {
                        file: directory.join(filename),
                        directory,
                    };
                    let file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&upload.file)
                        .map_err(|error| {
                            RequestError::new(500, format!("无法建立上传文件：{error}"))
                        })?;
                    return Ok((upload, file));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(RequestError::new(500, format!("无法建立上传目录：{error}")));
                }
            }
        }
    }
}

impl Drop for Upload {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn local_addresses() -> Vec<Ipv4Addr> {
    let mut addresses = BTreeSet::new();
    #[cfg(unix)]
    unsafe {
        let mut first = std::ptr::null_mut();
        if libc::getifaddrs(&mut first) == 0 {
            let mut current = first;
            while !current.is_null() {
                let interface = &*current;
                if !interface.ifa_addr.is_null()
                    && !interface.ifa_name.is_null()
                    && (*interface.ifa_addr).sa_family as i32 == libc::AF_INET
                {
                    let flags = interface.ifa_flags as i32;
                    let name = std::ffi::CStr::from_ptr(interface.ifa_name).to_bytes();
                    if flags & libc::IFF_UP != 0
                        && flags & libc::IFF_BROADCAST != 0
                        && flags & (libc::IFF_LOOPBACK | libc::IFF_POINTOPOINT) == 0
                        && (name.starts_with(b"en") || name.starts_with(b"bridge"))
                    {
                        let address = &*(interface.ifa_addr as *const libc::sockaddr_in);
                        let address = Ipv4Addr::from(u32::from_be(address.sin_addr.s_addr));
                        if !address.is_loopback() && !address.is_unspecified() {
                            addresses.insert(address);
                        }
                    }
                }
                current = interface.ifa_next;
            }
            libc::freeifaddrs(first);
        }
    }
    if addresses.is_empty() {
        addresses.insert(Ipv4Addr::LOCALHOST);
    }
    addresses.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn session() -> (tempfile::TempDir, Storage, Transfer) {
        let directory = tempfile::tempdir().unwrap();
        let storage = Storage::new(directory.path().join("Documents")).unwrap();
        let transfer = Transfer::start(storage.clone()).unwrap();
        (directory, storage, transfer)
    }

    fn address(transfer: &Transfer) -> SocketAddr {
        let url = transfer.urls().first().unwrap();
        let port = url.split('/').nth(2).unwrap().split(':').nth(1).unwrap();
        format!("127.0.0.1:{port}").parse().unwrap()
    }

    fn token(transfer: &Transfer) -> &str {
        transfer.urls()[0].split('#').nth(1).unwrap()
    }

    fn request(transfer: &Transfer, bytes: &[u8]) -> (u16, String, Vec<u8>) {
        let mut stream = TcpStream::connect(address(transfer)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        stream.write_all(bytes).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut response = Vec::new();
        if let Err(error) = stream.read_to_end(&mut response) {
            // Rejected overlong requests can still have unread bytes when the
            // server closes. TCP may reset after delivering the JSON response.
            assert_eq!(error.kind(), io::ErrorKind::ConnectionReset);
            assert!(!response.is_empty());
        }
        let boundary = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap()
            + 4;
        let headers = String::from_utf8(response[..boundary].to_vec()).unwrap();
        let status = headers.split(' ').nth(1).unwrap().parse().unwrap();
        (status, headers, response[boundary..].to_vec())
    }

    fn wait_for(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(4);
        while !condition() {
            assert!(Instant::now() < deadline, "condition did not become true");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn staging(storage: &Storage) -> PathBuf {
        storage
            .music_dir()
            .parent()
            .unwrap()
            .join(".nano/transfers")
    }

    #[test]
    fn only_the_static_page_is_public_and_api_requires_the_current_token() {
        let (_directory, storage, transfer) = session();
        let (status, headers, body) =
            request(&transfer, b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert_eq!(status, 200);
        assert!(String::from_utf8(body).unwrap().contains("拖入音乐文件"));
        assert!(
            !headers
                .to_ascii_lowercase()
                .contains("access-control-allow-origin")
        );
        assert_eq!(
            request(
                &transfer,
                b"GET /api/library HTTP/1.1\r\nHost: localhost\r\n\r\n"
            )
            .0,
            401
        );
        let bad_token = format!(
            "GET /api/library HTTP/1.1\r\nX-Nano-Token: {}\r\n\r\n",
            "0".repeat(32)
        );
        assert_eq!(request(&transfer, bad_token.as_bytes()).0, 401);
        let authorized = format!(
            "GET /api/library HTTP/1.1\r\nX-Nano-Token: {}\r\n\r\n",
            token(&transfer)
        );
        let (status, _, body) = request(&transfer, authorized.as_bytes());
        assert_eq!(status, 200);
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["count"], 0);
        assert!(fs::read_dir(staging(&storage)).unwrap().next().is_none());
    }

    #[test]
    fn malformed_paths_lengths_and_chunked_bodies_are_rejected_without_files() {
        let (_directory, storage, transfer) = session();
        let auth = token(&transfer);
        let cases = [
            (
                format!(
                    "POST /api/upload HTTP/1.1\r\nX-Nano-Token: {auth}\r\nContent-Length: 1\r\nX-Nano-Filename: ..%2Fescape.wav\r\n\r\n"
                ),
                400,
            ),
            (
                format!(
                    "POST /api/upload HTTP/1.1\r\nX-Nano-Token: {auth}\r\nContent-Length: 1\r\nX-Nano-Filename: bad%00.wav\r\n\r\n"
                ),
                400,
            ),
            (
                format!(
                    "POST /api/upload HTTP/1.1\r\nX-Nano-Token: {auth}\r\nContent-Length: {}\r\nX-Nano-Filename: song.wav\r\n\r\n",
                    MAX_UPLOAD + 1
                ),
                413,
            ),
            (
                format!(
                    "POST /api/upload HTTP/1.1\r\nX-Nano-Token: {auth}\r\nX-Nano-Filename: song.wav\r\n\r\n"
                ),
                411,
            ),
            (
                format!(
                    "POST /api/upload HTTP/1.1\r\nX-Nano-Token: {auth}\r\nTransfer-Encoding: chunked\r\n\r\n"
                ),
                400,
            ),
            (
                format!(
                    "POST /api/upload HTTP/1.1\r\nX-Nano-Token: {auth}\r\nContent-Length: 1\r\nContent-Length: 2\r\nX-Nano-Filename: song.wav\r\n\r\n"
                ),
                400,
            ),
            (
                format!("GET /api/../../library.sqlite3 HTTP/1.1\r\nX-Nano-Token: {auth}\r\n\r\n"),
                404,
            ),
        ];
        for (bytes, status) in cases {
            assert_eq!(request(&transfer, bytes.as_bytes()).0, status);
        }
        assert_eq!(
            request(
                &transfer,
                format!(
                    "GET / HTTP/1.1\r\nOversize: {}\r\n\r\n",
                    "x".repeat(MAX_HEADERS)
                )
                .as_bytes()
            )
            .0,
            431
        );
        assert!(storage.load().unwrap().is_empty());
        assert!(fs::read_dir(staging(&storage)).unwrap().next().is_none());
    }

    #[test]
    fn binary_upload_enters_the_shared_library_and_emits_an_event() {
        let (_directory, storage, transfer) = session();
        let audio = include_bytes!("../../tests/fixtures/quiet.wav");
        let mut bytes = format!("POST /api/upload HTTP/1.1\r\nX-Nano-Token: {}\r\nX-Nano-Filename: %E9%9F%B3%E4%B9%90.wav\r\nContent-Length: {}\r\n\r\n", token(&transfer), audio.len()).into_bytes();
        bytes.extend_from_slice(audio);
        let (status, _, body) = request(&transfer, &bytes);
        assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
        let response: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(response["ok"], true);
        assert_eq!(response["count"], 1);
        let tracks = storage.load().unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].path.file_name().unwrap(), "音乐.wav");
        assert_eq!(fs::read(&tracks[0].path).unwrap(), audio);
        assert!(fs::read_dir(staging(&storage)).unwrap().next().is_none());
        let mut changed = false;
        while let Some(event) = transfer.poll_event() {
            if let TransferEvent::LibraryChanged(result) = event {
                assert_eq!(result.tracks.len(), 1);
                changed = true;
            }
        }
        assert!(changed);
        let authorized = format!(
            "GET /api/library HTTP/1.1\r\nX-Nano-Token: {}\r\n\r\n",
            token(&transfer)
        );
        let (_, _, body) = request(&transfer, authorized.as_bytes());
        let response: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(response["tracks"][0]["title"], "音乐");
        assert!(response["tracks"][0].get("path").is_none());

        let invalid = format!(
            "POST /api/upload HTTP/1.1\r\nX-Nano-Token: {}\r\nX-Nano-Filename: broken.mp3\r\nContent-Length: 3\r\n\r\nbad",
            token(&transfer)
        );
        let (status, _, body) = request(&transfer, invalid.as_bytes());
        assert_eq!(status, 422);
        let response: Value = serde_json::from_slice(&body).unwrap();
        assert!(response["error"].as_str().unwrap().contains("无法识别音频"));
        assert_eq!(storage.load().unwrap().len(), 1);
        assert!(fs::read_dir(staging(&storage)).unwrap().next().is_none());
    }

    #[test]
    fn disconnect_and_explicit_stop_remove_partial_files_and_invalidate_the_session() {
        let (_directory, storage, mut transfer) = session();
        let header = format!(
            "POST /api/upload HTTP/1.1\r\nX-Nano-Token: {}\r\nX-Nano-Filename: partial.wav\r\nContent-Length: 4096\r\n\r\n",
            token(&transfer)
        );
        let mut stream = TcpStream::connect(address(&transfer)).unwrap();
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(b"partial audio").unwrap();
        wait_for(|| fs::read_dir(staging(&storage)).unwrap().next().is_some());
        stream.shutdown(Shutdown::Both).unwrap();
        wait_for(|| fs::read_dir(staging(&storage)).unwrap().next().is_none());
        let mut active = TcpStream::connect(address(&transfer)).unwrap();
        active.write_all(header.as_bytes()).unwrap();
        active.write_all(b"partial audio").unwrap();
        wait_for(|| fs::read_dir(staging(&storage)).unwrap().next().is_some());
        let old_address = address(&transfer);
        let old_token = token(&transfer).to_owned();
        let start = Instant::now();
        transfer.stop();
        assert!(start.elapsed() < Duration::from_millis(100));
        wait_for(|| fs::read_dir(staging(&storage)).unwrap().next().is_none());
        wait_for(|| TcpStream::connect(old_address).is_err());
        assert!(storage.load().unwrap().is_empty());
        let next = Transfer::start(storage.clone()).unwrap();
        assert_ne!(old_token, token(&next));
        let bytes = format!("GET /api/library HTTP/1.1\r\nX-Nano-Token: {old_token}\r\n\r\n");
        assert_eq!(request(&next, bytes.as_bytes()).0, 401);
    }

    fn controlled_session() -> (
        tempfile::TempDir,
        Storage,
        Transfer,
        Arc<AtomicBool>,
        Arc<std::sync::atomic::AtomicU64>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let storage = Storage::new(directory.path().to_owned()).unwrap();
        let allowed = Arc::new(AtomicBool::new(true));
        let epoch = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let read_allowed = Arc::clone(&allowed);
        let read_epoch = Arc::clone(&epoch);
        let transfer = Transfer::start_with_access(
            storage.clone(),
            Arc::new(move || {
                (
                    read_allowed.load(Ordering::Acquire),
                    read_epoch.load(Ordering::Acquire),
                )
            }),
        )
        .unwrap();
        (directory, storage, transfer, allowed, epoch)
    }

    fn partial_upload(transfer: &Transfer, storage: &Storage) -> TcpStream {
        let mut stream = TcpStream::connect(address(transfer)).unwrap();
        let header = format!(
            "POST /api/upload HTTP/1.1\r\nX-Nano-Token: {}\r\nX-Nano-Filename: partial.wav\r\nContent-Length: 4096\r\n\r\n",
            token(transfer),
        );
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(b"partial audio").unwrap();
        wait_for(|| fs::read_dir(staging(storage)).unwrap().next().is_some());
        stream
    }

    #[test]
    fn background_expiration_cleans_partial_upload_and_foreground_resumes_same_address() {
        let (_directory, storage, mut transfer, allowed, epoch) = controlled_session();
        let original_urls = transfer.urls().to_owned();
        let original_token = token(&transfer).to_owned();
        let _stream = partial_upload(&transfer, &storage);
        allowed.store(false, Ordering::Release);
        epoch.fetch_add(1, Ordering::AcqRel);
        wait_for(|| fs::read_dir(staging(&storage)).unwrap().next().is_none());
        assert!(
            transfer.is_running(),
            "expiry pauses requests rather than revoking the service"
        );
        assert!(transfer.is_paused());
        assert_eq!(transfer.urls(), original_urls);
        assert!(storage.load().unwrap().is_empty());

        allowed.store(true, Ordering::Release);
        let get = format!("GET /api/library HTTP/1.1\r\nX-Nano-Token: {original_token}\r\n\r\n");
        assert_eq!(request(&transfer, get.as_bytes()).0, 200);
        assert_eq!(transfer.urls(), original_urls);
        let audio = include_bytes!("../../tests/fixtures/quiet.wav");
        let mut upload = format!("POST /api/upload HTTP/1.1\r\nX-Nano-Token: {original_token}\r\nX-Nano-Filename: resumed.wav\r\nContent-Length: {}\r\n\r\n", audio.len()).into_bytes();
        upload.extend_from_slice(audio);
        assert_eq!(request(&transfer, &upload).0, 200);
        assert_eq!(storage.load().unwrap().len(), 1);

        allowed.store(false, Ordering::Release);
        transfer.stop();
        assert!(
            !transfer.is_running(),
            "manual Stop remains final while paused"
        );
        wait_for(|| TcpStream::connect(address(&transfer)).is_err());
        assert!(fs::read_dir(staging(&storage)).unwrap().next().is_none());
    }

    #[test]
    fn rapid_foreground_resume_does_not_publish_an_expired_request() {
        let (_directory, storage, transfer, allowed, epoch) = controlled_session();
        let _stream = partial_upload(&transfer, &storage);
        // A suspended worker can miss a short false interval. Per-request
        // epochs still reject its incomplete body after foreground resumption.
        allowed.store(false, Ordering::Release);
        epoch.fetch_add(1, Ordering::AcqRel);
        allowed.store(true, Ordering::Release);
        wait_for(|| fs::read_dir(staging(&storage)).unwrap().next().is_none());
        assert!(transfer.is_running());
        assert!(storage.load().unwrap().is_empty());
        let get = format!(
            "GET /api/library HTTP/1.1\r\nX-Nano-Token: {}\r\n\r\n",
            token(&transfer)
        );
        assert_eq!(request(&transfer, get.as_bytes()).0, 200);
    }
}
