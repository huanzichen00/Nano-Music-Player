use nano_music_player::library::playlists;
use rusqlite::Connection;
use std::path::PathBuf;

#[test]
fn adds_playlist_tables_without_changing_existing_library_tables() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("library.sqlite3");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute("CREATE TABLE tracks (path TEXT PRIMARY KEY, modified INTEGER NOT NULL, metadata BLOB NOT NULL)", [])
        .unwrap();
    connection
        .execute("INSERT INTO tracks VALUES ('song', 3, X'0102')", [])
        .unwrap();
    drop(connection);

    assert!(playlists::load(&database).unwrap().is_empty());
    let connection = Connection::open(&database).unwrap();
    let track: (String, i64, Vec<u8>) = connection
        .query_row("SELECT path, modified, metadata FROM tracks", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(track, ("song".into(), 3, vec![1, 2]));
}

#[test]
fn playlist_and_order_survive_database_restart_and_missing_files() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("library.sqlite3");
    let paths = vec![
        directory.path().join("不存在-🎵.flac"),
        directory.path().join("second.mp3"),
    ];
    let created = playlists::create(&database, "  夜の曲  ", &paths).unwrap();
    assert_eq!(created.name, "夜の曲");
    drop(created);
    let loaded = playlists::load(&database).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].tracks, paths);
    assert!(!loaded[0].tracks[0].exists());
}

#[test]
fn duplicate_names_are_rejected_case_insensitively() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("library.sqlite3");
    let path = PathBuf::from("song.flac");
    playlists::create(&database, "Morning", std::slice::from_ref(&path)).unwrap();
    assert!(playlists::create(&database, "mORNING", &[PathBuf::from("other.flac")]).is_err());
    assert_eq!(playlists::load(&database).unwrap().len(), 1);
}

#[test]
fn validation_and_storage_failures_leave_no_partial_playlist() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("library.sqlite3");
    let path = PathBuf::from("song.flac");
    assert!(playlists::create(&database, " \n ", std::slice::from_ref(&path)).is_err());
    assert!(playlists::create(&database, &"界".repeat(101), std::slice::from_ref(&path)).is_err());
    assert!(playlists::create(&database, "bad\tname", std::slice::from_ref(&path)).is_err());
    assert!(playlists::create(&database, "empty", &[]).is_err());
    assert!(playlists::create(&database, "repeat", &[path.clone(), path]).is_err());

    // Invalid inputs fail before opening the database, so initialize the playlist schema first.
    playlists::load(&database).unwrap();
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_playlist_track BEFORE INSERT ON playlist_tracks
             BEGIN SELECT RAISE(ABORT, 'forced failure'); END;",
        )
        .unwrap();
    drop(connection);
    assert!(playlists::create(&database, "atomic", &[PathBuf::from("valid.flac")]).is_err());
    assert!(playlists::load(&database).unwrap().is_empty());
}
