# Source-file deletion monitoring plan

## Current behavior

The library stores each imported track in SQLite under its path. Import refreshes selected folders, while startup loads cached records even when their audio files are unavailable. Playlist entries are path based, and manual lyrics and romaji are stored separately by audio path. Consequently, deleting an audio file does not currently remove its cached record or hide it from library views.

## Design

Use the cross-platform `notify` filesystem watcher on a dedicated worker thread. Keep explicit imported roots and watch them recursively. The worker coalesces bursts into bounded batches, waits briefly for an event burst to settle, then verifies candidate paths against the filesystem before sending confirmed missing audio paths to the application. Notifications are hints: event loss, root disappearance, watcher errors, or an overflow trigger a cautious root health/reconciliation path, never immediate deletion of every path under an unavailable root. Do not poll the whole library every frame.

Represent identity as a normalized absolute lexical path (canonicalize existing roots/files when possible, retain normalized paths for missing ones). Never key asynchronous results only by a visible row/index. A deletion result must include path identity and a generation/revision captured when the work began; the receiver applies it only if that path is still absent and the generation still matches. This prevents stale imports, artwork, AI work, and audio events from mutating a newly restored or index-reused track.

On confirmed deletion, remove only the cached track record and its visible library entry. Keep playlist path order and all lyrics, romaji, translation, and other user-authored path associations intact. A restored file at the same stable path can be rediscovered on the next watcher create/rename event or explicit import and regain those associations. The monitor never removes or modifies source media.

Protect active playback and queued items by path identity. When the current or a queued file disappears, stop or skip that item safely, invalidate its playback generation, and retain the playlist's saved path entry. Ensure late decoder/audio callbacks from the old generation are ignored.

Persist the set of monitored roots and a user-facing enable/disable setting if integration permits. Root selection/import must register roots without scanning on the UI thread. Turning monitoring off stops subscriptions but does not erase library or association data. On re-enable, check roots individually; unavailable roots remain dormant and do not cause mass removals.

## Observable behavior

- A confirmed file deletion is removed from visible browsing, search, album and artist counts, and the track cache promptly after the event settles (target debounce: a few hundred milliseconds, with bounded batch work).
- A temporary drive disconnect/root disappearance suspends that root. Its cached tracks and associations remain available as dormant records until the root is available again; no absence-based mass deletion occurs.
- If an individual audio file is confirmed missing while its watched root remains healthy, only that track is hidden and evicted from the track cache. Playlist paths and lyrics/romaji/translation associations remain.
- When the same path returns, it is re-imported/refreshed and becomes visible. Preserved associations apply again. A file restored at a different path is a distinct identity and requires normal import/association handling.
- Watcher setup/runtime errors are surfaced as monitor status and fall back to dormant/conservative behavior. They never authorize deleting records.

## Validation and implementation sequence

1. Build a monitor/domain harness in an isolated Cargo target with temporary fixture directories. Test event coalescing, confirmed individual deletion, root disappearance, restoration at the same path, no source-file deletion, and bounded event handling. Do not touch the normal library database or user media.
2. Add a small production monitor module and dependency, then exercise it against temporary files. Keep filesystem work off the Slint/UI thread.
3. Integrate path identities and generations with library/cache updates, playback/queue invalidation, asynchronous import/artwork/AI result handling, and settings once shared controller/UI ownership is released.
4. Add accurate localized labels for monitoring state/settings in English, Simplified Chinese, and Japanese, and persist the preference and roots.
5. Run formatting, checks, focused tests, full tests, and strict lint. Record platform-specific watcher limitations and observed behavior.

## Watcher API research

The upstream `notify` documentation describes its recommended cross-platform watcher and directs users needing debounced events to its debouncer crates. Events are OS notifications and can be imperfect, so this design treats them as prompts to verify current state rather than as a complete transaction log. See [notify crate documentation](https://docs.rs/notify/latest/notify/) and the [upstream notify-rs repository](https://github.com/notify-rs/notify).
