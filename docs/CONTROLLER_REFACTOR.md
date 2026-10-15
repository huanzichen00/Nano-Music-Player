# Controller refactoring verification record

Base: `d938c9fbdf3828878932a695834313daabd5e699` (v0.1.1). Both local and
remote `codex/integrated-player` were verified at this commit on 2026-10-15.
There are no newer source-branch commits. Main is deliberately not the base.
The original controller is 5,552 lines, including its regression tests.

## Ownership and integration protocol

Lead `/root` owns `refactor/controller-v011`, shared controller integration,
architecture, sequential cherry-picks, regression checks and final verification.
Worktree: `/Users/huanzichen/.codex/worktrees/controller-v011/rust-music-player`.

| Agent | Branch / worktree name | Exclusive implementation scope |
| --- | --- | --- |
| `/root/statistics` | `refactor/controller-statistics` / `controller-statistics` | `src/app/controller/statistics.rs`, module tests; later limited library tasks |
| `/root/lyrics` | `refactor/controller-lyrics` / `controller-lyrics` | `src/app/controller/lyrics.rs`, module tests |
| `/root/genius` | `refactor/controller-genius` / `controller-genius` | `src/app/controller/genius.rs`, then `playlists.rs`, module tests |
| `/root/video` | `refactor/controller-video` / `controller-video` | `src/app/controller/video.rs`, module tests |
| `/root/reviewer` | `refactor/controller-review` / `controller-review` | Read-only review, no production edits |

The environment permits four active agents including the lead. The three
specialists analyzed concurrently and then began isolated implementation after
the lead consolidated their findings. The reviewer joined when a slot opened.
Workers do not edit shared controller integration files. Only the lead integrates.

## Agreed service boundaries

Controller remains the application coordinator and owns State/Action/Effect,
UI guards, Slint projection, navigation and cross-media playback decisions.
Services do not accept Controller, State or Slint components.

* Statistics owns its SQLite worker, private commands, acknowledgements, pending
  rating generations and confirmed rollback values. It emits ratings, rollback,
  smart-list snapshots and errors. ListeningSession and its exact playback hooks
  initially remain in Controller to avoid changing transport semantics.
* Lyrics owns romaji workers/storage ordering, translation cache and source keys,
  generation/cancellation, online lookup and continuation identity, display and
  shortcut preference. It returns structured outcomes. Controller retains the
  existing manual/online SQLite storage receiver and publishes tracks only after
  durable storage. T hides without cancelling; completion cannot re-enable display.
* Genius owns frozen candidate snapshots, request gate/cancellation, seed and
  variation, metadata/availability validation and ordered plain preview entries.
  Controller maps entries to Slint, installs queues and performs playlist handoff.
* Playlists owns draft/candidate order and persistence lifecycle. Initial
  loading retains the existing shared worker ordering: playlist migration
  before the library metadata SELECT. Errors retain drafts; shutdown drains saves
  after statistics, as before.

Private worker fields stay private. Test-only injection helpers may preserve
existing asynchronous regressions without exposing mutable production internals.
Network workers are cancelled without blocking joins. Statistics and playlist
persistence workers drain and join; romaji retains its existing Store lifetime.
Library/video jobs retain detached lifetimes. No generic service framework or
new shared locks is needed.

## Protected boundaries

Existing dirty `docs/FEATURE_PLAN.md` files in the original and feature worktrees,
and original untracked `tests/back_scroll.rs` / `tests/monitor_file_budget.rs`,
are outside all changes. All pre-existing worktrees and branches are preserved.
No push, merge into main, installed app operation, signing/TCC change or writable
production data access is part of this refactor.

Automatic monitoring remains forcibly disabled. Audio decoding, output, intent
epochs, native media integration, video engine, UI gesture ownership, spring
physics and animation timing are protected. Playback extraction requires separate
lead/reviewer agreement and sufficient verification; otherwise it stays intact.

## Verification

Actual baseline and milestone results will be recorded below. Historical release
counts are not attributed to this refactor. Native test executables are distinct
from the installed application and do not certify physical trackpad feel or
audible gaplessness.

### Baseline

`cargo fmt --all -- --check` and `cargo check`: PASS. Initial default-symbol
all-target test linking exhausted disk before tests could run. Only this run's
new `target/debug` and `target/tmp` were removed from the source worktree; its
pre-existing candidate app bundle was preserved. Subsequent checks use the
refactor worktree's own target with `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=2`.
No source/dependency/test changes were needed for this build-resource adjustment.
`cargo test --all-targets`: **301 passed, 0 failed, 25 ignored**.
Logs: `/tmp/nano-controller-baseline{,-check}.log`.
The existing isolated native manifest has 53 existing media paths and its
isolated SQLite snapshot has 53 tracks; native comparison tests copy it again.

### Statistics milestone

Worker commit `65057bf` cherry-picked as `ed2ef1c`. Service owns 5 former
Controller fields; listening-session/clock hooks remain unchanged. `cargo check`
and formatting pass. Focused service tests: **7 passed**; ratings, rating UI,
feature library, Back scroll and both overscroll suites: **12 passed**.
Full `cargo test --all-targets`: **308 passed, 0 failed, 25 ignored**.
Independent reviewer approved both module and actual integration, with no required
fixes. Explicit statistics shutdown remains before playlist-writer join.
Logs: `/tmp/nano-controller-statistics-{focused,regression,all}.log`.

### Lyrics milestone

Worker `764856f` cherry-picked as `2e9ca60`; Controller integrates a service owning
14 previous fields. Interface correction: unavailable tracks borrow the existing
BTreeSet without copying. Formatting and `cargo check`: PASS. Controller/service
unit checks: **37 passed, 1 ignored**, including all **13 new** lyric tests.
Lyric UI/storage/runtime suites: **11 passed, 1 ignored**. An initially mistyped
integration test target was corrected to `online_lyrics_runtime`; no test changed.
Full `cargo test --all-targets`: **321 passed, 0 failed, 25 ignored**.
Independent reviewer approved the actual diff. Existing T/cache/stale/edit/source
assertions remain; fixture access uses test-only helpers. Logs:
`/tmp/nano-controller-lyrics-{check,focused,regression,all}.log`.

### Genius milestone

Worker `8091977` cherry-picked as `90250f7`. Service owns 5 previous fields and
plain preview entries; queue installation and persistence handoff remain with
Controller. Formatting and `cargo check`: PASS. All library unit tests:
**196 passed, 8 ignored** (4 new Genius cases). Focused Genius/playlist/shuffle UI
checks: **8 passed**. Full suite: **325 passed, 0 failed, 25 ignored**.
Independent reviewer approved the actual diff. A new Unix fixture initially tried
writing an invalid-UTF8 filename, rejected by APFS; corrected to keep invalid paths
in memory and write their valid Unicode projection, retaining the matching
assertions. Existing lossy UI path identity remains unchanged. A test-only Track
reference was qualified to eliminate an unused production import. Logs:
`/tmp/nano-controller-genius-{check,focused,regression,all}.log`.

### Playlist milestone

Worker `f35e2eb` cherry-picked as `492504a`. Service owns 7 former draft/worker
fields; Controller retains the UI candidate cache. Formatting/check: PASS.
All library unit tests: **200 passed, 8 ignored** (4 new playlist cases).
Playlist storage/editing, UI, modal-navigation and shuffle checks: **10 passed**.
Full suite: **329 passed, 0 failed, 25 ignored**. Independent review identified
and resolved per-drag String-to-SharedString allocations: candidate projection is
cached at refresh, and draft updates clone Slint strings as before. Existing
lossy path selection, draft recovery, failed saves, ID/order, missing originals
and statistics-before-playlist shutdown are retained. Actual diff approved.
Logs: `/tmp/nano-controller-playlists-{check,focused,regression,all}.log`.

### Video task milestone

Worker `f6073ac` cherry-picked as `0998d95`. VideoTasks owns 3 receiver fields;
engine/transport, handoffs, frame cadence, subtitle application and landscape stay
with Controller. Formatting/check: PASS. Library units: **207 passed, 8 ignored**
(7 new video-task tests); video/UI/metrics/modal checks: **10 passed, 1 ignored**.
Native generated-video import/frames/rotation/music handoff: **1 passed**.
Full suite: **336 passed, 0 failed, 25 ignored**. Actual diff approved by reviewer.
No engine, UI component or media-format support changes. Logs:
`/tmp/nano-controller-video-{check,focused,regression,native,all}.log`.

## Resulting architecture and remaining coupling

Original Controller coordinated transport/prefetch/listening, native media,
video/import/subtitles, library startup/import/monitor reconciliation, statistics,
ratings, lyrics/romaji/translation, Genius, playlist drafts, settings, navigation,
input binding and Slint model synchronization.

The final arrangement keeps `src/app/controller.rs` as coordinator and introduces
six private child modules rather than moving methods into more Controller impls:

| File | Owner type and contract |
| --- | --- |
| `controller/statistics.rs` | StatisticsService: private worker commands, rating generations/confirmed rollback, projected events, FIFO shutdown/join |
| `controller/lyrics.rs` | LyricsService: source-bound caches, requests, cancellation/generation, romaji ordering, shortcut/display preference; typed lookup/romaji outcomes |
| `controller/genius.rs` | GeniusService: frozen request snapshots, admission permit, seed/variation, validation, ordered plain preview entries and save snapshot |
| `controller/playlists.rs` | PlaylistsService: recoverable draft/order, candidate admission, startup receiver, save snapshot/completion and shutdown; Slint cache stays Controller-owned |
| `controller/video.rs` | VideoTasks: video load/import and captured subtitle-picker results; no transport ownership |
| `controller/library.rs` | LibraryTasks: startup/import receivers, one-event readiness reduction and existing bounded startup bridge |

Interfaces are `pub(super)` inside private modules. Services never receive State,
Controller or Slint components. Commands/results retain existing domain types and
error messages. No competing state store, trait framework, new shared locks,
provider API, dependency, schema, cache/settings format or polling timer is added.
Controller now constructs services, dispatches operations, consumes outcomes,
projects State/UI and explicitly drains statistics before playlist persistence.
T display preference remains separate from cancellation; a late translation may
be cached while hidden. Online lyrics still publish only after durable storage.

**Deliberately retained:** listening sessions/clock, all playback commands,
epoch/prefetch/handoff hooks, native media publication, video transport/orientation,
subtitle application, library generation/monitor/restore/delete application and
UI model/navigation policy. Both monitors remain forcibly suspended, including
old preferences. Broader playback and monitoring extraction was deferred by lead
and independent reviewer because these boundaries cross queue/availability and
native timing semantics. Native tests cover bounded scenarios rather than full
hardware or audible continuity. No speculative audio fix is included.

## Agent provenance and actual parallel work

All worker worktrees are under `/Users/huanzichen/.codex/worktrees/`, with
`rust-music-player` as workspace directory. Original worker commits are preserved;
the lead cherry-picked each owned change sequentially and separately committed
shared integration after verification.

| Agent identity | Branch | Worktree directory | Owned files | Worker commits |
| --- | --- | --- | --- | --- |
| `/root` | `refactor/controller-v011` | `controller-v011/rust-music-player` | Controller integration and this report | See integration history below |
| `/root/statistics` | `refactor/controller-statistics` | `controller-statistics/rust-music-player` | `controller/statistics.rs`, `controller/library.rs` | `65057bf`, `9a12097` |
| `/root/lyrics` | `refactor/controller-lyrics` | `controller-lyrics/rust-music-player` | `controller/lyrics.rs` | `764856f` |
| `/root/genius` | `refactor/controller-genius` | `controller-genius/rust-music-player` | `controller/genius.rs`, `controller/playlists.rs` | `8091977`, `f35e2eb`, `ea43d86` |
| `/root/video` | `refactor/controller-video` | `controller-video/rust-music-player` | `controller/video.rs` | `f6073ac` |
| `/root/reviewer` | `refactor/controller-review` | `controller-review/rust-music-player` | None: read-only | None |

Statistics, Lyrics and Genius analyzed concurrently, then implemented separate
files concurrently after the lead agreed interfaces. Reviewer joined when the
Statistics worker freed a slot. Video implementation overlapped lead Lyrics/
Genius integration; later Library implementation overlapped Genius/Playlist
integration. Reviewer audited proposals and actual diffs during lead validation.
Lead alone integrated and ran Cargo, serializing build/test work to limit disk
and native-output contention. Workers independently ran formatting, diff and
byte-comparison checks, and committed only their assigned files after lead
validation. Their Cargo test results below are explicitly **lead-executed**.

* Statistics: 7 new worker/ack/shutdown tests and 12 targeted checks; later Library:
  8 new startup/import tests, 215 total library units and 17 targeted checks.
* Lyrics: 13 new service tests; 37 Controller/service checks (1 ignored), plus
  11 lyric integration tests (1 ignored).
* Genius: 4 new service tests, 196 total library units (8 ignored), 8 targeted.
  Playlists: 4 new service tests, 200 total units (8 ignored), 10 targeted.
  Two test-only Clippy cleanups passed the same 4 service tests afterward.
* Video: 7 new task tests, 207 total library units (8 ignored), 10 targeted
  (1 ignored), and a separate passing native video/music handoff test.
* Reviewer: no tests or production edits. Approved all six applied integrations;
  independently compared 20 protected Controller methods with baseline and found
  them byte-identical. No unresolved code-review findings remain.

Resolved review findings: retain lossy UI filename matching, restore Genius seed
mutation parity, retain cached Slint strings during playlist drag. Final Clippy
also found two unnecessary clones in new test expectations; assertions now use
slice references without suppressions or weakened checks.

## Metrics and scope

* Controller: **5,552 → 4,780 lines** (772 fewer); existing regression tests remain
  in the file, with fixture access adapted to owned services.
* Controller fields: **92 → 61**. **37 former fields** move into six owners; six
  service handles replace them. No public mutable service fields are exposed.
* Fields relocated: statistics channels/join/pending/counter (5); lyrics request,
  cache, cancellation and preferences (14); Genius request/preview (5); playlist
  draft/persistence (7); video task receivers (3); library tasks/readiness (3).
* Six modules added, **43 new tests**: 7 statistics, 13 lyrics, 4 Genius,
  4 playlist, 7 video, 8 library. Existing integration test files are unchanged;
  Controller test assertions remain, with only ownership-aware fixture plumbing.
* Audio/video engines, platform code, UI/overscroll, State/Action/Navigation,
  app polling setup, Cargo files and packaging/signing configuration are unchanged.

The code remains intentionally coupled around playback and library availability.
This is incremental ownership extraction, not a claim that every original
Controller responsibility has been separated.

### Library task milestone and final standard checks

Worker `9a12097` cherry-picked as `146dd4e`; integration is
`bac5b2ca06468a160b03c0b755427af6ecfc51a5`. LibraryTasks owns 3 former fields;
monitoring/generation logic and all State/queue/thumbnail effects are unchanged.
Formatting and `cargo check`: PASS. Library units: **215 passed, 8 ignored**
(including all 8 new cases); startup/cache/library/ratings/Back/overscroll/Home/
screen checks: **17 passed**. Final full `cargo test --all-targets`:
**344 passed, 0 failed, 25 ignored**. Actual integration and final protected-method
comparison approved by independent reviewer. Logs:
`/tmp/nano-controller-library-{check,focused,regression,all}.log`.

All-target Clippy passes with warnings denied and **only** the documented baseline
categories allowed:

```sh
cargo clippy --all-targets -- -D warnings \
  -A clippy::collapsible_if -A clippy::needless_borrow \
  -A clippy::field_reassign_with_default
```

No new lint suppressions are added. Two redundant clones in new playlist test
expectations were fixed in worker `ea43d86`, cherry-picked as `becc155`; the four
playlist service tests were re-run and passed. Clippy log:
`/tmp/nano-controller-clippy.log`.

| Full standard milestone | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Baseline | 301 | 0 | 25 |
| Statistics | 308 | 0 | 25 |
| Lyrics | 321 | 0 | 25 |
| Genius | 325 | 0 | 25 |
| Playlists | 329 | 0 | 25 |
| Video tasks | 336 | 0 | 25 |
| Library tasks / final code | 344 | 0 | 25 |

The initial disk/linker failure and APFS fixture error were resolved and are not
passing test runs. No pre-existing source test failures were found in the actual
baseline. No failing assertions were weakened or tests disabled.

## Final native verification and limits

All commands below ran against the final source, sequentially, using test
executables. Temporary configurations/fixtures isolate writes. The comparison
input is the existing 53-path read-only manifest and an isolated 53-track SQLite
snapshot, which the test copies again before use.

| Command | Actual result |
| --- | --- |
| `cargo test --test playback_runtime -- --ignored --test-threads=1 --nocapture` | **4 passed**, 0 failed; 14.36 s |
| `cargo test --test playback_comparison -- --ignored --test-threads=1 --nocapture` | **8 passed**, 0 failed; 48.03 s |
| `cargo test --test video_ui actual_video_import_frames_rotation_and_music_handoff -- --ignored --test-threads=1 --nocapture` | **1 passed**, 0 failed; 3.30 s |

Comparison environment:

```sh
NANO_PLAYBACK_PATHS=/private/tmp/nano-native-incident/library-paths.json
NANO_PLAYBACK_DB_COPY=/private/tmp/nano-native-incident/config/library.sqlite3
```

Logs: `/tmp/nano-controller-final-native-{audio,comparison,video}.log`.
The video milestone separately passed the same native test before LibraryTasks
integration. Thirteen opt-in test cases pass explicitly; the normal suite still
reports 25 ignored tests. Native coverage includes transport, listening/artwork,
translation failure during playback, shuffle, current/historical local media,
old monitoring preference at a 256-descriptor limit, ReplayGain, same-sink
transitions and generated video/music handoff.

**Unverified:** physical trackpad feel/native frame pacing, audible gaplessness
or clipping, hardware output loss, broad hardware format/rate transitions, native
Windows/Linux, and the installed app after these source changes. Test output
consumption is bounded evidence, not an audible-quality measurement. No app bundle
was built/replaced/launched, signing/TCC settings changed, production database
opened for writing, or user media modified. The previously resolved macOS folder
authorization incident remains an authorization issue, not an inferred engine bug.
No known functional regression remains in the executed checks.

## Git handoff and preservation

* Start: `d938c9fbdf3828878932a695834313daabd5e699`.
* Final implementation: `bac5b2ca06468a160b03c0b755427af6ecfc51a5`; the subsequent
  report commit changes only this document. The final response supplies that tip.
* Integration branch: `refactor/controller-v011`.
* Worktree: `/Users/huanzichen/.codex/worktrees/controller-v011/rust-music-player`.
* Main remains `129f83698357db9ad4d56033d3e7b4c860217541`; source release branch
  remains `d938c9f`. No push, PR, squash or merge into main was performed.
* Four retained worker branches are not ancestors of the integration tip:
  `refactor/controller-statistics`, `refactor/controller-lyrics`,
  `refactor/controller-genius`, `refactor/controller-video`. All completed patches
  are cherry-picked; there is no outstanding implementation to integrate. The
  read-only review branch remains at baseline. All six new worktrees are retained.
* All four pre-existing dirty/untracked user files match their recorded SHA-256
  values; original branches/worktrees and both dirty plans are preserved. Only
  Controller, six private modules and this report differ from the release base.

Verified implementation history (oldest first):

```text
b1c0841 docs(app): record controller refactor boundaries and baseline
ed2ef1c refactor(app): extract statistics service
42955b3 refactor(app): coordinate statistics through owned service
2e9ca60 refactor(app): isolate lyrics request and cache lifecycle
e4751f5 refactor(app): dispatch lyric work through lifecycle service
90250f7 refactor(app): extract genius recommendation lifecycle
37748f3 refactor(app): coordinate genius through frozen request service
492504a refactor(app): separate playlist draft and persistence ownership
a4a7208 refactor(app): delegate playlist drafts and asynchronous saves
0998d95 refactor(app): own video import and subtitle task lifecycles
578f794 refactor(app): delegate background video task coordination
becc155 test(app): avoid redundant playlist fixture clones
146dd4e refactor(app): own library startup and import task lifecycle
bac5b2c refactor(app): coordinate library startup and imports through task owner
```

Worker-to-integration mapping: `65057bf→ed2ef1c`, `764856f→2e9ca60`,
`8091977→90250f7`, `f35e2eb→492504a`, `f6073ac→0998d95`,
`ea43d86→becc155`, `9a12097→146dd4e`. Each worker file was byte-compared to the
integrated copy. Milestone Controller commits contain only reviewed integration.

Completed work is intentionally left isolated and reviewable. Future extraction
should begin with focused verification of the retained playback/library coupling,
not by re-enabling monitoring or treating authorization as an audio defect.

Final formatting check, `cargo check` and `git diff --check` pass. Final check log:
`/tmp/nano-controller-final-check.log`. All six integrated module files match
their independently committed worker copies byte-for-byte.
