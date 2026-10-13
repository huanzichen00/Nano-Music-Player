# Nano Music Player

A Rust/Slint desktop music player inspired by the seventh-generation iPod nano. Device Mode fits the window to a narrow bezel and Home button; Screen Mode shows the display alone. Music playback, library browsing, keyboard navigation and synchronized lyrics work together through one action dispatcher.

## Run

Requires Rust 1.92 or newer and a working system audio output.

```sh
cargo run
# Use a separate library and configuration directory:
cargo run -- --config-dir ./local-config
```

On macOS, `scripts/package-macos.sh` builds `target/Nano Music Player.app`. Add `--debug` for a quicker development build. The UI images are embedded in the executable. The app bundle is for local use; signing and notarization are not configured.

Use **File → Import Music Folder** or **Cmd/Ctrl + O** to import folders recursively. Importing reads your music and keeps a metadata cache in SQLite; it does not rewrite song files. Audio remains at its original location; SQLite contains paths, metadata, artwork and lyrics. There is no configured library cap; practical limits are disk space and RAM. MP3, M4A, FLAC and WAV are supported. M4A playback uses Symphonia's AAC/ALAC decoders; AAC M4A is covered by an actual fixture test. Unsupported or damaged files are reported and skipped.

On macOS, File, Playback, View and Help are native menus at the top of the screen. Slint’s Winit/Muda integration handles them; software-rendered test snapshots show fallback menus inside the window.

## Keyboard and mouse

| Category | Shortcut | Behavior |
| --- | --- | --- |
| Playback | Space | Play / pause |
| Playback | Cmd/Ctrl + Right / Left | Next / previous track |
| Playback | Cmd/Ctrl + Up / Down | Volume; holding the keys repeats |
| Playback | M / S | Mute / shuffle |
| Playback | R | Show / hide romaji on Now Playing; cycle repeat elsewhere |
| Playback | N | Now Playing |
| Playback | Y | Toggle lyrics on Now Playing |
| Navigation | Up / Down or K / J | Move visible selection |
| Navigation | L / Enter | Open or activate selection in lists; Enter opens Home icons |
| Navigation | H / Escape / Backspace | Back in lists, restoring the previous selection |
| Navigation | Shift + H | Physical Home button |
| Navigation | F, then A–Z | Jump to the first matching list entry and scroll it into view |
| Navigation | Left / Right | Home grid/pages; back/activate in lists; seek on Now Playing; volume when its slider is selected |
| Application | Cmd/Ctrl + O / F | Import folder / search library |
| Application | Cmd/Ctrl + Comma | Settings |
| Application | Cmd/Ctrl + 1 / 2 | Device Mode / Screen Mode |
| Application | Cmd/Ctrl + Plus / Minus | Zoom; Cmd/Ctrl + Equals also zooms in |

**Home** (the round bezel button, or Shift + H outside text entry) returns to the Home screen through in-app dialogs. In Genius, **H / Left / Escape** closes the preview and **L / Right / Enter** creates or retries the playlist, then plays a ready result. Configured navigation bindings apply there too. Home hides an unfinished playlist draft; **New Playlist** resumes it. A save already started finishes in the background.

All app shortcuts pause while the search field has focus, preserving native text editing. Enter or Escape returns focus to results. Playback remains accessible across screens and during background scanning. **Help → Keyboard Shortcuts** displays the currently loaded bindings, grouped into the three categories above.

Available lyrics appear by default over the artwork in a translucent, left-aligned panel. Click the cover or lyrics to toggle this overlay together with the seek bar, repeat, shuffle and album track-list controls. Artwork fits the available region without cropping, including portrait and landscape images. Transport and volume remain available at the bottom. Lists and lyrics use Slint's native wheel/trackpad scrolling. Horizontal scrolling changes Home pages; the page-dot area also supports a mouse drag. Drag the bezel to move the window. Resize its edges or use zoom shortcuts; the display keeps its aspect ratio. Device Mode has a rounded transparent outer silhouette. **Settings → Always on Top** (also in the native View menu) starts enabled and persists your choice. macOS three-finger dragging is controlled by the system's trackpad settings.

## Lyrics

In Now Playing, swipe or scroll horizontally to the right across the artwork, lyrics or song details to return to the previous list. The small **Back** control at the top left does the same; H/Escape also works. Playback continues. Vertical movement scrolls lyrics, and dragging the seek/volume sliders keeps their normal behavior. Buttons, icons and sliders use the normal arrow cursor. An optional AeroSpace workspace-follow helper is documented in [macOS integration](docs/MACOS_INTEGRATION.md).

Use **File → Import / Replace Lyrics** or **Settings → Lyrics for Track** to associate a `.txt` or `.lrc` file with the selected song (or the playing song outside a song list). Manual imports take precedence and persist their content across restarts and rescans, even if the source lyrics file moves. **Remove Lyrics** suppresses automatic lyrics; **Use Automatic Lyrics** clears that choice and discovers lyrics again.

Automatic discovery checks a same-name `.lrc` beside the audio first, then `.txt`, then embedded synchronized/plain tags. Extension matching is case-insensitive. Blank sidecars fall back to the next source. TXT remains literal, scrollable text.

LRC supports repeated timestamps, decimal fractions, positive/negative offsets, UTF-8, UTF-16 and GBK sidecars. The current line follows the audio engine's playback position and scrolls into view. Seeking updates the highlight. Enhanced LRC word tags are reduced to text; highlighting is per line. Songs without lyrics or artwork still play normally and show an appropriate fallback. Unsupported SYLT timestamp units fall back to plain lyrics.

**Settings → DeepSeek API…** accepts a masked API key and model name (default `deepseek-flash`). You can alternatively set `DEEPSEEK_API_KEY` before launching; this takes precedence and is never copied into the credential file. Opening the dialog leaves the key field empty, and saving an empty field retains the saved key. Credentials are kept separately in `deepseek.json` inside the configuration directory, with owner-only file permissions on macOS/Linux. This is a local plaintext file, and both it and temporary credential files are excluded from Git.

Open **Lyrics for Track → Generate Romaji** for a song with Japanese lyrics. This explicit action sends its title, artist and existing Japanese lyric lines to the [DeepSeek API](https://api-docs.deepseek.com/api/create-chat-completion/); DeepSeek account usage applies. Generation runs in the background and adds smaller romaji beneath the original lines without changing lyric text or LRC timestamps. Repeated lines share one reading, and non-Japanese lines stay unchanged. Romaji persists in SQLite and works offline after generation. Editing the source text or timestamps hides outdated annotations until you regenerate.

Plain **R** shows/hides romaji only in the song interface (including volume selection); **Show Romaji** in Settings persists this preference. Existing custom R bindings take precedence. **Cancel Romaji Generation** discards pending results and stops further batches; an HTTP request already in flight can finish within its 120-second timeout. **Remove Romaji** deletes the annotation while keeping original lyrics. Incomplete or invalid responses never replace a previous annotation. Kanji readings can be ambiguous, so generated readings should be checked. Original lyrics and timestamps remain unchanged.

Open **Music → Genius** or the Genius control on Now Playing to create a playlist based on the selected song. DeepSeek chooses from a frozen snapshot of available local song metadata. The preview retains recommendation order; **Refresh**, **Play** and **Save** use that order and the existing playlist storage. It is a local-library recommendation feature; see [`docs/DEEPSEEK_GENIUS_TRANSLATION_PLAN.md`](docs/DEEPSEEK_GENIUS_TRANSLATION_PLAN.md) for its validation and cancellation rules.

Choose **Playback → Translation: English / 简体中文 / 日本語**, then **Translate Lyrics**. The target language persists independently of the interface language. Original lyrics, translation and optional romaji stay paired by original line and timestamp. Validated translations are cached locally for offline use; source or model changes invalidate old results. **Regenerate**, **Show / Hide** and **Remove Translation** manage the overlay. Requests occur only after an explicit action and send source lyric text to DeepSeek. Cancelling discards the result; a provider call already running remains bounded by the client timeout.

## Playlists

Open **Music → Playlists → Add Playlist** or **File → New Playlist…**. The Add Playlist entry participates in J/K navigation above All Songs; press Enter to open it. Optionally enter a name, press Enter to leave the name field, and select songs with checkboxes or **J/K + Space**. Review is available as soon as you select a song; a blank name receives a unique New Playlist N name. Choose Review, arrange the order with Move Up/Move Down or Remove, then Create. Saved playlists appear in Music → Playlists and survive restarts; their playback queue follows your chosen order. Cancel leaves the library and playback unchanged. Saving happens in the background; a failed save retains the draft for retry.

Names must be nonempty, single-line and at most 100 characters; duplicate names are rejected regardless of case. Missing paths remain stored and songs absent from the current library are skipped. Editing/deleting saved playlists and iTunes synchronization are not included yet.

## Configuration

The first launch creates `shortcuts.json` (version 4); `settings.json` saves display preferences and interface language. **Settings → Language** selects English (default), 简体中文 or 日本語. **Settings → Font** selects Classic, Maple Mono or JetBrains Mono, with bundled licensed fonts and a persisted preference; see [font notes](docs/FONTS.md). Music metadata and lyrics keep their original text. On macOS the default directory is `~/Library/Application Support/Nano.Nano-Music-Player`; Windows and Linux use their native application configuration directories. `--config-dir` overrides this.

**Settings → Keyboard Shortcuts** edits bindings in the app. Select an action with J/K, open it with L/Enter, choose **Replace keys** or **Add key**, then press a key combination. **Remove keys** disables that action’s shortcuts, **Default keys** restores its bindings, and **Restore All Defaults** resets the complete keymap. Changes are validated for conflicts, saved atomically, and applied immediately; Escape cancels recording or closes the editor. You can also edit JSON while the app is closed and restart to apply changes. Examples are in [`examples/`](examples/). Shortcut entries contain a semantic action, a key, and exact modifiers. `primary` means Command on macOS and Control elsewhere; `secondary` means Control on macOS and Super elsewhere. Remove a binding to disable it, or add an alias. Duplicate/ambiguous bindings are rejected with a visible message and defaults are used.

Untouched version 1–3 defaults upgrade to the current navigation and F-prefix shortcut. Customized maps remain unchanged; Jump to letter can be assigned in Settings. On Home, H/L move horizontally between icons and pages, J/K move vertically, and Enter opens the selection. In lists, H/L go back/open and J/K move by one row. Arrow keys additionally provide seeking and volume adjustments on Now Playing.

`zoom` ranges from 0.75 to 3.0. A saved `window_width` takes precedence over zoom at startup; `window_height` is used when only height is provided. Remove both dimensions to size the window from `zoom`. Screen Mode preserves 240:432; Device Mode follows the complete hardware ratio 39.6:76.5. The LCD uses one logical coordinate system at every zoom. Resizing persists the resulting dimensions.

## Structure and verification

- `src/input/`: platform key conversion, configurable bindings, centralized context/repeat routing.
- `src/app/`: navigation stack, shared action reducer, controller and settings.
- `src/audio/`: queue/repeat state and a worker owning the audio output, decoder and sink.
- `src/library/`: background scanning, Lofty metadata, SQLite cache and lyrics.
- `ui/`: declarative Slint screens; one root `capture-key-pressed` callback.

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

Tests include dispatcher/state regressions, small original MP3/M4A/FLAC/WAV fixtures, embedded lyrics, and headless Slint key/pointer events plus rendered screenshots. GUI test snapshots are written to the system temporary directory. Platform-specific audio output and gestures still benefit from native testing on each target OS.

Settings includes customizable shortcuts, display mode, zoom, per-track lyrics and language. The remaining nonmusic Home icons currently open placeholders. Radio, clock, fitness, photos and podcasts remain outside this implementation; Playlists includes All Songs and saved user-created lists. Videos imports and plays local files with aspect-preserving frames, transport/seek/volume controls and whole-device landscape rotation. Playback requires a compatible libmpv runtime; import requires ffprobe. See [`docs/VIDEO_IMPLEMENTATION.md`](docs/VIDEO_IMPLEMENTATION.md) for installation, verified formats and platform limits. The player layout follows the provided seventh-generation nano photos and the Apollomaniacs music guide. See [`docs/RESOURCES.md`](docs/RESOURCES.md) for sources, asset provenance and licenses.

Artists, Albums and global Songs sort alphabetically using case-insensitive keys with deterministic tie-breakers. Individual albums retain track-number order. Album and artist rows show representative cover thumbnails.

See [`docs/MACOS_INTEGRATION.md`](docs/MACOS_INTEGRATION.md) for the stable bundle ID and targeted AeroSpace floating rule. The app leaves your window-manager configuration untouched. [`docs/FUTURE_MEDIA.md`](docs/FUTURE_MEDIA.md) records the original video feasibility study and future podcast architecture; local video playback is now implemented. Implementation phases and verified checks are tracked in [`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md).

The Rust/Slint architecture supports Windows and Linux, but this implementation has been built and exercised on macOS only. Native Windows/Linux compilation, audio output, packaging and UI behavior remain unverified.
