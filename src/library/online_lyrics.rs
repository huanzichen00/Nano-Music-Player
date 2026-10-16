//! Explicit online lookup. LRCLIB metadata first, optional DeepSeek discovery;
//! never from the model's prose. Requests run outside playback and UI processing.
use super::{Track, lyrics::Lyrics};
use crate::{ai::DeepSeek, translation::Cancellation};
use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::Deserialize;
use std::{io::Read, path::PathBuf, time::Duration};
mod cache;
mod matching;

const ENDPOINT: &str = "https://lrclib.net/api/get";
const RESPONSE_LIMIT: u64 = 262_144;
const SEARCH_RESPONSE_LIMIT: u64 = 2_097_152;

#[derive(Debug, Clone)]
pub struct Request {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
}

impl Request {
    pub fn new(track: &Track) -> Self {
        Self {
            path: track.path.clone(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            duration: track.duration,
        }
    }

    pub fn matches(&self, track: &Track) -> bool {
        self.path == track.path
            && self.title == track.title
            && self.artist == track.artist
            && self.album == track.album
            && self.duration == track.duration
            && track.lyrics.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct Found {
    pub lyrics: Lyrics,
    pub source_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    id: u64,
    track_name: String,
    artist_name: String,
    album_name: String,
    duration: f64,
    instrumental: bool,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

fn validate_record(request: &Request, record: Record) -> Result<Found, String> {
    if matching::rank(request, &record).is_none() {
        return Err("Online lyrics did not match this recording".into());
    }
    if record.instrumental {
        return Err("This recording is instrumental".into());
    }
    let mut lyrics = record
        .synced_lyrics
        .as_deref()
        .map(Lyrics::parse)
        .unwrap_or_default();
    if lyrics.timed.iter().all(|line| line.text.trim().is_empty()) {
        lyrics = record
            .plain_lyrics
            .as_deref()
            .map(Lyrics::plain_text)
            .unwrap_or_default();
    }
    if lyrics.is_empty() {
        return Err("No matching online lyrics found".into());
    }
    if lyrics
        .timed
        .iter()
        .any(|line| line.at_ms as f64 > (request.duration + 2.0) * 1000.0)
    {
        return Err("Online lyrics did not match this recording".into());
    }
    Ok(Found {
        lyrics,
        source_url: format!("{ENDPOINT}/{}", record.id),
    })
}

fn lrclib_id(source: &str) -> Option<u64> {
    let url = Url::parse(source).ok()?;
    if url.scheme() != "https" || !matches!(url.host_str(), Some("lrclib.net" | "www.lrclib.net")) {
        return None;
    }
    url.path().strip_prefix("/api/get/")?.parse().ok()
}

fn source_ids(sources: &[String]) -> Vec<u64> {
    let mut seen = std::collections::HashSet::new();
    sources
        .iter()
        .filter_map(|source| lrclib_id(source))
        .filter(|id| seen.insert(*id))
        .take(3)
        .collect()
}

pub fn lookup(
    request: &Request,
    provider: Option<&DeepSeek>,
    cancellation: &Cancellation,
) -> Result<Found, String> {
    lookup_with_search(request, ENDPOINT, cancellation, || {
        provider.map_or_else(
            || Ok(Vec::new()),
            |provider| {
                provider.search(&format!(
                    "{} {} {} lyrics LRCLIB",
                    request.title, request.artist, request.album
                ))
            },
        )
    })
}

fn lookup_with_search(
    request: &Request,
    endpoint: &str,
    cancellation: &Cancellation,
    search: impl FnOnce() -> Result<Vec<String>, String>,
) -> Result<Found, String> {
    if request.title.trim().is_empty()
        || request.artist.trim().is_empty()
        || !request.duration.is_finite()
        || !(1.0..=3600.0).contains(&request.duration)
    {
        return Err("Title, artist and duration are required for online lyrics".into());
    }
    if cancellation.is_cancelled() {
        return Err("Online lyric search cancelled".into());
    }
    if let Some(error) = cache::get(endpoint, request) {
        return Err(error);
    }
    let client = lyrics_client()?;
    // Most lookups need no model call. Search all LRCLIB candidates locally
    // before spending a native search request or trusting its first result.
    let direct = retrieve(request, endpoint, &[], cancellation);
    if cancellation.is_cancelled() {
        return Err("Online lyric search cancelled".into());
    }
    if direct.is_ok() || direct.as_ref().is_err_and(|error| !candidate_miss(error)) {
        return direct;
    }
    let sources = search().unwrap_or_default();
    if cancellation.is_cancelled() {
        return Err("Online lyric search cancelled".into());
    }
    let mut records = Vec::new();
    for id in source_ids(&sources) {
        if cancellation.is_cancelled() {
            return Err("Online lyric search cancelled".into());
        }
        request_slot(endpoint, cancellation)?;
        match fetch_record(&client, request, endpoint, Query::Record(id)) {
            Ok(record) => records.push(record),
            Err(FetchError::Candidate(_)) => {}
            Err(FetchError::Service(error)) => return Err(error.into()),
            Err(FetchError::RateLimited) => {
                return Err("Online lyrics rate limit reached; try again later".into());
            }
        }
    }
    let result = if records.is_empty() {
        direct
    } else {
        select(request, records)
    };
    if cancellation.is_cancelled() {
        return Err("Online lyric search cancelled".into());
    }
    if let Err(error) = &result
        && candidate_miss(error)
    {
        cache::insert(endpoint, request, error.clone());
    }
    result
}

#[derive(Clone, Copy)]
enum Query {
    Record(u64),
    Signature,
    NormalizedSearch,
}

fn candidate_miss(error: &str) -> bool {
    matches!(
        error,
        "No matching online lyrics found"
            | "Online lyrics did not match this recording"
            | "This recording is instrumental"
    )
}

fn select(request: &Request, records: Vec<Record>) -> Result<Found, String> {
    let mut candidates = Vec::new();
    let mut last_miss = "No matching online lyrics found".to_owned();
    for record in records {
        let Some(rank) = matching::rank(request, &record) else {
            continue;
        };
        let distance = (record.duration - request.duration).abs();
        match validate_record(request, record) {
            Ok(found) => candidates.push((rank, distance, found)),
            Err(error) => last_miss = error,
        }
    }
    candidates.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| {
                a.2.lyrics
                    .timed
                    .is_empty()
                    .cmp(&b.2.lyrics.timed.is_empty())
            })
            .then_with(|| a.1.total_cmp(&b.1))
    });
    let Some((rank, _, best)) = candidates.first() else {
        return Err(last_miss);
    };
    let content = matching::key(&best.lyrics.lines().join(" "));
    if candidates.iter().skip(1).any(|(other_rank, _, found)| {
        *other_rank == *rank && matching::key(&found.lyrics.lines().join(" ")) != content
    }) {
        return Err(
            "Multiple online lyric recordings match; import lyrics for this version".into(),
        );
    }
    Ok(candidates.remove(0).2)
}

#[derive(Debug)]
enum FetchError {
    Candidate(String),
    Service(&'static str),
    RateLimited,
}

fn retrieve(
    request: &Request,
    endpoint: &str,
    sources: &[String],
    cancellation: &Cancellation,
) -> Result<Found, String> {
    let client = lyrics_client()?;
    let mut queries: Vec<_> = source_ids(sources).into_iter().map(Query::Record).collect();
    queries.push(Query::NormalizedSearch);
    queries.push(Query::Signature);
    let mut last_miss = "No matching online lyrics found".to_owned();
    let mut retried = false;
    for query in queries {
        if cancellation.is_cancelled() {
            return Err("Online lyric search cancelled".into());
        }
        request_slot(endpoint, cancellation)?;
        let execute = || match query {
            Query::NormalizedSearch => fetch_search(&client, request, endpoint, query)
                .and_then(|records| select(request, records).map_err(FetchError::Candidate)),
            _ => fetch_record(&client, request, endpoint, query).and_then(|record| {
                if matching::rank(request, &record) != Some((0, 0)) {
                    return Err(FetchError::Candidate(
                        "Online lyrics did not match this recording".into(),
                    ));
                }
                validate_record(request, record).map_err(FetchError::Candidate)
            }),
        };
        let mut result = execute();
        if matches!(result, Err(FetchError::RateLimited))
            && !retried
            && crate::ai::rate_limit::remaining(endpoint) <= Duration::from_secs(5)
        {
            retried = true;
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !crate::ai::rate_limit::ready(endpoint) {
                if cancellation.is_cancelled() {
                    return Err("Online lyric search cancelled".into());
                }
                if std::time::Instant::now() >= deadline {
                    return Err("Online lyrics rate limit reached; try again later".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            request_slot(endpoint, cancellation)?;
            result = execute();
        }
        match result {
            Ok(found) => return Ok(found),
            Err(FetchError::Candidate(error)) if error.starts_with("Multiple online") => {
                return Err(error);
            }
            Err(FetchError::Candidate(error)) => last_miss = error,
            Err(FetchError::Service(error)) => return Err(error.into()),
            Err(FetchError::RateLimited) => {
                return Err("Online lyrics rate limit reached; try again later".into());
            }
        }
    }
    Err(last_miss)
}

fn request_slot(endpoint: &str, cancellation: &Cancellation) -> Result<(), String> {
    loop {
        if cancellation.is_cancelled() {
            return Err("Online lyric search cancelled".into());
        }
        if !crate::ai::rate_limit::ready(endpoint) {
            return Err("Online lyrics rate limit reached; try again later".into());
        }
        let delay = crate::ai::rate_limit::try_turn(endpoint, Duration::from_millis(1500));
        if delay.is_zero() {
            return Ok(());
        }
        std::thread::sleep(delay.min(Duration::from_millis(50)));
    }
}

fn lyrics_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .redirect(Policy::none())
        .user_agent(concat!(
            "NanoMusicPlayer/",
            env!("CARGO_PKG_VERSION"),
            " (https://github.com/huanzichen00/Nano-Music-Player)"
        ))
        .build()
        .map_err(|_| "Cannot connect to online lyrics".into())
}

fn fetch_record(
    client: &Client,
    request: &Request,
    endpoint: &str,
    query: Query,
) -> Result<Record, FetchError> {
    let url = match query {
        Query::Record(id) => format!("{endpoint}/{id}"),
        _ => endpoint.to_owned(),
    };
    let mut http = client.get(&url);
    if !matches!(query, Query::Record(_)) {
        http = http.query(&[
            ("track_name", request.title.clone()),
            ("artist_name", request.artist.clone()),
            ("duration", request.duration.to_string()),
        ]);
        if matches!(query, Query::Signature) && !request.album.trim().is_empty() {
            http = http.query(&[("album_name", &request.album)]);
        }
    }
    let bytes = download(http, &url, RESPONSE_LIMIT)?;
    serde_json::from_slice(&bytes)
        .map_err(|_| FetchError::Service("Online lyrics returned an invalid response"))
}

fn fetch_search(
    client: &Client,
    request: &Request,
    endpoint: &str,
    query: Query,
) -> Result<Vec<Record>, FetchError> {
    let url = format!("{}/search", endpoint.trim_end_matches("/get"));
    let (title, artist) = match query {
        Query::NormalizedSearch => matching::search_metadata(request),
        _ => (request.title.clone(), request.artist.clone()),
    };
    let bytes = download(
        client
            .get(&url)
            .query(&[("track_name", title), ("artist_name", artist)]),
        &url,
        SEARCH_RESPONSE_LIMIT,
    )?;
    // Skip a malformed individual entry without losing other usable candidates.
    let records: Vec<serde_json::Value> = serde_json::from_slice(&bytes)
        .map_err(|_| FetchError::Service("Online lyrics returned an invalid response"))?;
    if records.len() > 200 {
        return Err(FetchError::Service("Online lyrics response is too large"));
    }
    Ok(records
        .into_iter()
        .filter_map(|record| serde_json::from_value(record).ok())
        .collect())
}

fn download(
    http: reqwest::blocking::RequestBuilder,
    url: &str,
    limit: u64,
) -> Result<Vec<u8>, FetchError> {
    if !crate::ai::rate_limit::ready(url) {
        return Err(FetchError::RateLimited);
    }
    let response = http
        .send()
        .map_err(|_| FetchError::Service("Cannot connect to online lyrics"))?;
    match response.status().as_u16() {
        200 => {}
        404 => {
            return Err(FetchError::Candidate(
                "No matching online lyrics found".into(),
            ));
        }
        429 => {
            crate::ai::rate_limit::observe(url, response.headers());
            return Err(FetchError::RateLimited);
        }
        _ => return Err(FetchError::Service("Online lyrics service is unavailable")),
    }
    let mut bytes = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| FetchError::Service("Cannot read online lyrics"))?;
    if bytes.len() as u64 > limit {
        return Err(FetchError::Service("Online lyrics response is too large"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
