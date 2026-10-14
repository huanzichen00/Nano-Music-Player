//! Lyric-translation contracts, provider validation, and durable exact-source cache.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub const PROMPT_VERSION: &str = "lyrics-translation-v1";
pub const MAX_LINES: usize = 20_000;

/// Shared by the native menu and the iPod settings rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Translate,
    Regenerate,
    Cancel,
    Toggle,
    Remove,
    FindLyrics,
    SetTarget(TargetLanguage),
}

impl Command {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "translate_lyrics" => Self::Translate,
            "regenerate_translation" => Self::Regenerate,
            "cancel_translation" => Self::Cancel,
            "toggle_translation" => Self::Toggle,
            "remove_translation" => Self::Remove,
            "find_online_lyrics" => Self::FindLyrics,
            "translation_target_en" => Self::SetTarget(TargetLanguage::English),
            "translation_target_zh" => Self::SetTarget(TargetLanguage::SimplifiedChinese),
            "translation_target_ja" => Self::SetTarget(TargetLanguage::Japanese),
            _ => return None,
        })
    }
}

/// Adapter boundary for `crate::ai::DeepSeek::json`; test implementations remain offline.
pub trait JsonProvider {
    fn json(&self, system: &str, input: &Value) -> Result<Value, String>;
}

impl JsonProvider for crate::ai::DeepSeek {
    fn json(&self, system: &str, input: &Value) -> Result<Value, String> {
        crate::ai::DeepSeek::json(self, system, input)
    }
}

#[derive(Clone, Default)]
pub struct Cancellation(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// Nonblocking admission gate; callers queue/retry Busy rather than creating unbounded workers.
pub struct RequestGate(std::sync::atomic::AtomicUsize);
impl RequestGate {
    pub const fn new(max_in_flight: usize) -> Self {
        Self(std::sync::atomic::AtomicUsize::new(max_in_flight))
    }
    pub fn try_acquire(&self) -> Result<RequestPermit<'_>, Error> {
        self.0
            .fetch_update(
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
                |available| available.checked_sub(1),
            )
            .map(|_| RequestPermit(self))
            .map_err(|_| Error::Busy)
    }
}
pub struct RequestPermit<'a>(&'a RequestGate);
impl Drop for RequestPermit<'_> {
    fn drop(&mut self) {
        self.0.0.fetch_add(1, std::sync::atomic::Ordering::Release);
    }
}

const SYSTEM_PROMPT: &str = "Translate each supplied lyric line to the requested language. Return JSON {\"translations\":[one translated string per input line in the same order]}. Preserve blank lines as empty strings. Do not return timestamps or modify the source.";

pub fn generate(
    request: &Request,
    provider: &impl JsonProvider,
    cancellation: &Cancellation,
) -> Result<Translation, Error> {
    request.validate()?;
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let response = provider
        .json(SYSTEM_PROMPT, &request.prompt_payload()?)
        .map_err(|_| Error::Provider)?;
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let response = serde_json::to_string(&response).map_err(|_| Error::InvalidResponse)?;
    request.validate_response(&response)
}

pub fn generate_bounded(
    request: &Request,
    provider: &impl JsonProvider,
    cancellation: &Cancellation,
    gate: &RequestGate,
) -> Result<Translation, Error> {
    let _permit = gate.try_acquire()?;
    generate(request, provider, cancellation)
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum TargetLanguage {
    #[default]
    English,
    SimplifiedChinese,
    Japanese,
}

impl TargetLanguage {
    pub const ALL: [Self; 3] = [Self::English, Self::SimplifiedChinese, Self::Japanese];

    pub fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::SimplifiedChinese => "简体中文",
            Self::Japanese => "日本語",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh-Hans",
            Self::Japanese => "ja",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceLine {
    pub at_ms: Option<i64>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub request_id: u64,
    pub track_id: PathBuf,
    pub target: TargetLanguage,
    pub provider_version: String,
    pub source: Vec<SourceLine>,
}

impl Request {
    pub fn validate(&self) -> Result<(), Error> {
        if self.track_id.as_os_str().is_empty() {
            return Err(Error::InvalidRequest);
        }
        if self.source.is_empty() || self.source.len() > MAX_LINES {
            return Err(Error::InvalidRequest);
        }
        if self.provider_version.is_empty() || self.provider_version.len() > 128 {
            return Err(Error::InvalidRequest);
        }
        if self
            .source
            .iter()
            .any(|line| line.at_ms.is_some_and(|ms| ms < 0))
        {
            return Err(Error::InvalidRequest);
        }
        Ok(())
    }

    /// A stable, non-cryptographic fingerprint; cache hits also compare the complete source.
    pub fn source_fingerprint(&self) -> String {
        let bytes =
            serde_json::to_vec(&self.source).expect("SourceLine serialization is infallible");
        format!("fnv1a64:{:016x}", fnv1a64(&bytes))
    }

    pub fn cache_key(&self) -> CacheKey {
        CacheKey {
            track_id: self.track_id.clone(),
            source_fingerprint: self.source_fingerprint(),
            source: self.source.clone(),
            target: self.target,
            provider_version: self.provider_version.clone(),
        }
    }

    pub fn prompt_payload(&self) -> Result<serde_json::Value, Error> {
        self.validate()?;
        Ok(serde_json::json!({
            "target_language": self.target.code(),
            "lines": self.source.iter().enumerate().map(|(index,line)| serde_json::json!({
                "index": index, "text": line.text
            })).collect::<Vec<_>>()
        }))
    }

    pub fn validate_response(&self, response: &str) -> Result<Translation, Error> {
        self.validate()?;
        let parsed: ProviderResponse =
            serde_json::from_str(response).map_err(|_| Error::InvalidResponse)?;
        if parsed.translations.len() != self.source.len() {
            return Err(Error::Incomplete);
        }
        for (source, translated) in self.source.iter().zip(&parsed.translations) {
            if source.text.trim().is_empty() && !translated.is_empty() {
                return Err(Error::InvalidResponse);
            }
            if translated.chars().any(char::is_control) {
                return Err(Error::InvalidResponse);
            }
            if !source.text.trim().is_empty() && translated.trim().is_empty() {
                return Err(Error::Incomplete);
            }
        }
        Ok(Translation {
            key: self.cache_key(),
            lines: parsed.translations,
        })
    }

    pub fn accepts(
        &self,
        translation: &Translation,
        current_request_id: u64,
        current_track: &Path,
        current_target: TargetLanguage,
        current_source: &[SourceLine],
        current_provider_version: &str,
    ) -> bool {
        translation.key == self.cache_key()
            && self.request_id == current_request_id
            && self.track_id == current_track
            && self.target == current_target
            && self.source == current_source
            && self.provider_version == current_provider_version
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderResponse {
    translations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CacheKey {
    pub track_id: PathBuf,
    pub source_fingerprint: String,
    /// Stored to verify exact source equality, not merely fingerprint equality.
    pub source: Vec<SourceLine>,
    pub target: TargetLanguage,
    pub provider_version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Translation {
    pub key: CacheKey,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cache {
    entries: BTreeMap<String, Translation>,
}

impl Cache {
    pub fn get(&self, key: &CacheKey) -> Option<&Translation> {
        self.entries
            .get(&key_id(key))
            .filter(|entry| &entry.key == key)
    }
    pub fn insert(&mut self, value: Translation) {
        self.entries.insert(key_id(&value.key), value);
    }
    pub fn remove(&mut self, key: &CacheKey) -> bool {
        self.entries.remove(&key_id(key)).is_some()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn load(path: &Path) -> Result<Self, Error> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(path).map_err(|_| Error::Storage)?;
        let cache: Self = serde_json::from_slice(&bytes).map_err(|_| Error::Storage)?;
        if cache.entries.values().any(|entry| {
            entry.lines.len() != entry.key.source.len()
                || entry.key.source_fingerprint != fingerprint(&entry.key.source)
                || entry
                    .lines
                    .iter()
                    .any(|line| line.chars().any(char::is_control))
        }) {
            return Err(Error::Storage);
        }
        Ok(cache)
    }

    /// Write a complete cache snapshot then atomically replace the previous file.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| Error::Storage)?;
        }
        let suffix = format!(
            "{}.{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let temp: PathBuf = path.with_extension(suffix);
        let result = (|| {
            let bytes = serde_json::to_vec(self).map_err(|_| Error::Storage)?;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            use std::io::Write;
            let mut file = options.open(&temp).map_err(|_| Error::Storage)?;
            file.write_all(&bytes).map_err(|_| Error::Storage)?;
            file.sync_all().map_err(|_| Error::Storage)?;
            std::fs::rename(&temp, path).map_err(|_| Error::Storage)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result
    }
}

fn key_id(key: &CacheKey) -> String {
    // JSON's escaping and field boundaries avoid collisions from delimiters in paths/versions.
    serde_json::to_string(key).expect("CacheKey serialization is infallible")
}
fn fingerprint(source: &[SourceLine]) -> String {
    let bytes = serde_json::to_vec(source).expect("SourceLine serialization is infallible");
    format!("fnv1a64:{:016x}", fnv1a64(&bytes))
}
fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidRequest,
    InvalidResponse,
    Incomplete,
    Storage,
    Provider,
    Cancelled,
    Busy,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request {
        Request {
            request_id: 1,
            track_id: "/music/song.flac".into(),
            target: TargetLanguage::Japanese,
            provider_version: PROMPT_VERSION.into(),
            source: vec![
                SourceLine {
                    at_ms: Some(1200),
                    text: "Hello".into(),
                },
                SourceLine {
                    at_ms: Some(2500),
                    text: "World".into(),
                },
            ],
        }
    }
    #[test]
    fn pairs_complete_lines_and_rejects_incomplete_or_extra_fields() {
        let req = request();
        let result = req
            .validate_response(r#"{"translations":["こんにちは","世界"]}"#)
            .unwrap();
        assert_eq!(result.lines, ["こんにちは", "世界"]);
        assert_eq!(result.key.source, req.source);
        assert!(
            req.validate_response(r#"{"translations":["こんにちは"]}"#)
                .is_err()
        );
        assert!(
            req.validate_response(r#"{"translations":["こんにちは","世界"],"source":"changed"}"#)
                .is_err()
        );
    }

    #[test]
    fn blank_lyric_rows_remain_blank_and_keep_their_timestamps() {
        let mut req = request();
        req.source.insert(
            1,
            SourceLine {
                at_ms: Some(1800),
                text: "  ".into(),
            },
        );
        let result = req
            .validate_response(r#"{"translations":["こんにちは","","世界"]}"#)
            .unwrap();
        assert_eq!(result.key.source, req.source);
        assert_eq!(result.lines[1], "");
        assert!(
            req.validate_response(r#"{"translations":["こんにちは","invented lyric","世界"]}"#)
                .is_err()
        );
    }
    #[test]
    fn result_is_rejected_after_target_source_or_request_changes() {
        let req = request();
        let result = req
            .validate_response(r#"{"translations":["こんにちは","世界"]}"#)
            .unwrap();
        assert!(req.accepts(
            &result,
            1,
            Path::new("/music/song.flac"),
            TargetLanguage::Japanese,
            &req.source,
            PROMPT_VERSION
        ));
        assert!(!req.accepts(
            &result,
            2,
            Path::new("/music/song.flac"),
            TargetLanguage::Japanese,
            &req.source,
            PROMPT_VERSION
        ));
        assert!(!req.accepts(
            &result,
            1,
            Path::new("/music/song.flac"),
            TargetLanguage::English,
            &req.source,
            PROMPT_VERSION
        ));
    }
    #[test]
    fn cache_survives_restart_and_uses_exact_source_key() {
        let req = request();
        let value = req
            .validate_response(r#"{"translations":["こんにちは","世界"]}"#)
            .unwrap();
        let mut cache = Cache::default();
        cache.insert(value.clone());
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("lyrics-cache.json");
        cache.save(&path).unwrap();
        let mut loaded = Cache::load(&path).unwrap();
        assert_eq!(loaded.get(&req.cache_key()), Some(&value));
        let mut changed = req.cache_key();
        changed.source[0].text.push('!');
        assert!(loaded.get(&changed).is_none());
        assert!(loaded.remove(&req.cache_key()));
        assert!(loaded.is_empty());
    }

    #[test]
    fn provider_adapter_and_cancellation_are_offline() {
        struct Fixture(Value);
        impl JsonProvider for Fixture {
            fn json(&self, _: &str, _: &Value) -> Result<Value, String> {
                Ok(self.0.clone())
            }
        }
        let req = request();
        let token = Cancellation::default();
        let fixture = Fixture(serde_json::json!({"translations":["こんにちは","世界"]}));
        assert_eq!(
            generate(&req, &fixture, &token).unwrap().lines,
            ["こんにちは", "世界"]
        );
        token.cancel();
        assert_eq!(generate(&req, &fixture, &token), Err(Error::Cancelled));
    }
}
