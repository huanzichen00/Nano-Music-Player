//! Isolated Genius request contracts and validation.
//!
//! This module is intentionally not wired into the app while shared controller/UI ownership is
//! reserved. It contains no network client; callers must explicitly inject a provider.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, path::PathBuf};

pub const PROMPT_VERSION: &str = "genius-local-v1";
pub const MAX_CANDIDATES: usize = 50_000;

/// Adapter boundary for `crate::ai::DeepSeek::json`; test implementations remain offline.
pub trait JsonProvider {
    fn json(&self, system: &str, input: &Value) -> Result<Value, String>;
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

const SYSTEM_PROMPT: &str = "Choose related songs only from the supplied local candidate list. Return JSON {\"ids\":[candidate IDs in listening order]}. Do not invent IDs, include the seed, or return paths. A short list is acceptable when few candidates fit.";

pub fn generate(
    request: &Request,
    provider: &impl JsonProvider,
    cancellation: &Cancellation,
) -> Result<ResultSet, Error> {
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// Request-local opaque ID. Never accept a filesystem path from provider output.
    pub id: String,
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub duration_seconds: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub request_id: u64,
    pub seed_path: PathBuf,
    pub library_revision: u64,
    pub variation: u64,
    pub limit: usize,
    pub candidates: Vec<Candidate>,
}

impl Request {
    pub fn validate(&self) -> Result<(), Error> {
        if self.limit == 0 || self.limit > 100 {
            return Err(Error::InvalidRequest("limit must be from 1 to 100"));
        }
        if self.candidates.is_empty() || self.candidates.len() > MAX_CANDIDATES {
            return Err(Error::InvalidRequest(
                "candidate snapshot is empty or too large",
            ));
        }
        let mut ids = HashSet::with_capacity(self.candidates.len());
        let mut paths = HashSet::with_capacity(self.candidates.len());
        for candidate in &self.candidates {
            if candidate.id.is_empty() || candidate.id.len() > 128 || !ids.insert(&candidate.id) {
                return Err(Error::InvalidRequest(
                    "candidate IDs must be unique and bounded",
                ));
            }
            if candidate.path.as_os_str().is_empty() || !paths.insert(&candidate.path) {
                return Err(Error::InvalidRequest(
                    "candidate paths must be nonempty and unique",
                ));
            }
            if !candidate.duration_seconds.is_finite() || candidate.duration_seconds < 0.0 {
                return Err(Error::InvalidRequest("candidate duration is invalid"));
            }
        }
        if !self.candidates.iter().any(|c| c.path == self.seed_path) {
            return Err(Error::InvalidRequest("seed is absent from the snapshot"));
        }
        Ok(())
    }

    pub fn prompt_payload(&self) -> Result<serde_json::Value, Error> {
        self.validate()?;
        let candidates: Vec<_> = self
            .candidates
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id, "title": c.title, "artist": c.artist, "album": c.album,
                    "genre": c.genre, "duration_seconds": c.duration_seconds
                })
            })
            .collect();
        Ok(serde_json::json!({
            "seed_id": self.candidates.iter().find(|c| c.path == self.seed_path).unwrap().id,
            "limit": self.limit,
            "variation": self.variation,
            "candidates": candidates
        }))
    }

    /// Validate a strict `{"ids":[...]}` response and map IDs back to frozen local paths.
    pub fn validate_response(&self, response: &str) -> Result<ResultSet, Error> {
        self.validate()?;
        let parsed: ProviderResponse =
            serde_json::from_str(response).map_err(|_| Error::InvalidResponse)?;
        if parsed.ids.len() > self.limit {
            return Err(Error::InvalidResponse);
        }
        let sparse = parsed.ids.len() < self.limit;
        let mut seen = HashSet::with_capacity(parsed.ids.len());
        let seed = &self.seed_path;
        let mut tracks = Vec::with_capacity(parsed.ids.len());
        for id in parsed.ids {
            if !seen.insert(id.clone()) {
                return Err(Error::DuplicateId);
            }
            let candidate = self
                .candidates
                .iter()
                .find(|candidate| candidate.id == id)
                .ok_or(Error::UnknownId)?;
            if &candidate.path == seed {
                return Err(Error::SeedRepeated);
            }
            tracks.push(candidate.path.clone());
        }
        Ok(ResultSet {
            request_id: self.request_id,
            seed_path: self.seed_path.clone(),
            library_revision: self.library_revision,
            variation: self.variation,
            ordered_paths: tracks,
            sparse,
        })
    }

    pub fn accepts_result(
        &self,
        result: &ResultSet,
        current_request_id: u64,
        current_seed: &std::path::Path,
        current_library_revision: u64,
    ) -> bool {
        result.request_id == self.request_id
            && result.request_id == current_request_id
            && result.seed_path == self.seed_path
            && current_seed == self.seed_path
            && result.library_revision == self.library_revision
            && current_library_revision == self.library_revision
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderResponse {
    ids: Vec<String>,
}

// Kept separate from provider input to ensure output cannot supply identity or metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultSet {
    pub request_id: u64,
    pub seed_path: PathBuf,
    pub library_revision: u64,
    pub variation: u64,
    pub ordered_paths: Vec<PathBuf>,
    pub sparse: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidRequest(&'static str),
    InvalidResponse,
    DuplicateId,
    UnknownId,
    SeedRepeated,
    Provider,
    Cancelled,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(reason) => write!(f, "invalid Genius request: {reason}"),
            Self::InvalidResponse => f.write_str("DeepSeek returned an invalid Genius response"),
            Self::DuplicateId => f.write_str("DeepSeek returned a duplicate Genius candidate"),
            Self::UnknownId => f.write_str("DeepSeek returned an unknown Genius candidate"),
            Self::SeedRepeated => f.write_str("DeepSeek repeated the Genius seed"),
            Self::Provider => f.write_str("DeepSeek Genius request failed"),
            Self::Cancelled => f.write_str("Genius request cancelled"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request {
        Request {
            request_id: 7,
            seed_path: "/music/seed.flac".into(),
            library_revision: 4,
            variation: 2,
            limit: 3,
            candidates: vec![
                candidate("seed", "/music/seed.flac"),
                candidate("a", "/music/a.flac"),
                candidate("b", "/music/b.flac"),
            ],
        }
    }
    fn candidate(id: &str, path: &str) -> Candidate {
        Candidate {
            id: id.into(),
            path: path.into(),
            title: id.into(),
            artist: "artist".into(),
            album: "album".into(),
            genre: "genre".into(),
            duration_seconds: 120.0,
        }
    }
    #[test]
    fn maps_ids_to_paths_in_provider_order_and_marks_sparse() {
        let mut req = request();
        req.limit = 2;
        let result = req.validate_response(r#"{"ids":["b","a"]}"#).unwrap();
        assert_eq!(
            result.ordered_paths,
            vec![
                PathBuf::from("/music/b.flac"),
                PathBuf::from("/music/a.flac")
            ]
        );
        assert!(!result.sparse);
    }
    #[test]
    fn rejects_invalid_or_untrusted_provider_output() {
        let req = request();
        for body in [
            "{}",
            r#"{"ids":["a","a"]}"#,
            r#"{"ids":["nope"]}"#,
            r#"{"ids":["seed"]}"#,
            r#"{"ids":["a"],"paths":["/tmp/x"]}"#,
        ] {
            assert!(req.validate_response(body).is_err(), "accepted {body}");
        }
    }
    #[test]
    fn seed_and_revision_gate_application() {
        let req = request();
        let result = req.validate_response(r#"{"ids":["a"]}"#).unwrap();
        assert!(req.accepts_result(&result, 7, std::path::Path::new("/music/seed.flac"), 4));
        assert!(!req.accepts_result(&result, 8, std::path::Path::new("/music/seed.flac"), 4));
        assert!(!req.accepts_result(&result, 7, std::path::Path::new("/music/seed.flac"), 5));
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
        let fixture = Fixture(serde_json::json!({"ids":["a"]}));
        assert_eq!(
            generate(&req, &fixture, &token).unwrap().ordered_paths,
            [PathBuf::from("/music/a.flac")]
        );
        token.cancel();
        assert_eq!(generate(&req, &fixture, &token), Err(Error::Cancelled));
    }
}
