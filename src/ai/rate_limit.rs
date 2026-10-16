//! Provider-wide, nonblocking cooldowns. Reopening a dialog or changing songs
//! must not immediately resend a request that the provider already rate-limited.
use reqwest::{Url, header::HeaderMap};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

#[derive(Default)]
struct Limits(HashMap<String, Instant>);

#[derive(Default)]
struct Pacer(HashMap<String, Instant>);
impl Pacer {
    fn try_turn(&mut self, origin: String, interval: Duration, now: Instant) -> Duration {
        let next = self.0.entry(origin).or_insert(now);
        if *next > now {
            return next.duration_since(now);
        }
        *next = now + interval;
        Duration::ZERO
    }
}

pub(crate) fn try_turn(endpoint: &str, interval: Duration) -> Duration {
    static PACER: OnceLock<Mutex<Pacer>> = OnceLock::new();
    PACER
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .try_turn(origin(endpoint), interval, Instant::now())
}

impl Limits {
    fn ready(&mut self, origin: &str, now: Instant) -> bool {
        self.0.retain(|_, until| *until > now);
        !self.0.contains_key(origin)
    }

    fn defer(&mut self, origin: String, wait: Duration, now: Instant) {
        // A malformed or extreme Retry-After must not overflow Instant.
        let wait = wait.clamp(Duration::from_secs(1), Duration::from_secs(86_400));
        let until = now + wait;
        self.0
            .entry(origin)
            .and_modify(|old| *old = (*old).max(until))
            .or_insert(until);
    }
}

fn limits() -> &'static Mutex<Limits> {
    static LIMITS: OnceLock<Mutex<Limits>> = OnceLock::new();
    LIMITS.get_or_init(Default::default)
}

fn origin(endpoint: &str) -> String {
    Url::parse(endpoint)
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_default()
}

pub(crate) fn ready(endpoint: &str) -> bool {
    limits()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .ready(&origin(endpoint), Instant::now())
}

pub(crate) fn remaining(endpoint: &str) -> Duration {
    limits()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .0
        .get(&origin(endpoint))
        .map(|until| until.saturating_duration_since(Instant::now()))
        .unwrap_or_default()
}

fn retry_after(headers: &HeaderMap, now: SystemTime) -> Duration {
    let value = headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok());
    if let Some(seconds) = value.and_then(|value| value.parse::<u64>().ok()) {
        return Duration::from_secs(seconds);
    }
    value
        .and_then(|value| chrono::DateTime::parse_from_rfc2822(value).ok())
        .and_then(|date| SystemTime::from(date).duration_since(now).ok())
        .unwrap_or(Duration::from_secs(30))
}

pub(crate) fn observe(endpoint: &str, headers: &HeaderMap) {
    let wait = retry_after(headers, SystemTime::now());
    eprintln!(
        "Provider rate-limit cooldown: {} for {}s",
        origin(endpoint),
        wait.as_secs()
    );
    limits().lock().unwrap_or_else(|e| e.into_inner()).defer(
        origin(endpoint),
        wait,
        Instant::now(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pacing_spaces_requests_across_tracks_and_endpoints() {
        let mut pacer = Pacer::default();
        let now = Instant::now();
        let interval = Duration::from_millis(1500);
        assert_eq!(
            pacer.try_turn("lrclib".into(), interval, now),
            Duration::ZERO
        );
        assert_eq!(
            pacer.try_turn("lrclib".into(), interval, now + Duration::from_millis(100)),
            Duration::from_millis(1400)
        );
        assert_eq!(
            pacer.try_turn("lrclib".into(), interval, now + interval),
            Duration::ZERO
        );
        assert_eq!(
            pacer.try_turn("deepseek".into(), interval, now),
            Duration::ZERO
        );
    }
    #[test]
    fn cooldown_is_shared_by_origin_and_expires_without_sleeping() {
        let mut limits = Limits::default();
        let now = Instant::now();
        let origin = super::origin("https://api.deepseek.com/chat/completions");
        limits.defer(origin, Duration::from_secs(60), now);
        assert!(!limits.ready(
            &super::origin("https://api.deepseek.com/anthropic/v1/messages"),
            now
        ));
        assert!(limits.ready(&super::origin("https://lrclib.net/api/get"), now));
        assert!(limits.ready(
            &super::origin("https://api.deepseek.com/chat/completions"),
            now + Duration::from_secs(60)
        ));
    }

    #[test]
    fn retry_after_supports_seconds_dates_and_invalid_headers() {
        let now = SystemTime::UNIX_EPOCH;
        for (value, seconds) in [
            ("120", 120),
            ("Thu, 01 Jan 1970 00:02:00 GMT", 120),
            ("garbage", 30),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert("retry-after", value.parse().unwrap());
            assert_eq!(retry_after(&headers, now), Duration::from_secs(seconds));
        }
    }
}
