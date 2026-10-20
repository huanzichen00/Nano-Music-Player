#![cfg(target_os = "macos")]
use nano_music_player::library::refresh;
use std::fs::File;

#[test]
fn refreshing_leaves_file_handles_for_playback_under_macos_limit() {
    if std::env::var_os("NANO_REFRESH_FD_CHILD").is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "refreshing_leaves_file_handles_for_playback_under_macos_limit",
                "--nocapture",
            ])
            .env("NANO_REFRESH_FD_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "child failed: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    #[repr(C)]
    struct Limit {
        current: u64,
        maximum: u64,
    }
    unsafe extern "C" {
        fn getrlimit(resource: i32, limit: *mut Limit) -> i32;
        fn setrlimit(resource: i32, limit: *const Limit) -> i32;
    }
    let mut limit = Limit {
        current: 0,
        maximum: 0,
    };
    unsafe {
        assert_eq!(getrlimit(8, &mut limit), 0); // Darwin RLIMIT_NOFILE
        limit.current = 256.min(limit.maximum);
        assert_eq!(setrlimit(8, &limit), 0);
    }
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("music");
    std::fs::create_dir(&music).unwrap();
    for i in 0..512 {
        std::fs::write(
            music.join(format!("{i}.wav")),
            include_bytes!("fixtures/quiet.wav"),
        )
        .unwrap();
    }
    let database = tmp.path().join("library.sqlite3");
    let source = music.clone();
    let refreshed = std::thread::spawn(move || refresh::run(&database, &[source]).unwrap())
        .join()
        .unwrap();
    assert_eq!(refreshed.tracks.len(), 512);
    assert!(refreshed.warnings.is_empty());
    let handles: Vec<_> = (0..32)
        .map(|_| File::open(music.join("0.wav")).expect("refresh left no handles for playback"))
        .collect();
    let decoder = rodio::Decoder::try_from(File::open(music.join("1.wav")).unwrap()).unwrap();
    assert!(decoder.take(10).all(|sample| sample.is_finite()));
    drop(handles);
}
