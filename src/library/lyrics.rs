use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub at_ms: i64,
    pub text: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Lyrics {
    pub timed: Vec<Line>,
    pub plain: Vec<String>,
}

impl Lyrics {
    pub fn parse(text: &str) -> Self {
        let text = text.trim_start_matches('\u{feff}');
        // Reuse lrc's metadata handling and ordered timestamp collection. Normalize decimal
        // fractions here because its textual parser only supports hundredths, not millisecond LRC.
        let mut parsed = lrc::Lyrics::new();
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let mut rest = line;
            let mut timestamps = Vec::new();
            while let Some(tag) = rest.strip_prefix('[').and_then(|v| v.split_once(']')) {
                let Some(time) = parse_time(tag.0) else {
                    break;
                };
                timestamps.push(time);
                rest = tag.1;
            }
            if timestamps.is_empty() {
                if let Ok(part) = lrc::Lyrics::from_str(line) {
                    for tag in part.metadata.iter() {
                        parsed.metadata.insert(tag.clone());
                    }
                    for line in part.get_lines() {
                        let _ = parsed.add_line(line);
                    }
                }
            } else {
                for time in timestamps {
                    let _ = parsed.add_timed_line(lrc::TimeTag::new(time), rest.trim());
                }
            }
        }
        // LRC offset is the advance applied to playback: positive means show the line earlier.
        let offset = parsed
            .metadata
            .get_text("offset")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0);
        let timed = parsed
            .get_timed_lines()
            .iter()
            .map(|(time, text)| Line {
                at_ms: time.get_timestamp().saturating_sub(offset).max(0),
                text: strip_word_tags(text),
            })
            .collect();
        Self {
            timed,
            plain: parsed.get_lines().to_vec(),
        }
    }

    pub fn active(&self, position: f64) -> Option<usize> {
        let milliseconds = (position.max(0.0) * 1000.0).round() as i64;
        self.timed
            .partition_point(|line| line.at_ms <= milliseconds)
            .checked_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.timed.is_empty() && self.plain.is_empty()
    }
    pub fn lines(&self) -> Vec<String> {
        if self.timed.is_empty() {
            self.plain.clone()
        } else {
            self.timed.iter().map(|line| line.text.clone()).collect()
        }
    }
}

fn strip_word_tags(text: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        result.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        if let Some(end) = tail.find('>')
            && parse_time(&tail[..end]).is_some()
        {
            rest = &tail[end + 1..];
            continue;
        }
        result.push('<');
        rest = tail;
    }
    result.push_str(rest);
    result
}

fn parse_time(tag: &str) -> Option<i64> {
    let (minutes, seconds) = tag.split_once(':')?;
    let minutes = minutes.parse::<i64>().ok()?;
    let (seconds, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let seconds = seconds.parse::<i64>().ok()?;
    if minutes < 0 || !(0..60).contains(&seconds) || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let fraction = if fraction.is_empty() {
        0
    } else {
        let digits: String = fraction.chars().take(3).collect();
        digits.parse::<i64>().ok()? * 10_i64.pow(3 - digits.len() as u32)
    };
    minutes
        .checked_mul(60_000)?
        .checked_add(seconds * 1000)?
        .checked_add(fraction)
}

pub fn sidecar(path: &Path) -> Option<Lyrics> {
    let bytes = std::fs::read(path.with_extension("lrc"))
        .or_else(|_| std::fs::read(path.with_extension("LRC")))
        .ok()?;
    let text = if bytes.starts_with(&[0xff, 0xfe]) {
        encoding_rs::UTF_16LE.decode(&bytes).0.into_owned()
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        encoding_rs::UTF_16BE.decode(&bytes).0.into_owned()
    } else {
        String::from_utf8(bytes.clone())
            .unwrap_or_else(|_| encoding_rs::GBK.decode(&bytes).0.into_owned())
    };
    let lyrics = Lyrics::parse(&text);
    (!lyrics.is_empty()).then_some(lyrics)
}

pub fn embedded_sylt(path: &Path) -> Option<Lyrics> {
    use lofty::{
        config::ParseOptions,
        file::AudioFile,
        id3::v2::{Frame, FrameId, SyncTextContentType, SynchronizedTextFrame, TimestampFormat},
        mpeg::MpegFile,
    };
    if !path.extension()?.eq_ignore_ascii_case("mp3") {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let file = MpegFile::read_from(&mut file, ParseOptions::new().read_properties(false)).ok()?;
    let Frame::Binary(frame) = file.id3v2()?.get(&FrameId::Valid("SYLT".into()))? else {
        return None;
    };
    let frame = SynchronizedTextFrame::parse(&frame.data, frame.flags()).ok()?;
    if frame.timestamp_format != TimestampFormat::MS
        || frame.content_type != SyncTextContentType::Lyrics
    {
        return None;
    }
    let mut timed: Vec<Line> = frame
        .content
        .into_iter()
        .map(|(at, text)| Line {
            at_ms: i64::from(at),
            text: text.trim().to_owned(),
        })
        .collect();
    timed.sort_by_key(|line| line.at_ms);
    Some(Lyrics {
        timed,
        plain: Vec::new(),
    })
}
