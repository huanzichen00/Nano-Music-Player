# Implementation plan

Status: original phases, romaji, Music Genius entry and Albums optimization verified; playlist UI verified with backend queued. User-requested font selection is the next active phase. Existing work preserved in checkpoint `9d4fced` (58 tests pass).

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

## Phase 3 — Library artwork and navigation (verified)

Decisions: sort view rows without moving stable track indexes used by queues/audio events. Case-folded ordering with original text and path tie-breakers; album tracks by track number then title/path. Representative artwork chosen deterministically among group members, decoded/cached for list thumbnails. Add configurable jump-prefix semantic action (plain F by default); next unmodified ASCII letter selects first matching row, consumes the sequence, times out/cancels, and does not intercept text input/modals or Command/Ctrl+F.

Acceptance: mixed case/ties deterministic, album order preserved, artist/album thumbnails and missing-art fallback visible; F+A–Z scrolls selection into view; text editing, customized bindings and playback shortcuts remain safe. Format/check/tests, visual inspection, commit.

User refinement: the first 64-unit padded cover rows were rejected as oversized. Re-inspected the actual Albums illustration on manual page 10 (the Flick gesture example), extracting its original 340×612 screenshot. It has 82-pixel rows, an 81-pixel cover column and a 38-pixel header: at a 240-unit LCD width these become approximately 58-unit rows, 57-unit covers and a 27-unit header. Use 58-unit cover rows with 1-unit padding and a single compact 26-unit header, restoring seven visible rows. Keep the small Back affordance, full artwork aspect ratio and artist subtitles. Slint exports row/viewport metrics so decoding and scrolling share the actual geometry. Ordinary song/settings rows retain 48 units.

## Phase 4 — Internationalization (verified)

Decisions: persisted enum English/Simplified Chinese/Japanese; Settings language screen. Central translation catalog for interface strings, action labels, menus, notices, dialogs and editor; leave metadata, lyrics and keys unchanged. Default English, migrate existing settings with serde defaults.

Acceptance: language changes immediately, persists across restart, all three catalogs have complete keys, metadata remains verbatim; menus/Home/settings/lyrics/empty states readable. Format/check/tests, screenshots, commit.

## Phase 5 — macOS, future architecture, delivery (verified)

Decisions: keep stable `org.nano.music-player` bundle identifier; document targeted AeroSpace `on-window-detected` floating rule, never edit global config. Evaluate video backend/rendering/AV sync and podcast RSS/download/resume architecture without implementation. Update user docs and example configuration; build bundle and launch with the existing normal library.

Acceptance: bundle ID verified, rule documented against official AeroSpace docs, future work has dependencies/risks/schema/interface proposals; full fmt/check/test/clippy pass, focused commit; packaged app runs and existing library is retained. Windows/Linux native builds only claimed if actually tested.

## Phase 6 — DeepSeek Japanese romaji (verified)

Scope: romanize existing lyrics first; lyric filling, translation and real Genius generation remain deferred. The separate UI chat owns paired lyric rows, the masked settings dialog and an explicitly labeled Genius mock. Apple Genius research is recorded in `GENIUS_PLAYLIST_RESEARCH.md` and committed independently.

Decisions: use DeepSeek's JSON chat endpoint with `deepseek-flash` by default, thinking disabled and bounded output/timeouts. Requests run on a background worker only after an explicit Generate Romaji action. Deduplicate repeated Japanese lines, batch with stable line IDs, reject missing/duplicate/unknown IDs, non-ASCII/multiline output and incomplete responses, and publish only a complete annotation. Original lyrics and timestamps are immutable; exact source snapshots bind the SQLite annotation and prevent applying a result after lyrics change. Cancel discards the result and stops subsequent batches; an in-flight request remains bounded by its timeout. Serialize local persistence writes and drain them at shutdown so deletion cannot be reversed by a queued save.

Credentials: a separate atomically written `deepseek.json` with Unix mode 0600, redacted debug formatting and generic provider errors. The masked editor never preloads the saved secret; blank saves preserve it. `DEEPSEEK_API_KEY` overrides the saved key without being persisted. Credential and temporary files are ignored by Git. The fixed HTTPS endpoint disables redirects. Romaji visibility persists in normal settings, and plain R uses the existing Repeat binding to toggle only on the Now Playing surface, with text/modal guards and customized bindings preserved.

Acceptance: local HTTP tests cover API request shape, error statuses, truncated/malformed/oversized replies and redacted errors; generation tests cover repeated LRC lines, blanks, English, unchanged timestamps, invalid IDs, cancellation and later-batch failure without partial results. Restart/controller coverage checks cached display, persistent visibility, blank-key saves, invalid model rejection, cancelling the settings editor and annotation removal without changing source lyrics. Native Windows/Linux validation and live provider reading accuracy remain unverified. A real DeepSeek request requires the user's own key; no credential was supplied in this chat.

## Phase 7 — Manual playlist creation UI (verified; backend queued)

User requested a playlist creation interface after the romaji/Genius milestone. Added a declarative name/song-selection and ordered review interface, with reordering/removal callbacks and explicit unsaved state. Keep actual library-model population and ordered-path persistence queued as a separate backend phase; the live unconnected editor communicates that selection is coming next. Do not show saved success before persistence is implemented. The exact model/callback contract and rendered example previews are in `GENIUS_ROMAJI_UI.md`. This phase only changes UI, its narrow translation keys, tests and documentation.

Acceptance: name editing suppresses playback shortcuts, candidate callbacks carry path identities, review preserves supplied order, reordering retains focused identity, Remove/Create emit the intended callbacks, unsaved status stays honest, labels translate while metadata/name remain literal, and empty/unconnected UI does not claim the existing music library is empty. Full formatting, compilation, 93 tests, strict Clippy and whitespace checks pass; rendered English/Japanese screens reviewed. Library selection and saving remain scheduled work.

## Phase 8 — Font selection (verified)

User requested Maple Mono and JetBrains Mono in Settings after noticing the lyric letter s. Add Settings → Font with Classic, Maple Mono and JetBrains Mono, an immediate whole-interface font choice and persisted enum with Classic defaults for old settings. Bundle unmodified regular/semibold/bold TTFs from their official stable releases, with the full SIL OFL licenses. Do not install fonts system-wide. Keep font names/metadata/lyrics literal; translate interface labels. Font rows preview each face, including Latin and CJK. Japanese/Chinese use system glyph fallback where the chosen font lacks coverage. No per-character size rules: each lyric line uses one font/size, active highlighting retains existing weights.

Acceptance: switch via keyboard/pointer, persist/reopen selection, old settings migrate and invalid font values reject, different bundled fonts actually change rendered Latin glyphs without altering lyric text, wrapping/highlight/scroll and compact controls stay readable. Verify Classic/Maple/JetBrains lyric screenshots with s/e/a/o/n and Japanese/Chinese examples. Run formatting, compilation, tests and strict lint; commit, build and launch the combined app.

## Verification log and blockers

- Fonts: full combined tests pass (103, three ignored manual profiles), formatting/compilation/strict Clippy/whitespace checks pass. Settings switching, persisted restart, old-config defaults, unchanged metadata/lyrics and distinct rendered lyric glyph regions verified. Inspected Classic/Maple/JetBrains previews with Latin s/e/a/o/n and CJK; licenses bundled. Native Windows/Linux validation remains unverified.


- Albums entry: coordinated optimization commit `1f9cde4` publishes ready 96-pixel covers with loaded/imported tracks, then sharpens visible rows off-thread. On the read-only 42-track/six-album debug fixture, cold entry changed from 8,465 ms to 0.13–0.41 ms; first software-rendered frame takes 44.60–45.63 ms. Preparation is background startup/import work, distinct from entry latency. Full combined tests pass (102, three ignored manual profiles); strict Clippy and formatting pass. Synthetic direct-merge UI fixtures wait for workers before cover screenshots. See `LIBRARY_PERFORMANCE.md` for exact limits and measurements.

- Music Genius entry: the comprehensive rendered UI test and 44 core tests pass. Pointer/keyboard activation preserves the Music parent, playback and queue; no-song entry blocks Create/Enter and prompts for a song. Original Songs/Albums row positions are preserved. Formatting/whitespace checks pass. Full strict checks will run after the concurrent thumbnail worker is integrated.
- Latest playlist/romaji debug bundle was built and launched with the normal configuration; native playback confirms all 42 tracks. At the user's subsequent explicit request, backed up `~/.aerospace.toml`, added the targeted `org.nano.music-player` rule, validated/reloaded AeroSpace 0.21.2, and confirmed window 86512 has `floating` layout. The earlier no-automatic-config-change policy is superseded by this user-authorized edit; the player itself still does not modify that configuration.

- Checkpoint: `cargo test --all-targets` — 42 core + 15 format + 1 UI tests pass.
- Manual has malformed cross-reference hints; Poppler recovered text and rendered relevant pages successfully.
- Phase 1: fmt + cargo check + UI test pass. Added rendered pixel checks for portrait/landscape/square artwork and 0/25/100% progress, resize checks across both modes, default-lyrics assertion. Visually inspected lyrics overlay against manual page 18 and photograph 4.
- Phase 2: fmt/check and 64 tests pass (42 core, 15 codecs, 6 association, 1 comprehensive UI). Worker-load regressions cover sidecar changes, deleted manual source, changed audio, removal tombstone and reset. UI regressions cover real remove/reset effects and seek highlight/scroll; wrapped rows use measured Slint heights. Inspected plain and synced lyrics screenshots.
- Phase 3: fmt/check and 68 tests pass. Sorting uses lowercase text, original spelling and path tie-breakers; unknown album track numbers sort after known numbers. UI sequence tests cover scroll-to-match, no-match letter consuming, Escape, Command/Ctrl+F, text entry and Home guards. Representative thumbnail screenshots inspected. Original 112×112 firmware Settings icon used directly; generated Home assets retained.
- Phase 4: fmt/check and 72 tests pass (3 catalog, 43 core, 15 format, 4 ordering, 6 association, 1 comprehensive UI). Catalog coverage checks static UI keys and customizable action titles. Settings-driven changes, old-config English defaults and persisted languages tested. Metadata deliberately named Music/Settings/Lyrics remains unchanged. Inspected Chinese/Japanese Home, Settings and language-selection screenshots; native menus receive the same translated bindings.
- Album-list refinement: fmt/check and comprehensive UI test pass; inspected larger-cover screenshots. The initial ten-album regression verified shared row geometry. Corrected page-10 density passes the comprehensive UI regression: seven cover rows fit the LCD and F+Z scrolls to the final album. Inspected Albums and Artists renderings alongside the extracted manual screenshot. Packaged and relaunched the combined app after verification.
- Phase 5: cargo fmt --all -- --check, cargo check, cargo test --all-targets, cargo clippy --all-targets -- -D warnings and git diff --check pass. The shared tree includes a separately coordinated playback-artwork performance change, with 78 passing tests and one ignored manual benchmark. Kept commits/file ownership separate.
- Packaged a local debug app with stable bundle ID org.nano.music-player and relaunched it using the normal configuration directory. Existing library has 42 tracks. Native Home/playback/lyrics and macOS menus inspected; corrected album-list proportions verified in rendered UI tests against the extracted page-10 screenshot. The user is actively interacting with the launched app, so further navigation was left to them.
- AeroSpace rule verified against the official current guide and documented; global user configuration was not modified. Video/podcast architecture is documented only. No current blockers; native Windows/Linux build/runtime validation remains unavailable on this macOS host.
- Romaji/Genius interface milestone: `cargo test --all-targets` passes 93 tests with one ignored manual benchmark; `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` and `git diff --check` pass. The shared tree also contains the independently owned progressive artwork change; only its agreed Cargo dependency and controller thumbnail handoff are included with the romaji backend. Reviewed rendered romaji, masked DeepSeek settings and labeled Genius preview. The UI milestone is commit `4fe7203`; research is commit `3257e75`. No live provider call or real Genius/translation engine was implemented in this pass.
