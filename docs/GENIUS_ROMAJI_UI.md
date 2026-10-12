# Genius preview and romaji UI

Status: phases 1–2 verified; phase 3 queued. UI ownership is limited to Slint, UI assets, `tests/ui.rs`, and UI documentation/previews. The parallel chat owns Rust actions, translation catalog, configuration, network requests, caching, and persistence.

## Phase 1 — Genius interface preview (verified)

- Add a nano-style Genius symbol between Repeat and Shuffle in expanded Now Playing controls.
- Freeze the seed title/artist when opening a preview. Show clearly labeled example rows; never present All Songs as recommendations.
- Demonstrate Create, Refresh, Play, and Save states without changing audio playback or saving a playlist. Expose a callback for future backend integration.
- Verify pointer entry, seed stability, refresh, close, mock state transitions, and keyboard isolation.

## Phase 2 — Romaji and API settings (verified)

- Add `romaji-lines: [string]` and `romaji-visible: bool` to AppWindow and NowPlayingView. Entries align with original lyric indices; empty entries add no secondary row.
- Measure each lyric row including wrapped romaji, preserving active highlighting and seek/scroll alignment. Original text remains unchanged.
- Add the exact agreed DeepSeek dialog properties and `deepseek-command(string)` callback. Mask API input, leave saved keys unfilled, retain a configured key when Save receives blank input, and isolate editing from playback shortcuts.
- Rust supplies Settings/per-track actions (`generate_romaji`, `remove_romaji`, `toggle_romaji`, `configure_deep_seek`), translated strings, and storage/network behavior.
- Verify empty/hidden/multiline romaji, scrolling and seeking, masked editing, Save/Cancel callbacks, and text-input shortcut isolation. Inspect rendered screenshots in Screen and Device modes.

## Integration boundary

AppWindow's dialog inputs: `deepseek-settings-open`, `deepseek-key-configured`, `deepseek-message`. Two-way values: `deepseek-api-key` (blank initially) and `deepseek-model` (default `deepseek-flash`). Callback: `deepseek-command` with `save` or `cancel`.

Genius is an interface demonstration only. The future engine, analysis, ordered playlist persistence, and actual Play/Save are deferred as described in `GENIUS_PLAYLIST_RESEARCH.md`.

## Phase 3 — Manual playlist creation interface (queued)

The user requested this next UI phase while romaji/Genius integration is underway. Provide a playlist name, library-song selection, an ordered draft preview, and Create/Cancel actions. Keep manual selection separate from Genius example data. Establish an explicit callback/model contract before playlist persistence is connected; do not claim saved playlists until the backend supports them.

## Shortcut refinement

User requirement: plain R toggles romaji only in the song interface. The backend chat owns contextual resolution in the central dispatcher. Existing plain-R Repeat resolves to ToggleRomaji on NowPlaying/NowPlayingVolume; outside those screens it keeps Repeat. Text editing and modals take precedence. Existing custom R assignments and remapped Repeat keys remain effective. UI tests will verify scope and modal isolation.

## Acceptance and verification

The combined checkout compiles and the comprehensive `cargo test --test ui` passes. New regressions cover contextual R (including volume focus, Home, and search), API typing isolation, equal-length keys rendering as identical masks, blank saved-key input, Save/Cancel, Genius seed stability, Create/Refresh/Play/Save, unchanged playback/queue, pointer Close and Escape, aligned lyric scrolling and backward highlighting, hidden/empty readings.

Inspected rendered Device-mode romaji, configured-key dialog and Genius preview. Fixed the Genius header's explicit bounds after visual review. Images are in `docs/previews/romaji.png`, `deepseek.png`, and `genius.png`; fixtures are original example lyrics and synthetic artwork. Formatting and diff whitespace checks pass. The Rust integration chat separately verified 93 combined tests with one ignored manual benchmark; final packaging/relaunch belongs to that chat after milestone commits.

The first rendered preview was deliberately made from an isolated HEAD Rust snapshot while the shared backend was temporarily incomplete. The final images and UI checks use the combined shared checkout. No live API key or external generation was used for UI verification.

### Genius UI contract

AppWindow exposes `open-genius-preview()`, `genius-action(string)`, `genius-preview-open`, `genius-preview-stage` (0 initial, 1 ready, 2 mock Play, 3 mock Save), `genius-preview-variation`, and `genius-command(string)` (`create`, `refresh`, `play`, `save`, `close`). Seed title/artist are captured at open. Example rows remain separate from the real library. The callback has no connected backend; Play/Save report explicitly that audio/storage are unchanged. Native Playback menu also offers Genius Preview. Inside the preview, Enter creates/plays, R refreshes, S demonstrates Save, and Escape closes; underlying shortcuts are isolated. The hand-drawn SVG follows the reference's atom-shaped Genius glyph and is not claimed to be an extracted Apple asset.
