# Playback recovery comparison

Recovery work lives in the isolated `restore/reliable-playback` worktree. Main at
`129f836` and its uncommitted files remain intact. A second, unchanged worktree
contains `47b8f89`, the release immediately before Gapless/ReplayGain and the
related feature batch. The user identified that period as the working baseline.

This document records the recovery milestone. The later UI changes, shorter Back
interval, expanded test totals and final installed-app checks are recorded in
`docs/UI_REFERENCE_VERIFICATION.md`; production audio-core source remains unchanged.

## Acceptance and plan

1. Compare the exact historical engine with the current engine on real CoreAudio,
   with Gain Off and no successor prefetch. Include transport and existing media.
2. Reproduce startup/playback failures through the controller under macOS resource
   limits; change only the demonstrated failing integration.
3. Verify ReplayGain separately, then same-sink Gapless transitions. Run the normal
   tests/checks and native playback/video handoff acceptance before delivery.

## Architecture assessment

The historical engine is 184 lines: one command worker owns the output, decoder
and Sink. It polls progress/EOF every 100 ms. It has no prefetch/source-event layer
and only track IDs distinguish sessions. That path is sufficient for ordinary
playback. The current engine adds synchronous generation/seek intent, source
identities, first-sample start/EOF notifications, backend failure routing and a
worker state reducer. These are substantially more complex than ordinary playback
requires; most complexity supports cancellable same-sink successors and rejection
of stale events. This comparison supplies no reason to add another abstraction.

An unconditional engine rollback would remove those safeguards and would not
remove the two independent library watchers. Both engines passed identical native
ordinary transport tests and started all 53 current library paths, with actual
output consumption beyond 0.2 seconds per path. EOF-then-Seek/Stop also responded
on both in this run; no historical native deadlock is claimed. There is no measured
engine-only failure or reliable-playback advantage from reverting it in these cases.
No production audio-core changes are made in this recovery.

## Reproduced failure and minimal recovery

At a 256-descriptor soft limit, recursive kqueue monitoring of 512 fixture files
failed with `Too many open files (os error 24)`. More importantly, an isolated copy
of the user's real 53-song database, with the existing two monitors enabled, failed
through Controller with `Playback error: Too many open files (os error 24)`.
Direct playback of those same paths succeeded with both engines. The newer feature
batch added a second watcher over the same trees; kqueue retains descriptors for
files/directories instead of using a constant-sized watch service.

The FSEvents trial allowed the failing native controller test to play, but failed
all five native library-notification tests. It is discarded, including its Cargo
changes. The recovery release suspends both automatic monitors, including when old
settings request them, and defaults monitoring Off. It retains roots, library data,
watcher implementation/tests, manual import, startup cache publication and background
source refresh. Automatic additions/deletions/renames during a session are disabled.
The persisted monitoring flag cannot re-enable this service in the recovery build.
A regular controller regression checks that old preferences create no monitors or
monitor reconciliation worker and preserve roots/Gain/Gapless preferences.

## Native evidence

`tests/playback_comparison.rs` includes the exact historical engine in
`tests/support/legacy_engine.rs`; it is test-only and excluded from production.
Opt-in tests use actual Rodio/CoreAudio output at low volume and isolated settings.
Local paths are read from an explicitly supplied JSON manifest; database testing
requires an explicit snapshot and copies it again before use. Original media and
the normal database are not inputs to a writable test connection.

- Historical/current ordinary playback: all 53 M4A paths start and advance; rapid
  skipping, paused seek, resume, stable paused position, natural fixture completion,
  Stop ownership acknowledgement and same-ID replay pass on both.
- Recovery controller: real library startup and advancing playback pass at the
  same 256-descriptor limit with old monitoring preferences enabled in the input.
- ReplayGain: Track/Album tags in temporary WAV, FLAC, MP3 and M4A files are read;
  each normalized source advances and completes through native output.
- Gapless: native WAV→FLAC→MP3→M4A same-sink transitions report all three successors
  in order and finish once. These fixtures share channel count/sample rate.
- All eight comparison tests pass after the recovery change. The first controller
  regression failed on `129f836` before any recovery code changes.

Reproduce locally with explicit snapshots:

```sh
NANO_PLAYBACK_PATHS=/path/to/read-only-paths.json \
NANO_PLAYBACK_DB_COPY=/path/to/copied-library.sqlite3 \
cargo test --test playback_comparison -- --ignored --test-threads=1
```

Native output consumption verifies device-backed decoding/control responsiveness,
not audible continuity, clipping/artifacts, full-length decoding of every song or
mixed-rate hardware boundaries. Rodio callback decoding/I/O and canceled successors
remaining queued until consumed are unchanged. These remain limitations; they do
not justify a speculative concurrency rewrite.

The Apple Recent Items accessibility crash in the supplied reports predates the
Gapless feature batch. Its stack is in native AppKit/CoreServices menu validation,
not the audio worker. Removing file-handle pressure is not proof that this separate
crash is fixed. Native menu/startup checks and any remaining limits are recorded
with the final delivery verification.

A historical UI QA relaunch dropped its `--config-dir` argument and rewrote normal
preferences using the old schema. That process was closed and the exact pre-test
settings snapshot restored. Historical native app launches are not counted as
isolated playback acceptance. The engine/controller test evidence above uses
explicit temporary directories and was completed separately.

## Trackpad Back regression

The real Slint scroll dispatcher reproduces a second Back when a large horizontal
momentum delta arrives 260 ms after the first Back, after the screen transition
settles. A shared UI burst latch now persists across Library/Now Playing/Home
views. Horizontal activity restarts a single 300 ms quiet timer after consumption;
disabling/re-enabling a view does not clear it. Pointer swipes and keyboard Back
retain their existing path. Continuous-tail, new-gesture, immediate keyboard and
Home-page tests exercise actual dispatch through the controller. TouchArea supplies
no physical gesture phase, so this is quiet-interval suppression; real Mac trackpad
gestures still require human verification.

## Final verification

- Formatting and `cargo check` pass. The complete default suite passes: **270
  passed, 21 ignored**. The ignored comparison cases are run explicitly below.
- All-target Clippy with `-D warnings` passes with only the three existing lint
  classes allowed (`collapsible_if`, `needless_borrow`,
  `field_reassign_with_default`); see the earlier audio verification for locations.
- `tests/playback_runtime.rs --ignored --test-threads=1`: **2 passed**, including
  completion/statistics/artwork and seek/skip/pause/resume/Stop on real output.
- The generated native video import/rotation/music-handoff test: **1 passed**.
- Debug and optimized-release native historical/current comparison: **8 passed
  each**, including the real
  library controller regression at a 256-descriptor soft limit.
- A packaged native debug candidate, with a fixed isolated config launcher, loaded
  the real library copy. Its song position advanced from 0:00 to 0:51, and the native
  Playback menu opened and accepted an action. Numeric open descriptors were **14**
  while playing; the earlier installed process had **554** (different processes,
  not a controlled benchmark). The native candidate shut down with exit status 0.
- The QA bundle's distinct ID was tiled by AeroSpace, fighting the app's aspect
  maintenance. Its window was explicitly floated for QA; no global AeroSpace
  configuration was changed. The production `org.nano.music-player` ID retains
  the existing user floating rule.
- Before delivery, the normal library still contains **53 tracks and one playlist**.
  Settings/database snapshots and the previous app bundle are retained for rollback.
  Main's original plan SHA-256 is unchanged; production `src/audio` matches `129f836`.

The separate native Apple Recent Items crash has not been reproduced or proven
fixed. Ordinary app menu validation does not cover that system-menu failure.
Audible Gapless quality, physical trackpad gestures and hardware output loss remain
unverified. Disabling monitoring is a deliberate functionality reduction for
reliability; it does not assert that every reported crash is eliminated.

The user confirmed that the installed 500 ms latch eliminated the double Back,
but reported that the next slide felt delayed. The quiet interval is reduced to
300 ms. Continuous momentum, including a delta 260 ms after navigation, still
restarts the latch. The regression now requires another deliberate gesture to
work after 350 ms of quiet; it fails with the old 500 ms interval. Screen animation
and its input blocking duration are unchanged. Human confirmation of the shorter
interval remains pending.
