//! Short negative cache: repeated clicks must not redo every discovery stage.
use super::Request;
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(PartialEq)]
struct Key(String, String, String, String, u64);
fn key(endpoint: &str, request: &Request) -> Key {
    Key(
        endpoint.into(),
        request.title.clone(),
        request.artist.clone(),
        request.album.clone(),
        request.duration.to_bits(),
    )
}

#[derive(Default)]
struct Cache(VecDeque<(Key, Instant, String)>);
impl Cache {
    fn get(&mut self, key: &Key, now: Instant) -> Option<String> {
        self.0.retain(|(_, until, _)| *until > now);
        self.0
            .iter()
            .find(|(saved, _, _)| saved == key)
            .map(|(_, _, error)| error.clone())
    }
    fn insert(&mut self, key: Key, error: String, now: Instant) {
        self.0
            .retain(|(saved, until, _)| *until > now && *saved != key);
        while self.0.len() >= 128 {
            self.0.pop_front();
        }
        self.0
            .push_back((key, now + Duration::from_secs(60), error));
    }
}
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}
pub(super) fn get(endpoint: &str, request: &Request) -> Option<String> {
    cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key(endpoint, request), Instant::now())
}
pub(super) fn insert(endpoint: &str, request: &Request, error: String) {
    cache().lock().unwrap_or_else(|e| e.into_inner()).insert(
        key(endpoint, request),
        error,
        Instant::now(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failures_expire_and_metadata_changes_bypass_the_cache() {
        let mut cache = Cache::default();
        let now = Instant::now();
        let key = Key(
            "endpoint".into(),
            "song".into(),
            "artist".into(),
            "album".into(),
            60,
        );
        cache.insert(key, "missing".into(), now);
        assert_eq!(
            cache.get(
                &Key(
                    "endpoint".into(),
                    "song".into(),
                    "artist".into(),
                    "album".into(),
                    60
                ),
                now
            ),
            Some("missing".into())
        );
        assert_eq!(
            cache.get(
                &Key(
                    "endpoint".into(),
                    "song (live)".into(),
                    "artist".into(),
                    "album".into(),
                    60
                ),
                now
            ),
            None
        );
        assert_eq!(
            cache.get(
                &Key(
                    "endpoint".into(),
                    "song".into(),
                    "artist".into(),
                    "album".into(),
                    60
                ),
                now + Duration::from_secs(60)
            ),
            None
        );
    }
}
