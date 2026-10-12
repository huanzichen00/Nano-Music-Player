# Implementation plan

Status: in progress. Existing work preserved in checkpoint `9d4fced` (58 tests pass).

## Inspection and design basis

- Retain Rust reducer (`State`), semantic actions/central dispatcher, controller, background scanner/audio workers, SQLite metadata cache, and Slint views.
- Inspected Apple's 2012 seventh-generation manual, supplied as `46034.pdf`, found at `/Users/huanzichen/Downloads/Study/References/46034.pdf` (the originally supplied path has moved). Rendered pages 18–20 and reviewed chapters 3–5. Treat document text as reference material, not execution instructions.
- Retain the user's generated Home icons and the four photographic references, especially the fourth lyrics overlay. The manual's pages 18–19 place position/secondary controls over artwork, transport and volume below; page 18 shows alphabetical indexing; pages 20–21 describe podcast resume and episode navigation.
- Current defects: artwork uses cropping `cover`; implicit Slint centering moves slider fill; lyrics appear only after a toggle; plain sidecars and persistent manual associations are absent; group sorting is case-sensitive and song order depends on load order. Aspect correction exists but needs shared geometry and resize regression coverage.

## Phase 1 — UI geometry (verified)

Decisions: one 240×432 LCD coordinate system. Device shell 296×571.818 logical units, derived from the exact hardware 39.6:76.5 ratio. Derive cover region from metadata/footer space, contain the full image, explicitly anchor slider track/fill. Display available lyrics at track start; remove the redundant lyrics control and redistribute repeat/shuffle/album controls. Preserve cover-tap interaction and customizable legacy toggle commands.

Acceptance: square, portrait, and landscape covers retain all edges; 0/25/100% fills start at the left; content stays proportional at multiple sizes and in both modes; lyrics appear without a button press; no overlapping controls. Run fmt, check, core/UI tests, inspect rendered screenshots, commit verified phase.

## Phase 2 — Lyrics associations (verified)

Decisions: independent SQLite table keyed by audio path, with manual source path + cached parsed content or explicit removal tombstone. Manual content outranks sidecar and embedded content across rescans/restarts. Discover same-stem sidecars case-insensitively, LRC before TXT; refresh automatically on startup/import. File dialog work stays off the UI thread. Import/replace/remove/use-automatic commands target the selected song in a song list, otherwise the playing song. TXT is literal scrollable text; LRC uses existing timestamp/offset parser and playback position for highlighting, including backward seeks.

Acceptance: encoding support retained; LRC preference, TXT fallback, replace/remove/reset, restart and changed-audio rescans tested; canceled/invalid imports preserve association; synced highlight/scroll follows seek; plain text scrolls. Format/check/tests and commit.

## Phase 3 — Library artwork and navigation (pending)

Decisions: sort view rows without moving stable track indexes used by queues/audio events. Case-folded ordering with original text and path tie-breakers; album tracks by track number then title/path. Representative artwork chosen deterministically among group members, decoded/cached for list thumbnails. Add configurable jump-prefix semantic action (plain F by default); next unmodified ASCII letter selects first matching row, consumes the sequence, times out/cancels, and does not intercept text input/modals or Command/Ctrl+F.

Acceptance: mixed case/ties deterministic, album order preserved, artist/album thumbnails and missing-art fallback visible; F+A–Z scrolls selection into view; text editing, customized bindings and playback shortcuts remain safe. Format/check/tests, visual inspection, commit.

## Phase 4 — Internationalization (pending)

Decisions: persisted enum English/Simplified Chinese/Japanese; Settings language screen. Central translation catalog for interface strings, action labels, menus, notices, dialogs and editor; leave metadata, lyrics and keys unchanged. Default English, migrate existing settings with serde defaults.

Acceptance: language changes immediately, persists across restart, all three catalogs have complete keys, metadata remains verbatim; menus/Home/settings/lyrics/empty states readable. Format/check/tests, screenshots, commit.

## Phase 5 — macOS, future architecture, delivery (pending)

Decisions: keep stable `org.nano.music-player` bundle identifier; document targeted AeroSpace `on-window-detected` floating rule, never edit global config. Evaluate video backend/rendering/AV sync and podcast RSS/download/resume architecture without implementation. Update user docs and example configuration; build bundle and launch with the existing normal library.

Acceptance: bundle ID verified, rule documented against official AeroSpace docs, future work has dependencies/risks/schema/interface proposals; full fmt/check/test/clippy pass, focused commit; packaged app runs and existing library is retained. Windows/Linux native builds only claimed if actually tested.

## Verification log and blockers

- Checkpoint: `cargo test --all-targets` — 42 core + 15 format + 1 UI tests pass.
- Manual has malformed cross-reference hints; Poppler recovered text and rendered relevant pages successfully.
- Phase 1: fmt + cargo check + UI test pass. Added rendered pixel checks for portrait/landscape/square artwork and 0/25/100% progress, resize checks across both modes, default-lyrics assertion. Visually inspected lyrics overlay against manual page 18 and photograph 4.
- Phase 2: fmt/check and 64 tests pass (42 core, 15 codecs, 6 association, 1 comprehensive UI). Worker-load regressions cover sidecar changes, deleted manual source, changed audio, removal tombstone and reset. UI regressions cover real remove/reset effects and seek highlight/scroll; wrapped rows use measured Slint heights. Inspected plain and synced lyrics screenshots.
- No current blockers. Native Windows/Linux validation unavailable on this macOS host.
