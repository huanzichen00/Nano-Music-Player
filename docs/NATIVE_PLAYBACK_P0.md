# Native playback: macOS folder authorization

Investigation date: 2026-10-15 (Asia/Shanghai).

**Classification: required macOS folder authorization, not an application bug.**
The initial P0 concern is closed: playback was waiting for the user to approve
Downloads access. Granting that access allowed the unchanged installed app to
play normally. Real playback, transport, consecutive tracks, relaunch and shutdown
were verified. No software fix was required or made. The filename retains the
original investigation name for existing references.

A rebuilt ad-hoc-signed bundle may require renewed folder authorization. This
is an authorization requirement, not evidence of a playback-engine defect. The
previously intermittent CoreAudio startup delay is separate and remains
unexplained; it was not reproduced in this investigation.

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

Plan: capture the installed playback wait before changing process state; run the
existing tests using the existing isolated real-library manifest/snapshot;
compare original protected files with temporary copies; check the installed
process before and after legitimate authorization/relaunch; repair only a
demonstrated code defect. Do not merge this branch into main automatically.

## Evidence captured before native UI access

This section records the 10:46–10:53 snapshot, before the user unlocked the Mac
and granted permission. The post-authorization results below supersede its
pending verification state.

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

## Authorization and playback verification

The observed authorization wait began after replacing the ad-hoc-signed release,
launching the normal installed bundle, selecting a real Downloads-library song,
and requesting Play. The earlier native UI check stayed at 0:00; samples of that
process at 10:46 and 10:53 still find the file-open wait. Do not deliberately
reset privacy authorization to reproduce it.

At approximately 12:02 the user confirmed "Unlocked and allowed". TCC records
`authValue=2, authReason=2` at 12:02:44.113 for the original pending request
`81288.21`, followed by successful replies to the other outstanding Downloads
requests. The app's worker, sampled again after that grant, is waiting normally
for commands in `recv_timeout`; it is no longer in file open. No executable,
engine code, signing identity or bundle identifier changed between the blocked
and playing observations.

The same PID 83075 now shows advancing playback on the original Downloads file,
from 1:01 to 1:14. The user explicitly confirms "Yes, I hear the music". Thus the
authorization change releases the same stalled process, rather than merely
coinciding with a successful different test executable.

No new installation or bundle replacement was needed. Do not deliberately reset
privacy authorization to reproduce this wait. A new signing identity, bundle
identifier or permissive custom requirement is not an approved workaround.

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
| `cargo test --test playback_comparison native_local_library -- --ignored --test-threads=1 --nocapture` with the two controlled copies | PASS: 2 passed, 6 filtered out |

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

## Installed acceptance after authorization

These checks operate the actual installed bundle with the normal configuration
and original library. No direct database edits or test connections target the
production library. Normal playback can record its ordinary listening statistics.
The saved settings are preserved: Gapless Off, ReplayGain Off, clipping prevention
On and monitoring Off. The saved +3 dB preamp is inactive with ReplayGain Off.

| Case | Observed result |
| --- | --- |
| Previously stalled process | PID 83075 advances from 1:01 to 1:14 after the grant; worker no longer blocks in open |
| Native audible output | User confirms hearing the actual music |
| Pause | Position remains 1:37 across observations; Play control displayed |
| Paused seek / resume | Seek changes 1:37 to 0:54 while paused; resume advances through 0:55 to 1:15 |
| Two consecutive original tracks | Two-song album queue: seek first track near 2:51 / 2:53, then natural transition to track 2 and advancing 0:15 / 3:55 |
| Previous / Next | Previous restarts a track after 3 s as designed; from paused zero it selects track 1; Next selects track 2 |
| End of queue | Last track reaches 3:55 and changes to the Play control without hanging |
| First shutdown | Normal Quit removes PID 83075; no new app crash report is found |
| Same installed bundle, fresh launch | PID 2172 starts normally, loads the existing library, opens the same original file and advances from 0:00 through 0:11 to 1:50, without another folder prompt |
| Quit during active playback | Normal Quit removes PID 2172 without a hang |
| Temporary fixture in the installed bundle | The four-second WAV starts and naturally advances to the next copied real song |
| Temporary real files in the installed bundle | Both copies advance, observed at 0:29 / 5:28 and 0:14 / 4:38 |
| Isolated-session shutdown | Normal Quit removes PID 2960 while it is playing, without a hang |
| Final normal-library restoration | Fresh PID 3359 uses the normal configuration and production database; the original song list loads, with 129 tracks in the read-only database count |

The UI has no separate music Stop action. Explicit Stop acknowledgements are
verified by the native API/transport tests; the installed UI checks cover natural
queue completion and quitting during playback. No Stop control is added for QA.

The controlled-copy manifest contains two actual temporary copies selected from
the existing 53-file manifest. Their bytes are recorded in the private receipt;
both current and historical engines play them through native output. A separate
three-track configuration is derived from the established SQLite snapshot and
contains only these copies and the existing four-second WAV fixture. It has no
production roots or credentials. Native computer use blocks Terminal, so the
user ran the exact existing-bundle `open -n ... --args --config-dir ...` command
and confirmed "Launched". PID 2960's arguments confirm the isolated configuration.
Its open descriptors identify the copied audio and test database, with no
production database handle. All three temporary-media cases then play in this
same installed executable. No alternate signing identity or launcher bundle was
substituted.

The temporary-media installed cases ran after the user's grant. A controlled
installed protected-versus-unprotected comparison before that grant was not
possible while the Mac was locked, and authorization was not reset afterward.
The before/after evidence instead follows the same blocked process and its
original outstanding TCC requests through their successful release.

After restoring the normal application, all saved settings values match the
pre-investigation snapshot and the shortcuts file is byte-for-byte unchanged.
The installed executable still matches its starting SHA-256 and passes strict
signature verification. No new Nano crash report is found since the permission
grant. The normal app is left open at its production Songs list without active
playback.

**NOT TESTED:** installation of another changed executable, authorization
persistence across future ad-hoc rebuilds, a dedicated development GUI executable,
installed Gapless/ReplayGain On in this baseline pass, physical output loss,
full-length playback of every song, or objective output gap/clipping measurements.
The user heard ordinary music playback; that does not establish audible Gapless
continuity or absence of artifacts across every format/device.

## Assessment

This case is confirmed as a pending macOS Downloads authorization, not an
application bug. After the rebuilt bundle no longer matched the previous code
requirement, macOS required the user's approval again. The OS blocked
file opening before decoding/output setup. Explicit user authorization released
the outstanding requests and restored playback in the same process; the exact
same installed bundle also works after a normal quit/relaunch. Descriptor
exhaustion, a decoder deadlock and a controller synchronization defect are not
supported as the cause of this instance. CoreAudio's earlier intermittent startup
delay was not reproduced and is not asserted fixed.

No production patch or new regression test is justified by these observations.
Rewriting the engine or relaxing its timeouts would not satisfy the required
privacy authorization. The existing tests provide the relevant code
regressions; macOS's code-requirement/prompt behavior needs native acceptance.

Ad-hoc signatures designate this executable by code hash. A future rebuild can
change that requirement and require renewed authorization again, as this release
demonstrates. This investigation does not claim durable authorization across
changed builds. Choosing a stable signing identity requires an appropriate
certificate and separate explicit user approval; no identity, permissive signing
requirement, application ID or system-wide permission changes are made here.

Repository changes remain documentation only. Main, overscroll work and the
installed executable are preserved, and no branch is merged automatically.
The user's explicit folder grant is the only privacy permission change involved
in this verification.

Verified milestones are recorded in two documentation-only commits: the initial
blocked evidence snapshot `45f8a4d` and the post-authorization acceptance update
on `codex/native-playback-p0`. No production fix commit is claimed.

Private evidence (bundle backup, raw process samples, TCC logs, command results
and copied media) is retained under
`/private/tmp/nano-p0-investigation-20261015-104619/`. Raw library paths and song
names are intentionally excluded from this document and commits. The comparison
suite uses the existing `/private/tmp/nano-native-incident/library-paths.json`
and `/private/tmp/nano-native-incident/config/library.sqlite3` through the
documented `NANO_PLAYBACK_PATHS` and `NANO_PLAYBACK_DB_COPY` variables; it never
opens the production database for writing.
