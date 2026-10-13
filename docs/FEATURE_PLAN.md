# Playback, Library, and Smart Playlist Features

Status: implementation complete; hardware-specific validation remains outstanding

## Device behavior and desktop additions

The Apple iPod nano (7th generation) User Guide (`/Users/huanzichen/Downloads/Study/References/46034.pdf`, 68 pages) is the interaction reference. The guide describes a compact Now Playing screen with controls revealed by tapping, swipe-right navigation, playback status in the status bar, previous/next and seek controls, repeat and shuffle, rating, Sound Check, and an Audio Crossfade preference. Preserve those patterns and the existing nano-style layout and Genius action.

The guide does not describe desktop media keys, macOS Now Playing, ReplayGain, filesystem monitoring, or listening statistics. Those are desktop integrations. Smart playlists are exposed in the existing Music > Playlists area using the same nano-style list rows; their automatic membership is a desktop enhancement. ReplayGain is distinct from Apple's Sound Check setting and will remain a separately named preference.

## Existing architecture and shared contracts

- `src/audio/engine.rs` owns the dedicated Rodio worker and receives commands without blocking Slint. Keep audio decoding, transition scheduling, seeking, and gain application on this worker.
- `src/app/state.rs` owns semantic playback state and queue transitions. `src/app/controller.rs` polls worker events and dispatches commands; it is the integration point for playback sessions, system media events, and library monitor events.
- `src/library/scanner.rs` reads metadata off the UI thread and persists it through `src/library/database.rs`. `Track.path` is the stable identity; UI vector indices are temporary and must not be persisted in feature stores.
- `src/library/deletion_monitor.rs` watches known tracks for missing/restored paths. The new library monitor must coexist with its safe root handling and avoid treating a disconnected root as mass deletion.
- `src/library/playlists.rs` persists user-authored ordered playlists. Smart playlists are computed views and must not be written over or alter Genius/user playlists.
- Settings are serialized by `src/app/config.rs` and reconstructed in `Controller::save_settings`. New persisted fields need serde defaults and round-trip coverage.
- Keep Genius behavior and its request/cancellation/preview implementation intact. New playlist UI must leave Genius rows and save semantics untouched.

Shared integration rules:

1. Cross-module library/statistics identity is a canonicalized audio `PathBuf`, never a UI index.
2. Workers emit typed events through channels; the controller applies updates on its normal poll loop. No filesystem scan, database operation, or Objective-C call may block a Slint callback.
3. `AudioCommand` remains the audio worker boundary. `AudioEvent::Progress` is the position source; any new event must be generation/track guarded like current events.
4. A listening session is counted once after reaching `min(30 seconds, half the known duration)`, with a 3-second floor for very short tracks. Seeks do not count as listening time. A per-session latch prevents repeated progress polls or end-of-track handling from double-counting. Explicit restarts create a new session.
5. Statistics and smart-playlist persistence are path based and independent of manual playlist tables. Missing paths may retain history but are excluded from visible smart-playlist rows.

## Phases, ownership, dependencies, and acceptance

| Phase | Owner / files | Dependencies | Acceptance criteria |
|---|---|---|---|
| 0. Interfaces and manual review | Main: this document; `src/audio/`, `src/library/`, `src/app/` inspection | None | Existing ownership, device behavior, desktop-only additions, worker boundaries, stable IDs, and play threshold are documented before delegation. |
| 1. Gapless playback and ReplayGain | Agent 1: `src/audio/engine.rs`, new `src/audio/gain.rs` and focused tests only. Main integrates config/controller/UI. | Phase 0; audio worker stays sole Rodio owner | Pre-open the next eligible queue item and append it without a stop/recreate gap; queue changes, repeat, shuffle, seek, pause, and video handoff cannot play stale prefetched audio. Parse track/album ReplayGain metadata, support track/album mode, preamp/limiter/clipping prevention, and preserve defaults when metadata is absent. Existing playback path remains functional. |
| 2. macOS media integration | Agent 2: new `src/platform/macos_media.rs` and platform-local tests only. Main integrates controller and module registration. | Phase 0; typed media snapshot/command interface | macOS publishes title, artist, album, artwork, duration, elapsed position, and playback state; remote play/pause/toggle/next/previous/seek commands return to the controller; stale media is cleared on stop/video handoff. Non-macOS builds compile with a no-op adapter. Do not claim native behavior verified without running on macOS. |
| 3. Incremental library monitoring | Agent 3: new `src/library/library_monitor.rs` and focused tests only. Main integrates scanner/controller. | Phase 0; existing deletion watcher retained | Added, removed, renamed, and modified supported audio files are debounced and represented by typed events. Metadata reads and DB writes happen off the UI thread. Rename is reconciled as old-path removal plus new-path addition while preserving safe behavior for unavailable roots. UI applies updates without replacing unrelated tracks or blocking. |
| 4. Playback statistics and smart playlists | Agent 4: new `src/library/statistics.rs` and focused tests only. Main integrates controller, database/scanner, state, playlist UI. | Phase 0; stats session contract; watcher path updates | Persist play counts, last-played timestamps, and bounded listening history across restart. Threshold/latch prevents short skips and duplicate increments. Compute Recently Added, Most Played, Recently Played, and Never Played deterministically; visible entries resolve against current library paths. User/Genius playlists remain unchanged. |
| 5. Integration and verification | Main: shared `src/app/controller.rs`, `src/app/state.rs`, `src/app/config.rs`, `src/library/database.rs`, UI, docs | Phases 1–4 | Add serde-default settings and UI entry points while preserving nano geometry and Genius. Run `cargo fmt`, `cargo check`, focused/new tests, available runtime checks, and review diffs. Report native-only checks and any unavailable verification honestly. Update this plan with outcomes and remaining limitations. |

## Delegation boundaries

Agents implement isolated modules and tests only; they do not modify the shared controller, state, config, UI, library database, or Genius files. Their module APIs must use the shared path-based and typed-event boundaries above. Main integrates these modules after the independent work is ready, resolves API mismatches, and owns cross-feature tests and the final plan status.

## Verification record

- Manual: identified and extracted the complete 68-page PDF with Poppler; reviewed the music, controls, playlist, and settings chapters and the overview/gesture conventions. Poppler reports malformed cross-reference entries in this supplied copy, but the full text extracts and `pdfinfo` reports all 68 pages.
- Implemented: same-worker gapless successor prefetch/transition and ReplayGain processing; macOS media snapshot/remote-command bridge; debounced incremental filesystem monitor; SQLite playback statistics and bounded listening history; four computed smart playlists; persisted settings and nano-style UI entries. Genius and manual playlists remain on their existing paths.
- Historical verification on 2026-10-13 confirmed these modules compile and have unit coverage; it did not establish end-to-end feature correctness. A subsequent native playback crash remains under investigation.
- The supplied manual at `/Users/huanzichen/Downloads/Study/References/46034.pdf` was extracted with Poppler and its music, controls, playlist, and settings chapters reviewed. Poppler reports malformed cross-reference entries in this copy, but text extraction succeeds and `pdfinfo` reports all 68 pages.
- `cargo fmt --all -- --check`: passed on 2026-10-13.
- `cargo check`: passed on 2026-10-13 for the current macOS host build.
- `cargo test --lib`: 77 passed, 6 ignored, 0 failed on 2026-10-13. This supersedes the earlier sandbox run that could not bind loopback sockets.
- `git diff --check`: passed.
- Not verified on physical playback hardware: audible gapless transitions, output-level clipping behavior, and interactions across real device audio outputs. Not verified against the live macOS Now Playing surface or physical media keys. Unit tests and compilation do not establish those runtime behaviors.
- Monitoring tests exercise event normalization/debouncing and unavailable-root safety; a long-running user-library add/rename/delete session was not performed. Statistics persistence and smart-list ordering have SQLite unit coverage; no end-to-end listening session was run.
- Four isolated project-thread delegations were attempted for focused feature audits, but the desktop thread-creation API rejected the supplied arguments. The main workspace audit and verification were completed directly; no delegated changes were applied.
- The manual also describes Audio Crossfade. This implementation adds the requested gapless playback setting and does not add a separate crossfade control.
