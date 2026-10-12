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
| Playback | M / S / R | Mute / shuffle / cycle repeat Off, All, One |
| Playback | N | Now Playing |
| Playback | Y | Toggle lyrics on Now Playing |
| Navigation | Up / Down or K / J | Move visible selection |
| Navigation | L / Enter | Open or activate selection in lists; Enter opens Home icons |
| Navigation | H / Escape / Backspace | Back in lists, restoring the previous selection |
| Navigation | Shift + H | Physical Home button |
| Navigation | Left / Right | Home grid/pages; back/activate in lists; seek on Now Playing; volume when its slider is selected |
| Application | Cmd/Ctrl + O / F | Import folder / search library |
| Application | Cmd/Ctrl + Comma | Settings |
| Application | Cmd/Ctrl + 1 / 2 | Device Mode / Screen Mode |
| Application | Cmd/Ctrl + Plus / Minus | Zoom; Cmd/Ctrl + Equals also zooms in |

All app shortcuts pause while the search field has focus, preserving native text editing. Enter or Escape returns focus to results. Playback remains accessible across screens and during background scanning. **Help → Keyboard Shortcuts** displays the currently loaded bindings, grouped into the three categories above.

Click the cover to reveal the seek bar, repeat, lyrics, shuffle and album track-list controls. Lyrics stay over the artwork in a translucent, left-aligned panel. Click the cover or lyrics again to dismiss the overlay. The lyrics button can hide just the words while keeping the controls open. Transport and volume remain available at the bottom. Lists and lyrics use Slint's native wheel/trackpad scrolling. Horizontal scrolling changes Home pages; the page-dot area also supports a mouse drag. Drag the bezel to move the window. Resize its edges or use zoom shortcuts; the display keeps its aspect ratio. macOS three-finger dragging is controlled by the system's trackpad settings.

## Lyrics

The player tries these sources in order:

1. A same-name `.lrc` or `.LRC` beside the song.
2. MP3 ID3 SYLT synchronized lyrics with millisecond timestamps.
3. Embedded lyrics tags, including ordinary text and LRC content.

LRC supports repeated timestamps, decimal fractions, positive/negative offsets, UTF-8, UTF-16 and GBK sidecars. The current line follows the audio engine's playback position and scrolls into view. Seeking updates the highlight. Enhanced LRC word tags are reduced to text; highlighting is per line. Songs without lyrics or artwork still play normally and show an appropriate fallback. Unsupported SYLT timestamp units fall back to plain lyrics.

## Configuration

The first launch creates `shortcuts.json` (version 3); `settings.json` is saved when the window changes mode/zoom and when the app closes. On macOS the default directory is `~/Library/Application Support/Nano.Nano-Music-Player`; Windows and Linux use their native application configuration directories. `--config-dir` overrides this.

**Settings → Keyboard Shortcuts** edits bindings in the app. Select an action with J/K, open it with L/Enter, choose **Replace keys** or **Add key**, then press a key combination. **Remove keys** disables that action’s shortcuts, **Default keys** restores its bindings, and **Restore All Defaults** resets the complete keymap. Changes are validated for conflicts, saved atomically, and applied immediately; Escape cancels recording or closes the editor. You can also edit JSON while the app is closed and restart to apply changes. Examples are in [`examples/`](examples/). Shortcut entries contain a semantic action, a key, and exact modifiers. `primary` means Command on macOS and Control elsewhere; `secondary` means Control on macOS and Super elsewhere. Remove a binding to disable it, or add an alias. Duplicate/ambiguous bindings are rejected with a visible message and defaults are used.

Unmodified version 1 or 2 defaults are automatically upgraded to the Yazi-style navigation. Custom version 1 maps are preserved. On Home, H/L move horizontally between icons and pages, J/K move vertically, and Enter opens the selection. In lists, H/L go back/open and J/K move by one row. Arrow keys additionally provide seeking and volume adjustments on Now Playing.

`zoom` ranges from 0.75 to 3.0. A saved `window_width` takes precedence over zoom at startup; `window_height` is used when only height is provided. Remove both dimensions to size the window from `zoom`. Resizing keeps the display proportions and persists the resulting dimensions.

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

Settings opens a working settings screen for shortcuts, display mode and zoom. The remaining nonmusic Home icons currently open placeholders. Radio, clock, fitness, photos, podcasts and video playback are outside this implementation; Playlists currently exposes All Songs. The player layout follows the provided seventh-generation nano photos and the Apollomaniacs music guide. See [`docs/RESOURCES.md`](docs/RESOURCES.md) for sources, asset provenance and licenses.
