//! iOS services with a desktop implementation for the mobile preview.
//!
//! Native callbacks only enqueue events. The bridge and UIKit remain on the
//! main thread; the mobile controller owns playback and applies these events.

use super::macos_media::{MediaSnapshot, RemoteCommand};
use std::sync::mpsc::{self, Receiver};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
};

#[derive(Debug, PartialEq)]
pub enum Event {
    FilesSelected(Vec<PathBuf>),
    ImportFinished {
        imported: usize,
        errors: Vec<String>,
    },
    Remote(RemoteCommand),
    InterruptionBegan {
        epoch: u64,
    },
    InterruptionEnded {
        epoch: u64,
        should_resume: bool,
    },
    OutputDisconnected,
    EnteredBackground,
    TransferPaused,
    TransferResumed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

pub struct Bridge {
    events: Receiver<Event>,
    activated_through: Cell<Option<u64>>,
    #[cfg(target_os = "ios")]
    native: native::Native,
    #[cfg(not(target_os = "ios"))]
    sender: mpsc::Sender<Event>,
}

impl Bridge {
    pub fn new() -> Self {
        let (sender, events) = mpsc::channel();
        Self {
            events,
            activated_through: Cell::new(None),
            #[cfg(target_os = "ios")]
            native: native::Native::new(sender),
            #[cfg(not(target_os = "ios"))]
            sender,
        }
    }

    pub fn poll_event(&self) -> Option<Event> {
        loop {
            let event = self.events.try_recv().ok()?;
            let epoch = match event {
                Event::InterruptionBegan { epoch } | Event::InterruptionEnded { epoch, .. } => {
                    Some(epoch)
                }
                _ => None,
            };
            if epoch
                .zip(self.activated_through.get())
                .is_some_and(|(epoch, activated)| epoch <= activated)
            {
                continue;
            }
            return Some(event);
        }
    }

    /// iOS copies into `destination` before reporting ImportFinished. The
    /// preview reports paths so the portable storage importer can copy them.
    pub fn pick_music(&self, destination: &Path) -> Result<(), String> {
        #[cfg(target_os = "ios")]
        {
            self.native.pick_music(destination)
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = destination;
            let sender = self.sender.clone();
            std::thread::Builder::new()
                .name("mobile-file-picker".into())
                .spawn(move || {
                    let files = rfd::FileDialog::new()
                        .set_title("Import music")
                        .add_filter(
                            "Audio",
                            &["mp3", "flac", "m4a", "aac", "ogg", "wav", "aiff"],
                        )
                        .pick_files()
                        .unwrap_or_default();
                    let _ = sender.send(Event::FilesSelected(files));
                })
                .map(|_| ())
                .map_err(|error| format!("Cannot open music picker: {error}"))
        }
    }

    /// Activate AVAudioSession before opening the output stream or resuming.
    pub fn prepare_playback(&self) -> Result<(), String> {
        #[cfg(target_os = "ios")]
        {
            let epoch = self.native.prepare_playback()?;
            self.activated_through.set(Some(epoch));
            Ok(())
        }
        #[cfg(not(target_os = "ios"))]
        Ok(())
    }

    pub fn publish(&self, snapshot: Option<MediaSnapshot>) {
        #[cfg(target_os = "ios")]
        self.native.publish(snapshot);
        #[cfg(not(target_os = "ios"))]
        let _ = snapshot;
    }

    /// Keep the foreground transfer page awake. UIKit grants only a finite
    /// allowance when inactive; suspension keeps the same transfer session.
    pub fn set_transfer_active(&self, active: bool) {
        #[cfg(target_os = "ios")]
        self.native.set_transfer_active(active);
        #[cfg(not(target_os = "ios"))]
        let _ = active;
    }
}

impl Default for Bridge {
    fn default() -> Self {
        Self::new()
    }
}

pub fn documents_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "ios")]
    {
        native::documents_dir()
    }
    #[cfg(not(target_os = "ios"))]
    {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if let Some(index) = args.iter().position(|arg| arg == "--config-dir") {
            return args
                .get(index + 1)
                .map(PathBuf::from)
                .ok_or_else(|| "--config-dir needs a directory".into());
        }
        directories::ProjectDirs::from("", "Nano", "Nano Music Player Mobile")
            .map(|dirs| dirs.data_local_dir().to_owned())
            .ok_or_else(|| "Cannot locate mobile preview data directory".into())
    }
}

pub fn safe_area() -> Insets {
    #[cfg(target_os = "ios")]
    {
        native::safe_area()
    }
    #[cfg(not(target_os = "ios"))]
    Insets::default()
}

/// Bottom keyboard occlusion in window logical points, including the home
/// indicator area. Subtract the existing bottom safe inset only in UI layout.
pub fn keyboard_height() -> f32 {
    #[cfg(target_os = "ios")]
    {
        native::keyboard_height()
    }
    #[cfg(not(target_os = "ios"))]
    0.0
}

/// Write text from a UI callback without reading the user's clipboard.
pub fn copy_text(window: &slint::Window, text: &str) -> Result<(), String> {
    #[cfg(target_os = "ios")]
    {
        let _ = window;
        native::copy_text(text)
    }
    #[cfg(not(target_os = "ios"))]
    {
        // Slint 1.18.1 exposes clipboard writes through its platform trait.
        // Borrow this window's existing context rather than creating another
        // backend or window. This private re-export is tied to our pinned Slint.
        let context = slint::private_unstable_api::re_exports::WindowInner::from_pub(window)
            .try_context()
            .ok_or_else(|| {
                "The clipboard is unavailable until the window is initialized".to_owned()
            })?;
        context
            .platform()
            .set_clipboard_text(text, slint::platform::Clipboard::DefaultClipboard);
        Ok(())
    }
}

/// Read text only in response to the user's explicit Paste action.
pub fn paste_text(window: &slint::Window) -> Result<String, String> {
    #[cfg(target_os = "ios")]
    {
        let _ = window;
        native::paste_text()
    }
    #[cfg(not(target_os = "ios"))]
    {
        let context = slint::private_unstable_api::re_exports::WindowInner::from_pub(window)
            .try_context()
            .ok_or_else(|| {
                "The clipboard is unavailable until the window is initialized".to_owned()
            })?;
        context
            .platform()
            .clipboard_text(slint::platform::Clipboard::DefaultClipboard)
            .ok_or_else(|| "The clipboard has no text or paste access was denied".to_owned())
    }
}

/// Foreground status without waiting for Slint to process lifecycle events.
/// Transfers use `transfer_allowed` to include UIKit's finite background time.
pub fn is_foreground() -> bool {
    #[cfg(target_os = "ios")]
    {
        native::is_foreground()
    }
    #[cfg(not(target_os = "ios"))]
    true
}

/// Whether UIKit currently permits foreground or finite background transfer.
pub fn transfer_allowed() -> bool {
    #[cfg(target_os = "ios")]
    {
        native::transfer_allowed()
    }
    #[cfg(not(target_os = "ios"))]
    true
}

/// Changes when a background allowance expires or the transfer is stopped.
/// Only in-flight requests capture it; a paused session keeps its address.
pub fn background_epoch() -> u64 {
    #[cfg(target_os = "ios")]
    {
        native::background_epoch()
    }
    #[cfg(not(target_os = "ios"))]
    0
}

// Also compiled for tests so malformed native events can be checked without
// opening a picker, registering system handlers, or activating audio.
#[cfg(any(target_os = "ios", test))]
fn parse_event(bytes: &[u8]) -> Option<Event> {
    let payload: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    match payload.get("type")?.as_str()? {
        "import_finished" => Some(Event::ImportFinished {
            imported: usize::try_from(payload.get("imported")?.as_u64()?).ok()?,
            errors: payload
                .get("errors")?
                .as_array()?
                .iter()
                .map(|error| error.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()?,
        }),
        "remote" => {
            let command = match payload.get("command")?.as_str()? {
                "play" => RemoteCommand::Play,
                "pause" => RemoteCommand::Pause,
                "toggle" => RemoteCommand::Toggle,
                "next" => RemoteCommand::Next,
                "previous" => RemoteCommand::Previous,
                "seek" => {
                    let position = payload.get("position")?.as_f64()?;
                    if !position.is_finite() {
                        return None;
                    }
                    RemoteCommand::SeekTo(position.max(0.0))
                }
                _ => return None,
            };
            Some(Event::Remote(command))
        }
        "interruption_began" => {
            let epoch = payload.get("epoch")?.as_u64()?;
            crate::shared::diagnostics::record(
                "native_interruption_began",
                serde_json::json!({
                    "epoch": epoch, "suspended": payload.get("suspended").and_then(serde_json::Value::as_bool),
                    "reason": payload.get("reason").and_then(serde_json::Value::as_u64)
                }),
            );
            Some(Event::InterruptionBegan { epoch })
        }
        "interruption_ended" => Some(Event::InterruptionEnded {
            epoch: payload.get("epoch")?.as_u64()?,
            should_resume: payload.get("should_resume")?.as_bool()?,
        }),
        "output_disconnected" => Some(Event::OutputDisconnected),
        "entered_background" => Some(Event::EnteredBackground),
        "transfer_paused" => Some(Event::TransferPaused),
        "transfer_resumed" => Some(Event::TransferResumed),
        _ => None,
    }
}

#[cfg(target_os = "ios")]
mod native {
    use super::*;
    use crate::platform::macos_media::PlaybackState;
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::marker::PhantomData;
    use std::ptr::NonNull;
    use std::rc::Rc;

    unsafe extern "C" {
        fn nm_bridge_create(
            context: *mut c_void,
            callback: unsafe extern "C" fn(*mut c_void, *const c_char),
            release: unsafe extern "C" fn(*mut c_void),
        ) -> *mut c_void;
        fn nm_bridge_destroy(bridge: *mut c_void);
        fn nm_bridge_pick_music(bridge: *mut c_void, destination: *const c_char) -> *mut c_char;
        fn nm_bridge_prepare_playback(bridge: *mut c_void, covered_epoch: *mut u64) -> *mut c_char;
        fn nm_bridge_set_transfer_active(bridge: *mut c_void, active: bool);
        fn nm_bridge_publish(
            bridge: *mut c_void,
            metadata: *const c_char,
            artwork: *const u8,
            artwork_length: usize,
        );
        fn nm_documents_dir() -> *mut c_char;
        fn nm_copy_text(text: *const c_char) -> *mut c_char;
        fn nm_paste_text(error: *mut *mut c_char) -> *mut c_char;
        fn nm_safe_area(top: *mut f32, right: *mut f32, bottom: *mut f32, left: *mut f32);
        fn nm_keyboard_height() -> f32;
        fn nm_is_foreground() -> bool;
        fn nm_transfer_allowed() -> bool;
        fn nm_background_epoch() -> u64;
        fn nm_string_free(string: *mut c_char);
    }

    pub(super) struct Native {
        handle: Option<NonNull<c_void>>,
        // Native UI ownership stays with the controller on the main thread.
        _main_thread: PhantomData<Rc<()>>,
    }

    unsafe extern "C" fn event_callback(context: *mut c_void, json: *const c_char) {
        if context.is_null() || json.is_null() {
            return;
        }
        // NativeBridge retains this box and invokes/relinquishes it only on
        // the main queue. The borrowed JSON string lives through this call.
        let sender = unsafe { &*context.cast::<mpsc::Sender<Event>>() };
        let bytes = unsafe { CStr::from_ptr(json) }.to_bytes();
        if let Some(event) = parse_event(bytes) {
            let _ = sender.send(event);
        }
    }

    unsafe extern "C" fn release_context(context: *mut c_void) {
        if !context.is_null() {
            drop(unsafe { Box::from_raw(context.cast::<mpsc::Sender<Event>>()) });
        }
    }

    fn take_string(pointer: *mut c_char) -> Option<String> {
        if pointer.is_null() {
            return None;
        }
        let text = unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned();
        unsafe { nm_string_free(pointer) };
        Some(text)
    }

    impl Native {
        pub(super) fn new(sender: mpsc::Sender<Event>) -> Self {
            let context = Box::into_raw(Box::new(sender)).cast::<c_void>();
            // Ownership of context transfers even if native creation fails.
            let handle = unsafe { nm_bridge_create(context, event_callback, release_context) };
            Self {
                handle: NonNull::new(handle),
                _main_thread: PhantomData,
            }
        }

        fn handle(&self) -> Result<*mut c_void, String> {
            self.handle
                .map(NonNull::as_ptr)
                .ok_or_else(|| "Cannot create iOS service bridge".into())
        }

        pub(super) fn pick_music(&self, destination: &Path) -> Result<(), String> {
            use std::os::unix::ffi::OsStrExt;
            let path = CString::new(destination.as_os_str().as_bytes())
                .map_err(|_| "Music directory contains a NUL byte".to_owned())?;
            match take_string(unsafe { nm_bridge_pick_music(self.handle()?, path.as_ptr()) }) {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }

        pub(super) fn prepare_playback(&self) -> Result<u64, String> {
            let mut epoch = 0;
            match take_string(unsafe { nm_bridge_prepare_playback(self.handle()?, &mut epoch) }) {
                Some(error) => Err(error),
                None => Ok(epoch),
            }
        }

        pub(super) fn set_transfer_active(&self, active: bool) {
            if let Some(handle) = self.handle {
                unsafe { nm_bridge_set_transfer_active(handle.as_ptr(), active) };
            }
        }

        pub(super) fn publish(&self, snapshot: Option<MediaSnapshot>) {
            let Ok(handle) = self.handle() else { return };
            let Some(snapshot) = snapshot else {
                unsafe { nm_bridge_publish(handle, std::ptr::null(), std::ptr::null(), 0) };
                return;
            };
            let duration = snapshot
                .duration_seconds
                .filter(|value| value.is_finite() && *value > 0.0);
            let elapsed = if snapshot.elapsed_seconds.is_finite() {
                snapshot.elapsed_seconds.max(0.0)
            } else {
                0.0
            };
            let metadata = serde_json::json!({
                "title": snapshot.title,
                "artist": snapshot.artist,
                "album": snapshot.album,
                "duration": duration,
                "elapsed": elapsed,
                "state": match snapshot.playback_state {
                    PlaybackState::Playing => "playing",
                    PlaybackState::Paused => "paused",
                    PlaybackState::Stopped => "stopped",
                },
            });
            let Ok(json) = CString::new(metadata.to_string()) else {
                return;
            };
            let artwork = snapshot.artwork.as_deref().unwrap_or_default();
            unsafe { nm_bridge_publish(handle, json.as_ptr(), artwork.as_ptr(), artwork.len()) };
        }
    }

    impl Drop for Native {
        fn drop(&mut self) {
            if let Some(handle) = self.handle.take() {
                unsafe { nm_bridge_destroy(handle.as_ptr()) };
            }
        }
    }

    pub(super) fn documents_dir() -> Result<PathBuf, String> {
        take_string(unsafe { nm_documents_dir() })
            .map(PathBuf::from)
            .ok_or_else(|| "Cannot locate iOS Documents directory".into())
    }

    pub(super) fn copy_text(text: &str) -> Result<(), String> {
        let text =
            CString::new(text).map_err(|_| "Clipboard text contains a NUL byte".to_owned())?;
        match take_string(unsafe { nm_copy_text(text.as_ptr()) }) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(super) fn paste_text() -> Result<String, String> {
        let mut error = std::ptr::null_mut();
        let text = take_string(unsafe { nm_paste_text(&mut error) });
        match take_string(error) {
            Some(error) => Err(error),
            None => text.ok_or_else(|| "Clipboard text could not be copied".to_owned()),
        }
    }

    pub(super) fn safe_area() -> Insets {
        let mut insets = Insets::default();
        unsafe {
            nm_safe_area(
                &mut insets.top,
                &mut insets.right,
                &mut insets.bottom,
                &mut insets.left,
            )
        };
        insets
    }

    pub(super) fn keyboard_height() -> f32 {
        unsafe { nm_keyboard_height() }
    }

    pub(super) fn is_foreground() -> bool {
        // Native code uses a C11 atomic, so this never touches UIKit off-thread.
        unsafe { nm_is_foreground() }
    }

    pub(super) fn background_epoch() -> u64 {
        unsafe { nm_background_epoch() }
    }

    pub(super) fn transfer_allowed() -> bool {
        unsafe { nm_transfer_allowed() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_events_parse_import_results_and_cancellation() {
        assert_eq!(
            parse_event(
                br#"{"type":"import_finished","imported":2,"errors":["Cloud file unavailable"]}"#
            ),
            Some(Event::ImportFinished {
                imported: 2,
                errors: vec!["Cloud file unavailable".into()]
            })
        );
        assert_eq!(
            parse_event(br#"{"type":"import_finished","imported":0,"errors":[]}"#),
            Some(Event::ImportFinished {
                imported: 0,
                errors: vec![]
            })
        );
        assert!(parse_event(br#"{"type":"import_finished","imported":-1,"errors":[]}"#).is_none());
        assert!(parse_event(br#"{"type":"import_finished","imported":0,"errors":[1]}"#).is_none());
    }

    #[test]
    fn native_events_validate_remote_commands_and_interruptions() {
        assert_eq!(
            parse_event(br#"{"type":"remote","command":"seek","position":-2}"#),
            Some(Event::Remote(RemoteCommand::SeekTo(0.0)))
        );
        assert_eq!(
            parse_event(br#"{"type":"interruption_ended","epoch":3,"should_resume":false}"#),
            Some(Event::InterruptionEnded {
                epoch: 3,
                should_resume: false
            })
        );
        assert_eq!(
            parse_event(br#"{"type":"entered_background"}"#),
            Some(Event::EnteredBackground)
        );
        assert_eq!(
            parse_event(br#"{"type":"transfer_paused"}"#),
            Some(Event::TransferPaused)
        );
        assert_eq!(
            parse_event(br#"{"type":"transfer_resumed"}"#),
            Some(Event::TransferResumed)
        );
        assert!(parse_event(br#"{"type":"remote","command":"seek","position":null}"#).is_none());
        assert!(parse_event(br#"{"type":"interruption_ended"}"#).is_none());
        assert!(parse_event(br#"{"type":"unknown"}"#).is_none());
        assert!(parse_event(b"not json").is_none());
    }

    #[test]
    fn successful_activation_filters_old_interruptions_but_keeps_new_interruptions_and_user_pause()
    {
        let bridge = Bridge::new();
        bridge.activated_through.set(Some(4));
        bridge
            .sender
            .send(Event::InterruptionBegan { epoch: 4 })
            .unwrap();
        bridge
            .sender
            .send(Event::InterruptionEnded {
                epoch: 4,
                should_resume: true,
            })
            .unwrap();
        bridge
            .sender
            .send(Event::Remote(RemoteCommand::Pause))
            .unwrap();
        bridge
            .sender
            .send(Event::InterruptionBegan { epoch: 5 })
            .unwrap();
        assert_eq!(
            bridge.poll_event(),
            Some(Event::Remote(RemoteCommand::Pause))
        );
        assert_eq!(
            bridge.poll_event(),
            Some(Event::InterruptionBegan { epoch: 5 })
        );
        assert!(bridge.poll_event().is_none());
        let failed_activation = Bridge::new();
        failed_activation
            .sender
            .send(Event::InterruptionBegan { epoch: 1 })
            .unwrap();
        assert_eq!(
            failed_activation.poll_event(),
            Some(Event::InterruptionBegan { epoch: 1 })
        );
        assert!(parse_event(br#"{"type":"interruption_began","epoch":-1}"#).is_none());
        assert!(parse_event(br#"{"type":"interruption_began"}"#).is_none());
    }
}
