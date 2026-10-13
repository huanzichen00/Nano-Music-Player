//! System media integration boundary.
//!
//! The controller publishes snapshots and polls typed commands. On macOS all
//! MediaPlayer/Objective-C work is serialized on a private thread so calls do
//! not run from a Slint callback. Other platforms get the same API as a no-op.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

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
    updates: Sender<WorkerMessage>,
    commands: Receiver<RemoteCommand>,
}

impl MediaBridge {
    pub fn new() -> Self {
        let (command_tx, commands) = mpsc::channel();
        #[cfg(target_os = "macos")]
        let updates = macos::start(command_tx);
        #[cfg(not(target_os = "macos"))]
        let _ = command_tx;

        Self {
            #[cfg(target_os = "macos")]
            updates,
            commands,
        }
    }

    pub fn publish(&self, snapshot: Option<MediaSnapshot>) {
        #[cfg(target_os = "macos")]
        {
            let _ = self.updates.send(WorkerMessage::Snapshot(snapshot));
        }
        #[cfg(not(target_os = "macos"))]
        let _ = snapshot;
    }

    pub fn poll_command(&self) -> Option<RemoteCommand> {
        match self.commands.try_recv() {
            Ok(command) => Some(command),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
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
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::ffi::{CString, c_char, c_double, c_int, c_long, c_void};
    use std::ptr;
    use std::sync::{Mutex, OnceLock};
    use std::thread;

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
        static _NSConcreteGlobalBlock: c_void;
    }

    #[link(name = "Foundation", kind = "framework")]
    unsafe extern "C" {}
    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}
    #[link(name = "MediaPlayer", kind = "framework")]
    unsafe extern "C" {}

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

    unsafe fn dictionary(keys: &[&str], values: &[Obj]) -> Obj {
        let key_objects: Vec<Obj> = keys.iter().map(|key| unsafe { ns_string(key) }).collect();
        let array_class = class("NSArray");
        let keys_array = unsafe {
            let f: unsafe extern "C" fn(Obj, Sel, *const Obj, c_long) -> Obj =
                std::mem::transmute(objc_msgSend as *const ());
            f(
                array_class,
                sel("arrayWithObjects:count:"),
                key_objects.as_ptr(),
                key_objects.len() as c_long,
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
    }

    #[repr(C)]
    struct RemoteBlock {
        isa: *const c_void,
        flags: c_int,
        reserved: c_int,
        invoke: unsafe extern "C" fn(*mut RemoteBlock, Obj) -> c_long,
        descriptor: *const BlockDescriptor,
        command: c_int,
    }
    unsafe impl Sync for RemoteBlock {}

    static BLOCK_DESCRIPTOR: BlockDescriptor = BlockDescriptor {
        reserved: 0,
        size: std::mem::size_of::<RemoteBlock>(),
    };
    static COMMAND_SENDER: OnceLock<Mutex<Sender<RemoteCommand>>> = OnceLock::new();

    unsafe extern "C" fn handle_remote(block: *mut RemoteBlock, event: Obj) -> c_long {
        let code = unsafe { (*block).command };
        let command = match code {
            0 => RemoteCommand::Play,
            1 => RemoteCommand::Pause,
            2 => RemoteCommand::Toggle,
            3 => RemoteCommand::Next,
            4 => RemoteCommand::Previous,
            5 => {
                let time = unsafe { send_return_double(event, sel("positionTime")) };
                RemoteCommand::SeekTo(time.max(0.0))
            }
            _ => return 1,
        };
        if let Some(sender) = COMMAND_SENDER.get() {
            if sender
                .lock()
                .map(|sender| sender.send(command).is_ok())
                .unwrap_or(false)
            {
                return 0; // MPRemoteCommandHandlerStatusSuccess
            }
        }
        1 // MPRemoteCommandHandlerStatusCommandFailed
    }

    fn make_block(command: c_int) -> &'static RemoteBlock {
        let isa = std::ptr::addr_of!(_NSConcreteGlobalBlock);
        Box::leak(Box::new(RemoteBlock {
            isa,
            flags: 1, // BLOCK_IS_GLOBAL
            reserved: 0,
            invoke: handle_remote,
            descriptor: &BLOCK_DESCRIPTOR,
            command,
        }))
    }

    fn install_remote_commands() {
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
                let _ = send_id_id(
                    remote,
                    sel("addTargetWithHandler:"),
                    make_block(command) as *const _ as Obj,
                );
                send_bool(remote, sel("setEnabled:"), true);
            }
        }
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

    pub(super) fn start(commands: Sender<RemoteCommand>) -> Sender<WorkerMessage> {
        let (updates, receiver) = mpsc::channel();
        let _ = COMMAND_SENDER.set(Mutex::new(commands));
        thread::Builder::new()
            .name("macos-media-bridge".into())
            .spawn(move || {
                let _pool = Pool::new();
                install_remote_commands();
                while let Ok(WorkerMessage::Snapshot(snapshot)) = receiver.recv() {
                    let _pool = Pool::new();
                    publish_native(snapshot.as_ref());
                }
            })
            .expect("start macOS media worker");
        updates
    }

    fn publish_native(snapshot: Option<&MediaSnapshot>) {
        unsafe {
            let center = send_id(class("MPNowPlayingInfoCenter"), sel("defaultCenter"));
            let Some(snapshot) = snapshot else {
                send_void_id(center, sel("setNowPlayingInfo:"), ptr::null_mut());
                send_long(center, sel("setPlaybackState:"), 0);
                return;
            };
            let mut keys = vec![
                "title",
                "artist",
                "albumTitle",
                "playbackDuration",
                "elapsedPlaybackTime",
                "playbackRate",
            ];
            let mut values = vec![
                ns_string(&snapshot.title),
                ns_string(&snapshot.artist),
                ns_string(&snapshot.album),
            ];
            values.push(number_double(snapshot.duration_seconds.unwrap_or(0.0)));
            values.push(number_double(snapshot.elapsed_seconds.max(0.0)));
            values.push(number_double(
                if snapshot.playback_state == PlaybackState::Playing {
                    1.0
                } else {
                    0.0
                },
            ));
            if let Some(bytes) = snapshot.artwork.as_deref() {
                let data = {
                    let f: unsafe extern "C" fn(Obj, Sel, *const u8, usize) -> Obj =
                        std::mem::transmute(objc_msgSend as *const ());
                    f(
                        class("NSData"),
                        sel("dataWithBytes:length:"),
                        bytes.as_ptr(),
                        bytes.len(),
                    )
                };
                let image = send_id_id(class("NSImage"), sel("imageWithData:"), data);
                let artwork = send_id(class("MPMediaItemArtwork"), sel("alloc"));
                let artwork = send_id_id(artwork, sel("initWithImage:"), image);
                keys.push("artwork");
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
                    PlaybackState::Stopped => 0,
                },
            );
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
