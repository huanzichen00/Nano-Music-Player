use nano_music_player::library::{database::Database, scanner, statistics::StatisticsStore};
use std::{fs, path::PathBuf};

#[test]
fn ratings_migrate_persist_clear_and_leave_audio_and_statistics_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("library.sqlite3");
    let audio = tmp.path().join("song.wav");
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/quiet.wav"),
        &audio,
    )
    .unwrap();
    let original = fs::read(&audio).unwrap();
    let track = scanner::read_track(audio.clone()).unwrap();
    Database::open(&db).unwrap().save(&track, 0).unwrap();
    {
        let store = StatisticsStore::open(&db).unwrap();
        assert!(store.ratings().unwrap().is_empty());
        store
            .register_library_paths(std::slice::from_ref(&audio), 100)
            .unwrap();
        store.record_play(&audio, 200, 30).unwrap();
        for value in 0..=5 {
            store.set_rating(&audio, value).unwrap();
            assert_eq!(store.ratings().unwrap()[&audio], value);
        }
        assert!(store.set_rating(&audio, 6).is_err());
        assert_eq!(store.ratings().unwrap()[&audio], 5);
    }
    let store = StatisticsStore::open(&db).unwrap();
    assert_eq!(store.ratings().unwrap()[&audio], 5);
    scanner::refresh_track(&Database::open(&db).unwrap(), &audio).unwrap();
    assert_eq!(
        store.ratings().unwrap()[&audio],
        5,
        "rescans preserve user ratings"
    );
    assert_eq!(
        store.track_statistics(&audio).unwrap().unwrap().play_count,
        1
    );
    assert_eq!(store.listening_history(10).unwrap().len(), 1);
    store.set_rating(&audio, 0).unwrap();
    drop(store);
    assert_eq!(
        StatisticsStore::open(&db).unwrap().ratings().unwrap()[&audio],
        0
    );
    assert_eq!(
        fs::read(audio).unwrap(),
        original,
        "rating never rewrites audio tags"
    );
}

#[test]
fn confirmed_renames_transfer_ratings_without_requiring_playback_history() {
    let tmp = tempfile::tempdir().unwrap();
    let store = StatisticsStore::open(&tmp.path().join("library.sqlite3")).unwrap();
    let old = PathBuf::from("/music/old.wav");
    let new = PathBuf::from("/music/new.wav");
    store.set_rating(&old, 4).unwrap();
    store.set_rating(&new, 2).unwrap();
    store.rename_path(&old, &new).unwrap();
    assert!(!store.ratings().unwrap().contains_key(&old));
    assert_eq!(store.ratings().unwrap()[&new], 4);
    store.rename_path(&old, &new).unwrap();
    assert_eq!(
        store.ratings().unwrap()[&new],
        4,
        "repeated rename is idempotent"
    );
    store.set_rating(&old, 0).unwrap();
    store.rename_path(&old, &new).unwrap();
    assert_eq!(
        store.ratings().unwrap()[&new],
        0,
        "explicit zero follows the source"
    );
}

#[test]
fn failed_rating_write_preserves_last_saved_value() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("library.sqlite3");
    let store = StatisticsStore::open(&db).unwrap();
    let audio = PathBuf::from("/music/song.wav");
    store.set_rating(&audio, 2).unwrap();
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_rating BEFORE INSERT ON track_ratings WHEN NEW.stars = 5 BEGIN SELECT RAISE(ABORT, 'rating failure'); END;").unwrap();
    assert!(store.set_rating(&audio, 5).is_err());
    assert_eq!(store.ratings().unwrap()[&audio], 2);
}
