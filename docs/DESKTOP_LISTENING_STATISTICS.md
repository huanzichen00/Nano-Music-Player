# Desktop listening statistics

Open **Statistics** on the second Home page. The visual refresh follows the
provided Statistics reference while retaining the existing Home icon grid; it
does not recreate the reference's Home list. The first-page icons and
Clock/Settings positions remain unchanged. The Statistics landing page has a blue
Total Plays landscape card and entries for Overview, Top Artists, Top Albums,
Top Tracks, Recently Played, Daily Plays, Time Range and Refresh.

**Overview** shows the selected range's Total Plays, Artists, Albums and Tracks
in four compact counters, followed by a native 30-day bar chart and a Top Artists
preview. The group counters describe played, currently available library entries
in that range. Counter cards open their corresponding lists, the chart opens
Daily Plays, and See All opens Top Artists. Range buttons provide 7 Days, 30 Days
and All Time; the same choices remain available through Time Range.

Rankings show rank, representative artwork, play count and a proportional blue
bar; Top Tracks places the artist below the title and the count at the right.
Artist details show a cover hero, Play Artist, Albums, Top Tracks and See All
Tracks. Album details show an 80px cover header, Play Album, Top Tracks, See All
Tracks and a short track preview. Missing artwork uses a local placeholder.
Browsing details does not change playback; play actions and song lists reuse the
existing queue and shuffle behavior. Recently Played shows a cover and local
relative time. Daily Plays combines a fixed 30-day native bar chart with
date/count/bar rows, including dates with no plays. No WebView or chart library
is used.

The existing status bar, title/Back strip, device proportions, keyboard
selection and elastic scrolling remain. The history-limit footer stays visible;
on failure it offers a click-to-retry action, and Refresh also retries or reloads
external changes. New controls have English, Simplified Chinese and Japanese
translations. This UI and the shortcuts below are desktop-only; iOS screens and
mobile behavior are unchanged.

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

`statistics_state.rs` supplies lightweight entries, detail actions and localized
copy; `statistics_projection.rs` formats counts, chart scales and local relative
times without querying SQLite. `StatisticsView` renders the cards, native bars
and virtualized rows using the same `ElasticFlickable` primitive as the existing
library. It exposes row/viewport geometry for keyboard selection and retains
outgoing-page snapshots during navigation.

Navigation uses the original stack, selected row, Back handling and keyboard
dispatcher. Artist/album drilldowns carry exact path membership into the existing
song-list/playback flow. See All Tracks includes available members with zero
plays; Top Tracks remains a played-song ranking. Albums retain track-number
order, subject to the existing global shuffle setting when starting playback.
Recent events open a single-song list so repeated history cannot create duplicate
queue items. The shared Home component defaults the Statistics entry off; only
the desktop shell enables it.

## Desktop information shortcuts

**B** opens or closes Device Information. **D** opens Track Details for the
selected song, the pinned lyric-editing song, or the playing song when no song
row is selected. The rounded dialog shows the cover and full title, artist and
album. Each long field starts at the beginning, pauses for 800ms, then moves
horizontally at approximately 25 logical pixels per second to the end, pauses
and returns. Short or empty fields do not move. Native Text rendering preserves
Unicode and glyph shaping without slicing strings.

The displayed song stays pinned if playback changes. Escape/Backspace closes
the information panel before navigating the page underneath; Home closes it and
returns Home. B and D replace one another, respect existing custom bindings, and
do not take keystrokes from search, editors or other modal dialogs.

On music **Now Playing**, **R** toggles matching cached romaji. If it is missing
or outdated, R starts the existing background generation request for the current
track's Japanese lyrics, using configured DeepSeek credentials. Validation or
request errors leave playback and original lyrics intact. A request for another
song cannot enable the current song's display. **Settings → Lyrics → Japanese
Readings** only persists the display preference and does not send a generation
request. Custom R bindings retain precedence; outside the song interface its
default action remains Cycle Repeat. iOS behavior is unchanged.

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

`tests/statistics_details.rs` checks artist/album identity, exact membership,
unplayed members, stable artwork choice, detail navigation and reuse of existing
playback/shuffle policy. `tests/desktop_shortcuts.rs` sends real Slint key events
through the Controller for B/D, long-text scrolling, pinned details, close order,
repeat suppression, and editor/modal priority. It tests R's missing/stale-cache
generation branch with a deterministic local validation failure, then verifies
matching-cache toggles and the Settings-only preference action without making
external requests.

No Windows/Linux native runtime or physical touchpad/audio-device certification
is implied by software-renderer and compile checks. A timezone change to another
zone with the same current UTC offset can retain a cached date projection until
Refresh, a new play, a library change or the next local day.

## Changed files

| Area | Files |
| --- | --- |
| Queries | `src/library/statistics.rs`, `src/library/statistics/desktop.rs` |
| Worker and integration | `src/app/controller/statistics.rs`, `src/app/controller.rs`, `src/app/mod.rs` |
| Navigation and projection | `src/app/navigation.rs`, `src/app/state.rs`, `src/app/statistics_state.rs`, `src/app/statistics_projection.rs`, `src/app/i18n.rs` |
| Desktop UI | `ui/app.slint`, `ui/home.slint`, `ui/statistics.slint`, `ui/track-details.slint`, `assets/icons/statistics.svg`, `assets/statistics/` |
| Desktop shortcuts | `src/app/action.rs`, `src/input/bindings.rs` |
| Regression tests | `tests/listening_statistics.rs`, `tests/statistics_details.rs`, `tests/desktop_shortcuts.rs`, `tests/core.rs`, `tests/reference_library.rs`, `tests/playback_runtime.rs` |
| Documentation | `README.md`, `README.zh-CN.md`, this file |

The pre-existing uncommitted `docs/FEATURE_PLAN.md` edits are outside this change.

## Initial implementation verification — 2026-10-22

These results describe the initial Statistics implementation before the visual
and information-shortcut update described above.

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

## Visual, shortcut and window follow-up — 2026-10-22

- `cargo fmt --all -- --check`, `cargo check`, and
  `cargo clippy --all-targets` passed. Only the two existing Genius/translation
  `Atomic::fetch_update` deprecation warnings remain.
- Final `cargo test --no-fail-fast`: **485 passed, 0 failed, 27 ignored**.
  The ignored checks retain their prior hardware, network, external-tool or
  profiling requirements.
- `cargo check --features mobile` and
  `cargo check --target aarch64-apple-ios --lib` passed.
- The opt-in native gapless/statistics/artwork smoke passed separately:
  **1 passed** on the local audio output.
- The statistics/details suite passed 13 tests. The screenshot matrix covers
  eight page types in three languages, two modes and three zoom levels, with
  additional empty/loading/error/range scenes. Tests verify real pointer clicks,
  visible range checkmarks, header artwork and chart bars.
- B/D/R integration, keymap migration, input and modal priorities passed. R's
  generation-branch check uses a local validation failure, avoiding provider
  traffic. The existing UI test uses a matching local cache to test toggling.
- The first full run exposed ordinary-list scroll resets and initial menu/video
  sizing regressions introduced by the update. Hidden views now own separate
  offsets; only the active view can update the canonical list position. Initial
  synchronous menu measurements are retained. The failed cases and final full
  suite passed after those fixes.

Window handling now uses macOS native live-resize evidence. Focus/occlusion/DPI
events invalidate pending measurements; workspace or window-manager frames are
restored to the last requested logical size rather than saved as zoom. Native
pixel rounding is tolerated, menu height is cached from initial or acknowledged
stable layout, and settings persist accepted dimensions. Platforms without a
native gesture signal retain their ordinary resize behavior. Tests cover native
intent, focus/occlusion, delayed DPI acknowledgements, fractional pixels,
restoration, genuine dragging and saving during an unclassified WM frame.

`scripts/package-macos.sh` built the arm64 Release App. An isolated native bundle
verified B, Statistics/Overview and Screen Mode. The installed copy at
`~/Applications/Nano Music Player.app` was backed up and updated; Bundle ID,
ad-hoc signature, plist and matching executable SHA-256 were verified. The
existing daily-player process was left running and needs a restart to load this
build. The rebuild/install workflow is documented in `AGENTS.md`.

Native Windows/Linux runtime, physical touchpad acceptance and the actual
AeroSpace workspace-follow callback remain unverified. The isolated QA bundle
has a different ID and did not participate in that callback, so native panel
checks do not certify the user's complete workspace-follow setup. The macOS
resize guard and persistence regressions are covered by the tests above.
