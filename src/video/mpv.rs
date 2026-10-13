//! Minimal dynamically loaded libmpv ABI, following include/mpv/{client,render}.h.
//! All pointers stay on the video worker. After renderer creation, only async
//! client commands, observations and wait_event are used (render-thread-safe).
use super::{
    Generation, Status,
    engine::{Command, Event, Frame, Latest, Progress},
    library::Video,
};
use libloading::Library;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_ulong, c_void},
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
};

type Handle = *mut c_void;
#[repr(C)]
struct Param {
    kind: c_int,
    data: *mut c_void,
}
#[repr(C)]
struct NativeEvent {
    kind: c_int,
    error: c_int,
    userdata: u64,
    data: *mut c_void,
}
#[repr(C)]
struct Property {
    name: *const c_char,
    format: c_int,
    data: *mut c_void,
}
#[repr(C)]
struct EndFile {
    reason: c_int,
    error: c_int,
}

struct Api {
    _library: Library,
    create: unsafe extern "C" fn() -> Handle,
    initialize: unsafe extern "C" fn(Handle) -> c_int,
    destroy: unsafe extern "C" fn(Handle),
    option: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    command: unsafe extern "C" fn(Handle, u64, *const *const c_char) -> c_int,
    observe: unsafe extern "C" fn(Handle, u64, *const c_char, c_int) -> c_int,
    wait: unsafe extern "C" fn(Handle, f64) -> *const NativeEvent,
    error: unsafe extern "C" fn(c_int) -> *const c_char,
    render_create: unsafe extern "C" fn(*mut Handle, Handle, *mut Param) -> c_int,
    render_free: unsafe extern "C" fn(Handle),
    render_update: unsafe extern "C" fn(Handle) -> u64,
    render_callback:
        unsafe extern "C" fn(Handle, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    render: unsafe extern "C" fn(Handle, *mut Param) -> c_int,
}

impl Api {
    fn load() -> Result<Self, String> {
        let mut paths = Vec::new();
        if let Some(path) = std::env::var_os("NANO_MPV_LIBRARY") {
            paths.push(path);
        }
        paths.extend(
            [
                "/opt/homebrew/lib/libmpv.2.dylib",
                "/usr/local/lib/libmpv.2.dylib",
                "libmpv.2.dylib",
                "libmpv.so.2",
                "mpv-2.dll",
            ]
            .map(Into::into),
        );
        let library = paths.iter().find_map(|p| {
            // SAFETY: only libmpv library candidates are loaded; symbols are checked below.
            unsafe { Library::new(p).ok() }
        }).ok_or("Video requires libmpv 0.35+ (macOS: brew install mpv). Set NANO_MPV_LIBRARY for a custom installation.")?;
        macro_rules! symbol {
            ($name:literal) => {
                // SAFETY: signatures match libmpv's stable C ABI; library is retained in Api.
                unsafe {
                    *library
                        .get(concat!($name, "\0").as_bytes())
                        .map_err(|e| e.to_string())?
                }
            };
        }
        let version: unsafe extern "C" fn() -> c_ulong = symbol!("mpv_client_api_version");
        // SAFETY: no-argument version query from the retained library.
        let version = unsafe { version() };
        if version >> 16 != 2 {
            return Err("Video requires libmpv client API 2 (mpv 0.35+).".into());
        }
        Ok(Self {
            create: symbol!("mpv_create"),
            initialize: symbol!("mpv_initialize"),
            destroy: symbol!("mpv_terminate_destroy"),
            option: symbol!("mpv_set_option_string"),
            command: symbol!("mpv_command_async"),
            observe: symbol!("mpv_observe_property"),
            wait: symbol!("mpv_wait_event"),
            error: symbol!("mpv_error_string"),
            render_create: symbol!("mpv_render_context_create"),
            render_free: symbol!("mpv_render_context_free"),
            render_update: symbol!("mpv_render_context_update"),
            render_callback: symbol!("mpv_render_context_set_update_callback"),
            render: symbol!("mpv_render_context_render"),
            _library: library,
        })
    }

    fn check(&self, result: c_int) -> Result<(), String> {
        if result >= 0 {
            return Ok(());
        }
        // SAFETY: mpv_error_string returns a static, NUL-terminated string for every error code.
        Err(unsafe { CStr::from_ptr((self.error)(result)) }
            .to_string_lossy()
            .into_owned())
    }
}

// Guarantee the 64-byte alignment recommended by the software rendering API.
#[repr(C, align(64))]
#[derive(Clone)]
struct Block([u8; 64]);

pub(super) struct Session {
    api: Api,
    handle: Handle,
    renderer: Handle,
    update: Box<AtomicBool>,
    pixels: Vec<Block>,
    stride: usize,
    size: [c_int; 2],
    pub generation: Generation,
    position: f64,
    duration: f64,
    paused: bool,
    loaded: bool,
    finished: bool,
    audio_output: Option<String>,
}

unsafe extern "C" fn render_updated(data: *mut c_void) {
    // SAFETY: points to boxed AtomicBool retained until render context has been freed.
    unsafe { &*data.cast::<AtomicBool>() }.store(true, Ordering::Release);
}

impl Session {
    pub fn open(
        generation: Generation,
        video: &Video,
        start: f64,
        volume: f32,
        paused: bool,
        silent: bool,
    ) -> Result<Self, String> {
        let api = Api::load()?;
        // SAFETY: creates a new exclusively owned native handle.
        let handle = unsafe { (api.create)() };
        if handle.is_null() {
            return Err("libmpv could not create a player".into());
        }
        let (width, height) = video.render_size();
        let stride = (width as usize * 4).next_multiple_of(64);
        let mut session = Self {
            api,
            handle,
            renderer: ptr::null_mut(),
            update: Box::new(AtomicBool::new(false)),
            pixels: vec![Block([0; 64]); stride * height as usize / 64],
            stride,
            size: [width as c_int, height as c_int],
            generation,
            position: 0.0,
            duration: video.duration,
            paused,
            loaded: false,
            finished: false,
            audio_output: None,
        };
        for (key, value) in [
            ("config", "no"),
            ("terminal", "no"),
            ("vo", "libmpv"),
            ("hwdec", "no"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            ("osc", "no"),
            ("osd-level", "0"),
            ("sid", "no"),
            ("audio-display", "no"),
            ("keep-open", "no"),
            ("video-sync", "audio"),
            ("pause", if paused { "yes" } else { "no" }),
        ] {
            session.option(key, value)?;
        }
        session.option("volume", &(volume.clamp(0.0, 1.0) * 100.0).to_string())?;
        session.option("start", &start.max(0.0).to_string())?;
        if silent {
            session.option("ao", "null")?;
        }
        // SAFETY: handle initialized once; software API parameter is a static C string.
        unsafe {
            session.api.check((session.api.initialize)(handle))?;
            let mut params = [
                Param {
                    kind: 1,
                    data: c"sw".as_ptr().cast_mut().cast(),
                },
                Param {
                    kind: 0,
                    data: ptr::null_mut(),
                },
            ];
            session.api.check((session.api.render_create)(
                &mut session.renderer,
                handle,
                params.as_mut_ptr(),
            ))?;
            (session.api.render_callback)(
                session.renderer,
                Some(render_updated),
                (&*session.update as *const AtomicBool).cast_mut().cast(),
            );
            for (id, name, format) in [
                (1, c"time-pos", 5),
                (2, c"duration", 5),
                (3, c"pause", 3),
                (4, c"current-ao", 1),
            ] {
                session
                    .api
                    .check((session.api.observe)(handle, id, name.as_ptr(), format))?;
            }
        }
        let path = video.path.to_str().ok_or("Video path is not valid UTF-8")?;
        session.async_command(&["loadfile", path, "replace"])?;
        Ok(session)
    }

    fn option(&self, name: &str, value: &str) -> Result<(), String> {
        let name = CString::new(name).map_err(|e| e.to_string())?;
        let value = CString::new(value).map_err(|e| e.to_string())?;
        // SAFETY: strings live through the call; called only before renderer creation.
        self.api
            .check(unsafe { (self.api.option)(self.handle, name.as_ptr(), value.as_ptr()) })
    }

    fn async_command(&self, values: &[&str]) -> Result<(), String> {
        let strings: Vec<_> = values
            .iter()
            .map(|v| CString::new(*v))
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        let mut pointers: Vec<_> = strings.iter().map(|v| v.as_ptr()).collect();
        pointers.push(ptr::null());
        // SAFETY: mpv copies the null-terminated array and strings before returning.
        self.api
            .check(unsafe { (self.api.command)(self.handle, self.generation, pointers.as_ptr()) })
    }

    pub fn command(&mut self, command: Command) -> Result<(), String> {
        if self.finished
            && matches!(command, Command::Pause { generation, paused: true } if generation == self.generation)
        {
            return Ok(());
        }
        if self.finished
            && matches!(&command, Command::Pause { generation, .. } | Command::Seek { generation, .. } if *generation == self.generation)
        {
            return Err("Video has finished; reopen it to play or seek".into());
        }
        match command {
            Command::Pause { generation, paused } if generation == self.generation => {
                self.async_command(&["set", "pause", if paused { "yes" } else { "no" }])?;
                self.paused = paused;
            }
            Command::Seek {
                generation,
                seconds,
            } if generation == self.generation && seconds.is_finite() => {
                self.async_command(&[
                    "seek",
                    &seconds.clamp(0.0, self.duration.max(0.0)).to_string(),
                    "absolute+exact",
                ])?;
            }
            Command::Volume { generation, volume }
                if generation == self.generation && volume.is_finite() =>
            {
                self.async_command(&[
                    "set",
                    "volume",
                    &(volume.clamp(0.0, 1.0) * 100.0).to_string(),
                ])?;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn error_snapshot(&self) -> Progress {
        Progress {
            generation: self.generation,
            status: Status::Error,
            position: self.position,
            duration: self.duration,
            audio_output: self.audio_output.clone(),
        }
    }

    pub fn poll(
        &mut self,
        latest: &Arc<Mutex<Latest>>,
        events: &Sender<Event>,
    ) -> Result<(), String> {
        loop {
            // SAFETY: wait_event(0) is nonblocking and safe on render threads. Event data
            // is consumed before the next wait, as required by the ABI.
            let event = unsafe { &*(self.api.wait)(self.handle, 0.0) };
            match event.kind {
                0 => break,
                5 => {
                    self.api.check(event.error)?;
                }
                7 if !event.data.is_null() => {
                    // SAFETY: END_FILE points to mpv_event_end_file; read its ABI-stable prefix.
                    let end = unsafe { &*event.data.cast::<EndFile>() };
                    if end.reason == 4 {
                        self.api.check(end.error)?;
                    }
                    if end.reason == 0 && !self.finished {
                        self.finished = true;
                        self.position = self.duration;
                        let _ = events.send(Event::Finished(self.generation));
                    }
                }
                8 => self.loaded = true,
                22 if !event.data.is_null() => {
                    // SAFETY: PROPERTY_CHANGE carries Property; formats determine pointee type.
                    let property = unsafe { &*event.data.cast::<Property>() };
                    if !property.data.is_null() {
                        match (event.userdata, property.format) {
                            (1, 5) => self.position = unsafe { *property.data.cast::<f64>() },
                            (2, 5) => self.duration = unsafe { *property.data.cast::<f64>() },
                            (3, 3) => self.paused = unsafe { *property.data.cast::<c_int>() != 0 },
                            (4, 1) => {
                                // SAFETY: MPV_FORMAT_STRING data points to a C string pointer.
                                let value = unsafe { *property.data.cast::<*const c_char>() };
                                if !value.is_null() {
                                    self.audio_output = Some(
                                        unsafe { CStr::from_ptr(value) }
                                            .to_string_lossy()
                                            .into_owned(),
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        if self.update.swap(false, Ordering::AcqRel) {
            // SAFETY: exclusively owned context, every callback update is serviced on this thread.
            if unsafe { (self.api.render_update)(self.renderer) } & 1 != 0 {
                let mut params = [
                    Param {
                        kind: 17,
                        data: self.size.as_mut_ptr().cast(),
                    },
                    Param {
                        kind: 18,
                        data: c"rgb0".as_ptr().cast_mut().cast(),
                    },
                    Param {
                        kind: 19,
                        data: (&mut self.stride as *mut usize).cast(),
                    },
                    Param {
                        kind: 20,
                        data: self.pixels.as_mut_ptr().cast(),
                    },
                    Param {
                        kind: 0,
                        data: ptr::null_mut(),
                    },
                ];
                // SAFETY: 64-byte aligned surface owns stride*height writable bytes.
                self.api
                    .check(unsafe { (self.api.render)(self.renderer, params.as_mut_ptr()) })?;
                let width = self.size[0] as usize;
                let height = self.size[1] as usize;
                let mut rgba = Vec::with_capacity(width * height * 4);
                // SAFETY: Block is exactly 64 bytes, initialized and contiguous in this Vec.
                let bytes = unsafe {
                    std::slice::from_raw_parts(
                        self.pixels.as_ptr().cast::<u8>(),
                        self.pixels.len() * 64,
                    )
                };
                for row in bytes.chunks_exact(self.stride) {
                    for pixel in row[..width * 4].as_chunks::<4>().0 {
                        rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                    }
                }
                latest.lock().unwrap().frame = Some(Frame {
                    generation: self.generation,
                    width: width as u32,
                    height: height as u32,
                    rgba,
                });
            }
        }
        latest.lock().unwrap().progress = Some(Progress {
            generation: self.generation,
            position: self.position,
            duration: self.duration,
            audio_output: self.audio_output.clone(),
            status: if self.finished {
                Status::Finished
            } else if !self.loaded {
                Status::Loading
            } else if self.paused {
                Status::Paused
            } else {
                Status::Playing
            },
        });
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: context freed before core, callback state and library outlive both.
        unsafe {
            if !self.renderer.is_null() {
                (self.api.render_free)(self.renderer);
            }
            (self.api.destroy)(self.handle);
        }
    }
}
