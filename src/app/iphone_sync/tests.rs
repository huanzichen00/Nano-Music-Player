use super::*;
use serde_json::json;
use std::time::Instant;

const FIXTURE: &str = r#"
import json, pathlib, sys, time
a=sys.argv[1:]
root=pathlib.Path(a[a.index('--config-dir')+1])
def emit(**kw): print(json.dumps(kw), flush=True)
library=dict(available_tracks=2,unavailable_tracks=0,audio_bytes=12000000,playlists=[dict(id=7,name='Mix',tracks=2)])
if '--list-devices' in a:
 if (root/'FAIL_DEVICES').exists():
  emit(phase='error',code='device_discovery_failed');sys.exit(1)
 emit(phase='devices',devices=[dict(id='usb-test',name='Test iPhone')])
elif '--inspect-library' in a: emit(phase='library',library=library)
elif '--plan' in a: emit(phase='plan',plan=library)
else:
 out=pathlib.Path(a[a.index('--output')+1]); (out/'arguments.json').write_text(json.dumps(a))
 cancel=pathlib.Path(a[a.index('--cancel-file')+1])
 emit(phase='copy',cancellable=True)
 end=time.monotonic()+6
 while not (out/'RELEASE_COPY').exists():
  if cancel.exists(): emit(phase='cancelled');sys.exit(130)
  if time.monotonic()>end: sys.exit(2)
  time.sleep(.005)
 emit(phase='ready',cancellable=False)
 while not (out/'FINISH').exists():
  if time.monotonic()>end: sys.exit(3)
  time.sleep(.005)
 emit(phase='complete',receipt=dict(source_id='a'*64,export_id='b'*64,complete=True,tracks=2,playlists=1,history_added=3,conflicts=1,warnings=['Kept phone rating for Track']))
"#;

fn fixture(directory: &Path, code: &str) -> Runtime {
    let script = directory.join("fake-sync.py");
    std::fs::write(&script, code).unwrap();
    Runtime {
        python: "/usr/bin/python3".into(),
        script,
    }
}

fn pump(panel: &SyncPanel, ready: impl Fn(&Panel) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        panel.state.borrow_mut().poll();
        if ready(&panel.state.borrow()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "sync UI timed out: {}",
            panel.state.borrow().message
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn device_preview_selection_transfer_cancel_and_receipt_use_the_real_process_adapter() {
    slint::platform::set_platform(Box::new(i_slint_backend_testing::TestingBackend::new(
        Default::default(),
    )))
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let panel = SyncPanel::create(
        directory.path().into(),
        Language::English,
        fixture(directory.path(), FIXTURE),
    )
    .unwrap();
    pump(&panel, |p| p.editable());
    {
        let mut p = panel.state.borrow_mut();
        assert!(p.can_preview());
        assert!(!p.can_sync(), "preview is mandatory");
        p.ui.set_scope_all(false);
        p.command("options-changed");
        assert!(
            !p.can_preview(),
            "empty selected-playlists scope is invalid"
        );
        p.playlists.insert(7);
        p.ui.set_include_lyrics(false);
        p.ui.set_include_statistics(false);
        p.command("preview");
    }
    pump(&panel, |p| p.editable());
    {
        let mut p = panel.state.borrow_mut();
        assert!(p.can_sync());
        p.command("options-changed");
        assert!(!p.can_sync(), "changed options invalidate the old plan");
        p.command("preview");
    }
    pump(&panel, |p| p.editable());
    panel.state.borrow_mut().command("sync");
    pump(&panel, |p| p.phase == "Transferring over USB");
    let output = panel
        .state
        .borrow()
        .cancel_file
        .as_ref()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let args: Vec<String> =
        serde_json::from_slice(&std::fs::read(output.join("arguments.json")).unwrap()).unwrap();
    assert!(args.windows(2).any(|args| args == ["--playlist-id", "7"]));
    assert!(args.iter().any(|arg| arg == "--no-include-lyrics"));
    assert!(args.iter().any(|arg| arg == "--no-include-statistics"));
    panel.state.borrow_mut().command("hide");
    assert!(
        panel.state.borrow().busy(),
        "hiding cannot cancel a transfer"
    );
    std::fs::write(output.join("RELEASE_COPY"), b"").unwrap();
    pump(&panel, |p| p.committed);
    {
        let mut p = panel.state.borrow_mut();
        p.receive_sync(&json!({"phase":"copy","cancellable":true}));
        assert!(
            !p.ui.get_can_cancel(),
            "commit boundary survives late progress"
        );
        p.command("cancel");
        assert!(!p.cancel_file.as_ref().unwrap().exists());
    }
    std::fs::write(output.join("FINISH"), b"").unwrap();
    pump(&panel, |p| p.editable());
    assert!(panel.state.borrow().completed);
    assert!(panel.state.borrow().ui.get_result().contains("2 songs"));
    assert!(
        panel
            .state
            .borrow()
            .ui
            .get_conflicts()
            .contains("phone edits preserved")
    );

    // A second run cancels while copying, before READY; exit 130 is expected.
    panel.state.borrow_mut().command("preview");
    pump(&panel, |p| p.editable());
    panel.state.borrow_mut().command("sync");
    pump(&panel, |p| p.phase == "Transferring over USB");
    panel.state.borrow_mut().command("cancel");
    {
        let mut p = panel.state.borrow_mut();
        p.receive_sync(&json!({"phase":"copy","cancellable":true}));
        assert!(!p.ui.get_can_cancel());
    }
    pump(&panel, |p| p.editable());
    assert!(panel.state.borrow().cancelled);
    assert!(!panel.state.borrow().completed);

    std::fs::write(directory.path().join("FAIL_DEVICES"), b"").unwrap();
    panel.state.borrow_mut().command("refresh");
    pump(&panel, |p| p.editable());
    assert!(panel.state.borrow().devices.is_empty());
    assert!(
        !panel.state.borrow().can_preview(),
        "failed refresh cannot reuse a stale device"
    );
}

#[test]
fn exit_zero_without_a_valid_receipt_and_malformed_events_are_failures() {
    let directory = tempfile::tempdir().unwrap();
    let (sender, _receiver) = mpsc::channel();
    for code in [
        "print('{\"phase\":\"copy\",\"cancellable\":true}')",
        "print('{\"phase\":\"complete\",\"receipt\":{\"complete\":true}}')",
        "print('not json')",
        "import sys; print('private-key-must-not-surface',file=sys.stderr); sys.exit(1)",
    ] {
        let runtime = fixture(directory.path(), code);
        let error = runtime.run(Task::Sync, 1, &[], &sender).unwrap_err();
        assert!(!error.contains("private-key"));
    }
}

#[test]
fn oversized_progress_lines_are_drained_before_the_next_record() {
    let mut bytes = vec![b'x'; 4 * 1024 * 1024 + 1];
    bytes.extend_from_slice(b"\n{\"phase\":\"devices\",\"devices\":[]}\n");
    let mut reader = BufReader::new(bytes.as_slice());
    assert!(matches!(
        read_cli_line(&mut reader).unwrap(),
        Some(CliLine::Invalid)
    ));
    assert!(matches!(
        read_cli_line(&mut reader).unwrap(),
        Some(CliLine::Text(_))
    ));
    assert!(read_cli_line(&mut reader).unwrap().is_none());
}
