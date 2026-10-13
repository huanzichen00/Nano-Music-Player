use nano_music_player::library::{database::Database, scanner};
use std::fs;

#[test]
fn imported_root_includes_empty_folders_and_new_sibling_albums() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("music");
    fs::create_dir_all(&root).unwrap();
    let db_path = tmp.path().join("library.sqlite3");
    let initial = scanner::scan_folders(&db_path, std::slice::from_ref(&root)).unwrap();
    assert_eq!(initial.roots, vec![root.clone()]);
    assert!(initial.tracks.is_empty());
    fs::create_dir(root.join("new-album")).unwrap();
    let audio = root.join("new-album/one.wav");
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/quiet.wav"),
        &audio,
    )
    .unwrap();
    let snapshot = scanner::reconcile_root(&root, &[]).unwrap();
    assert_eq!(snapshot.paths, vec![audio.clone()]);
    let scanned = scanner::scan_folders(&db_path, std::slice::from_ref(&root)).unwrap();
    assert_eq!(scanned.tracks.len(), 1);
    assert_eq!(
        Database::open(&db_path).unwrap().load().unwrap()[0].path,
        audio
    );
}

#[test]
fn root_reconciliation_detects_nested_add_remove_and_preserves_unavailable_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("music");
    fs::create_dir_all(root.join("album")).unwrap();
    let old = root.join("old.wav");
    let new = root.join("album/new.wav");
    fs::write(&new, b"audio hint; decoding is a separate worker job").unwrap();
    let snapshot = scanner::reconcile_root(&root, std::slice::from_ref(&old)).unwrap();
    assert_eq!(snapshot.paths, vec![new.clone()]);
    assert_eq!(snapshot.missing, vec![old.clone()]);
    fs::rename(&root, tmp.path().join("disconnected")).unwrap();
    let snapshot = scanner::reconcile_root(&root, &[old, new]).unwrap();
    assert!(snapshot.paths.is_empty());
    assert!(snapshot.missing.is_empty());
}

#[test]
fn gapless_queue_cursor_handles_missing_tracks_duplicate_entries_and_repeat() {
    use nano_music_player::{app::state::State, audio::Repeat, library::Track};
    let mut state = State::default();
    state.tracks = (0..3)
        .map(|id| Track {
            path: format!("/music/{id}.wav").into(),
            title: id.to_string(),
            artist: "Artist".into(),
            album: "Album".into(),
            genre: String::new(),
            number: id,
            duration: 180.0,
            artwork: Vec::new(),
            lyrics: Default::default(),
        })
        .collect();
    state.queue.set(vec![0, 1, 0, 2], 0, false);
    state.current = Some(0);
    let missing = state.tracks[1].path.clone();
    state.mark_track_available(&missing, false);
    assert_eq!(state.gapless_successor(), Some((2, 0)));
    state.queue.position = 2;
    assert_eq!(state.gapless_successor(), Some((3, 2)));
    state.queue.position = 3;
    state.current = Some(2);
    assert_eq!(state.gapless_successor(), None);
    state.repeat = Repeat::All;
    assert_eq!(state.gapless_successor(), Some((0, 0)));
    state.repeat = Repeat::One;
    assert_eq!(state.gapless_successor(), Some((3, 2)));
}

#[test]
#[cfg(unix)]
fn root_reconciliation_preserves_known_audio_replaced_by_valid_symlinks() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("music");
    fs::create_dir_all(root.join("album")).unwrap();
    let file = root.join("one.wav");
    let nested = root.join("album/two.wav");
    fs::write(&file, b"audio hint").unwrap();
    fs::write(&nested, b"audio hint").unwrap();
    let moved_file = tmp.path().join("kept.wav");
    fs::rename(&file, &moved_file).unwrap();
    symlink(&moved_file, &file).unwrap();
    let moved_album = tmp.path().join("kept-album");
    fs::rename(root.join("album"), &moved_album).unwrap();
    symlink(&moved_album, root.join("album")).unwrap();
    let snapshot = scanner::reconcile_root(&root, &[file.clone(), nested.clone()]).unwrap();
    assert!(snapshot.missing.is_empty());
    assert!(snapshot.paths.contains(&file));
    assert!(snapshot.paths.contains(&nested));
    fs::remove_file(&file).unwrap();
    let snapshot = scanner::reconcile_root(&root, &[file.clone(), nested]).unwrap();
    assert_eq!(snapshot.missing, vec![file]);
}
