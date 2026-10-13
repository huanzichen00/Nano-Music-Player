//! System media integration boundary.
//!
//! The controller publishes snapshots and polls typed commands. On macOS,
//! publication and registration run on a private worker; native callbacks only
//! enqueue typed commands. Other platforms get the same API as a no-op.

use std::sync::mpsc::{self, Receiver};

#[derive(Clone, Debug, PartialEq)]
pub struct MediaSnapshot {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Encoded PNG or JPEG artwork bytes.
    pub artwork: Option<Vec<u8>>,
    pub duration_seconds: Option<f64>,
    pub elapsed_seconds: f64,
    pub playback_state: PlaybackState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlaybackState {
    Playing,
    Paused,
    #[default]
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RemoteCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    SeekTo(f64),
}

/// Controller-facing bridge. `publish` is fire-and-forget; poll commands from
/// the controller's normal event loop and map them to its existing actions.
pub struct MediaBridge {
    #[cfg(target_os = "macos")]
    worker: macos::Worker,
    commands: Receiver<RemoteCommand>,
}

impl MediaBridge {
    pub fn new() -> Self {
        let (command_tx, commands) = mpsc::channel();
        #[cfg(target_os = "macos")]
        let worker = macos::start(command_tx);
        #[cfg(not(target_os = "macos"))]
        let _ = command_tx;

        Self {
            #[cfg(target_os = "macos")]
            worker,
            commands,
        }
    }

    pub fn publish(&self, snapshot: Option<MediaSnapshot>) {
        #[cfg(target_os = "macos")]
        {
            let _ = self.worker.updates.send(WorkerMessage::Snapshot(snapshot));
        }
        #[cfg(not(target_os = "macos"))]
        let _ = snapshot;
    }

    pub fn poll_command(&self) -> Option<RemoteCommand> {
        self.commands.try_recv().ok()
    }
}

impl Default for MediaBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
enum WorkerMessage {
    Snapshot(Option<MediaSnapshot>),
    Shutdown,
    #[cfg(test)]
    Barrier(std::sync::mpsc::Sender<()>),
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::ffi::{CString, c_char, c_double, c_int, c_long, c_void};
    use std::ptr;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc::Sender;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }

    // Blocks are supplied by libSystem. Captures must survive native copies.
    unsafe extern "C" {
        static _NSConcreteStackBlock: c_void;
        fn _Block_copy(block: *const c_void) -> *mut c_void;
        fn _Block_release(block: *const c_void);
    }

    #[link(name = "Foundation", kind = "framework")]
    unsafe extern "C" {}
    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}
    #[link(name = "MediaPlayer", kind = "framework")]
    unsafe extern "C" {
        static MPMediaItemPropertyTitle: Obj;
        static MPMediaItemPropertyArtist: Obj;
        static MPMediaItemPropertyAlbumTitle: Obj;
        static MPMediaItemPropertyPlaybackDuration: Obj;
        static MPMediaItemPropertyArtwork: Obj;
        static MPNowPlayingInfoPropertyElapsedPlaybackTime: Obj;
        static MPNowPlayingInfoPropertyPlaybackRate: Obj;
    }

    type Obj = *mut c_void;
    type Sel = *mut c_void;
    type SendId = unsafe extern "C" fn(Obj, Sel) -> Obj;
    type SendIdId = unsafe extern "C" fn(Obj, Sel, Obj) -> Obj;
    type SendIdIdId = unsafe extern "C" fn(Obj, Sel, Obj, Obj) -> Obj;
    type SendIdDouble = unsafe extern "C" fn(Obj, Sel, c_double) -> Obj;
    type SendBool = unsafe extern "C" fn(Obj, Sel, bool);
    type SendLong = unsafe extern "C" fn(Obj, Sel, c_long);
    type SendVoidId = unsafe extern "C" fn(Obj, Sel, Obj);
    type SendDouble = unsafe extern "C" fn(Obj, Sel) -> c_double;
    type SendVoid = unsafe extern "C" fn(Obj, Sel);

    fn sel(name: &str) -> Sel {
        let name = CString::new(name).expect("selector has no nul byte");
        unsafe { sel_registerName(name.as_ptr()) }
    }

    fn class(name: &str) -> Obj {
        let name = CString::new(name).expect("class has no nul byte");
        unsafe { objc_getClass(name.as_ptr()) }
    }

    unsafe fn send_id(receiver: Obj, selector: Sel) -> Obj {
        let f: SendId = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(receiver, selector) }
    }

    unsafe fn send_id_id(receiver: Obj, selector: Sel, arg: Obj) -> Obj {
        let f: SendIdId = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(receiver, selector, arg) }
    }

    unsafe fn send_id_id_id(receiver: Obj, selector: Sel, a: Obj, b: Obj) -> Obj {
        let f: SendIdIdId = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(receiver, selector, a, b) }
    }

    unsafe fn send_bool(receiver: Obj, selector: Sel, value: bool) {
        let f: SendBool = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(receiver, selector, value) };
    }

    unsafe fn send_long(receiver: Obj, selector: Sel, value: c_long) {
        let f: SendLong = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(receiver, selector, value) };
    }

    unsafe fn send_void_id(receiver: Obj, selector: Sel, arg: Obj) {
        let f: SendVoidId = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(receiver, selector, arg) };
    }

    unsafe fn send_void(receiver: Obj, selector: Sel) {
        let f: SendVoid = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(receiver, selector) }
    }

    unsafe fn send_return_double(receiver: Obj, selector: Sel) -> f64 {
        let f: SendDouble = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(receiver, selector) }
    }

    unsafe fn ns_string(value: &str) -> Obj {
        let value = CString::new(value.replace('\0', "�")).expect("sanitized string");
        unsafe {
            send_id_id(
                class("NSString"),
                sel("stringWithUTF8String:"),
                value.as_ptr() as *mut c_void,
            )
        }
    }

    unsafe fn number_double(value: f64) -> Obj {
        let f: SendIdDouble = unsafe { std::mem::transmute(objc_msgSend as *const ()) };
        unsafe { f(class("NSNumber"), sel("numberWithDouble:"), value) }
    }

    unsafe fn dictionary(keys: &[Obj], values: &[Obj]) -> Obj {
        let array_class = class("NSArray");
        let keys_array = unsafe {
            let f: unsafe extern "C" fn(Obj, Sel, *const Obj, c_long) -> Obj =
                std::mem::transmute(objc_msgSend as *const ());
            f(
                array_class,
                sel("arrayWithObjects:count:"),
                keys.as_ptr(),
                keys.len() as c_long,
            )
        };
        let values_array = unsafe {
            let f: unsafe extern "C" fn(Obj, Sel, *const Obj, c_long) -> Obj =
                std::mem::transmute(objc_msgSend as *const ());
            f(
                array_class,
                sel("arrayWithObjects:count:"),
                values.as_ptr(),
                values.len() as c_long,
            )
        };
        unsafe {
            send_id_id_id(
                class("NSDictionary"),
                sel("dictionaryWithObjects:forKeys:"),
                values_array,
                keys_array,
            )
        }
    }

    #[repr(C)]
    struct BlockDescriptor {
        reserved: usize,
        size: usize,
        copy: unsafe extern "C" fn(Obj, Obj),
        dispose: unsafe extern "C" fn(Obj),
        signature: *const c_char,
    }
    // Descriptors only contain static data and function pointers.
    unsafe impl Sync for BlockDescriptor {}

    const BLOCK_HAS_COPY_DISPOSE: c_int = 1 << 25;
    const BLOCK_HAS_SIGNATURE: c_int = 1 << 30;
    const STATUS_SUCCESS: c_long = 0;
    const STATUS_NO_ITEM: c_long = 110;
    const STATUS_FAILED: c_long = 200;

    // The framework centers are process-wide. A replacement bridge owns the
    // surface; shutting down an older bridge must not erase its replacement.
    static NATIVE_LOCK: Mutex<()> = Mutex::new(());
    static NEXT_BRIDGE: AtomicU64 = AtomicU64::new(1);
    static ACTIVE_BRIDGE: AtomicU64 = AtomicU64::new(0);

    struct CommandRoute {
        id: u64,
        alive: AtomicBool,
        sender: Sender<RemoteCommand>,
    }

    impl CommandRoute {
        fn new(sender: Sender<RemoteCommand>) -> Arc<Self> {
            Arc::new(Self {
                id: NEXT_BRIDGE.fetch_add(1, Ordering::Relaxed),
                alive: AtomicBool::new(true),
                sender,
            })
        }
    }

    #[repr(C)]
    struct RemoteBlock {
        isa: *const c_void,
        flags: c_int,
        reserved: c_int,
        invoke: unsafe extern "C" fn(*mut RemoteBlock, Obj) -> c_long,
        descriptor: *const BlockDescriptor,
        command: c_int,
        route: *const CommandRoute,
    }

    static REMOTE_DESCRIPTOR: BlockDescriptor = BlockDescriptor {
        reserved: 0,
        size: std::mem::size_of::<RemoteBlock>(),
        copy: copy_remote,
        dispose: dispose_remote,
        signature: c"q@?@".as_ptr(),
    };

    unsafe extern "C" fn copy_remote(_destination: Obj, source: Obj) {
        // The runtime has already copied the literal bytes to the destination.
        let route = unsafe { (*(source as *const RemoteBlock)).route };
        unsafe { Arc::increment_strong_count(route) };
    }

    unsafe extern "C" fn dispose_remote(block: Obj) {
        let route = unsafe { (*(block as *const RemoteBlock)).route };
        unsafe { Arc::decrement_strong_count(route) };
    }

    unsafe extern "C" fn handle_remote(block: *mut RemoteBlock, event: Obj) -> c_long {
        let route = unsafe { &*(*block).route };
        if !route.alive.load(Ordering::Acquire) {
            return STATUS_FAILED;
        }
        if ACTIVE_BRIDGE.load(Ordering::Acquire) != route.id {
            return STATUS_NO_ITEM;
        }
        let code = unsafe { (*block).command };
        let command = match code {
            0 => RemoteCommand::Play,
            1 => RemoteCommand::Pause,
            2 => RemoteCommand::Toggle,
            3 => RemoteCommand::Next,
            4 => RemoteCommand::Previous,
            5 => {
                if event.is_null() {
                    return STATUS_FAILED;
                }
                let time = unsafe { send_return_double(event, sel("positionTime")) };
                if !time.is_finite() {
                    return STATUS_FAILED;
                }
                RemoteCommand::SeekTo(time.max(0.0))
            }
            _ => return STATUS_FAILED,
        };
        if route.sender.send(command).is_ok() {
            STATUS_SUCCESS
        } else {
            STATUS_FAILED
        }
    }

    struct CopiedBlock(Obj);

    impl CopiedBlock {
        unsafe fn new(literal: Obj) -> Self {
            let block = unsafe { _Block_copy(literal) };
            assert!(!block.is_null(), "copy native media block");
            Self(block)
        }
    }

    impl Drop for CopiedBlock {
        fn drop(&mut self) {
            unsafe { _Block_release(self.0) }
        }
    }

    fn make_remote_block(command: c_int, route: &Arc<CommandRoute>) -> CopiedBlock {
        let literal = RemoteBlock {
            isa: ptr::addr_of!(_NSConcreteStackBlock),
            flags: BLOCK_HAS_COPY_DISPOSE | BLOCK_HAS_SIGNATURE,
            reserved: 0,
            invoke: handle_remote,
            descriptor: &REMOTE_DESCRIPTOR,
            command,
            route: Arc::as_ptr(route),
        };
        unsafe { CopiedBlock::new(&literal as *const _ as Obj) }
    }

    /// A +1 Objective-C reference, confined to the native worker.
    struct OwnedObject(Obj);

    impl Drop for OwnedObject {
        fn drop(&mut self) {
            unsafe { send_void(self.0, sel("release")) }
        }
    }

    struct Registration {
        remote: OwnedObject,
        target: OwnedObject,
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            unsafe { send_void_id(self.remote.0, sel("removeTarget:"), self.target.0) }
        }
    }

    fn install_remote_commands(route: &Arc<CommandRoute>) -> Vec<Registration> {
        let mut registrations = Vec::new();
        unsafe {
            let center = send_id(class("MPRemoteCommandCenter"), sel("sharedCommandCenter"));
            for (property, command) in [
                ("playCommand", 0),
                ("pauseCommand", 1),
                ("togglePlayPauseCommand", 2),
                ("nextTrackCommand", 3),
                ("previousTrackCommand", 4),
                ("changePlaybackPositionCommand", 5),
            ] {
                let remote = send_id(center, sel(property));
                let block = make_remote_block(command, route);
                let target = send_id_id(remote, sel("addTargetWithHandler:"), block.0);
                if !remote.is_null() && !target.is_null() {
                    registrations.push(Registration {
                        remote: OwnedObject(send_id(remote, sel("retain"))),
                        target: OwnedObject(send_id(target, sel("retain"))),
                    });
                    if ACTIVE_BRIDGE.load(Ordering::Acquire) == route.id {
                        send_bool(remote, sel("setEnabled:"), true);
                    }
                }
            }
        }
        registrations
    }

    struct Pool(Obj);
    impl Pool {
        fn new() -> Self {
            unsafe {
                let allocated = send_id(class("NSAutoreleasePool"), sel("alloc"));
                Self(send_id(allocated, sel("init")))
            }
        }
    }
    impl Drop for Pool {
        fn drop(&mut self) {
            unsafe { send_void(self.0, sel("drain")) }
        }
    }

    pub(super) struct Worker {
        pub(super) updates: Sender<WorkerMessage>,
        route: Arc<CommandRoute>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl Drop for Worker {
        fn drop(&mut self) {
            // Do not run Objective-C or join a native worker from the UI thread.
            // Late callbacks fail immediately; removal and clearing run below.
            self.route.alive.store(false, Ordering::Release);
            let _ = self.updates.send(WorkerMessage::Shutdown);
            let _ = self.thread.take();
        }
    }

    pub(super) fn start(commands: Sender<RemoteCommand>) -> Worker {
        #[cfg(not(test))]
        {
            start_native(commands)
        }
        #[cfg(test)]
        {
            // Controller unit tests create bridges concurrently. They use an
            // inactive adapter; native tests explicitly opt into start_native.
            let (updates, receiver) = mpsc::channel();
            drop(receiver);
            Worker {
                updates,
                route: CommandRoute::new(commands),
                thread: None,
            }
        }
    }

    fn start_native(commands: Sender<RemoteCommand>) -> Worker {
        let (updates, receiver) = mpsc::channel();
        let route = CommandRoute::new(commands);
        // Claim ownership before spawning: thread scheduling must not let a
        // delayed older worker supersede a bridge created more recently.
        ACTIVE_BRIDGE.fetch_max(route.id, Ordering::AcqRel);
        let worker_route = route.clone();
        let thread = thread::Builder::new()
            .name("macos-media-bridge".into())
            .spawn(move || {
                let _pool = Pool::new();
                let registrations = {
                    let _lock = NATIVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                    install_remote_commands(&worker_route)
                };
                let mut artwork = ArtworkCache::default();
                while let Ok(message) = receiver.recv() {
                    let _pool = Pool::new();
                    match message {
                        WorkerMessage::Snapshot(snapshot) => {
                            let _lock = NATIVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                            if ACTIVE_BRIDGE.load(Ordering::Acquire) == worker_route.id {
                                publish_native(snapshot.as_ref(), &mut artwork);
                            }
                        }
                        WorkerMessage::Shutdown => break,
                        #[cfg(test)]
                        WorkerMessage::Barrier(ack) => {
                            let _ = ack.send(());
                        }
                    }
                }
                worker_route.alive.store(false, Ordering::Release);
                let _lock = NATIVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                if ACTIVE_BRIDGE
                    .compare_exchange(worker_route.id, 0, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    publish_native(None, &mut artwork);
                    for registration in &registrations {
                        unsafe { send_bool(registration.remote.0, sel("setEnabled:"), false) }
                    }
                }
                drop(registrations);
            })
            .expect("start macOS media worker");
        Worker {
            updates,
            route,
            thread: Some(thread),
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Size {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    struct ArtworkBlock {
        isa: *const c_void,
        flags: c_int,
        reserved: c_int,
        invoke: unsafe extern "C" fn(*mut ArtworkBlock, Size) -> Obj,
        descriptor: *const BlockDescriptor,
        image: Obj,
    }

    static ARTWORK_DESCRIPTOR: BlockDescriptor = BlockDescriptor {
        reserved: 0,
        size: std::mem::size_of::<ArtworkBlock>(),
        copy: copy_artwork,
        dispose: dispose_artwork,
        signature: c"@@?{CGSize=dd}".as_ptr(),
    };

    unsafe extern "C" fn copy_artwork(_destination: Obj, source: Obj) {
        let image = unsafe { (*(source as *const ArtworkBlock)).image };
        unsafe { send_id(image, sel("retain")) };
    }

    unsafe extern "C" fn dispose_artwork(block: Obj) {
        let image = unsafe { (*(block as *const ArtworkBlock)).image };
        unsafe { send_void(image, sel("release")) };
    }

    unsafe extern "C" fn handle_artwork(block: *mut ArtworkBlock, _size: Size) -> Obj {
        unsafe { (*block).image }
    }

    fn decode_artwork(bytes: &[u8]) -> Option<OwnedObject> {
        if bytes.is_empty() {
            return None;
        }
        unsafe {
            let send_data: unsafe extern "C" fn(Obj, Sel, *const u8, usize) -> Obj =
                std::mem::transmute(objc_msgSend as *const ());
            let data = send_data(
                class("NSData"),
                sel("dataWithBytes:length:"),
                bytes.as_ptr(),
                bytes.len(),
            );
            // initWithData: is an instance initializer, and may return nil.
            let image = OwnedObject(send_id_id(
                send_id(class("NSImage"), sel("alloc")),
                sel("initWithData:"),
                data,
            ));
            let send_valid: unsafe extern "C" fn(Obj, Sel) -> bool =
                std::mem::transmute(objc_msgSend as *const ());
            if image.0.is_null() || !send_valid(image.0, sel("isValid")) {
                return None;
            }
            let send_size: unsafe extern "C" fn(Obj, Sel) -> Size =
                std::mem::transmute(objc_msgSend as *const ());
            let size = send_size(image.0, sel("size"));
            if !size.width.is_finite()
                || !size.height.is_finite()
                || size.width <= 0.0
                || size.height <= 0.0
            {
                return None;
            }
            let literal = ArtworkBlock {
                isa: ptr::addr_of!(_NSConcreteStackBlock),
                flags: BLOCK_HAS_COPY_DISPOSE | BLOCK_HAS_SIGNATURE,
                reserved: 0,
                invoke: handle_artwork,
                descriptor: &ARTWORK_DESCRIPTOR,
                image: image.0,
            };
            let block = CopiedBlock::new(&literal as *const _ as Obj);
            let init_artwork: unsafe extern "C" fn(Obj, Sel, Size, Obj) -> Obj =
                std::mem::transmute(objc_msgSend as *const ());
            let artwork = init_artwork(
                send_id(class("MPMediaItemArtwork"), sel("alloc")),
                sel("initWithBoundsSize:requestHandler:"),
                size,
                block.0,
            );
            (!artwork.is_null()).then_some(OwnedObject(artwork))
        }
    }

    #[derive(Default)]
    struct ArtworkCache {
        bytes: Option<Vec<u8>>,
        artwork: Option<OwnedObject>,
    }

    impl ArtworkCache {
        fn update(&mut self, bytes: Option<&[u8]>) -> Option<Obj> {
            if self.bytes.as_deref() != bytes {
                self.artwork = bytes.and_then(decode_artwork);
                self.bytes = bytes.map(<[u8]>::to_vec);
            }
            self.artwork.as_ref().map(|artwork| artwork.0)
        }
    }

    fn nonnegative_finite(value: f64) -> f64 {
        if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        }
    }

    fn publish_native(snapshot: Option<&MediaSnapshot>, artwork: &mut ArtworkCache) {
        unsafe {
            let center = send_id(class("MPNowPlayingInfoCenter"), sel("defaultCenter"));
            let Some(snapshot) = snapshot else {
                send_void_id(center, sel("setNowPlayingInfo:"), ptr::null_mut());
                send_long(center, sel("setPlaybackState:"), 3); // stopped, not unknown
                artwork.update(None);
                return;
            };
            let mut keys = vec![
                MPMediaItemPropertyTitle,
                MPMediaItemPropertyArtist,
                MPMediaItemPropertyAlbumTitle,
                MPNowPlayingInfoPropertyElapsedPlaybackTime,
                MPNowPlayingInfoPropertyPlaybackRate,
            ];
            let mut values = vec![
                ns_string(&snapshot.title),
                ns_string(&snapshot.artist),
                ns_string(&snapshot.album),
            ];
            values.push(number_double(nonnegative_finite(snapshot.elapsed_seconds)));
            values.push(number_double(
                if snapshot.playback_state == PlaybackState::Playing {
                    1.0
                } else {
                    0.0
                },
            ));
            if let Some(duration) = snapshot
                .duration_seconds
                .filter(|v| v.is_finite() && *v > 0.0)
            {
                keys.push(MPMediaItemPropertyPlaybackDuration);
                values.push(number_double(duration));
            }
            if let Some(artwork) = artwork.update(snapshot.artwork.as_deref()) {
                keys.push(MPMediaItemPropertyArtwork);
                values.push(artwork);
            }
            let info = dictionary(&keys, &values);
            send_void_id(center, sel("setNowPlayingInfo:"), info);
            send_long(
                center,
                sel("setPlaybackState:"),
                match snapshot.playback_state {
                    PlaybackState::Playing => 1,
                    PlaybackState::Paused => 2,
                    PlaybackState::Stopped => 3,
                },
            );
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::ffi::CStr;
        use std::time::Duration;

        // Both framework centers are process-wide, including in a test binary.
        static TEST_LOCK: Mutex<()> = Mutex::new(());
        const PNG: &[u8] = include_bytes!("../../assets/original/settings.png");

        fn native_bridge() -> MediaBridge {
            let (commands, receiver) = mpsc::channel();
            MediaBridge {
                worker: start_native(commands),
                commands: receiver,
            }
        }

        fn flush(bridge: &MediaBridge) {
            let (ack, done) = mpsc::channel();
            bridge
                .worker
                .updates
                .send(WorkerMessage::Barrier(ack))
                .unwrap();
            done.recv_timeout(Duration::from_secs(10)).unwrap();
            assert!(!bridge.worker.thread.as_ref().unwrap().is_finished());
        }

        fn shutdown(mut bridge: MediaBridge) {
            let thread = bridge.worker.thread.take().unwrap();
            drop(bridge); // Exercise the same asynchronous cleanup as the UI.
            thread.join().unwrap();
        }

        fn snapshot(artwork: Option<Vec<u8>>) -> MediaSnapshot {
            MediaSnapshot {
                title: "Track\0Title".into(),
                artist: "Artist".into(),
                album: "Album".into(),
                artwork,
                duration_seconds: Some(180.0),
                elapsed_seconds: 12.5,
                playback_state: PlaybackState::Playing,
            }
        }

        unsafe fn info_center() -> Obj {
            unsafe { send_id(class("MPNowPlayingInfoCenter"), sel("defaultCenter")) }
        }

        unsafe fn info_value(key: Obj) -> Obj {
            let info = unsafe { send_id(info_center(), sel("nowPlayingInfo")) };
            unsafe { send_id_id(info, sel("objectForKey:"), key) }
        }

        unsafe fn string_value(key: Obj) -> String {
            let string = unsafe { info_value(key) };
            assert!(!string.is_null());
            let utf8 = unsafe { send_id(string, sel("UTF8String")) };
            unsafe { CStr::from_ptr(utf8 as *const c_char) }
                .to_str()
                .unwrap()
                .into()
        }

        unsafe fn playback_state() -> c_long {
            let send: unsafe extern "C" fn(Obj, Sel) -> c_long =
                unsafe { std::mem::transmute(objc_msgSend as *const ()) };
            unsafe { send(info_center(), sel("playbackState")) }
        }

        #[test]
        fn native_worker_publishes_metadata_artwork_and_clears_on_shutdown() {
            let _test = TEST_LOCK.lock().unwrap();
            let bridge = native_bridge();
            let mut track = snapshot(Some(PNG.to_vec()));
            bridge.publish(Some(track.clone()));
            flush(&bridge); // Assertions only run after the native worker finished.
            {
                let _lock = NATIVE_LOCK.lock().unwrap();
                let _pool = Pool::new();
                unsafe {
                    assert_eq!(string_value(MPMediaItemPropertyTitle), "Track�Title");
                    assert_eq!(string_value(MPMediaItemPropertyArtist), "Artist");
                    assert_eq!(string_value(MPMediaItemPropertyAlbumTitle), "Album");
                    assert_eq!(
                        send_return_double(
                            info_value(MPMediaItemPropertyPlaybackDuration),
                            sel("doubleValue")
                        ),
                        180.0
                    );
                    assert_eq!(
                        send_return_double(
                            info_value(MPNowPlayingInfoPropertyElapsedPlaybackTime),
                            sel("doubleValue")
                        ),
                        12.5
                    );
                    assert_eq!(
                        send_return_double(
                            info_value(MPNowPlayingInfoPropertyPlaybackRate),
                            sel("doubleValue")
                        ),
                        1.0
                    );
                    assert_eq!(playback_state(), 1);
                    let artwork = info_value(MPMediaItemPropertyArtwork);
                    assert!(!artwork.is_null());
                    let image_with_size: unsafe extern "C" fn(Obj, Sel, Size) -> Obj =
                        std::mem::transmute(objc_msgSend as *const ());
                    // The worker's autorelease pool has drained. The retained
                    // artwork block must still own a valid NSImage capture.
                    assert!(
                        !image_with_size(
                            artwork,
                            sel("imageWithSize:"),
                            Size {
                                width: 32.0,
                                height: 32.0
                            }
                        )
                        .is_null()
                    );
                }
            }
            track.playback_state = PlaybackState::Paused;
            bridge.publish(Some(track));
            flush(&bridge);
            {
                let _lock = NATIVE_LOCK.lock().unwrap();
                let _pool = Pool::new();
                unsafe {
                    assert_eq!(playback_state(), 2);
                    assert_eq!(
                        send_return_double(
                            info_value(MPNowPlayingInfoPropertyPlaybackRate),
                            sel("doubleValue")
                        ),
                        0.0
                    );
                }
            }
            assert_eq!(bridge.poll_command(), None);
            shutdown(bridge);
            let _lock = NATIVE_LOCK.lock().unwrap();
            let _pool = Pool::new();
            unsafe {
                assert!(send_id(info_center(), sel("nowPlayingInfo")).is_null());
                assert_eq!(playback_state(), 3);
            }
        }

        #[test]
        fn native_worker_omits_invalid_artwork_and_nonfinite_times() {
            let _test = TEST_LOCK.lock().unwrap();
            let bridge = native_bridge();
            let mut track = snapshot(Some(b"corrupt image".to_vec()));
            track.duration_seconds = Some(f64::INFINITY);
            track.elapsed_seconds = f64::NAN;
            bridge.publish(Some(track));
            flush(&bridge);
            {
                let _lock = NATIVE_LOCK.lock().unwrap();
                let _pool = Pool::new();
                unsafe {
                    assert_eq!(string_value(MPMediaItemPropertyArtist), "Artist");
                    assert!(info_value(MPMediaItemPropertyArtwork).is_null());
                    assert!(info_value(MPMediaItemPropertyPlaybackDuration).is_null());
                    assert_eq!(
                        send_return_double(
                            info_value(MPNowPlayingInfoPropertyElapsedPlaybackTime),
                            sel("doubleValue")
                        ),
                        0.0
                    );
                }
            }
            bridge.publish(None);
            flush(&bridge);
            {
                let _lock = NATIVE_LOCK.lock().unwrap();
                let _pool = Pool::new();
                unsafe {
                    assert!(send_id(info_center(), sel("nowPlayingInfo")).is_null());
                }
            }
            shutdown(bridge);
        }

        #[test]
        fn copied_remote_callbacks_dispatch_to_their_own_channels_and_release_captures() {
            let _test = TEST_LOCK.lock().unwrap();
            let _lock = NATIVE_LOCK.lock().unwrap();
            let (sender, receiver) = mpsc::channel();
            let route = CommandRoute::new(sender);
            let weak = Arc::downgrade(&route);
            ACTIVE_BRIDGE.store(route.id, Ordering::Release);
            for (code, expected) in [
                (0, RemoteCommand::Play),
                (1, RemoteCommand::Pause),
                (2, RemoteCommand::Toggle),
                (3, RemoteCommand::Next),
                (4, RemoteCommand::Previous),
            ] {
                let block = make_remote_block(code, &route);
                assert_eq!(Arc::strong_count(&route), 2);
                let native = block.0 as *mut RemoteBlock;
                assert_eq!(
                    unsafe { ((*native).invoke)(native, ptr::null_mut()) },
                    STATUS_SUCCESS
                );
                assert_eq!(receiver.try_recv().unwrap(), expected);
            }
            assert_eq!(Arc::strong_count(&route), 1);
            let block = make_remote_block(0, &route);
            let (new_sender, new_receiver) = mpsc::channel();
            let replacement = CommandRoute::new(new_sender);
            let new_block = make_remote_block(2, &replacement);
            ACTIVE_BRIDGE.store(replacement.id, Ordering::Release);
            unsafe {
                assert_eq!(
                    handle_remote(block.0 as *mut RemoteBlock, ptr::null_mut()),
                    STATUS_NO_ITEM
                );
                assert_eq!(
                    handle_remote(new_block.0 as *mut RemoteBlock, ptr::null_mut()),
                    STATUS_SUCCESS
                );
            }
            assert_eq!(new_receiver.try_recv().unwrap(), RemoteCommand::Toggle);
            assert!(receiver.try_recv().is_err());
            replacement.alive.store(false, Ordering::Release);
            assert_eq!(
                unsafe { handle_remote(new_block.0 as *mut RemoteBlock, ptr::null_mut()) },
                STATUS_FAILED
            );
            ACTIVE_BRIDGE.store(route.id, Ordering::Release);
            drop(receiver);
            assert_eq!(
                unsafe { handle_remote(block.0 as *mut RemoteBlock, ptr::null_mut()) },
                STATUS_FAILED
            );
            drop(route);
            assert!(weak.upgrade().is_some());
            drop(block);
            assert!(weak.upgrade().is_none());
            ACTIVE_BRIDGE.store(0, Ordering::Release);
        }

        #[test]
        fn native_registration_tokens_remove_handlers_and_release_the_route() {
            let _test = TEST_LOCK.lock().unwrap();
            let _lock = NATIVE_LOCK.lock().unwrap();
            let (sender, _receiver) = mpsc::channel();
            let route = CommandRoute::new(sender);
            let weak = Arc::downgrade(&route);
            {
                let _pool = Pool::new();
                let registrations = install_remote_commands(&route);
                assert_eq!(registrations.len(), 6);
                assert_eq!(Arc::strong_count(&route), 7);
                drop(registrations);
            }
            // A pool drain also releases autoreleased registration tokens.
            assert_eq!(Arc::strong_count(&route), 1);
            drop(route);
            assert!(weak.upgrade().is_none());
        }

        #[test]
        fn shutting_down_an_old_bridge_preserves_the_replacement_and_commands() {
            let _test = TEST_LOCK.lock().unwrap();
            let old = native_bridge();
            old.publish(Some(snapshot(None)));
            let replacement = native_bridge();
            let mut track = snapshot(None);
            track.title = "Replacement".into();
            replacement.publish(Some(track));
            flush(&replacement);
            shutdown(old);
            {
                let _lock = NATIVE_LOCK.lock().unwrap();
                let _pool = Pool::new();
                unsafe {
                    assert_eq!(string_value(MPMediaItemPropertyTitle), "Replacement");
                }
                let block = make_remote_block(0, &replacement.worker.route);
                assert_eq!(
                    unsafe { handle_remote(block.0 as *mut RemoteBlock, ptr::null_mut()) },
                    STATUS_SUCCESS
                );
            }
            assert_eq!(replacement.poll_command(), Some(RemoteCommand::Play));
            shutdown(replacement);
        }

        #[test]
        fn artwork_cache_reuses_images_and_clears_ownership() {
            let _test = TEST_LOCK.lock().unwrap();
            let _pool = Pool::new();
            let mut cache = ArtworkCache::default();
            let artwork = cache.update(Some(PNG)).unwrap();
            assert_eq!(cache.update(Some(PNG)), Some(artwork));
            assert_eq!(cache.update(Some(b"invalid")), None);
            assert_eq!(cache.update(Some(&[])), None);
            assert_eq!(cache.update(None), None);
            assert!(cache.artwork.is_none());
            assert!(cache.bytes.is_none());
        }

        #[test]
        fn seek_callback_reads_native_position_and_rejects_invalid_events() {
            let _test = TEST_LOCK.lock().unwrap();
            let _lock = NATIVE_LOCK.lock().unwrap();
            let _pool = Pool::new();
            static POSITION: AtomicU64 = AtomicU64::new(0);
            static EVENT_CLASS: std::sync::OnceLock<()> = std::sync::OnceLock::new();
            unsafe extern "C" fn position_time(_receiver: Obj, _selector: Sel) -> f64 {
                f64::from_bits(POSITION.load(Ordering::Relaxed))
            }
            unsafe extern "C" {
                fn objc_allocateClassPair(
                    superclass: Obj,
                    name: *const c_char,
                    extra: usize,
                ) -> Obj;
                fn objc_registerClassPair(class: Obj);
                fn class_addMethod(
                    class: Obj,
                    selector: Sel,
                    implementation: *const c_void,
                    types: *const c_char,
                ) -> bool;
            }
            // A test-only Objective-C event supplies the documented selector;
            // no private MediaPlayer event constructor or dispatch API is used.
            EVENT_CLASS.get_or_init(|| unsafe {
                let event_class = objc_allocateClassPair(
                    class("NSObject"),
                    c"NanoMediaTestPositionEvent".as_ptr(),
                    0,
                );
                assert!(!event_class.is_null());
                assert!(class_addMethod(
                    event_class,
                    sel("positionTime"),
                    position_time as *const c_void,
                    c"d@:".as_ptr()
                ));
                objc_registerClassPair(event_class);
            });
            let event = unsafe {
                OwnedObject(send_id(
                    send_id(class("NanoMediaTestPositionEvent"), sel("alloc")),
                    sel("init"),
                ))
            };
            let (sender, receiver) = mpsc::channel();
            let route = CommandRoute::new(sender);
            ACTIVE_BRIDGE.store(route.id, Ordering::Release);
            let block = make_remote_block(5, &route);
            for (position, expected) in [(42.25_f64, 42.25), (-3.0_f64, 0.0)] {
                POSITION.store(position.to_bits(), Ordering::Relaxed);
                assert_eq!(
                    unsafe { handle_remote(block.0 as *mut RemoteBlock, event.0) },
                    STATUS_SUCCESS
                );
                assert_eq!(
                    receiver.try_recv().unwrap(),
                    RemoteCommand::SeekTo(expected)
                );
            }
            for position in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                POSITION.store(position.to_bits(), Ordering::Relaxed);
                assert_eq!(
                    unsafe { handle_remote(block.0 as *mut RemoteBlock, event.0) },
                    STATUS_FAILED
                );
                assert!(receiver.try_recv().is_err());
            }
            assert_eq!(
                unsafe { handle_remote(block.0 as *mut RemoteBlock, ptr::null_mut()) },
                STATUS_FAILED
            );
            ACTIVE_BRIDGE.store(0, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_accepts_typed_snapshot_and_has_no_queued_command() {
        let bridge = MediaBridge::new();
        bridge.publish(Some(MediaSnapshot {
            title: "Track".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            artwork: None,
            duration_seconds: Some(180.0),
            elapsed_seconds: 1.0,
            playback_state: PlaybackState::Playing,
        }));
        assert_eq!(bridge.poll_command(), None);
    }
}
