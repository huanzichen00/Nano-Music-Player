# Desktop listening statistics

Open **Statistics** on the second Home page. The original first-page icons and
Clock/Settings positions are unchanged. Overview shows the selected range's total
and leading artist, album and song. Separate lists show Top Artists, Top Albums,
Top Tracks, Recently Played, and Daily Plays. Time Range selects All Time, Last 7
Days or Last 30 Days. Refresh retries a failed query or reloads external changes.
All new controls have English, Simplified Chinese and Japanese translations.

## Counts and retention

All Time reads durable `playback_statistics.play_count`. Its total includes
removed songs, whose counters remain stored. Rankings use currently available
library metadata and omit zero-play songs. Artist identity normalizes Unicode NFC,
whitespace and case, retaining punctuation, accents and complete collaboration
credits. Albums use normalized album name plus track artist: the current Track
model has no separate album-artist field. A compilation with different track
artists can therefore appear as separate album entries. Ties have deterministic
metadata/path ordering.

Date ranges use retained `listening_history` events only. The store still keeps
**the last 1,000 inserted qualified plays**, deleting older rows after each write.
Seven/thirty days mean today plus the preceding six/twenty-nine local calendar
dates, through the query time; future timestamps are excluded. Daily Plays always
shows thirty dates, including zero-count days, independently of the range chosen
for rankings. All event dates use the computer's current local timezone, including
its historical daylight-saving rules, rather than subtracting 24-hour periods.
Changing timezone can move historical events between dates.

**Date-range and daily totals can be incomplete because of the history cap.** The
warning remains visible on every statistics page, even when the store cannot
prove that history was pruned. All Time cumulative counters survive pruning.
Recently Played displays up to 100 retained events in the selected range. Repeated
plays remain separate events; a missing song displays its filename and an
Unavailable label. Its row cannot start playback.

## Integration

`src/library/statistics/desktop.rs`, reexported by `statistics.rs` only for desktop,
queries the existing SQLite tables in one consistent read transaction. No schema,
dependency, playback qualification, listening-duration algorithm, or audio event
listener is added. `ListeningSession`, gapless handling, ReplayGain and the audio
queue retain their existing implementations.

`src/app/controller/statistics.rs` extends the existing worker with query commands
and typed result events. SQLite stays on that worker. A request ID, library/play
revision, local date and current UTC offset guard results and caches. New plays
and library/metadata changes invalidate cached projections. Only visible pages
request fresh data; progress ticks do not recalculate rankings. Query failures
clear Loading and leave Refresh available; the desktop worker can reopen after a
startup database failure. The shared mobile worker keeps its prior implementation
and API, with desktop additions behind compile-time guards.

`statistics_state.rs` supplies lightweight rows and localized copy. Navigation
uses the original stack, selected row, Back handling, keyboard dispatcher and
LibraryView elastic scroll. Artist/album drilldowns carry exact path membership
into the existing song-list/playback flow. Albums retain track-number order;
recent events open a single-song list so repeated history cannot create duplicate
queue items. The Slint changes retain the original status bar, title/back strip
and device proportions. Daily Plays uses an ordinary date/count list for the
240px-wide display. The shared Home component defaults its new entry off; only
the desktop shell enables it.

## Verification

Focused tests cover empty/zero data, artist normalization and exact membership,
same-name albums, stable ordering, deleted songs, 1,005 plays pruned to 1,000,
7/30-day boundaries, future/clock-adjusted timestamps, UTC+14/UTC-12 date boundaries
and a real 23-hour daylight-saving day. Worker tests cover stale responses,
revision invalidation, cache reuse, Refresh followed by immediate subpage
navigation, SQL/open failure recovery, and nonblocking submission while SQLite
is exclusively locked.

`tests/listening_statistics.rs` verifies navigation, queue preservation, actual
pointer row activation, loading/retry and shrinking-list selection. Software
snapshots under `target/qa/statistics/` exercise Device/Screen modes, 0.75x/1x/1.5x,
and all three languages. `tests/playback_runtime.rs` also opens/closes/reopens
Statistics during real native playback and checks automatic totals across the
existing two-track gapless smoke test. Native playback checks remain opt-in:

```sh
cargo test --test playback_runtime native_playback_gapless_statistics_and_artwork_do_not_crash -- --ignored
```

No Windows/Linux native runtime or physical touchpad/audio-device certification
is implied by software-renderer and compile checks. A timezone change to another
zone with the same current UTC offset can retain a cached date projection until
Refresh, a new play, a library change or the next local day.

## Changed files

| Area | Files |
| --- | --- |
| Queries | `src/library/statistics.rs`, `src/library/statistics/desktop.rs` |
| Worker and integration | `src/app/controller/statistics.rs`, `src/app/controller.rs`, `src/app/mod.rs` |
| Navigation and projection | `src/app/navigation.rs`, `src/app/state.rs`, `src/app/statistics_state.rs`, `src/app/i18n.rs` |
| Desktop UI | `ui/app.slint`, `ui/home.slint`, `ui/statistics.slint`, `assets/icons/statistics.svg` |
| Regression tests | `tests/listening_statistics.rs`, `tests/playback_runtime.rs` |
| Documentation | `README.md`, `README.zh-CN.md`, this file |

The pre-existing uncommitted `docs/FEATURE_PLAN.md` edits are outside this change.

## Verification results — 2026-10-22

- Before changes: `cargo test --no-fail-fast` — 421 passed, 0 failed, 27 ignored.
- `cargo fmt --all -- --check` and `cargo check` — passed.
- `cargo test --no-fail-fast` — **442 passed, 0 failed, 27 ignored**. The 21 new
  tests comprise 13 backend, 5 worker/cache and 3 navigation/UI tests.
- `cargo clippy --all-targets` — passed. Both check and Clippy retain the two
  pre-existing `Atomic::fetch_update` deprecation warnings in Genius/translation.
- `cargo check --features mobile` — passed; shared mobile statistics-service
  regression tests — **7 passed**.
- `cargo check --target aarch64-apple-ios --lib` — passed, verifying desktop query
  types and events do not leak into the shared iOS service.
- Native gapless/statistics/artwork test above — **1 passed** separately on real
  audio output. Statistics was opened, closed and reopened while the first song
  played; both consecutive songs completed with one persisted count each, two
  history events, and an automatically refreshed total of two on the open page.
- Software snapshots visually reviewed: Statistics/Overview, empty and error
  states, long metadata, daily list, English/Chinese/Japanese, Device/Screen sizes.
- `git diff --check` — passed. No audio-engine, dependency, mobile/iOS source,
  database schema or play-qualification changes.

All requested first-version functionality is implemented. Native Windows/Linux
runtime and physical input/audio-device acceptance remain outside this host's
verification. The 27 ordinary-suite ignored checks retain their prior hardware,
network, external-tool or profiling requirements; the native smoke above is one
of those opt-in checks and is reported separately.
