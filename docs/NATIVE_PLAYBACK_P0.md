# Native playback P0 investigation

Investigation date: 2026-10-15. This is an evidence record, not a declaration that
installed playback is fixed. No production code has been changed.

## Isolation and plan

- Original checkout: `main`, `129f83698357db9ad4d56033d3e7b4c860217541`.
- Investigation base: `2a40c1e28d4da372d414d01fec4b0675d223fbd4`, matching
  the documented installed release's source history. New branch:
  `codex/native-playback-p0`, in its own managed worktree.
- The original dirty plan and two untracked tests are preserved. The separate
  `feat/rubber-band-overscroll` worktree has an untracked `ui/overscroll.slint`;
  no investigation edits are made there. Another feature worktree's dirty plan
  is also outside this investigation.
- The installed app was copied with `ditto` before any replacement or relaunch.
  Its executable and backup both have SHA-256
  `290f844497666cb62d121076a0c79562b06158690ea98c2edb046632213b23c4`.
  Settings and shortcuts were copied privately for reference.
- Read `UI_REFERENCE_VERIFICATION.md`, `PLAYBACK_RECOVERY.md`, and
  `AUDIO_ENGINE_REFACTOR.md` before investigating. Audio source matches
  `129f836`; the monitoring recovery is a controller change, not another engine
  rewrite.

Plan: capture the installed failure before changing process state; run the
existing tests using the existing isolated real-library manifest/snapshot;
compare original protected files with temporary copies; check the installed
process before and after legitimate authorization/relaunch; repair only a
demonstrated code defect. Do not merge this branch into main automatically.

## Evidence captured before native UI access

**VERIFIED:** the installed process, PID 83075, launched at 10:01:07, remains
alive. A fresh one-second sample at 10:46 finds its audio worker in
`AudioWorker::load -> open_decoder -> OpenOptions::__open -> __open` throughout
the sample. This is an operating-system file-open wait, not a sampled Rust mutex
or decoder loop. The UI's present clock/state has not yet been inspected because
the Mac is locked.

**VERIFIED:** the process has 10 numeric open descriptors. No present evidence
supports descriptor exhaustion as this instance's cause. This does not invalidate
the earlier, separately reproduced monitoring failure.

A second sample at 10:53, after the native tests completed, still finds the same
process in the same file-open call and still counts 10 descriptors. The
test-executable successes below did not release the installed process's wait.

**VERIFIED:** TCC logs covering this process's launch include:

- 10:01:12.097: failed match against the existing code requirement for
  `org.nano.music-player`, service `kTCCServiceSystemPolicyDownloadsFolder`.
- 10:01:12.098: `AUTHREQ_PROMPTING` for that app/process and service.
- 10:18:57.084: another code-requirement mismatch and prompt request attributed
  to the same responsible app/process.

The captured log contains no Downloads authorization result for those requests.
The previous verification's 0:00 symptom and this fresh file-open sample are
consistent with the unanswered permission request. Releasing the wait and
verifying playback after an explicit user grant are still needed to establish
causality end to end. No TCC database, privacy settings or permissions have been
modified.

**VERIFIED:** strict/deep signature verification passes. The installed bundle ID
is `org.nano.music-player`, signature is ad hoc, TeamIdentifier is absent, and
there are no reported entitlements. Its designated requirement is the executable
code hash `25c13e1b11d14ccbe03530200df85be4a36ed5e1`. The preserved prior release
has a different designated code hash, `2a2512e9575f3be08691034f82bfc66923d63eaf`.
Thus an unchanged bundle ID does not mean the recorded code requirements match.
No signing identity or requirements have been changed by this investigation.

**VERIFIED:** the previously established isolated manifest still contains 53
existing files, and its associated SQLite snapshot still contains 53 tracks.
Two files selected from that manifest are in Downloads. Separate command-line
open/read probes complete in 1.25 and 1.28 ms, respectively. Copies to the private
temporary evidence directory succeed (11.4 MB and 9.8 MB). These probes use a
different responsible process; they do not establish the installed app's access.

**VERIFIED:** system audio enumeration succeeds and reports the built-in stereo
speakers at 48 kHz as default output. This does not measure initial Rodio stream
setup or speaker output and does not explain the previously intermittent
CoreAudio initialization delays.

## Reproduction and remaining matrix

The observed installed failure began after replacing the ad-hoc-signed release,
launching the normal installed bundle, selecting a real Downloads-library song,
and requesting Play. The earlier native UI check stayed at 0:00; the current
process still waits in file open. Do not deliberately reset privacy authorization
to reproduce it.

Native UI access is currently blocked by the locked Mac. Inspect the real OS
permission dialog after unlock. Renewing Downloads access exposes that folder
to the app and requires the user's explicit approval. A new signing identity,
bundle identifier or permissive custom requirement is not an approved workaround.

Pending installed-app cases: generated temporary fixture; temporary real-song
copies; original Downloads files after legitimate authorization; current process
versus relaunch with the exact same bundle; transport controls, two consecutive
tracks and clean shutdown. No fresh installation or bundle replacement has been
performed during this investigation.

## Verification results

All commands below ran in the isolated investigation worktree against the
unchanged production source. `CARGO_TARGET_DIR` reused the existing dependency
build directory; no application bundle was rebuilt or replaced.

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo check` | PASS |
| `cargo test --all-targets` | PASS: 298 passed, 0 failed, 25 opt-in tests ignored |
| `cargo test --test playback_runtime -- --ignored --test-threads=1 --nocapture` | PASS: 4 passed on native CoreAudio; 16.34 s test time |
| `cargo test --test playback_comparison -- --ignored --test-threads=1 --nocapture` | PASS: 8 passed on native CoreAudio; 51.91 s test time |
| `cargo clippy --all-targets` | PASS with existing warnings described below |
| `cargo test --test video_ui actual_video_import_frames_rotation_and_music_handoff -- --ignored --test-threads=1 --nocapture` | PASS: 1 passed, 4 filtered out |

Clippy reports existing `collapsible_if` warnings in `src/ai.rs` and
`src/app/controller.rs`, `needless_borrow` in `src/library/scanner.rs`, and
`field_reassign_with_default` in `tests/feature_library.rs`. These files are
unchanged. This was not a warnings-denied run.

**VERIFIED in native test executables:** current and historical engines both
start and advance all 53 original manifest files; generated-fixture transport
checks pause/resume, seeking, rapid replacement, natural completion, Stop and
same-ID replay. The controller succeeds with its copied real library and old
monitoring preference at a 256-descriptor limit. ReplayGain formats, successive
same-sink mixed-format transitions, statistics, translation-worker failure during
playback, and the generated video/music handoff tests pass.

**NOT REPRODUCED in this run:** the previously intermittent CoreAudio
initialization timeout, an engine-specific decoder/worker deadlock, or present
descriptor exhaustion. Passing tests are bounded observations, not proof that
these failures can never happen.

**NOT TESTED:** current installed UI clock/state, authorized temporary-media
playback in the installed app, releasing the Downloads wait after explicit user
approval, same-bundle relaunch, installed transport/two-track/shutdown acceptance,
and physical speaker output. The Mac remained locked on both native UI attempts.
No new installation, privacy grant or signing change was attempted. The
currently stalled process was left intact for inspection of its pending prompt.

Device-backed sample consumption in the native tests is distinct from installed
playback acceptance. No one listened to or measured physical output in this
investigation. Audible correctness, gapless continuity and clipping/artifacts
remain unverified.

## Assessment and next necessary action

The installed worker stall is reproduced. A code-requirement mismatch and an
unanswered Downloads authorization request are directly recorded for that
process, while separately authorized test executables can play the original
files. This strongly supports a process-specific privacy authorization problem;
an unlock/explicit grant followed by advancing installed playback is still
required to close the causal test. A controller synchronization defect has not
been demonstrated; its copied-library native regression passes, but the current
installed interface cannot be inspected while locked.

No production fix or new regression test is justified by the evidence so far.
Rewriting the engine, relaxing its timeouts, or silently changing its designated
requirement would not resolve the outstanding authorization in a verified way.
The only repository change is this investigation record. Main, overscroll work,
the installed executable, bundle identity, signing requirements and privacy
configuration are preserved.

To continue: unlock the Mac and inspect the actual Downloads prompt; obtain the
user's explicit approval before allowing folder access. Then test the existing
process, a clean relaunch of the exact same installed bundle, the temporary
fixture/real-file copies and original library files. Confirm two consecutive
tracks and all transport/shutdown controls, and arrange actual listening or an
appropriate output measurement. Keep this branch separate from main until the
user reviews the result. Do not mark the installed playback P0 resolved yet.

Private evidence (bundle backup, raw process samples, TCC logs, command results
and copied media) is retained under
`/private/tmp/nano-p0-investigation-20261015-104619/`. Raw library paths and song
names are intentionally excluded from this document and commits. The comparison
suite uses the existing `/private/tmp/nano-native-incident/library-paths.json`
and `/private/tmp/nano-native-incident/config/library.sqlite3` through the
documented `NANO_PLAYBACK_PATHS` and `NANO_PLAYBACK_DB_COPY` variables; it never
opens the production database for writing.
