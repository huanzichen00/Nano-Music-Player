use super::*;
use crate::library::{lyrics::Lyrics, refresh};
use rusqlite::{Connection, params};
use std::fs;

fn audio(path: &Path) {
    fs::write(path, include_bytes!("../../../tests/fixtures/quiet.wav")).unwrap();
}

fn audit(connection: &Connection) {
    connection
        .execute_batch(
            "CREATE TABLE track_writes (path TEXT NOT NULL);
             CREATE TRIGGER track_insert AFTER INSERT ON tracks BEGIN
                 INSERT INTO track_writes VALUES (new.path);
             END;
             CREATE TRIGGER track_update AFTER UPDATE ON tracks BEGIN
                 INSERT INTO track_writes VALUES (new.path);
             END;
             CREATE TRIGGER track_delete AFTER DELETE ON tracks BEGIN
                 INSERT INTO track_writes VALUES (old.path);
             END;",
        )
        .unwrap();
}

fn writes(connection: &Connection) -> usize {
    connection
        .query_row("SELECT COUNT(*) FROM track_writes", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn unchanged_full_refresh_has_zero_track_writes_and_only_changed_sources_are_saved() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    fs::create_dir(&music).unwrap();
    let first = music.join("first.wav");
    let second = music.join("second.wav");
    audio(&first);
    audio(&second);
    let path = directory.path().join("library.sqlite3");
    scan_folders(&path, std::slice::from_ref(&music)).unwrap();
    let connection = Connection::open(&path).unwrap();
    audit(&connection);

    let snapshot = refresh::run(&path, std::slice::from_ref(&music)).unwrap();
    assert!(snapshot.warnings.is_empty());
    assert_eq!(snapshot.tracks.len(), 2);
    assert_eq!(writes(&connection), 0);

    fs::write(first.with_extension("lrc"), "[00:01]Changed source").unwrap();
    let snapshot = refresh::run(&path, std::slice::from_ref(&music)).unwrap();
    assert!(snapshot.warnings.is_empty());
    assert_eq!(writes(&connection), 1);
    let saved: String = connection
        .query_row("SELECT path FROM track_writes", [], |row| row.get(0))
        .unwrap();
    assert_eq!(Path::new(&saved), first);

    refresh::run(&path, &[music]).unwrap();
    assert_eq!(writes(&connection), 1, "the next refresh is read-only");
}

#[test]
fn manual_edits_and_tombstones_never_replace_cached_automatic_lyrics() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("song.wav");
    audio(&source);
    fs::write(source.with_extension("lrc"), "[00:00]Automatic source").unwrap();
    let path = directory.path().join("library.sqlite3");
    let database = Database::open(&path).unwrap();
    refresh_track(&database, &source).unwrap();
    let connection = Connection::open(&path).unwrap();
    audit(&connection);
    let manual = Lyrics::plain_text("Manual snapshot");
    database
        .set_manual_lyrics(&source, &directory.path().join("manual.txt"), &manual)
        .unwrap();
    assert_eq!(refresh_track(&database, &source).unwrap().lyrics, manual);
    assert_eq!(writes(&connection), 0);

    fs::write(source.with_extension("lrc"), "[00:00]New automatic source").unwrap();
    assert_eq!(refresh_track(&database, &source).unwrap().lyrics, manual);
    assert_eq!(writes(&connection), 1);
    assert_eq!(
        database.load().unwrap()[0].lyrics.lines(),
        ["New automatic source"]
    );
    database.use_automatic_lyrics(&source).unwrap();
    assert_eq!(
        refresh_track(&database, &source).unwrap().lyrics.lines(),
        ["New automatic source"]
    );

    database.remove_lyrics(&source).unwrap();
    assert!(refresh_track(&database, &source).unwrap().lyrics.is_empty());
    database.use_automatic_lyrics(&source).unwrap();
    assert_eq!(
        refresh_track(&database, &source).unwrap().lyrics.lines(),
        ["New automatic source"]
    );
    assert_eq!(writes(&connection), 1);
}

#[test]
fn legacy_projected_metadata_repairs_once_without_losing_manual_lyrics() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("song.wav");
    audio(&source);
    fs::write(source.with_extension("txt"), "Automatic source").unwrap();
    let path = directory.path().join("library.sqlite3");
    let database = Database::open(&path).unwrap();
    let mut legacy = read_track(source.clone()).unwrap();
    let manual = Lyrics::plain_text("Persisted manual snapshot");
    database
        .set_manual_lyrics(&source, &directory.path().join("manual.txt"), &manual)
        .unwrap();
    legacy.lyrics = manual.clone();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO tracks VALUES (?, ?, ?)",
            params![
                source.to_string_lossy(),
                metadata_stamp(&source),
                serde_json::to_vec(&legacy).unwrap()
            ],
        )
        .unwrap();
    audit(&connection);

    assert_eq!(
        database.cached(&source, metadata_stamp(&source)),
        Some(legacy)
    );
    assert_eq!(refresh_track(&database, &source).unwrap().lyrics, manual);
    assert_eq!(writes(&connection), 1);
    assert_eq!(
        database.load().unwrap()[0].lyrics.lines(),
        ["Automatic source"]
    );
    assert_eq!(refresh_track(&database, &source).unwrap().lyrics, manual);
    assert_eq!(writes(&connection), 1);
    database.use_automatic_lyrics(&source).unwrap();
    assert_eq!(
        refresh_track(&database, &source).unwrap().lyrics.lines(),
        ["Automatic source"]
    );
    assert_eq!(writes(&connection), 1);
}

#[test]
fn unreadable_legacy_source_keeps_its_snapshot_and_repairs_after_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("song.wav");
    fs::write(&source, "unreadable audio").unwrap();
    let path = directory.path().join("library.sqlite3");
    let database = Database::open(&path).unwrap();
    let legacy = Track {
        path: source.clone(),
        title: "Offline cached title".into(),
        artist: "Artist".into(),
        album: "Album".into(),
        genre: "Genre".into(),
        number: 1,
        duration: 30.0,
        artwork: vec![],
        lyrics: Lyrics::plain_text("Offline lyrics"),
    };
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO tracks VALUES (?, ?, ?)",
            params![
                source.to_string_lossy(),
                metadata_stamp(&source),
                serde_json::to_vec(&legacy).unwrap()
            ],
        )
        .unwrap();
    audit(&connection);
    assert_eq!(refresh_track(&database, &source).unwrap(), legacy);
    assert_eq!(writes(&connection), 0);
    assert!(
        database
            .cached_source(&source, metadata_stamp(&source))
            .is_none()
    );

    audio(&source);
    let recovered = refresh_track(&database, &source).unwrap();
    assert_eq!(recovered.title, "song");
    assert_eq!(writes(&connection), 1);
    assert_eq!(
        database.cached_source(&source, metadata_stamp(&source)),
        Some(recovered)
    );
}

#[test]
fn import_propagates_storage_failures_instead_of_reporting_a_skipped_source() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    fs::create_dir(&music).unwrap();
    audio(&music.join("song.wav"));
    let path = directory.path().join("library.sqlite3");
    Database::open(&path).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_track BEFORE INSERT ON tracks BEGIN
                 SELECT RAISE(FAIL, 'track storage unavailable');
             END;",
        )
        .unwrap();
    assert!(
        scan_folders(&path, &[music])
            .err()
            .unwrap()
            .contains("track storage unavailable")
    );
}

#[test]
fn indexed_import_keeps_existing_overrides_and_deduplicates_overlapping_roots() {
    let directory = tempfile::tempdir().unwrap();
    let first_root = directory.path().join("first");
    let second_root = directory.path().join("second");
    fs::create_dir(&first_root).unwrap();
    fs::create_dir(&second_root).unwrap();
    let first = first_root.join("first.wav");
    let second = second_root.join("second.wav");
    audio(&first);
    audio(&second);
    let path = directory.path().join("library.sqlite3");
    scan_folders(&path, std::slice::from_ref(&first_root)).unwrap();
    let database = Database::open(&path).unwrap();
    let manual = Lyrics::plain_text("Keep manual lyrics from an unvisited root");
    database
        .set_manual_lyrics(&first, &directory.path().join("manual.txt"), &manual)
        .unwrap();
    let imported = scan_folders(&path, &[second_root.clone(), second_root]).unwrap();
    assert_eq!(imported.tracks.len(), 2);
    assert_eq!(
        imported
            .tracks
            .iter()
            .find(|track| track.path == first)
            .unwrap()
            .lyrics,
        manual
    );
    assert_eq!(database.load().unwrap().len(), 2);
    assert!(database.load().unwrap()[0].lyrics.is_empty());
}

#[test]
fn generic_cache_writes_cannot_claim_source_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("song.wav");
    audio(&source);
    let path = directory.path().join("library.sqlite3");
    let database = Database::open(&path).unwrap();
    let mut track = read_track(source.clone()).unwrap();
    track.lyrics = Lyrics::plain_text("Potential projected lyrics");
    let stamp = metadata_stamp(&source);
    database.save(&track, stamp).unwrap();
    assert_eq!(database.cached(&source, stamp), Some(track));
    assert!(database.cached_source(&source, stamp).is_none());
    assert!(refresh_track(&database, &source).unwrap().lyrics.is_empty());
    assert!(database.cached_source(&source, stamp).is_some());
}

#[test]
fn artwork_repairs_preserve_both_trusted_and_legacy_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("song.wav");
    audio(&source);
    let database = Database::open(&directory.path().join("library.sqlite3")).unwrap();
    let mut track = read_track(source.clone()).unwrap();
    track.lyrics = Lyrics::plain_text("Cached lyrics stay intact");
    let stamp = metadata_stamp(&source);

    for trusted in [false, true] {
        if trusted {
            database.save_source(&track, stamp).unwrap();
        } else {
            database.save(&track, stamp).unwrap();
        }
        database
            .update_cached_artwork(&source, stamp, &[1, 2, 3])
            .unwrap();
        let cached = database.cached(&source, stamp).unwrap();
        assert_eq!(cached.artwork, [1, 2, 3]);
        assert_eq!(cached.lyrics, track.lyrics);
        assert_eq!(database.cached_source(&source, stamp).is_some(), trusted);
        database
            .update_cached_artwork(&source, stamp + 1, &[4, 5, 6])
            .unwrap();
        assert_eq!(database.cached(&source, stamp).unwrap(), cached);
    }
}
