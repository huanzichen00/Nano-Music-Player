//! A process-owned macOS global shortcut. Native callbacks only set a flag;
//! the normal Slint timer restores and focuses the player on the main thread.

use std::{
    cell::Cell,
    ffi::{c_ulong, c_void},
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

#[repr(C)]
struct EventType {
    class: u32,
    kind: u32,
}

#[repr(C)]
#[derive(Default, PartialEq, Eq)]
struct HotkeyId {
    signature: u32,
    id: u32,
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
pub(crate) struct FocusHotkey {
    hotkey: *mut c_void,
    handler: *mut c_void,
    // Stable address even when the guard moves into the Slint timer closure.
    pending: Box<Cell<bool>>,
}

impl FocusHotkey {
    pub(crate) fn register() -> Result<Self, String> {
        // SAFETY: pthread_main_np has no preconditions.
        if unsafe { libc::pthread_main_np() } == 0 {
            return Err("Global shortcuts must be registered on the main thread".into());
        }
        let mut registration = Self {
            hotkey: ptr::null_mut(),
            handler: ptr::null_mut(),
            pending: Box::new(Cell::new(false)),
        };
        let event_type = EventType {
            class: KEYBOARD,
            kind: HOTKEY_PRESSED,
        };
        // SAFETY: Called on the main thread. All out-pointers are valid, and
        // pending stays allocated until after RemoveEventHandler in Drop.
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                on_hotkey,
                1,
                &event_type,
                ptr::from_ref(registration.pending.as_ref())
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
        // kVK_ANSI_R, optionKey, kEventHotKeyExclusive. Exclusive registration
        // reports conflicts instead of also triggering another application's key.
        // SAFETY: Same main-thread lifetime as the installed handler above.
        let status = unsafe {
            RegisterEventHotKey(
                15,
                1 << 11,
                FOCUS_ID,
                GetApplicationEventTarget(),
                1,
                &mut registration.hotkey,
            )
        };
        if status != 0 {
            return Err(format!(
                "Could not register Option + R ({status}); another application may be using it"
            ));
        }
        Ok(registration)
    }

    pub(crate) fn take_pending(&self) -> bool {
        self.pending.replace(false)
    }
}

impl Drop for FocusHotkey {
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

unsafe extern "C" fn on_hotkey(
    _handler: *mut c_void,
    event: *mut c_void,
    user_data: *mut c_void,
) -> i32 {
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
    if status != 0 || id != FOCUS_ID {
        return NOT_HANDLED;
    }
    // SAFETY: Only our installed handler calls this callback with the live box.
    // No UI calls or event-loop re-entry occur while borrowing the Cell.
    unsafe { &*user_data.cast::<Cell<bool>>() }.set(true);
    0
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

    fn deliver(id: Option<HotkeyId>, pending: &Cell<bool>) -> i32 {
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
            let status = on_hotkey(
                ptr::null_mut(),
                event,
                ptr::from_ref(pending).cast_mut().cast(),
            );
            ReleaseEvent(event);
            status
        }
    }

    #[test]
    fn ignores_other_hotkeys_and_missing_parameters() {
        let pending = Cell::new(false);
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
            assert_eq!(deliver(id, &pending), NOT_HANDLED);
            assert!(!pending.get());
        }
    }

    #[test]
    fn repeated_presses_coalesce_into_one_focus_request() {
        let hotkey = FocusHotkey {
            hotkey: ptr::null_mut(),
            handler: ptr::null_mut(),
            pending: Box::new(Cell::new(false)),
        };
        assert!(!hotkey.take_pending());
        assert_eq!(deliver(Some(FOCUS_ID), &hotkey.pending), 0);
        assert_eq!(deliver(Some(FOCUS_ID), &hotkey.pending), 0);
        assert!(hotkey.take_pending());
        assert!(!hotkey.take_pending());
    }
}
