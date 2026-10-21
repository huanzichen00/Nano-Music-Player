//! Process-owned macOS global shortcuts. Native callbacks only set flags;
//! the normal Slint timer focuses or hides the player on the main thread.

use std::{
    cell::Cell,
    ffi::{c_char, c_ulong, c_void},
    ptr,
};

const KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
const HOTKEY_PRESSED: u32 = 5;
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
const HOTKEY_ID_TYPE: u32 = u32::from_be_bytes(*b"hkid");
const NOT_HANDLED: i32 = -9874;
const FOCUS_ID: HotkeyId = HotkeyId {
    signature: u32::from_be_bytes(*b"Nano"),
    id: 1,
};
const HIDE_ID: HotkeyId = HotkeyId {
    signature: FOCUS_ID.signature,
    id: 2,
};

#[repr(C)]
struct EventType {
    class: u32,
    kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct HotkeyId {
    signature: u32,
    id: u32,
}

struct HotkeyContext {
    id: HotkeyId,
    pending: Cell<bool>,
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        callback: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32,
        count: c_ulong,
        types: *const EventType,
        user_data: *mut c_void,
        handler: *mut *mut c_void,
    ) -> i32;
    fn RemoveEventHandler(handler: *mut c_void) -> i32;
    fn RegisterEventHotKey(
        key: u32,
        modifiers: u32,
        id: HotkeyId,
        target: *mut c_void,
        options: u32,
        hotkey: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(hotkey: *mut c_void) -> i32;
    fn GetEventParameter(
        event: *mut c_void,
        name: u32,
        desired_type: u32,
        actual_type: *mut u32,
        size: c_ulong,
        actual_size: *mut c_ulong,
        data: *mut c_void,
    ) -> i32;
}

/// Raw native handles keep this guard !Send/!Sync: Carbon registration,
/// callbacks, polling and teardown all belong to the application's main thread.
pub(crate) struct GlobalHotkey {
    hotkey: *mut c_void,
    handler: *mut c_void,
    // Stable address even when the guard moves into the Slint timer closure.
    context: Box<HotkeyContext>,
}

impl GlobalHotkey {
    pub(crate) fn register_focus() -> Result<Self, String> {
        Self::register::<{ FOCUS_ID.id }>(1 << 11, "Option + R")
    }

    pub(crate) fn register_hide() -> Result<Self, String> {
        Self::register::<{ HIDE_ID.id }>((1 << 11) | (1 << 9), "Option + Shift + R")
    }

    fn register<const ID: u32>(modifiers: u32, label: &str) -> Result<Self, String> {
        // SAFETY: pthread_main_np has no preconditions.
        if unsafe { libc::pthread_main_np() } == 0 {
            return Err("Global shortcuts must be registered on the main thread".into());
        }
        let mut registration = Self {
            hotkey: ptr::null_mut(),
            handler: ptr::null_mut(),
            context: Box::new(HotkeyContext {
                id: HotkeyId {
                    signature: FOCUS_ID.signature,
                    id: ID,
                },
                pending: Cell::new(false),
            }),
        };
        let event_type = EventType {
            class: KEYBOARD,
            kind: HOTKEY_PRESSED,
        };
        // SAFETY: Called on the main thread. All out-pointers are valid, and
        // context stays allocated until after RemoveEventHandler in Drop.
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                // Carbon rejects installing the same callback twice on a target.
                // Each ID has a distinct monomorphized callback, including the ID
                // check below, so both registrations can coexist.
                on_hotkey::<ID>,
                1,
                &event_type,
                ptr::from_ref(registration.context.as_ref())
                    .cast_mut()
                    .cast(),
                &mut registration.handler,
            )
        };
        if status != 0 {
            return Err(format!(
                "Could not install global shortcut handler ({status})"
            ));
        }
        // kVK_ANSI_R and kEventHotKeyExclusive. Modifiers use Carbon's optionKey
        // and shiftKey bits. Each chord has its own guard, so a conflict on one
        // doesn't disable the other. Exclusive registration
        // reports conflicts instead of also triggering another application's key.
        // SAFETY: Same main-thread lifetime as the installed handler above.
        let status = unsafe {
            RegisterEventHotKey(
                15,
                modifiers,
                registration.context.id,
                GetApplicationEventTarget(),
                1,
                &mut registration.hotkey,
            )
        };
        if status != 0 {
            return Err(format!(
                "Could not register {label} ({status}); another application may be using it"
            ));
        }
        Ok(registration)
    }

    pub(crate) fn take_pending(&self) -> bool {
        self.context.pending.replace(false)
    }
}

impl Drop for GlobalHotkey {
    fn drop(&mut self) {
        // SAFETY: The !Send guard stays on its registration thread. Stop events
        // and detach the handler before its boxed user data is released. This
        // also cleans up a handler if registering the key itself failed.
        unsafe {
            if !self.hotkey.is_null() {
                UnregisterEventHotKey(self.hotkey);
            }
            if !self.handler.is_null() {
                RemoveEventHandler(self.handler);
            }
        }
    }
}

unsafe extern "C" fn on_hotkey<const ID: u32>(
    _handler: *mut c_void,
    event: *mut c_void,
    user_data: *mut c_void,
) -> i32 {
    // SAFETY: Our installed handler supplies its live, stable boxed context.
    let context = unsafe { &*user_data.cast::<HotkeyContext>() };
    let mut id = HotkeyId::default();
    // SAFETY: Carbon supplies the event; id is a correctly sized output buffer.
    let status = unsafe {
        GetEventParameter(
            event,
            DIRECT_OBJECT,
            HOTKEY_ID_TYPE,
            ptr::null_mut(),
            size_of::<HotkeyId>() as c_ulong,
            ptr::null_mut(),
            ptr::from_mut(&mut id).cast(),
        )
    };
    if status != 0 || id != context.id || id.id != ID {
        return NOT_HANDLED;
    }
    // No UI calls or event-loop re-entry occur while borrowing the context.
    context.pending.set(true);
    0
}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

/// Use AppKit's Hide command, keeping Slint's window and event loop alive so
/// playback continues and the global focus shortcut remains registered.
pub(crate) fn set_application_hidden(hidden: bool) {
    // SAFETY: This function is only called by the main-thread Slint timer.
    // Selectors and typed objc_msgSend signatures match NSApplication's API.
    unsafe {
        let get_app: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as *const ());
        let app = get_app(
            objc_getClass(c"NSApplication".as_ptr()),
            sel_registerName(c"sharedApplication".as_ptr()),
        );
        let send: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
            std::mem::transmute(objc_msgSend as *const ());
        send(
            app,
            sel_registerName(if hidden { c"hide:" } else { c"unhide:" }.as_ptr()),
            ptr::null_mut(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" {
        fn CreateEvent(
            allocator: *mut c_void,
            class: u32,
            kind: u32,
            time: f64,
            attributes: u32,
            event: *mut *mut c_void,
        ) -> i32;
        fn SetEventParameter(
            event: *mut c_void,
            name: u32,
            kind: u32,
            size: c_ulong,
            data: *const c_void,
        ) -> i32;
        fn ReleaseEvent(event: *mut c_void);
    }

    fn deliver(id: Option<HotkeyId>, context: &HotkeyContext) -> i32 {
        let mut event = ptr::null_mut();
        // SAFETY: Use a real Carbon event to exercise parameter decoding. The
        // callback borrows a live Cell synchronously; the event is then released.
        unsafe {
            assert_eq!(
                CreateEvent(
                    ptr::null_mut(),
                    KEYBOARD,
                    HOTKEY_PRESSED,
                    0.0,
                    0,
                    &mut event
                ),
                0
            );
            if let Some(id) = id {
                assert_eq!(
                    SetEventParameter(
                        event,
                        DIRECT_OBJECT,
                        HOTKEY_ID_TYPE,
                        size_of::<HotkeyId>() as c_ulong,
                        ptr::from_ref(&id).cast(),
                    ),
                    0
                );
            }
            let callback = if context.id == HIDE_ID {
                on_hotkey::<2>
            } else {
                on_hotkey::<1>
            };
            let status = callback(
                ptr::null_mut(),
                event,
                ptr::from_ref(context).cast_mut().cast(),
            );
            ReleaseEvent(event);
            status
        }
    }

    #[test]
    fn ignores_other_hotkeys_and_missing_parameters() {
        let context = HotkeyContext {
            id: FOCUS_ID,
            pending: Cell::new(false),
        };
        for id in [
            None,
            Some(HotkeyId {
                signature: u32::from_be_bytes(*b"Else"),
                id: FOCUS_ID.id,
            }),
            Some(HotkeyId {
                signature: FOCUS_ID.signature,
                id: 2,
            }),
        ] {
            assert_eq!(deliver(id, &context), NOT_HANDLED);
            assert!(!context.pending.get());
        }
    }

    #[test]
    fn repeated_presses_coalesce_into_one_focus_request() {
        let hotkey = GlobalHotkey {
            hotkey: ptr::null_mut(),
            handler: ptr::null_mut(),
            context: Box::new(HotkeyContext {
                id: FOCUS_ID,
                pending: Cell::new(false),
            }),
        };
        assert!(!hotkey.take_pending());
        assert_eq!(deliver(Some(FOCUS_ID), &hotkey.context), 0);
        assert_eq!(deliver(Some(FOCUS_ID), &hotkey.context), 0);
        assert!(hotkey.take_pending());
        assert!(!hotkey.take_pending());
    }

    #[test]
    fn focus_and_hide_requests_are_independent() {
        let focus = HotkeyContext {
            id: FOCUS_ID,
            pending: Cell::new(false),
        };
        let hide = HotkeyContext {
            id: HIDE_ID,
            pending: Cell::new(false),
        };
        assert_eq!(deliver(Some(HIDE_ID), &focus), NOT_HANDLED);
        assert_eq!(deliver(Some(FOCUS_ID), &hide), NOT_HANDLED);
        assert!(!focus.pending.get());
        assert!(!hide.pending.get());
        assert_eq!(deliver(Some(HIDE_ID), &hide), 0);
        assert!(hide.pending.get());
        assert!(!focus.pending.get());
        hide.pending.set(false);
        assert_eq!(deliver(Some(FOCUS_ID), &focus), 0);
        assert!(focus.pending.get());
        assert!(!hide.pending.get());
    }
}
