//! Desktop battery and output status. Slow Bluetooth inventory stays off the UI/audio threads.
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Output {
    pub uid: String,
    pub name: String,
    pub bluetooth: bool,
    pub headphones: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Snapshot {
    pub battery_percent: Option<u8>,
    pub charging: bool,
    pub external_power: bool,
    pub output: Output,
    #[serde(skip)]
    pub accessory: AccessoryBattery,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccessoryBattery {
    pub left: Option<u8>,
    pub right: Option<u8>,
    pub case: Option<u8>,
    pub single: Option<u8>,
}

#[derive(Default)]
pub struct Monitor {
    pub snapshot: Snapshot,
    #[cfg(target_os = "macos")]
    native: Option<macos::Monitor>,
}

impl Monitor {
    pub fn new() -> Self {
        Self {
            snapshot: Snapshot::default(),
            #[cfg(target_os = "macos")]
            // Unit tests opt into individual native probes, not continuous workers.
            native: if cfg!(test) { None } else { macos::Monitor::start() },
        }
    }

    /// True once for a headphone/Bluetooth -> speaker transition. Never auto-resume.
    pub fn poll(&mut self) -> bool {
        #[cfg(target_os = "macos")]
        if let Some(native) = &self.native {
            return native.poll(&mut self.snapshot);
        }
        false
    }
}

#[cfg(any(target_os = "macos", test))]
fn percent(value: Option<&serde_json::Value>) -> Option<u8> {
    let value = value?;
    let number = if let Some(text) = value.as_str() {
        text.trim().trim_end_matches('%').parse::<u8>().ok()?
    } else {
        u8::try_from(value.as_u64()?).ok()?
    };
    (number <= 100).then_some(number)
}

#[cfg(any(target_os = "macos", test))]
fn bluetooth_battery(data: &serde_json::Value, output: &Output) -> AccessoryBattery {
    if !output.bluetooth || output.uid.is_empty() {
        return AccessoryBattery::default();
    }
    // CoreAudio Bluetooth UIDs contain the device address, sometimes followed
    // by :output. Prefer identity over a name (two paired sets can share a name).
    fn address(value: &str) -> String {
        value
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .flat_map(char::to_lowercase)
            .collect()
    }
    let uid = address(&output.uid);
    for adapter in data["SPBluetoothDataType"].as_array().into_iter().flatten() {
        for connected in adapter["device_connected"].as_array().into_iter().flatten() {
            for device in connected.as_object().into_iter().flat_map(|o| o.values()) {
                let Some(raw_address) = device["device_address"].as_str() else {
                    continue;
                };
                let identity = address(raw_address);
                if identity.len() != 12 || !uid.starts_with(&identity) {
                    continue;
                }
                return AccessoryBattery {
                    left: percent(device.get("device_batteryLevelLeft")),
                    right: percent(device.get("device_batteryLevelRight")),
                    case: percent(device.get("device_batteryLevelCase")),
                    single: percent(
                        device
                            .get("device_batteryLevelMain")
                            .or_else(|| device.get("device_batteryLevel")),
                    ),
                };
            }
        }
    }
    AccessoryBattery::default()
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::{
        ffi::{CStr, c_char, c_void},
        io::Read,
        process::{Command, Stdio},
        sync::mpsc::{self, Receiver, Sender},
        thread,
        time::{Duration, Instant},
    };

    unsafe extern "C" {
        fn nm_desktop_status_copy() -> *mut c_char;
        fn nm_desktop_watch_create() -> *mut c_void;
        fn nm_desktop_watch_take_events(watch: *mut c_void) -> u32;
        fn nm_desktop_watch_destroy(watch: *mut c_void);
    }

    pub(super) fn snapshot() -> Option<Snapshot> {
        unsafe {
            let json = nm_desktop_status_copy();
            if json.is_null() {
                return None;
            }
            let result = serde_json::from_slice(CStr::from_ptr(json).to_bytes()).ok();
            libc::free(json.cast());
            result
        }
    }

    struct Watch(*mut c_void);
    impl Drop for Watch {
        fn drop(&mut self) {
            unsafe { nm_desktop_watch_destroy(self.0) };
        }
    }

    enum Update {
        Status(Snapshot),
        Disconnected,
        Bluetooth(Output, AccessoryBattery),
    }

    pub(super) struct Monitor {
        updates: Receiver<Update>,
        stop: Sender<()>,
    }

    impl Monitor {
        pub(super) fn start() -> Option<Self> {
            let (sender, updates) = mpsc::channel();
            let (stop, stopping) = mpsc::channel();
            let (requests, queries) = mpsc::sync_channel::<Output>(1);
            let bluetooth_sender = sender.clone();
            thread::Builder::new()
                .name("desktop-bluetooth-status".into())
                .spawn(move || {
                    for output in queries {
                        let battery = bluetooth_inventory()
                            .map(|data| bluetooth_battery(&data, &output))
                            .unwrap_or_default();
                        if bluetooth_sender
                            .send(Update::Bluetooth(output, battery))
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .ok()?;
            thread::Builder::new()
                .name("desktop-device-status".into())
                .spawn(move || {
                    let watch = Watch(unsafe { nm_desktop_watch_create() });
                    let mut next_status = Instant::now();
                    let mut next_bluetooth = Instant::now();
                    let mut previous = Output::default();
                    loop {
                        let flags = unsafe { nm_desktop_watch_take_events(watch.0) };
                        if flags & 2 != 0 && sender.send(Update::Disconnected).is_err() {
                            break;
                        }
                        if flags & 1 != 0 || Instant::now() >= next_status {
                            if let Some(status) = snapshot() {
                                let changed = status.output != previous;
                                let output = status.output.clone();
                                previous = output.clone();
                                // Publish identity before its asynchronous battery response can arrive.
                                if sender.send(Update::Status(status)).is_err() {
                                    break;
                                }
                                if output.bluetooth && (changed || Instant::now() >= next_bluetooth)
                                {
                                    // Coalesce probes, and never block route notifications on system_profiler.
                                    if requests.try_send(output).is_ok() {
                                        next_bluetooth = Instant::now() + Duration::from_secs(30);
                                    }
                                }
                            }
                            next_status = Instant::now() + Duration::from_secs(2);
                        }
                        match stopping.recv_timeout(Duration::from_millis(50)) {
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            _ => break,
                        }
                    }
                })
                .ok()?;
            Some(Self { updates, stop })
        }

        pub(super) fn poll(&self, snapshot: &mut Snapshot) -> bool {
            let mut disconnected = false;
            while let Ok(update) = self.updates.try_recv() {
                match update {
                    Update::Disconnected => disconnected = true,
                    Update::Status(mut next) => {
                        if next.output == snapshot.output {
                            next.accessory = snapshot.accessory.clone();
                        }
                        *snapshot = next;
                    }
                    Update::Bluetooth(output, battery) if output == snapshot.output => {
                        snapshot.accessory = battery
                    }
                    _ => {} // Discard a probe for an output that has since changed.
                }
            }
            disconnected
        }
    }

    impl Drop for Monitor {
        fn drop(&mut self) {
            let _ = self.stop.send(());
        }
    }

    fn bluetooth_inventory() -> Option<serde_json::Value> {
        let mut child = Command::new("/usr/sbin/system_profiler")
            .args(["SPBluetoothDataType", "-json"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdout = child.stdout.take()?;
        let reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout.take(1024 * 1024).read_to_end(&mut bytes).ok()?;
            Some(bytes)
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let success = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.success(),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break false;
                }
            }
        };
        let bytes = reader.join().ok()??;
        success
            .then(|| serde_json::from_slice(&bytes).ok())
            .flatten()
    }

    #[test]
    fn native_read_only_snapshot_and_watch_lifecycle() {
        let snapshot = snapshot().expect("native status JSON");
        assert!(snapshot.battery_percent.is_none_or(|n| n <= 100));
        let watch = Watch(unsafe { nm_desktop_watch_create() });
        assert!(!watch.0.is_null());
        assert_eq!(unsafe { nm_desktop_watch_take_events(watch.0) } & !3, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn airpods_parts_match_only_the_connected_output_identity() {
        let data = json!({"SPBluetoothDataType": [{
            "device_connected": [{"Renamed buds": {"device_address": "AA-BB-CC-DD-EE-FF", "device_batteryLevelLeft": "0%", "device_batteryLevelRight": "66%", "device_batteryLevelCase": "101%"}}],
            "device_not_connected": [{"Old buds": {"device_address": "11-22-33-44-55-66", "device_batteryLevelLeft": "99%"}}]
        }]});
        let mut output = Output {
            uid: "AA:BB:CC:DD:EE:FF:output".into(),
            bluetooth: true,
            ..Default::default()
        };
        assert_eq!(
            bluetooth_battery(&data, &output),
            AccessoryBattery {
                left: Some(0),
                right: Some(66),
                ..Default::default()
            }
        );
        output.uid = "11:22:33:44:55:66:output".into();
        assert_eq!(
            bluetooth_battery(&data, &output),
            AccessoryBattery::default()
        );
        output.uid = "AA:BB:CC:DD:EE:FF:output".into();
        output.bluetooth = false;
        assert_eq!(
            bluetooth_battery(&data, &output),
            AccessoryBattery::default()
        );
    }

    #[test]
    fn unavailable_and_invalid_capacity_never_looks_full() {
        for value in [
            json!(null),
            json!("Unknown"),
            json!("-1%"),
            json!(101),
            json!(2.5),
        ] {
            assert_eq!(percent(Some(&value)), None);
        }
        assert_eq!(percent(Some(&json!("100%"))), Some(100));
        assert_eq!(percent(Some(&json!(0))), Some(0));
        assert_eq!(Snapshot::default().battery_percent, None);
    }
}
