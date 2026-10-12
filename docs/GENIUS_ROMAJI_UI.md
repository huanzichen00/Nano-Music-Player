# Genius preview and romaji UI

Status: all three UI phases verified. Manual playlist library-selection and persistence adapters are queued. UI ownership is limited to Slint, UI assets, `tests/ui.rs`, and UI documentation/previews, with narrow playlist translation additions agreed after the romaji backend commit. The parallel chat owns Rust actions, configuration, network requests, caching, and persistence.

## Phase 1 — Genius interface preview (verified)

- Add a nano-style Genius symbol between Repeat and Shuffle in expanded Now Playing controls.
- Add Music → Genius with a translated preview subtitle. Pointer and keyboard activation open the same preview while preserving Music navigation, playback and queue. Without a playing song, show a choose-song prompt and disable Create, including Enter.
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

## Phase 3 — Manual playlist creation interface (verified; backend queued)

The user requested this next UI phase while romaji/Genius integration was underway. Added File → New Playlist, name entry, candidate checkboxes, selected-song count, an ordered review with Move Up/Move Down/Remove, Create/Cancel, and keyboard isolation. Arrow keys browse rows, Space emits selection, Enter leaves name editing or opens review, and Escape returns to selection/closes. Interface labels are English/Chinese/Japanese; names and metadata remain literal.

The library adapter is intentionally queued separately from the romaji-first backend scope. The live app's unconnected editor says song selection is coming next, with Review/Create disabled until candidate/draft models are supplied. Create reports an unsaved draft, never a saved success. Rendered previews use supplied example models and simulated callback results; they are interface demonstrations rather than evidence of playlist persistence.

### Playlist handoff contract

`PlaylistCandidate`: `id: string` (audio path identity), `title: string`, `artist: string`, `selected: bool`. AppWindow inputs: `playlist-candidates`, `playlist-draft` (ordered), `playlist-persisted`. Two-way fields: `playlist-editor-open`, `playlist-name`, `playlist-message` (canonical UI key or literal message). Output: `playlist-reviewing`. Functions: `open-playlist-editor()`, `playlist-action(string, string)`. Callback: `playlist-command(command, id)` with `new`, `toggle`, `up`, `down`, `remove`, `create`, `cancel`.

The future adapter freezes a lightweight library snapshot at `new`, sorts candidates case-insensitively/deterministically, maintains selected path order, updates checkbox/draft models after each command, validates trimmed names and live local paths, and persists ordered paths only after an explicit Create. Set `playlist-persisted` and a saved message only after storage succeeds. Do not persist transient row indices. Preserve playback while editing, and retain the draft if saving fails. This adapter and playlist storage remain queued; Genius example rows never enter the manual candidate model.

Verification: `cargo check`, full 93 tests (one manual benchmark ignored), formatting, strict Clippy, and whitespace checks pass. Inspected English picker/review and Japanese review, fixed compact-button text sizing and translated-header spacing, and checked immediate status translation. UI tests cover name input (including R), callback path identities, ordered supplied rows, focus after reordering, Remove/Create, unsaved status, all three languages, Escape and empty/unconnected state. Previews: `docs/previews/playlist-selection.png`, `playlist-review.png`. Backend adapter and live-library persistence are not claimed as implemented.

## Shortcut refinement

User requirement: plain R toggles romaji only in the song interface. The backend chat owns contextual resolution in the central dispatcher. Existing plain-R Repeat resolves to ToggleRomaji on NowPlaying/NowPlayingVolume; outside those screens it keeps Repeat. Text editing and modals take precedence. Existing custom R assignments and remapped Repeat keys remain effective. UI tests will verify scope and modal isolation.

## Acceptance and verification

The combined checkout compiles and the comprehensive `cargo test --test ui` passes. New regressions cover contextual R (including volume focus, Home, and search), API typing isolation, equal-length keys rendering as identical masks, blank saved-key input, Save/Cancel, Genius seed stability, Create/Refresh/Play/Save, unchanged playback/queue, pointer Close and Escape, aligned lyric scrolling and backward highlighting, hidden/empty readings.

Inspected rendered Device-mode romaji, configured-key dialog and Genius preview. Fixed the Genius header's explicit bounds after visual review. Images are in `docs/previews/romaji.png`, `deepseek.png`, and `genius.png`; fixtures are original example lyrics and synthetic artwork. Formatting and diff whitespace checks pass. The Rust integration chat separately verified 93 combined tests with one ignored manual benchmark; final packaging/relaunch belongs to that chat after milestone commits.

The first rendered preview was deliberately made from an isolated HEAD Rust snapshot while the shared backend was temporarily incomplete. The final images and UI checks use the combined shared checkout. No live API key or external generation was used for UI verification.

### Genius UI contract

AppWindow exposes `open-genius-preview()`, `genius-action(string)`, `genius-preview-open`, `genius-preview-stage` (0 initial, 1 ready, 2 mock Play, 3 mock Save), `genius-preview-variation`, and `genius-command(string)` (`create`, `refresh`, `play`, `save`, `close`). Seed title/artist are captured at open. Example rows remain separate from the real library. The callback has no connected backend; Play/Save report explicitly that audio/storage are unchanged. Native Playback menu also offers Genius Preview. Inside the preview, Enter creates/plays, R refreshes, S demonstrates Save, and Escape closes; underlying shortcuts are isolated. The hand-drawn SVG follows the reference's atom-shaped Genius glyph and is not claimed to be an extracted Apple asset.
