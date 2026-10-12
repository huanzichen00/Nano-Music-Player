use lofty::{
    TextEncoding,
    config::WriteOptions,
    file::TaggedFileExt,
    id3::v2::{
        BinaryFrame, Frame, FrameId, Id3v2Tag, SyncTextContentType, SynchronizedTextFrame,
        TimestampFormat, UnsynchronizedTextFrame,
    },
    prelude::{Accessor, TagExt},
    probe::Probe,
    tag::{ItemKey, Tag},
};
use nano_music_player::library::{
    lyrics::{self, Line},
    scanner,
};
use rodio::{Decoder, Source};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

const FORMATS: [&str; 4] = ["mp3", "m4a", "flac", "wav"];

fn fixture(extension: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("quiet.{extension}"))
}

fn temporary_fixture(directory: &Path, extension: &str) -> PathBuf {
    let path = directory.join(format!("sample.{extension}"));
    fs::copy(fixture(extension), &path).unwrap();
    path
}

fn assert_decodes_quiet_audio(path: &Path) {
    // Decode only. This test must not open an audio output device or play sound.
    let decoder = Decoder::try_from(File::open(path).unwrap())
        .unwrap_or_else(|error| panic!("Cannot decode {}: {error}", path.display()));
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    assert_eq!(channels, 1, "{}", path.display());
    assert_eq!(sample_rate, 44_100, "{}", path.display());
    let samples: Vec<f32> = decoder.collect();
    let duration = samples.len() as f64 / f64::from(sample_rate) / f64::from(channels);
    assert!(
        (0.20..0.60).contains(&duration),
        "Decoded duration {duration} for {}",
        path.display()
    );
    assert!(samples.iter().all(|sample| sample.is_finite()));
    let peak = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max);
    assert!(
        (0.0001..0.02).contains(&peak),
        "Unexpected silent/loud samples in {}: peak {peak}",
        path.display()
    );
}

fn assert_scans_without_lyrics(extension: &str) {
    let path = fixture(extension);
    let track = scanner::read_track(path.clone())
        .unwrap_or_else(|error| panic!("Cannot scan {}: {error}", path.display()));
    assert_eq!(track.path, path);
    assert_eq!(
        track.title, "quiet",
        "{extension} must fall back to its filename"
    );
    assert_eq!(track.artist, "Unknown Artist");
    assert_eq!(track.album, "Unknown Album");
    assert_eq!(track.genre, "Unknown Genre");
    assert!(
        (0.20..0.60).contains(&track.duration),
        "{extension} duration {}",
        track.duration
    );
    assert!(track.artwork.is_empty());
    assert!(
        track.lyrics.is_empty(),
        "{extension} has no embedded or sidecar lyrics"
    );
    assert_eq!(track.lyrics.active(0.15), None);
}

macro_rules! format_tests {
    ($decoder:ident, $scanner:ident, $extension:literal) => {
        #[test]
        fn $decoder() {
            assert_decodes_quiet_audio(&fixture($extension));
        }
        #[test]
        fn $scanner() {
            assert_scans_without_lyrics($extension);
        }
    };
}

format_tests!(mp3_decodes, mp3_scans_without_lyrics, "mp3");
format_tests!(m4a_aac_decodes, m4a_scans_without_lyrics, "m4a");
format_tests!(flac_decodes, flac_scans_without_lyrics, "flac");
format_tests!(wav_pcm_decodes, wav_scans_without_lyrics, "wav");

fn embed_generic_lyrics(path: &Path, text: &str) {
    let audio = Probe::open(path).unwrap().read().unwrap();
    let mut tag = Tag::new(audio.file_type().primary_tag_type());
    tag.set_title("Synthetic title".to_owned());
    tag.set_artist("Synthetic artist".to_owned());
    tag.set_album("Synthetic album".to_owned());
    tag.set_genre("Test tone".to_owned());
    tag.set_track(7);
    assert!(
        tag.insert_text(ItemKey::Lyrics, text.to_owned()),
        "Lyrics unsupported in {}",
        path.display()
    );
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

#[test]
fn supported_formats_import_embedded_plain_lyrics_and_metadata() {
    let directory = tempfile::tempdir().unwrap();
    for extension in FORMATS {
        let path = temporary_fixture(directory.path(), extension);
        embed_generic_lyrics(&path, "First synthetic line\n第二句歌词");
        let track = scanner::read_track(path.clone()).unwrap();
        assert_eq!(track.title, "Synthetic title", "{extension}");
        assert_eq!(track.artist, "Synthetic artist", "{extension}");
        assert_eq!(track.album, "Synthetic album", "{extension}");
        assert_eq!(track.genre, "Test tone", "{extension}");
        assert_eq!(track.number, 7, "{extension}");
        assert_eq!(
            track.lyrics.lines(),
            ["First synthetic line", "第二句歌词"],
            "{extension}"
        );
        assert!(track.lyrics.timed.is_empty());
        assert_eq!(track.lyrics.active(0.2), None);
        assert_decodes_quiet_audio(&path);
    }
}

#[test]
fn supported_formats_import_embedded_lrc_lyrics_and_highlight_at_playback_time() {
    let directory = tempfile::tempdir().unwrap();
    for extension in FORMATS {
        let path = temporary_fixture(directory.path(), extension);
        embed_generic_lyrics(
            &path,
            "[ar:Synthetic artist]\n[offset:25]\n[00:00.050][00:00.200]Repeated\n[00:00.125]中间",
        );
        let track = scanner::read_track(path.clone()).unwrap();
        assert_eq!(
            track.lyrics.timed,
            [
                Line {
                    at_ms: 25,
                    text: "Repeated".to_owned()
                },
                Line {
                    at_ms: 100,
                    text: "中间".to_owned()
                },
                Line {
                    at_ms: 175,
                    text: "Repeated".to_owned()
                },
            ],
            "{extension}"
        );
        assert_eq!(track.lyrics.active(0.024), None);
        assert_eq!(track.lyrics.active(0.025), Some(0));
        assert_eq!(track.lyrics.active(0.100), Some(1));
        assert_eq!(track.lyrics.active(0.175), Some(2));
        assert_decodes_quiet_audio(&path);
    }
}

#[test]
fn sidecar_lyrics_override_embedded_lyrics_and_empty_sidecars_fall_back() {
    let directory = tempfile::tempdir().unwrap();
    for extension in FORMATS {
        let path = temporary_fixture(directory.path(), extension);
        embed_generic_lyrics(&path, "Embedded fallback");
        let sidecar = path.with_extension("lrc");
        fs::write(&sidecar, "[00:00.100]Sidecar wins").unwrap();
        let track = scanner::read_track(path.clone()).unwrap();
        assert_eq!(track.lyrics.lines(), ["Sidecar wins"], "{extension}");
        assert_eq!(track.lyrics.timed[0].at_ms, 100);
        fs::write(&sidecar, " \n\n ").unwrap();
        let track = scanner::read_track(path).unwrap();
        assert_eq!(track.lyrics.lines(), ["Embedded fallback"], "{extension}");
        fs::remove_file(&sidecar).unwrap();
    }
}

fn embed_sylt(path: &Path, timestamp_format: TimestampFormat, content_type: SyncTextContentType) {
    let synchronized = SynchronizedTextFrame::new(
        TextEncoding::UTF8,
        *b"eng",
        timestamp_format,
        content_type,
        Some("Synthetic synchronized lyrics".to_owned()),
        vec![(175, " Later ".to_owned()), (50, " First 中文 ".to_owned())],
    );
    let mut tag = Id3v2Tag::new();
    tag.set_title("SYLT test".to_owned());
    tag.insert(Frame::Binary(BinaryFrame::new(
        FrameId::Valid("SYLT".into()),
        synchronized.as_bytes().unwrap(),
    )));
    tag.insert(Frame::UnsynchronizedText(UnsynchronizedTextFrame::new(
        TextEncoding::UTF8,
        *b"eng",
        String::new(),
        "Plain fallback".to_owned(),
    )));
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

#[test]
fn mp3_sylt_millisecond_lyrics_are_sorted_and_preferred_to_plain_embedded_lyrics() {
    let directory = tempfile::tempdir().unwrap();
    let path = temporary_fixture(directory.path(), "mp3");
    embed_sylt(&path, TimestampFormat::MS, SyncTextContentType::Lyrics);
    let expected = vec![
        Line {
            at_ms: 50,
            text: "First 中文".to_owned(),
        },
        Line {
            at_ms: 175,
            text: "Later".to_owned(),
        },
    ];
    assert_eq!(lyrics::embedded_sylt(&path).unwrap().timed, expected);
    let track = scanner::read_track(path.clone()).unwrap();
    assert_eq!(track.lyrics.timed, expected);
    assert_eq!(track.lyrics.active(0.049), None);
    assert_eq!(track.lyrics.active(0.050), Some(0));
    assert_eq!(track.lyrics.active(0.175), Some(1));
    fs::write(
        path.with_extension("lrc"),
        "[00:00.100]Sidecar preferred to SYLT",
    )
    .unwrap();
    assert_eq!(
        scanner::read_track(path.clone()).unwrap().lyrics.lines(),
        ["Sidecar preferred to SYLT"]
    );
    assert_decodes_quiet_audio(&path);
}

#[test]
fn unsupported_sylt_time_units_and_non_lyric_content_fall_back_to_plain_lyrics() {
    let directory = tempfile::tempdir().unwrap();
    for (units, content) in [
        (TimestampFormat::MPEG, SyncTextContentType::Lyrics),
        (TimestampFormat::MS, SyncTextContentType::Events),
    ] {
        let path = temporary_fixture(directory.path(), "mp3");
        embed_sylt(&path, units, content);
        assert!(lyrics::embedded_sylt(&path).is_none());
        let track = scanner::read_track(path).unwrap();
        assert_eq!(track.lyrics.lines(), ["Plain fallback"]);
        assert!(track.lyrics.timed.is_empty());
    }
}

#[test]
fn malformed_sylt_is_ignored_without_losing_plain_lyrics_or_audio() {
    let directory = tempfile::tempdir().unwrap();
    let path = temporary_fixture(directory.path(), "mp3");
    let mut tag = Id3v2Tag::new();
    tag.insert(Frame::Binary(BinaryFrame::new(
        FrameId::Valid("SYLT".into()),
        vec![0xff, 0x00],
    )));
    tag.insert(Frame::UnsynchronizedText(UnsynchronizedTextFrame::new(
        TextEncoding::UTF8,
        *b"eng",
        String::new(),
        "Plain fallback".to_owned(),
    )));
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    assert!(lyrics::embedded_sylt(&path).is_none());
    assert_eq!(
        scanner::read_track(path.clone()).unwrap().lyrics.lines(),
        ["Plain fallback"]
    );
    assert_decodes_quiet_audio(&path);
}

#[test]
fn corrupt_audio_is_reported_as_a_read_error_for_each_supported_extension() {
    let directory = tempfile::tempdir().unwrap();
    for extension in FORMATS {
        let path = directory.path().join(format!("corrupt.{extension}"));
        fs::write(&path, b"This is not an audio file").unwrap();
        assert!(
            scanner::read_track(path.clone()).is_err(),
            "scanner accepted corrupt {extension}"
        );
        assert!(
            Decoder::try_from(File::open(path).unwrap()).is_err(),
            "decoder accepted corrupt {extension}"
        );
    }
}
