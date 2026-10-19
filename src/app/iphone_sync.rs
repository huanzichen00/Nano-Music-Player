//! Desktop USB device manager around the same validated mobile merge pipeline.
//! Python owns filesystem/device work; the UI thread only consumes NDJSON events.
use super::i18n::{Language, translate};
use crate::ui::{I18n, IPhoneSyncWindow, SyncDeviceRow, SyncPlaylistRow};
use serde::Deserialize;
use serde_json::Value;
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Default, Deserialize)]
struct Device {
    id: String,
    name: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Playlist {
    id: i64,
    name: String,
    tracks: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Library {
    available_tracks: u64,
    unavailable_tracks: u64,
    audio_bytes: u64,
    playlists: Vec<Playlist>,
}

#[derive(Clone, Debug, Deserialize)]
struct Receipt {
    source_id: String,
    export_id: String,
    complete: bool,
    tracks: u64,
    playlists: u64,
    history_added: u64,
    conflicts: u64,
    warnings: Vec<String>,
}

fn receipt(data: &Value) -> Option<Receipt> {
    let receipt: Receipt = serde_json::from_value(data.get("receipt")?.clone()).ok()?;
    let identifier = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    (receipt.complete && identifier(&receipt.source_id) && identifier(&receipt.export_id))
        .then_some(receipt)
}

fn cli_error(data: &Value) -> &'static str {
    match data.get("code").and_then(Value::as_str) {
        Some("unsupported_platform") => "USB sync requires macOS and Xcode with devicectl",
        Some("xcode_required") => "Install Xcode and select its developer tools to enable USB sync",
        Some("device_discovery_failed") => {
            "Unable to list USB devices; unlock and trust the iPhone, then retry"
        }
        Some("device_unavailable") => {
            "Selected iPhone is not connected over USB; unlock, trust and reconnect it"
        }
        Some("device_operation_failed") => {
            "USB device operation failed; unlock the iPhone and check its connection"
        }
        Some("restart_required") => {
            "Unable to restart the phone app; open Nano Music Player on the iPhone"
        }
        Some("invalid_arguments" | "invalid_input") => {
            "The library, sync selection or package is invalid; refresh the panel and retry"
        }
        Some("library_unavailable") => "Unable to read the selected desktop music library",
        Some("timeout") => {
            "The operation timed out; keep the phone app open and check the USB connection"
        }
        Some("cancelled") => "Sync cancelled before the phone import was committed",
        _ => "iPhone sync could not finish. Reconnect and unlock the phone, then retry.",
    }
}

enum CliLine {
    Text(String),
    Invalid,
}

/// Limit allocation before looking for a newline, and drain oversized records.
fn read_cli_line(reader: &mut impl BufRead) -> io::Result<Option<CliLine>> {
    const LIMIT: usize = 4 * 1024 * 1024;
    let mut line = Vec::new();
    let mut invalid = false;
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            if line.is_empty() && !invalid {
                return Ok(None);
            }
            break;
        }
        let newline = bytes.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(bytes.len(), |index| index + 1);
        if !invalid && line.len() + count <= LIMIT {
            line.extend_from_slice(&bytes[..count]);
        } else {
            invalid = true;
            line.clear();
        }
        reader.consume(count);
        if newline.is_some() {
            break;
        }
    }
    Ok(Some(if invalid {
        CliLine::Invalid
    } else {
        String::from_utf8(line).map_or(CliLine::Invalid, CliLine::Text)
    }))
}

fn valid_event(task: Task, event: &Value) -> bool {
    let phase = event.get("phase").and_then(Value::as_str);
    if matches!(phase, Some("error" | "cancelled")) {
        return true;
    }
    match task {
        Task::Devices => {
            phase == Some("devices")
                && event
                    .get("devices")
                    .is_some_and(|data| serde_json::from_value::<Vec<Device>>(data.clone()).is_ok())
        }
        Task::Library => {
            phase == Some("library")
                && event
                    .get("library")
                    .is_some_and(|data| serde_json::from_value::<Library>(data.clone()).is_ok())
        }
        Task::Preview => {
            phase == Some("plan")
                && event
                    .get("plan")
                    .is_some_and(|data| serde_json::from_value::<Library>(data.clone()).is_ok())
        }
        Task::Sync => {
            matches!(
                phase,
                Some("export" | "backup" | "copy" | "ready" | "importing")
            ) || phase == Some("complete") && receipt(event).is_some()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Task {
    Devices,
    Library,
    Preview,
    Sync,
}

enum Event {
    Data(Task, u64, Value),
    Finished(Task, u64, Result<(), String>),
}

#[derive(Clone)]
struct Runtime {
    python: PathBuf,
    script: PathBuf,
}

impl Runtime {
    fn prepare(directory: &Path) -> Result<Self, String> {
        let directory = directory.join("iphone-sync-runtime");
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let script = directory.join("sync_iphone.py");
        let temporary = directory.join("sync_iphone.py.next");
        std::fs::write(&temporary, include_str!("../../scripts/sync_iphone.py"))
            .and_then(|_| std::fs::rename(temporary, &script))
            .map_err(|_| {
                "Cannot prepare iPhone sync files. Check storage space and file permissions."
                    .to_owned()
            })?;
        Ok(Self {
            python: "/usr/bin/python3".into(),
            script,
        })
    }

    fn spawn(&self, task: Task, id: u64, arguments: Vec<String>, sender: Sender<Event>) {
        let runtime = self.clone();
        std::thread::spawn(move || {
            let result = runtime.run(task, id, &arguments, &sender);
            let _ = sender.send(Event::Finished(task, id, result));
        });
    }

    fn run(
        &self,
        task: Task,
        id: u64,
        arguments: &[String],
        sender: &Sender<Event>,
    ) -> Result<(), String> {
        let mut child = Command::new(&self.python)
            .arg("-u")
            .arg(&self.script)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| {
                "Cannot start iPhone sync. Install Xcode and its command line tools.".to_owned()
            })?;
        let stderr = child
            .stderr
            .take()
            .ok_or("Sync error output is unavailable")?;
        // Tool diagnostics never become UI text, even after a failed process.
        let errors = std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let _ = io::copy(&mut reader, &mut io::sink());
        });
        let stdout = child.stdout.take().ok_or("Sync output is unavailable")?;
        let mut reader = BufReader::new(stdout);
        let mut last_error = None;
        let mut terminal = false;
        let mut malformed = false;
        loop {
            let line = match read_cli_line(&mut reader) {
                Ok(Some(CliLine::Text(line))) => line,
                Ok(Some(CliLine::Invalid)) => {
                    malformed = true;
                    continue;
                }
                Ok(None) => break,
                Err(_) => {
                    malformed = true;
                    break;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<Value>(&line)
                && valid_event(task, &event)
            {
                let phase = event
                    .get("phase")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if matches!(phase, "error" | "cancelled") {
                    last_error = Some(cli_error(&event).to_owned());
                } else if task != Task::Sync || phase == "complete" {
                    terminal = true;
                }
                // A closed window is hidden rather than dropped. If the app
                // exits, let the device worker finish its current safe boundary.
                let _ = sender.send(Event::Data(task, id, event));
            } else {
                malformed = true;
            }
        }
        drop(reader);
        let status = child.wait().map_err(|_| "Cannot wait for iPhone sync")?;
        let _ = errors.join();
        if status.success() && terminal && !malformed && last_error.is_none() {
            Ok(())
        } else {
            Err(last_error.unwrap_or_else(|| {
                if malformed {
                    "Could not read iPhone sync progress. Refresh and retry."
                } else if task == Task::Sync && status.success() {
                    "No completion receipt was received. Keep the phone app open and retry later."
                } else {
                    "iPhone sync could not finish. Reconnect and unlock the phone, then retry."
                }
                .into()
            }))
        }
    }
}

pub(super) struct SyncPanel {
    state: Rc<RefCell<Panel>>,
    _timer: Timer,
}

struct Panel {
    ui: IPhoneSyncWindow,
    directory: PathBuf,
    runtime: Runtime,
    language: Language,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
    tasks: HashMap<Task, u64>,
    next_id: u64,
    devices: Vec<Device>,
    selected: String,
    library: Option<Library>,
    playlists: HashSet<i64>,
    preview: Option<Library>,
    cancel_file: Option<PathBuf>,
    cancel_requested: bool,
    committed: bool,
    phase: &'static str,
    message: String,
    receipt: Option<Receipt>,
    completed: bool,
    failed: bool,
    cancelled: bool,
    standalone: bool,
}

impl SyncPanel {
    pub(super) fn new(directory: PathBuf, language: Language) -> Result<Self, String> {
        let runtime = Runtime::prepare(&directory)?;
        Self::create(directory, language, runtime)
    }

    fn create(directory: PathBuf, language: Language, runtime: Runtime) -> Result<Self, String> {
        let ui = IPhoneSyncWindow::new().map_err(|error| error.to_string())?;
        let (sender, receiver) = mpsc::channel();
        ui.global::<I18n>().on_translate(|index, key| {
            translate(
                Language::ALL
                    .get(index as usize)
                    .copied()
                    .unwrap_or_default(),
                &key,
            )
            .into()
        });
        ui.global::<I18n>().set_language(language.index() as i32);
        let state = Rc::new(RefCell::new(Panel {
            ui,
            directory,
            runtime,
            language,
            sender,
            receiver,
            tasks: HashMap::new(),
            next_id: 0,
            devices: Vec::new(),
            selected: String::new(),
            library: None,
            playlists: HashSet::new(),
            preview: None,
            cancel_file: None,
            cancel_requested: false,
            committed: false,
            phase: "Ready to preview",
            message: String::new(),
            receipt: None,
            completed: false,
            failed: false,
            cancelled: false,
            standalone: false,
        }));
        let weak = Rc::downgrade(&state);
        state.borrow().ui.on_command(move |command| {
            if let Some(state) = weak.upgrade() {
                state.borrow_mut().command(&command);
            }
        });
        let weak = Rc::downgrade(&state);
        state.borrow().ui.on_device_selected(move |device| {
            if let Some(state) = weak.upgrade() {
                state.borrow_mut().select_device(&device);
            }
        });
        let weak = Rc::downgrade(&state);
        state.borrow().ui.on_playlist_toggled(move |id| {
            if let Some(state) = weak.upgrade() {
                let mut state = state.borrow_mut();
                if state.editable()
                    && let Ok(id) = id.parse()
                {
                    if !state.playlists.remove(&id) {
                        state.playlists.insert(id);
                    }
                    state.invalidate();
                    state.publish();
                }
            }
        });
        let weak = Rc::downgrade(&state);
        state.borrow().ui.window().on_close_requested(move || {
            if let Some(state) = weak.upgrade() {
                let state = state.borrow();
                if state.standalone && !state.busy() {
                    let _ = slint::quit_event_loop();
                }
            }
            slint::CloseRequestResponse::HideWindow
        });
        let timer = Timer::default();
        let weak = Rc::downgrade(&state);
        timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
            if let Some(state) = weak.upgrade() {
                state.borrow_mut().poll();
            }
        });
        state.borrow_mut().refresh();
        Ok(Self {
            state,
            _timer: timer,
        })
    }

    pub(super) fn show(&self, language: Language) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        state.language = language;
        state
            .ui
            .global::<I18n>()
            .set_language(language.index() as i32);
        state.publish();
        state.ui.show().map_err(|error| error.to_string())
    }

    pub(super) fn run(&self, language: Language) -> Result<(), Box<dyn std::error::Error>> {
        self.state.borrow_mut().standalone = true;
        self.show(language)?;
        slint::run_event_loop_until_quit()?;
        Ok(())
    }
}

impl Panel {
    fn text(&self, key: &str) -> slint::SharedString {
        translate(self.language, key).into()
    }

    fn message(&mut self, key: impl Into<String>) {
        self.message = key.into();
        self.ui.set_message(self.text(&self.message));
    }
    fn busy(&self) -> bool {
        self.tasks.contains_key(&Task::Sync)
    }
    fn editable(&self) -> bool {
        self.tasks.is_empty()
    }

    fn spawn(&mut self, task: Task, mut arguments: Vec<String>) {
        self.next_id += 1;
        self.tasks.insert(task, self.next_id);
        arguments.extend([
            "--json-events".into(),
            "--config-dir".into(),
            self.directory.to_string_lossy().into_owned(),
        ]);
        self.runtime
            .spawn(task, self.next_id, arguments, self.sender.clone());
    }

    fn refresh(&mut self) {
        if !self.editable() {
            return;
        }
        self.invalidate();
        self.message("Connect and unlock your iPhone, then select it in the sidebar.");
        self.spawn(Task::Devices, vec!["--list-devices".into()]);
        self.spawn(Task::Library, vec!["--inspect-library".into()]);
        self.publish();
    }

    fn selection_arguments(&self) -> Vec<String> {
        let mut arguments = vec![
            "--scope".into(),
            if self.ui.get_scope_all() {
                "all".into()
            } else {
                "playlists".into()
            },
        ];
        if !self.ui.get_scope_all() {
            let mut ids: Vec<_> = self.playlists.iter().copied().collect();
            ids.sort();
            for id in ids {
                arguments.extend(["--playlist-id".into(), id.to_string()]);
            }
        }
        if !self.ui.get_include_lyrics() {
            arguments.push("--no-include-lyrics".into());
        }
        if !self.ui.get_include_statistics() {
            arguments.push("--no-include-statistics".into());
        }
        arguments
    }

    fn invalidate(&mut self) {
        self.preview = None;
        self.receipt = None;
        self.phase = "Ready to preview";
        self.completed = false;
        self.failed = false;
        self.cancelled = false;
        self.cancel_requested = false;
        self.committed = false;
        self.ui.set_selection_summary("".into());
        self.ui.set_result("".into());
        self.ui.set_conflicts("".into());
        self.ui.set_progress_known(false);
        self.ui.set_progress(0.0);
    }

    fn can_preview(&self) -> bool {
        self.editable()
            && self.library.is_some()
            && self.devices.iter().any(|device| device.id == self.selected)
            && (self.ui.get_scope_all() || !self.playlists.is_empty())
    }

    fn can_sync(&self) -> bool {
        self.can_preview()
            && self
                .preview
                .as_ref()
                .is_some_and(|plan| plan.available_tracks > 0)
    }

    fn select_device(&mut self, id: &str) {
        if self.editable() && self.devices.iter().any(|device| device.id == id) {
            self.selected = id.into();
            self.invalidate();
            self.message("Preview first. Nothing is transferred until you click Sync.");
            self.publish();
        }
    }

    fn command(&mut self, command: &str) {
        match command {
            "hide" => {
                let _ = self.ui.hide();
                if self.standalone && !self.busy() {
                    let _ = slint::quit_event_loop();
                }
            }
            "cancel" if self.busy() && self.ui.get_can_cancel() => {
                if let Some(path) = &self.cancel_file {
                    match std::fs::write(path, b"cancel") {
                        Ok(()) => {
                            self.cancel_requested = true;
                            self.ui.set_can_cancel(false);
                            self.message("Cancelling safely; please keep the USB cable connected…");
                        }
                        Err(_) => self.message("Could not request cancellation. Keep the app open until sync finishes."),
                    }
                }
            }
            _ if !self.editable() => {}
            "refresh" => self.refresh(),
            "options-changed" => {
                self.invalidate();
                self.message("Preview first. Nothing is transferred until you click Sync.");
                self.publish();
            }
            "preview" if self.can_preview() => {
                self.message("Reading your music selection…");
                let mut args = self.selection_arguments();
                args.push("--plan".into());
                self.spawn(Task::Preview, args);
                self.publish();
            }
            "sync" if self.can_sync() => self.start_sync(),
            _ => {}
        }
    }

    fn start_sync(&mut self) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let output = self
            .directory
            .join("iphone-sync")
            .join(format!("{}-{stamp}", std::process::id()));
        if std::fs::create_dir_all(&output).is_err() {
            self.message(
                "Cannot prepare iPhone sync files. Check storage space and file permissions.",
            );
            return;
        }
        let cancel = output.join("CANCEL");
        let mut arguments = self.selection_arguments();
        arguments.extend([
            "--apply".into(),
            "--device".into(),
            self.selected.clone(),
            "--output".into(),
            output.to_string_lossy().into_owned(),
            "--cancel-file".into(),
            cancel.to_string_lossy().into_owned(),
        ]);
        self.cancel_file = Some(cancel);
        self.completed = false;
        self.failed = false;
        self.cancelled = false;
        self.cancel_requested = false;
        self.committed = false;
        self.receipt = None;
        self.ui.set_result("".into());
        self.ui.set_conflicts("".into());
        self.phase = "Preparing sync";
        self.ui.set_progress_known(false);
        self.ui.set_progress(0.0);
        self.ui.set_can_cancel(true);
        self.message("Preparing your selection. Keep the iPhone connected.");
        self.spawn(Task::Sync, arguments);
        self.publish();
    }

    fn poll(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Data(task, id, data) if self.tasks.get(&task) == Some(&id) => {
                    self.receive(task, data)
                }
                Event::Finished(task, id, result) if self.tasks.get(&task) == Some(&id) => {
                    self.tasks.remove(&task);
                    if let Err(error) = result {
                        if task == Task::Sync && self.cancelled {
                            // The CLI intentionally exits 130 after acknowledging cancellation.
                        } else {
                            if task == Task::Sync {
                                self.completed = false;
                                self.receipt = None;
                            }
                            self.message(error);
                            self.phase = "Sync could not finish";
                            self.failed = true;
                        }
                        match task {
                            Task::Devices => {
                                self.devices.clear();
                                self.selected.clear();
                            }
                            Task::Library => {
                                self.library = None;
                                self.playlists.clear();
                            }
                            Task::Preview => self.preview = None,
                            Task::Sync => {}
                        }
                    } else if task == Task::Sync
                        && !self.completed
                        && !self.cancelled
                        && !self.failed
                    {
                        self.failed = true;
                        self.phase = "Sync could not finish";
                        self.message("No completion receipt was received. Keep the phone app open and retry later.");
                    }
                    if task == Task::Sync {
                        self.ui.set_can_cancel(false);
                        self.cancel_file = None;
                        self.preview = None;
                    }
                    self.publish();
                    if self.standalone && !self.busy() && !self.ui.window().is_visible() {
                        let _ = slint::quit_event_loop();
                    }
                }
                _ => {}
            }
        }
    }

    fn receive(&mut self, task: Task, data: Value) {
        match task {
            Task::Devices => {
                if let Some(value) = data.get("devices") {
                    match serde_json::from_value::<Vec<Device>>(value.clone()) {
                        Ok(devices) => {
                            self.devices = devices;
                            if !self.devices.iter().any(|device| device.id == self.selected) {
                                self.selected = self
                                    .devices
                                    .first()
                                    .map(|device| device.id.clone())
                                    .unwrap_or_default();
                            }
                        }
                        Err(_) => {
                            self.message("Could not read connected devices. Refresh and retry.")
                        }
                    }
                }
            }
            Task::Library => {
                if let Some(value) = data.get("library") {
                    match serde_json::from_value::<Library>(value.clone()) {
                        Ok(library) => {
                            self.playlists.retain(|id| {
                                library.playlists.iter().any(|playlist| playlist.id == *id)
                            });
                            self.library = Some(library);
                        }
                        Err(_) => self.message("Could not read the desktop library."),
                    }
                }
            }
            Task::Preview => {
                if let Some(value) = data.get("plan") {
                    match serde_json::from_value::<Library>(value.clone()) {
                        Ok(plan) => {
                            self.message("Preview ready. Click Sync to transfer this selection.");
                            self.preview = Some(plan);
                        }
                        Err(_) => self.message("Could not read the sync preview."),
                    }
                }
            }
            Task::Sync => self.receive_sync(&data),
        }
        if data.get("phase").and_then(Value::as_str) == Some("error") {
            self.message(cli_error(&data));
        }
        self.publish();
    }

    fn receive_sync(&mut self, data: &Value) {
        let phase = data
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let label = match phase {
            "export" => "Preparing music",
            "backup" => "Backing up iPhone",
            "copy" => "Transferring over USB",
            "ready" => "Finishing transfer",
            "importing" => "Updating iPhone library",
            "complete" => "Sync complete",
            "cancelled" => "Sync cancelled",
            "error" => "Sync could not finish",
            _ => "Preparing sync",
        };
        self.phase = label;
        let progress = (phase == "export")
            .then(|| {
                data.get("progress")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite())
            })
            .flatten();
        self.ui.set_progress_known(progress.is_some());
        self.ui
            .set_progress(progress.unwrap_or(0.0).clamp(0.0, 1.0) as f32);
        self.committed |= matches!(phase, "ready" | "importing" | "complete");
        self.ui.set_can_cancel(
            !self.cancel_requested
                && !self.committed
                && matches!(phase, "export" | "backup" | "copy")
                && data
                    .get("cancellable")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
        );
        let message = if phase == "importing" {
            "Keep Nano Music Player open on the iPhone while it updates the library."
        } else if phase == "copy" {
            "Keep the USB cable connected. This step does not report a byte percentage."
        } else if matches!(phase, "ready" | "complete") {
            label
        } else if phase == "error" {
            cli_error(data)
        } else if phase == "cancelled" {
            "Sync cancelled before the phone import was committed"
        } else {
            data.get("message").and_then(Value::as_str).unwrap_or(label)
        };
        if self.cancel_requested && !self.committed && !matches!(phase, "cancelled" | "error") {
            self.message("Cancelling safely; please keep the USB cable connected…");
        } else {
            self.message(message);
        }
        if phase == "cancelled" {
            self.cancelled = true;
        }
        if phase == "error" {
            self.failed = true;
        }
        if phase == "complete"
            && let Some(receipt) = receipt(data)
        {
            self.completed = true;
            self.receipt = Some(receipt);
            self.ui.set_progress_known(true);
            self.ui.set_progress(1.0);
            self.ui.set_can_cancel(false);
        }
    }

    fn publish(&mut self) {
        self.ui.set_phase(self.text(self.phase));
        self.ui.set_message(self.text(&self.message));
        self.ui.set_busy(self.busy());
        self.ui.set_scanning(!self.busy() && !self.tasks.is_empty());
        self.ui.set_can_preview(self.can_preview());
        self.ui.set_can_sync(self.can_sync());
        if let Some(plan) = &self.preview {
            let mut summary = formatted(
                self.language,
                "{0} songs · {1}",
                &[plan.available_tracks.to_string(), size(plan.audio_bytes)],
            );
            if plan.unavailable_tracks > 0 {
                summary.push_str(" · ");
                summary.push_str(&formatted(
                    self.language,
                    "{0} unavailable files skipped",
                    &[plan.unavailable_tracks.to_string()],
                ));
            }
            self.ui.set_selection_summary(summary.into());
        } else {
            self.ui.set_selection_summary("".into());
        }
        if let Some(receipt) = &self.receipt {
            self.ui.set_result(
                formatted(
                    self.language,
                    "{0} songs · {1} playlists · {2} new history records",
                    &[
                        receipt.tracks.to_string(),
                        receipt.playlists.to_string(),
                        receipt.history_added.to_string(),
                    ],
                )
                .into(),
            );
            let mut warnings = receipt
                .warnings
                .iter()
                .map(|warning| warning_text(self.language, warning))
                .collect::<Vec<_>>();
            if receipt.conflicts > 0 {
                warnings.insert(
                    0,
                    formatted(
                        self.language,
                        "{0} phone edits preserved",
                        &[receipt.conflicts.to_string()],
                    ),
                );
            }
            self.ui.set_conflicts(warnings.join("\n").into());
        } else {
            self.ui.set_result("".into());
            self.ui.set_conflicts("".into());
        }
        self.ui.set_devices(ModelRc::new(VecModel::from(
            self.devices
                .iter()
                .map(|device| SyncDeviceRow {
                    id: device.id.as_str().into(),
                    name: device.name.as_str().into(),
                    subtitle: self.text("Connected via USB"),
                    selected: device.id == self.selected,
                })
                .collect::<Vec<_>>(),
        )));
        if let Some(device) = self
            .devices
            .iter()
            .find(|device| device.id == self.selected)
        {
            self.ui.set_device_name(device.name.as_str().into());
            self.ui
                .set_device_detail(self.text("USB · Music, playlists and listening data"));
        } else {
            self.ui.set_device_name("".into());
            self.ui.set_device_detail("".into());
        }
        if let Some(library) = &self.library {
            self.ui
                .set_library_song_count(library.available_tracks.min(i32::MAX as u64) as i32);
            self.ui
                .set_library_playlist_count(library.playlists.len().min(i32::MAX as usize) as i32);
            self.ui.set_library_size(size(library.audio_bytes).into());
            self.ui.set_playlists(ModelRc::new(VecModel::from(
                library
                    .playlists
                    .iter()
                    .map(|playlist| SyncPlaylistRow {
                        id: playlist.id.to_string().into(),
                        name: playlist.name.as_str().into(),
                        detail: formatted(
                            self.language,
                            "{0} songs",
                            &[playlist.tracks.to_string()],
                        )
                        .into(),
                        selected: self.playlists.contains(&playlist.id),
                    })
                    .collect::<Vec<_>>(),
            )));
        } else {
            self.ui.set_library_song_count(0);
            self.ui.set_library_playlist_count(0);
            self.ui.set_library_size("".into());
            self.ui.set_playlists(ModelRc::new(VecModel::default()));
        }
    }
}

fn formatted(language: Language, key: &str, values: &[String]) -> String {
    let mut text = translate(language, key);
    for (index, value) in values.iter().enumerate() {
        text = text.replace(&format!("{{{index}}}"), value);
    }
    text
}

fn warning_text(language: Language, warning: &str) -> String {
    for (prefix, key) in [
        ("Kept phone audio for ", "Kept phone audio for {0}"),
        (
            "Kept phone edits to playlist ",
            "Kept phone edits to playlist {0}",
        ),
        ("Kept phone lyrics for ", "Kept phone lyrics for {0}"),
        ("Kept phone readings for ", "Kept phone readings for {0}"),
        ("Kept phone rating for ", "Kept phone rating for {0}"),
    ] {
        if let Some(title) = warning.strip_prefix(prefix) {
            return formatted(language, key, &[title.to_owned()]);
        }
    }
    translate(language, warning)
}

fn size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB", bytes as f64 / 1_000_000_000.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    }
}

#[cfg(test)]
mod tests;
