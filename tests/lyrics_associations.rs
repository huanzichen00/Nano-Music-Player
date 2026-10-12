use nano_music_player::library::{
    Track,
    database::{Database, LyricsAssociation},
    lyrics, scanner,
};
use std::{fs, io::Write, path::PathBuf};

fn temp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn track(path: PathBuf) -> Track {
    scanner::read_track(path).unwrap()
}

fn cached(dbpath: &std::path::Path, audio: &std::path::Path) {
    let db = Database::open(dbpath).unwrap();
    db.save(&track(audio.to_path_buf()), 1).unwrap();
}

fn loaded(dbpath: &std::path::Path) -> Track {
    scanner::load(dbpath.to_path_buf())
        .recv()
        .unwrap()
        .unwrap()
        .into_iter()
        .next()
        .unwrap()
}

#[test]
fn manual_override_remove_reset_and_restart_are_persistent() {
    let dir = temp();
    let audio = dir.path().join("song.mp3");
    fs::copy("tests/fixtures/quiet.mp3", &audio).unwrap();
    let dbpath = dir.path().join("library.sqlite");
    let source = dir.path().join("manual.txt");
    fs::write(&source, "[literal]\n\nManual line").unwrap();
    {
        let db = Database::open(&dbpath).unwrap();
        let l = lyrics::read_source(&source).unwrap();
        db.set_manual_lyrics(&audio, &source, &l).unwrap();
        let mut t = track(audio.clone());
        db.apply_lyrics(&mut t).unwrap();
        assert_eq!(t.lyrics.lines(), ["[literal]", "", "Manual line"]);
    }
    {
        let db = Database::open(&dbpath).unwrap();
        assert!(matches!(
            db.lyrics_association(&audio).unwrap(),
            LyricsAssociation::Manual { .. }
        ));
        let mut t = track(audio.clone());
        db.apply_lyrics(&mut t).unwrap();
        assert_eq!(t.lyrics.lines()[0], "[literal]");
        db.remove_lyrics(&audio).unwrap();
        let mut t = track(audio.clone());
        db.apply_lyrics(&mut t).unwrap();
        assert!(t.lyrics.is_empty());
        db.use_automatic_lyrics(&audio).unwrap();
        assert_eq!(
            db.lyrics_association(&audio).unwrap(),
            LyricsAssociation::Automatic
        );
    }
}

#[test]
fn sidecars_choose_case_insensitive_lrc_then_txt_and_txt_is_literal() {
    let dir = temp();
    let audio = dir.path().join("song.mp3");
    fs::copy("tests/fixtures/quiet.mp3", &audio).unwrap();
    let txt = dir.path().join("SONG.TXT");
    fs::write(&txt, "[00:01.00]literal\n\n[meta]").unwrap();
    let from_txt = lyrics::sidecar(&audio).unwrap();
    assert_eq!(from_txt.lines(), ["[00:01.00]literal", "", "[meta]"]);
    let lrc = dir.path().join("Song.LrC");
    fs::write(&lrc, "[00:01.00]synced").unwrap();
    let from_lrc = lyrics::sidecar(&audio).unwrap();
    assert_eq!(from_lrc.timed[0].text, "synced");
    assert_eq!(from_lrc.plain, Vec::<String>::new());
}

#[test]
fn invalid_manual_input_does_not_replace_existing_override() {
    let dir = temp();
    let audio = dir.path().join("a.mp3");
    let db = Database::open(&dir.path().join("db.sqlite")).unwrap();
    let good = lyrics::Lyrics::plain_text("good");
    db.set_manual_lyrics(&audio, &dir.path().join("good.txt"), &good)
        .unwrap();
    let invalid = lyrics::Lyrics::default();
    assert!(
        db.set_manual_lyrics(&audio, &dir.path().join("bad.txt"), &invalid)
            .is_err()
    );
    let LyricsAssociation::Manual {
        lyrics: retained, ..
    } = db.lyrics_association(&audio).unwrap()
    else {
        panic!()
    };
    assert_eq!(retained.lines(), ["good"]);
}

#[test]
fn blank_sidecars_fall_through_to_the_next_usable_candidate() {
    let dir = temp();
    let audio = dir.path().join("song.mp3");
    fs::copy("tests/fixtures/quiet.mp3", &audio).unwrap();
    let lrc = dir.path().join("song.lrc");
    let txt = dir.path().join("song.txt");
    fs::write(&lrc, " \n\n").unwrap();
    fs::write(&txt, "   \n\t").unwrap();
    assert!(lyrics::Lyrics::plain_text(" \n\t").is_empty());
    assert!(lyrics::sidecar(&audio).is_none());
    fs::write(&txt, "fallback\n\nline").unwrap();
    assert_eq!(
        lyrics::sidecar(&audio).unwrap().lines(),
        ["fallback", "", "line"]
    );
}

#[test]
fn load_refreshes_automatic_sidecars_then_falls_back_after_removal() {
    let dir = temp();
    let audio = dir.path().join("song.mp3");
    fs::copy("tests/fixtures/quiet.mp3", &audio).unwrap();
    let dbpath = dir.path().join("library.sqlite");
    cached(&dbpath, &audio);

    let txt = dir.path().join("SONG.TXT");
    fs::write(&txt, "automatic txt").unwrap();
    assert_eq!(loaded(&dbpath).lyrics.lines(), ["automatic txt"]);

    let lrc = dir.path().join("song.LRC");
    fs::write(&lrc, "[00:01.00]automatic lrc").unwrap();
    assert_eq!(loaded(&dbpath).lyrics.timed[0].text, "automatic lrc");

    fs::remove_file(&lrc).unwrap();
    fs::remove_file(&txt).unwrap();
    assert!(loaded(&dbpath).lyrics.is_empty());
}

#[test]
fn manual_snapshot_and_tombstone_survive_source_and_audio_changes_on_load() {
    let dir = temp();
    let audio = dir.path().join("song.mp3");
    fs::copy("tests/fixtures/quiet.mp3", &audio).unwrap();
    let dbpath = dir.path().join("library.sqlite");
    cached(&dbpath, &audio);
    let source = dir.path().join("external.txt");
    fs::write(&source, "persistent manual").unwrap();
    let db = Database::open(&dbpath).unwrap();
    db.set_manual_lyrics(&audio, &source, &lyrics::read_source(&source).unwrap())
        .unwrap();
    drop(db);

    fs::remove_file(&source).unwrap();
    // Change the audio stamp/content after the cache entry and manual association were saved.
    fs::OpenOptions::new()
        .append(true)
        .open(&audio)
        .unwrap()
        .write_all(b"changed")
        .unwrap();
    assert_eq!(loaded(&dbpath).lyrics.lines(), ["persistent manual"]);

    let db = Database::open(&dbpath).unwrap();
    db.remove_lyrics(&audio).unwrap();
    drop(db);
    assert!(loaded(&dbpath).lyrics.is_empty());
    assert_eq!(
        Database::open(&dbpath)
            .unwrap()
            .lyrics_association(&audio)
            .unwrap(),
        LyricsAssociation::Removed
    );

    fs::write(audio.with_extension("txt"), "restored automatic").unwrap();
    let db = Database::open(&dbpath).unwrap();
    db.use_automatic_lyrics(&audio).unwrap();
    drop(db);
    assert_eq!(loaded(&dbpath).lyrics.lines(), ["restored automatic"]);
}
