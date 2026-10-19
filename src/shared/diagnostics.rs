//! Bounded playback diagnostics. UI/audio callbacks only enqueue small records;
//! a separate thread owns disk I/O. Never pass keys, clipboard text or lyrics.
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        mpsc::{self, SyncSender},
    },
    time::{SystemTime, UNIX_EPOCH},
};

static LOG: OnceLock<SyncSender<Value>> = OnceLock::new();
const LIMIT: u64 = 256 * 1024;

#[cfg_attr(not(any(target_os = "ios", feature = "mobile")), allow(dead_code))]
pub(crate) fn init(directory: &Path) {
    if LOG.get().is_some() {
        return;
    }
    let path = directory.join("playback.jsonl");
    let (sender, receiver) = mpsc::sync_channel(256);
    if LOG.set(sender).is_err() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("playback-diagnostics".into())
        .spawn(move || {
            let mut sink = Sink {
                path,
                file: None,
                size: 0,
            };
            for value in receiver {
                let _ = sink.write(&value, LIMIT);
            }
        });
    record("app_start", json!({"version": env!("CARGO_PKG_VERSION")}));
}

pub(crate) fn record(event: &'static str, fields: Value) {
    let Some(sender) = LOG.get() else {
        return;
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let _ = sender.try_send(
        json!({"time_ms": timestamp, "pid": std::process::id(), "event": event, "data": fields}),
    );
}

struct Sink {
    path: PathBuf,
    file: Option<File>,
    size: u64,
}

impl Sink {
    fn write(&mut self, value: &Value, limit: u64) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(value)?;
        if bytes.len() > 4096 {
            return Ok(());
        }
        bytes.push(b'\n');
        if self.file.is_none() {
            self.size = fs::metadata(&self.path).map_or(0, |m| m.len());
        }
        if self.size + bytes.len() as u64 > limit {
            self.file = None;
            let previous = self.path.with_file_name("playback.previous.jsonl");
            match fs::remove_file(&previous) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            if self.path.exists() {
                fs::rename(&self.path, previous)?;
            }
            self.size = 0;
        }
        if self.file.is_none() {
            let mut options = OpenOptions::new();
            options.create(true).append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            self.file = Some(options.open(&self.path)?);
        }
        self.file.as_mut().unwrap().write_all(&bytes)?;
        self.size += bytes.len() as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logs_survive_restarts_and_rotate_with_a_fixed_storage_bound() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("playback.jsonl");
        let value = json!({"event": "load", "session": 1});
        let mut first = Sink {
            path: path.clone(),
            file: None,
            size: 0,
        };
        first.write(&value, 128).unwrap();
        drop(first);
        let mut next = Sink {
            path: path.clone(),
            file: None,
            size: 0,
        };
        next.write(&value, 128).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
        for _ in 0..100 {
            next.write(&value, 128).unwrap();
        }
        for file in [path, directory.path().join("playback.previous.jsonl")] {
            let content = fs::read_to_string(file).unwrap();
            assert!(content.len() <= 128);
            for line in content.lines() {
                serde_json::from_str::<Value>(line).unwrap();
            }
        }
    }
}
