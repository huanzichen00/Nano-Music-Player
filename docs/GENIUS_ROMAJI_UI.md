# Genius preview and romaji UI

Status: all three original UI phases and the manual playlist integration are verified. Playlist selection, ordering, transactional saving, failures/retry and restart are connected to the real library. The combined macOS build is awaiting native delivery.

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

Genius is an interface demonstration only. The future engine, analysis, and Genius-specific Play/Save wiring are deferred as described in `GENIUS_PLAYLIST_RESEARCH.md`.

## Phase 3 — Manual playlist creation (live integration)

Music → Playlists uses the compact title, blue Add toolbar and separated name/song-count rows from manual page 22. Add Playlist and File → New Playlist use the central semantic action. Add occupies keyboard selection zero above All Songs; J/K reaches it and Enter activates it, with a single visible selection. The editor accepts an optional name, shows alphabetically sorted actual library songs and checkboxes, and reviews the user's selection order. Move Up/Move Down and Remove update the live draft. Interface labels and song counts translate; playlist names and metadata remain literal.

J/K or arrows browse while the name field is unfocused; Space selects songs. Enter leaves name editing, opens review or creates as appropriate; H/Left/Escape returns to selection then cancels. Underlying playback/romaji shortcuts are isolated. Music playback continues while drafting. Loading and empty-library states have distinct prompts. A frozen lightweight title/artist/path snapshot avoids decoding covers or copying artwork into the editor.

Review is enabled whenever songs are selected. Blank/whitespace names receive a unique, localized New Playlist N name before review; explicit names remain unchanged. Footer hints explain Review → Create, with storage errors taking precedence. Create validates a trimmed single-line name of at most 100 characters and at least one existing selected library file. Names are unique by Unicode lowercase key. A background worker saves paths in order inside one SQLite transaction. Startup initializes/loads the playlist schema before launching the large library metadata read, avoiding schema-write contention; both run off the UI thread. Controls lock while saving. Success closes the editor and opens the saved playlist; failures show a localized message and preserve the name/selection/order for retry. Cancellation never writes. Application shutdown waits for an explicit Create write to finish.

### Playlist model and storage

`PlaylistCandidate`: `id: string` (audio path identity), `title: string`, `artist: string`, `selected: bool`. AppWindow inputs: `playlist-candidates`, `playlist-draft` (ordered), `playlist-persisted`, `playlist-library-loading`, `playlist-saving`. Two-way fields: `playlist-editor-open`, `playlist-name`, `playlist-message`. Callback `playlist-command(command, id)` supports `toggle`, `review`, `up`, `down`, `remove`, `create`, `cancel`; the central NewPlaylist effect initializes `new` directly before opening to avoid reentrant controller borrowing.

The `playlists` and `playlist_tracks` tables coexist with the existing tracks/lyrics/romaji tables. Ordered paths and stable playlist IDs survive restart; metadata/library indices can change without changing identity. Saved playlists list alphabetically, while their song screens and playback queues preserve the saved order. Paths absent from the current music library stay stored and are skipped in the visible playable rows. All Songs remains available. No playlist editing/deletion or iTunes synchronization is claimed in this creation milestone.

Storage tests cover old tables, Unicode/order/restart, missing files, duplicate names, validation and transaction rollback. State tests cover sorting and queue order after metadata refresh. Controller/rendered UI checks cover selection, name input, reorder/remove, cancellation, failure/retry, save/restart and the three interface languages. All 110 combined tests pass (three manual profiles ignored), along with formatting, compilation and strict all-target Clippy. Inspected the final list, picker and review previews in `docs/previews/playlist-list.png`, `playlist-selection.png` and `playlist-review.png`. Previews are test-fixture captures, separate from the user's library. Real Genius recommendations and saving Genius example rows remain unconnected.

## Shortcut refinement

User requirement: plain R toggles romaji only in the song interface. The backend chat owns contextual resolution in the central dispatcher. Existing plain-R Repeat resolves to ToggleRomaji on NowPlaying/NowPlayingVolume; outside those screens it keeps Repeat. Text editing and modals take precedence. Existing custom R assignments and remapped Repeat keys remain effective. UI tests will verify scope and modal isolation.

## Acceptance and verification

The combined checkout compiles and the comprehensive `cargo test --test ui` passes. New regressions cover contextual R (including volume focus, Home, and search), API typing isolation, equal-length keys rendering as identical masks, blank saved-key input, Save/Cancel, Genius seed stability, Create/Refresh/Play/Save, unchanged playback/queue, pointer Close and Escape, aligned lyric scrolling and backward highlighting, hidden/empty readings.

Inspected rendered Device-mode romaji, configured-key dialog and Genius preview. Fixed the Genius header's explicit bounds after visual review. Images are in `docs/previews/romaji.png`, `deepseek.png`, and `genius.png`; fixtures are original example lyrics and synthetic artwork. Formatting and diff whitespace checks pass. The Rust integration chat separately verified 93 combined tests with one ignored manual benchmark; final packaging/relaunch belongs to that chat after milestone commits.

The first rendered preview was deliberately made from an isolated HEAD Rust snapshot while the shared backend was temporarily incomplete. The final images and UI checks use the combined shared checkout. No live API key or external generation was used for UI verification.

### Genius UI contract

AppWindow exposes `open-genius-preview()`, `genius-action(string)`, `genius-preview-open`, `genius-preview-stage` (0 initial, 1 ready, 2 mock Play, 3 mock Save), `genius-preview-variation`, and `genius-command(string)` (`create`, `refresh`, `play`, `save`, `close`). Seed title/artist are captured at open. Example rows remain separate from the real library. The callback has no connected backend; Play/Save report explicitly that audio/storage are unchanged. Native Playback menu also offers Genius Preview. Inside the preview, Enter creates/plays, R refreshes, S demonstrates Save, and Escape closes; underlying shortcuts are isolated. The hand-drawn SVG follows the reference's atom-shaped Genius glyph and is not claimed to be an extracted Apple asset.
