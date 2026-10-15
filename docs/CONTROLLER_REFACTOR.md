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
| `/root/statistics` | `refactor/controller-statistics` / `controller-statistics` | `src/app/controller/statistics.rs`, module tests; library analysis |
| `/root/lyrics` | `refactor/controller-lyrics` / `controller-lyrics` | `src/app/controller/lyrics.rs`, module tests |
| `/root/genius` | `refactor/controller-genius` / `controller-genius` | `src/app/controller/genius.rs`, then `playlists.rs`, module tests |
| Independent reviewer (assigned when a slot opens) | `refactor/controller-review` / `controller-review` | Read-only review, no production edits |

The environment permits four active agents including the lead. The three
specialists analyzed concurrently and then began isolated implementation after
the lead consolidated their findings. Review follows in an available slot.
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
* Playlists will own draft/candidate order and persistence lifecycle. Initial
  loading must retain the existing shared worker ordering: playlist migration
  before the library metadata SELECT. Errors retain drafts; shutdown drains saves
  after statistics, as before.

Private worker fields stay private. Test-only injection helpers may preserve
existing asynchronous regressions without exposing mutable production internals.
Network workers are cancelled without blocking joins; persistence workers drain
and join. No generic service framework or new shared locks is needed.

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
