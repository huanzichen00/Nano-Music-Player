use super::{Record, Request};
use unicode_normalization::UnicodeNormalization;

/// Normalize typography, preserving every word, including recording qualifiers.
pub(super) fn key(text: &str) -> String {
    let mut result = String::new();
    for c in text.nfkc().flat_map(char::to_lowercase) {
        match c {
            '\'' | '’' | '‘' => {}
            '&' => result.push_str(" and "),
            c if c.is_alphanumeric() || unicode_normalization::char::is_combining_mark(c) => {
                result.push(c)
            }
            _ => result.push(' '),
        }
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn album_key(text: &str) -> String {
    let mut album = key(text);
    // Release packaging and file-format labels do not describe a performance.
    loop {
        let suffix = [
            " deluxe edition",
            " expanded edition",
            " special edition",
            " bonus tracks edition",
            " deluxe",
            " ep",
            " single",
            " flac",
            " alac",
            " mp3",
            " aac",
            " wav",
        ]
        .into_iter()
        .find(|suffix| album.ends_with(suffix));
        match suffix {
            Some(suffix) => album.truncate(album.len() - suffix.len()),
            None => return album,
        }
    }
}

fn credits(text: &str) -> (String, Option<String>) {
    let original: String = text.nfkc().collect();
    for (start, open) in original
        .char_indices()
        .filter(|(_, c)| matches!(c, '(' | '['))
    {
        let close = if open == '(' { ')' } else { ']' };
        if let Some(end) = original[start + 1..].find(close).map(|end| start + 1 + end) {
            let group = key(&original[start + 1..end]);
            for marker in ["feat ", "ft ", "featuring "] {
                if let Some(guest) = group.strip_prefix(marker) {
                    return (
                        key(&format!("{} {}", &original[..start], &original[end + 1..])),
                        Some(guest.into()),
                    );
                }
            }
        }
    }
    let text = key(text);
    for marker in [" feat ", " ft ", " featuring "] {
        if let Some((base, guest)) = text.split_once(marker) {
            if performance_album(guest)
                || guest
                    .split_whitespace()
                    .any(|word| matches!(word, "mix" | "edit" | "version"))
            {
                return (text, None);
            }
            return (base.to_owned(), Some(guest.to_owned()));
        }
    }
    (text, None)
}

pub(super) fn search_metadata(request: &Request) -> (String, String) {
    (credits(&request.title).0, credits(&request.artist).0)
}

fn performance_album(album: &str) -> bool {
    album.split_whitespace().any(|word| {
        matches!(
            word,
            "live"
                | "acoustic"
                | "unplugged"
                | "demo"
                | "demos"
                | "remix"
                | "remixes"
                | "karaoke"
                | "instrumental"
                | "concert"
                | "sessions"
                | "session"
                | "rehearsal"
        )
    })
}

/// Lower scores are better. Different performances never qualify, even when
/// they happen to have the same duration. Album-less/reissue matches rank lower.
pub(super) fn rank(request: &Request, record: &Record) -> Option<(u8, u8)> {
    if !record.duration.is_finite() || (request.duration - record.duration).abs() > 2.0 {
        return None;
    }
    let (title, title_guest) = credits(&request.title);
    let (artist, artist_guest) = credits(&request.artist);
    let (found_title, found_title_guest) = credits(&record.track_name);
    let (found_artist, found_artist_guest) = credits(&record.artist_name);
    if title.is_empty() || artist.is_empty() || title != found_title || artist != found_artist {
        return None;
    }
    if title_guest
        .as_ref()
        .zip(artist_guest.as_ref())
        .is_some_and(|(a, b)| a != b)
        || found_title_guest
            .as_ref()
            .zip(found_artist_guest.as_ref())
            .is_some_and(|(a, b)| a != b)
    {
        return None;
    }
    let guest = title_guest.or(artist_guest);
    let found_guest = found_title_guest.or(found_artist_guest);
    let credit_rank = match (&guest, &found_guest) {
        (Some(a), Some(b)) if a != b => return None,
        (Some(_), None) | (None, Some(_)) => 1,
        _ => 0,
    };
    let album = album_key(&request.album);
    let found_album = album_key(&record.album_name);
    let album_rank = if album == found_album {
        0
    } else if album.is_empty() {
        1
    } else if performance_album(&album) || performance_album(&found_album) {
        return None;
    } else {
        2
    };
    Some((album_rank, credit_rank))
}
