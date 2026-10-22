# Nano Music Player

The iOS development version shares the desktop iPod interface and Rust services.
Signed device builds and basic playback are verified; lifecycle acceptance is
tracked in the [UI QA report](docs/IOS_UI_QA.md). See the [development plan](docs/IOS_DEVELOPMENT_PLAN.md),
[build instructions](ios/README.md) and [computer import tool](docs/COMPUTER_TRANSFER.md).
On the desktop, **File → Sync to iPhone…** opens a USB device manager for
previewing and merging music, playlists, lyrics and listening data into the phone.
See [iPhone sync](docs/IPHONE_SYNC.md) for scope and phone-edit preservation rules.

English | [简体中文](README.zh-CN.md)

A Rust/Slint desktop music player inspired by the seventh-generation iPod nano. Device Mode fits the window to a narrow bezel and Home button; Screen Mode shows the display alone. Music playback, library browsing, keyboard navigation and synchronized lyrics work together through one action dispatcher.

See the [0.1.1 integration record](docs/RELEASE_0_1_1.md) for included branches,
merge compatibility changes and verification results.

## Download

Prebuilt packages are available in [GitHub Releases](https://github.com/huanzichen00/Nano-Music-Player/releases/tag/v0.1.1):

| Platform | Package | Installation |
| --- | --- | --- |
| macOS Apple Silicon | [ZIP](https://github.com/huanzichen00/Nano-Music-Player/releases/download/v0.1.1/Nano-Music-Player-0.1.1-macos-arm64.zip) | Extract and move Nano Music Player.app to Applications |
| Windows x86-64 | [ZIP](https://github.com/huanzichen00/Nano-Music-Player/releases/download/v0.1.1/Nano-Music-Player-0.1.1-windows-x86_64.zip) | Extract the complete folder and run `nano-music-player.exe` |
| Linux x86-64 | [tar.gz](https://github.com/huanzichen00/Nano-Music-Player/releases/download/v0.1.1/Nano-Music-Player-0.1.1-linux-x86_64.tar.gz) | Extract and run `./nano-music-player`; built on Ubuntu 22.04 |

The release includes `SHA256SUMS`, installation notes and runtime requirements. macOS releases are ad-hoc signed and not notarized; Windows releases are unsigned. Windows/Linux native builds and download checksums have been verified, while physical audio output, desktop integration and gestures still need manual testing on those platforms.

## Run

Requires Rust 1.92 or newer and a working system audio output.

```sh
cargo run
# Use a separate library and configuration directory:
cargo run -- --config-dir ./local-config
```

On macOS, `scripts/package-macos.sh` builds `target/Nano Music Player.app`. Add `--debug` for a quicker development build. The UI images are embedded in the executable. The release workflow applies an ad-hoc signature; Developer ID signing and notarization are not configured.

Use **File → Import Music Folder** or **Cmd/Ctrl + O** to import folders recursively. Importing reads your music and keeps a metadata cache in SQLite; it does not rewrite song files. Audio remains at its original location; SQLite contains paths, metadata, artwork and lyrics. There is no configured library cap; practical limits are disk space and RAM. MP3, M4A, FLAC and WAV are supported. M4A playback uses Symphonia's AAC/ALAC decoders; AAC M4A is covered by an actual fixture test. Unsupported or damaged files are reported and skipped.

On macOS, File, Playback, View and Help are native menus at the top of the screen. Slint’s Winit/Muda integration handles them; software-rendered test snapshots show fallback menus inside the window.

## Keyboard and mouse

| Category | Shortcut | Behavior |
| --- | --- | --- |
| Playback | Space | Play / pause |
| Playback | Cmd/Ctrl + Right / Left | Next / previous track |
| Playback | Cmd/Ctrl + Up / Down | Volume; holding the keys repeats |
| Playback | M / S | Mute / shuffle; S opens/closes subtitles during video playback |
| Playback | R | Show / hide romaji on Now Playing; cycle repeat elsewhere |
| Playback | N | Now Playing |
| Playback | Y | Toggle lyrics on Now Playing |
| Playback | T | Show / hide translation on Now Playing; generate a missing translation when enabling |
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

On macOS, **Option + R** is a native global shortcut that restores and focuses the running player from other applications. **Option + Shift + R** hides the player while playback continues; Option + R brings it back. These work independently of AeroSpace and the in-app keymap. See [macOS integration](docs/MACOS_INTEGRATION.md#focus-with-option--r).

Added desktop tools—including AI Services, shortcut dialogs, playlist editing, Genius, podcast subscription, subtitles and computer-side iPhone sync—share rounded surfaces and blue actions. See the [desktop UI previews and verification](docs/DESKTOP_UI.md). The classic iPod screens and iPhone interface keep their existing appearance.

Available lyrics appear by default over the artwork in a translucent, left-aligned panel. Click the cover or lyrics to toggle this overlay together with the seek bar, repeat, shuffle and album track-list controls. Artwork fits the available region without cropping, including portrait and landscape images. Transport and volume remain available at the bottom. Lists and lyrics accept mouse-wheel, trackpad and pointer scrolling, with [rubber-band resistance and spring-back](docs/OVERSCROLL.md) at their boundaries. Horizontal scrolling changes Home pages; the page-dot area also supports a mouse drag. Drag the bezel to move the window. Resize its edges or use zoom shortcuts; the display keeps its aspect ratio. Device Mode has a rounded transparent outer silhouette. **Settings → Appearance → Keep on Top** (also in the native View menu) starts enabled and persists your choice. macOS three-finger dragging is controlled by the system's trackpad settings.

## Podcasts (desktop)

Open **Home → Podcasts → Add Subscription…**, paste a show's RSS URL, preview it, then subscribe. Show information, artwork and episodes are fetched automatically; audio is downloaded on demand. Public feeds usually need no account or API key.

Shows support manual refresh and unsubscribe. Tap an episode to play, or its information button for its description and download/played-status actions. The native-style lists use unread blue dots and chronological episode order, automatically continuing to the next unplayed/partly played episode. Playback shares the song screen, with position, repeat, 30-second rewind, speed and episode controls revealed by tapping the cover. Unsubscribing retains downloads and listening history.

Podcasts use a separate desktop database/cache and do not enter the music library or iPhone sync. Search and streaming are deferred. Half/normal/double speed currently changes pitch along with playback rate. See [implementation and verification](docs/PODCAST_IMPLEMENTATION.md).

## Lyrics

In Now Playing, swipe or scroll horizontally to the right across the artwork, lyrics or song details to return to the previous list. The small **Back** control at the top left does the same; H/Escape also works. Playback continues. Vertical movement scrolls lyrics, and dragging the seek/volume sliders keeps their normal behavior. Buttons, icons and sliders use the normal arrow cursor. An optional AeroSpace workspace-follow helper is documented in [macOS integration](docs/MACOS_INTEGRATION.md).

Use **File → Import / Replace Lyrics** or **Settings → Lyrics → Find / Edit Lyrics → choose a song → Lyrics for Track** to associate a `.txt` or `.lrc` file with the selected song (or the playing song outside a song list). Manual imports take precedence and persist their content across restarts and rescans, even if the source lyrics file moves. **Remove Lyrics** suppresses automatic lyrics; **Use Automatic Lyrics** clears that choice and discovers lyrics again from embedded audio tags and nearby lyric files. It does not search online.

Automatic discovery checks a same-name `.lrc` beside the audio first, then `.txt`, then embedded synchronized/plain tags. Extension matching is case-insensitive. Blank sidecars fall back to the next source. TXT remains literal, scrollable text.

LRC supports repeated timestamps, decimal fractions, positive/negative offsets, UTF-8, UTF-16 and GBK sidecars. The current line follows the audio engine's playback position and scrolls into view. Seeking updates the highlight. Enhanced LRC word tags are reduced to text; highlighting is per line. Songs without lyrics or artwork still play normally and show an appropriate fallback. Unsupported SYLT timestamp units fall back to plain lyrics.

**Settings → AI Services** accepts a masked API key and model name (default `deepseek-flash`). You can alternatively set `DEEPSEEK_API_KEY` before launching; this takes precedence and is never copied into the credential file. Opening the dialog leaves the key field empty, and saving an empty field retains the saved key. Credentials are kept separately in `deepseek.json` inside the configuration directory, with owner-only file permissions on macOS/Linux. This is a local plaintext file, and both it and temporary credential files are excluded from Git.

Open **Lyrics for Track → Generate Romaji** for a song with Japanese lyrics. This explicit action sends its title, artist and existing Japanese lyric lines to the [DeepSeek API](https://api-docs.deepseek.com/api/create-chat-completion/); DeepSeek account usage applies. Generation runs in the background and adds smaller romaji beneath the original lines without changing lyric text or LRC timestamps. Repeated lines share one reading, and non-Japanese lines stay unchanged. Romaji persists in SQLite and works offline after generation. Editing the source text or timestamps hides outdated annotations until you regenerate.

Plain **R** shows/hides romaji only in the song interface (including volume selection); **Japanese Readings** in Settings → Lyrics persists this preference. Existing custom R bindings take precedence. **Cancel Romaji Generation** discards pending results and stops further batches; an HTTP request already in flight can finish within its 120-second timeout. **Remove Romaji** deletes the annotation while keeping original lyrics. Incomplete or invalid responses never replace a previous annotation. Kanji readings can be ambiguous, so generated readings should be checked. Original lyrics and timestamps remain unchanged.

Open **Music → Genius** or the Genius control on Now Playing to create a playlist based on the selected song. DeepSeek chooses from a frozen snapshot of available local song metadata. The preview retains recommendation order; **Refresh**, **Play** and **Save** use that order and the existing playlist storage. It is a local-library recommendation feature; see [`docs/DEEPSEEK_GENIUS_TRANSLATION_PLAN.md`](docs/DEEPSEEK_GENIUS_TRANSLATION_PLAN.md) for its validation and cancellation rules.

Open **Settings → Lyrics** on the iPod screen. **Translate To** selects English / 简体中文 / 日本語; **Translate Current Song → Translate Lyrics** translates the playing song. The native Playback menu has the same commands. The target language persists independently of the interface language. Original lyrics, translation and optional romaji stay paired by original line and timestamp. A valid cached translation opens offline; **Regenerate Translation** requests a new result. **Show Translation** and plain **T** toggle the same persisted display setting. Turning T on also generates a missing translation, first finding source lyrics online if necessary; Settings' Show Translation only changes the display. Original lyrics stay visible; when On, translation appears underneath each original line and romaji is hidden. When Off, your existing **R**/romaji preference returns. A translation that finishes later cannot override T-off. **Cancel Translation** and **Remove Translation** manage pending requests and the cache.

When the song has no lyrics, **Translate Lyrics** first searches [LRCLIB](https://lrclib.net/docs) for a matching recording, using DeepSeek discovery as an optional fallback before translating. A plain-text result also allows DeepSeek to look for an existing timed source, checking the recording's studio/live version, album and duration; it never invents timestamps. Finding lyrics needs no DeepSeek key. Matching handles typography, featured credits and album release labels while preserving recording versions and duration checks; conflicting results continue through exact recording lookup and optional discovery before reporting that lyrics could not be verified. Requests are spaced, provider cooldowns are respected, and repeated misses are cached briefly. **Settings → Lyrics → Find / Edit Lyrics** lets you choose any song without playing it, then find lyrics from its dedicated page. **Find Lyrics Online** performs just that lookup and keeps its status visible. Existing synchronized lyrics and manual imports are retained; **F** can retry downloaded plain text and upgrade it when a matching timed source exists. Downloaded lyrics preserve real LRC timestamps, or stay untimed when only plain text is available. Manual imports/removals and changed songs take precedence over pending results. Downloads use the existing local lyrics association and keep the source URL, so they work after restart. Nothing modifies audio files. See [lookup and translation verification](docs/TRANSLATION_SETTINGS.md).

Network requests occur only after an explicit action. Lookup sends song metadata to LRCLIB and, only if needed and configured, DeepSeek; translation sends source lyric text to DeepSeek. DeepSeek account usage applies. Cancellation prevents further stages and result application; an HTTP call already in flight remains bounded by its timeout.

## Playlists

Open **Music → Playlists → Add Playlist** or **File → New Playlist…**. The Add Playlist entry participates in J/K navigation above All Songs; press Enter to open it. Optionally enter a name, press Enter to leave the name field, and select songs with checkboxes or **J/K + Space**. Review is available as soon as you select a song; a blank name receives a unique New Playlist N name. Choose Review, arrange the order with Move Up/Move Down or Remove, then Create. Saved playlists appear in Music → Playlists and survive restarts; their playback queue follows your chosen order. Cancel leaves the library and playback unchanged. Saving happens in the background; a failed save retains the draft for retry.

Names must be nonempty, single-line and at most 100 characters; duplicate names are rejected regardless of case. Missing paths remain stored and songs absent from the current library are skipped. Saved playlists support editing songs and their order; deleting an entire saved playlist and iTunes synchronization are not included yet.

## Song ratings

On Now Playing, tap the artwork to reveal controls, then tap the album track-list button. The five bullets above the album songs become stars as you tap or drag to set a rating from zero to five, following the iPod nano guide (page 23). Drag to the left of the first bullet, or press **0**, to clear it; **1–5** sets stars while this strip is visible. Custom shortcuts keep precedence, and text entry retains normal behavior.

The small title above the stars identifies the song being rated. It stays pinned if playback moves to the next song; reopening the track list targets the newly playing song. Ratings save locally in SQLite, survive restarts and metadata rescans, and follow confirmed file rename events. Saving runs in the background, and queued changes finish on close. Failed saves restore the last confirmed rating and show a notice. Audio tags remain unchanged. iTunes synchronization is outside this desktop player's scope.

## Playback, monitoring and smart playlists

**Settings → Playback → Gapless Playback** queues the next eligible song on the same audio sink. Repeat, shuffle, seeking and queue changes invalidate unstarted successors; encoder delay/padding is trimmed where the decoder format provides it. This preserves continuous albums without adding crossfade.

**Settings → Playback** places **Volume Leveling**, **Volume Boost** and **Prevent Distortion** together. Volume Leveling uses ReplayGain tags, with explicit Off, By Song and By Album choices; Volume Boost selects a preamp from −12 to +12 dB. Album mode falls back to track tags when album gain is absent; untagged songs keep their original level. Changes persist and apply to subsequent track loads, including the next gapless successor. User volume remains independent.

On macOS, the player publishes metadata, duration, playback position and optional artwork to Now Playing and accepts play/pause, next/previous and seek commands. Stopping playback or handing audio to video clears music metadata. Physical media-key and Control Center behavior still needs manual hardware verification.

The desktop library uses cached startup followed by a finite background refresh of imported folders. Use **File → Refresh Library** or **Settings → Refresh Library** to discover new files, update metadata and remove confirmed missing entries. Unavailable volumes and incomplete scans preserve cached songs; valid active playback and stable queue slots are retained. Persistent filesystem monitoring has been removed. Imported roots survive restarts and older settings migrate automatically. See [library refresh behavior](docs/LIBRARY_REFRESH.md).

Desktop **Home → second page → Statistics** shows totals, artist/album/song rankings, recent plays and a 30-day daily list. All Time uses cumulative counts; 7/30-day views use retained history and can be incomplete because only the latest 1,000 plays are kept. Queries run on the existing background worker. See [desktop listening statistics](docs/DESKTOP_LISTENING_STATISTICS.md) for range, timezone and album-identity details.

**Music → Playlists** includes Recently Added, Most Played, Recently Played and Never Played. These computed views use available library paths and leave user playlists and Genius intact. Statistics persist in SQLite, with the latest 1,000 qualified plays in listening history. A session counts once after `max(3 seconds, min(30 seconds, half the duration))` of observed listening; unknown durations require 30 seconds. Tracks shorter than three seconds count only on natural completion after observing at least half their duration. Pauses, stalled positions and seeks add no listening time; restarting begins a new session.

## Configuration

Desktop Settings groups Appearance, Playback, Lyrics, Language, Keyboard Shortcuts, AI Services and a direct **Translate Current Song** entry on one page. The translation entry also remains under **Settings → Lyrics**. Each preference has a short explanation; switches show their state, and selection pages mark the saved value. Appearance includes explicit Window Style and Window Size choices. Lyrics keeps display preferences separate from commands for the current song.

The first launch creates `shortcuts.json` (version 6); `settings.json` saves display preferences and interface language. **Settings → Language** selects English (default), 简体中文 or 日本語. **Settings → Appearance → Font** selects Classic, Maple Mono or JetBrains Mono, with bundled licensed fonts and a persisted preference; see [font notes](docs/FONTS.md). Music metadata and lyrics keep their original text. On macOS the default directory is `~/Library/Application Support/Nano.Nano-Music-Player`; Windows and Linux use their native application configuration directories. `--config-dir` overrides this.

Startup shows cached songs and saved manual lyrics before refreshing source files in the background. A slow or blocked file open cannot hide the existing library. Background updates preserve newer imports, lyric edits, and the playing queue.

Videos supports embedded subtitles and matching `.ass`/`.ssa` sidecars. **File → Subtitles…** selects Automatic, Off or a track, imports/replaces a subtitle, or removes its association. Choices persist per video; external files may live in a separate folder. ASS styling and timing are rendered by libmpv/libass. See [subtitle support and verification](docs/SUBTITLES_IMPLEMENTATION_PLAN.md).

Automatic library monitoring is suspended in the playback recovery build, including when older settings enabled it. Manual import and startup source refresh remain available; stored roots and lyrics are retained. This avoids the reproduced macOS file-descriptor exhaustion. See [playback recovery evidence](docs/PLAYBACK_RECOVERY.md).

**Settings → Keyboard Shortcuts** edits bindings in the app. Select an action with J/K, open it with L/Enter, choose **Replace keys** or **Add key**, then press a key combination. **Remove keys** disables that action’s shortcuts, **Default keys** restores its bindings, and **Restore All Defaults** resets the complete keymap. Changes are validated for conflicts, saved atomically, and applied immediately; Escape cancels recording or closes the editor. You can also edit JSON while the app is closed and restart to apply changes. Examples are in [`examples/`](examples/). Shortcut entries contain a semantic action, a key, and exact modifiers. `primary` means Command on macOS and Control elsewhere; `secondary` means Control on macOS and Super elsewhere. Remove a binding to disable it, or add an alias. Duplicate/ambiguous bindings are rejected with a visible message and defaults are used.

Untouched version 1–3 defaults upgrade to the current navigation and F-prefix shortcut. Customized maps remain unchanged; Jump to letter can be assigned in Settings. On the desktop Now Playing screen, plain **F** finds missing lyrics online and shows persistent search status. It uses the same lookup as **Find Lyrics Online**, preserves existing synchronized or manually imported lyrics, and does not request translation. Downloaded plain text can be upgraded to a matching timed source. In lists, **F** keeps its letter-jump prefix. Custom F assignments take priority; **Find Lyrics Online** can also be assigned its own shortcut. On Home, H/L move horizontally between icons and pages, J/K move vertically, and Enter opens the selection. In lists, H/L go back/open and J/K move by one row. Arrow keys additionally provide seeking and volume adjustments on Now Playing.

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

Settings includes customizable shortcuts, display mode, zoom, per-track lyrics and language. The remaining nonmusic Home icons currently open placeholders. Radio, clock, fitness and photos remain outside this implementation; Playlists includes All Songs and saved user-created lists. Videos imports and plays local files with aspect-preserving frames, transport/seek/volume controls and whole-device landscape rotation. Its list loads cached frame thumbnails for visible rows using FFmpeg in the background; missing/corrupt thumbnails retain a video placeholder. Playback requires a compatible libmpv runtime; import requires ffprobe. See [`docs/VIDEO_IMPLEMENTATION.md`](docs/VIDEO_IMPLEMENTATION.md) for installation, verified formats and platform limits. The player layout follows the provided seventh-generation nano photos and the Apollomaniacs music guide. See [`docs/RESOURCES.md`](docs/RESOURCES.md) for sources, asset provenance and licenses.

Artists, Albums and global Songs sort alphabetically using case-insensitive keys with deterministic tie-breakers. Individual albums retain track-number order. Album and artist rows show representative cover thumbnails.

See [`docs/MACOS_INTEGRATION.md`](docs/MACOS_INTEGRATION.md) for the stable bundle ID and targeted AeroSpace floating rule. The app leaves your window-manager configuration untouched. [`docs/FUTURE_MEDIA.md`](docs/FUTURE_MEDIA.md) records the original video and podcast architecture study; desktop video and podcast playback are now implemented. Implementation phases and verified checks are tracked in [`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md).

Native macOS Apple Silicon, Windows x86-64 and Linux x86-64 packages are built by GitHub Actions. macOS has additional local runtime verification; Windows/Linux physical audio output, desktop integration and UI behavior remain subject to manual testing.

Saved playlists can be edited from their **Edit** toolbar or **File → Edit Playlist…**. **Add** opens the name/song picker; the red minus then **Delete** removes a row from the draft. Drag the right-hand three-line grip (with edge scrolling), or use **Move Up/Down**, to change order; **Done** saves the existing playlist, and **Cancel** discards changes. Edit, Shuffle and songs share J/K selection; Enter/L opens the selected control. Missing songs remain visible and stored until explicitly removed. Home preserves the draft for reopening through New Playlist. Editing leaves the currently playing queue intact.

**Shuffle** at the top of Songs, album collections and individual album/playlist/genre lists starts a random run scoped to that list. Search shuffles only its results. The entry participates in J/K and Enter/L; F plus a letter skips the control and jumps to metadata. Turning Shuffle off restores the list’s canonical order.

Rightward pointer swipes and horizontal trackpad scrolling return to the parent from library lists. Vertical gestures retain scrolling; small/diagonal motions and text/modal/transition input are guarded.

Home pages use persistent icon trees with a 180 ms ease-out slide for horizontal trackpad input and H/L page changes. Continued momentum at a page boundary does not reset selection or submit more navigation. Turning **Settings → Appearance → Page Animation** off draws the destination immediately; pointer swipes over icons navigate without launching them.
