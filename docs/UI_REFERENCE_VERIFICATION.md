# Library and settings reference pass

The user's three iPod nano photos are the layout reference. The Music menu uses
its compact header and original icon identities, and shows the current title
under Now Playing with a trailing speaker. Albums starts with All Songs and its
available-track count. Playlists has Add/Edit buttons, Genius, a real My Top
Rated view (four- and five-star songs), Recently Played and saved playlists.
The older statistics views remain below those rows. Edit mode selects a saved
playlist for the existing editor; Back leaves edit mode.

The eight transparent icon recreations, built-in imagegen prompts and online
reference provenance are in `assets/icons/nano/`. They are generated recreations,
not official extracted assets. Keyboard selection retains its highlight;
pointer navigation follows the unselected white rows in the photos.

Settings now opens five groups: Display, Playback, Lyrics, Language and Keyboard
Shortcuts. Display holds window/font options. Playback holds Gapless and a
ReplayGain level with the existing gain controls. The rating album track list
uses black throughout its LCD header and body. Shuffle rows use the same height
as regular rows. Album song lists, including the black rating view, now omit the
Shuffle row too. Other song lists (including playlists) and artist views retain
it. Search and the Albums category have no Shuffle row.

Shift+S starts all available songs in shuffled order, independently of the
current view. Plain S retains its existing toggle. The keymap migration adds the
new binding only if its chord is free, and preserves custom bindings and an
explicitly unassigned current-format map.

The Back gesture quiet interval is 80 ms, down from 300 ms, and library slide
animation is 140 ms with 150 ms completion, down from 220/230 ms. A native Winit
wheel observer keeps burst ownership alive while outgoing/incoming views are
disabled. A regression sends continuous 20 ms wheel activity across transitions
and checks one Back, then checks that a new gesture is accepted after 100 ms of
silence. This is simulated wheel coverage, not a physical trackpad measurement.

macOS defaults to Skia after native comparisons reproduced broken small-text
strokes with OpenGL FemtoVG at the installed scale. Classic remains Helvetica
Neue; no font substitution is made. See `docs/FONTS.md` and the standalone native
fixture `examples/font_render_audit.rs`. Explicit SLINT_BACKEND overrides work.
Other platforms retain their prior renderer.

## Verification, 2026-10-14

- `cargo fmt --check`, `cargo check`, and release build pass.
- Final `cargo test --all-targets`: 275 passed, 22 intentionally ignored.
- Clippy runs with warnings denied and the same three pre-existing lint classes
  allowed: collapsible_if, needless_borrow and field_reassign_with_default.
- `tests/playback_runtime.rs --ignored --test-threads=1`: all three native CoreAudio
  tests pass, including the new Shift+S entry from an empty search and subsequent
  skip, plus existing playback/statistics/artwork and transport checks.
- The ignored native video import/frame/rotation/music-handoff test passes.
- Rendered fixtures check Music/Albums/Playlists, the Settings levels, black LCD
  pixels throughout the rating screen, rating persistence, row geometry and
  repeated/retargeted slide transitions. Their startup notice is deliberately
  frozen so wall-clock expiry does not alter an unrelated frame comparison.
- Audio-core source is unchanged from the already verified recovery branch.
  The original checkout's uncommitted files remain preserved.

No new live translation API call was made. Translation already exists through
DeepSeek, with English, Chinese and Japanese targets, cache and lyric overlays.
Audible gaplessness, clipping and physical trackpad momentum are not newly
verified by this pass. Native application replacement/GUI smoke results are
recorded below after the final check.

## Installed release verification, 2026-10-14

The optimized candidate was copied into the production bundle, signed ad hoc,
and installed with executable-hash and plist checks plus a recoverable previous
bundle backup. After the Mac was unlocked, the previous process was quit normally
and the installed app restarted. Its executable SHA-256 is
`1dae10be31634d90ec0b3e0f6dd169c01a060da2196690745362fe851298c1e4`.

The following were checked in the running production app, using its current
101-song library:

- Music has the reference icons, compact header and current song subtitle.
- Albums starts with All Songs, its 101-song count and generic music artwork.
- Playlists has the Add/Edit toolbar and Genius, rating and playlist icons.
- The rating album track list is black throughout the LCD header and body.
- Settings opens the five groups above; Display shows Classic and the existing
  display controls. Small gray subtitles render without the broken strokes seen
  in the earlier FemtoVG comparison at this scale.
- The native Playback menu's Shuffle All Songs action starts a shuffled library
  track. Ordinary playback was also started in the installed app and its elapsed
  position advanced from 0:00 to 0:17, with the pause control displayed.
- No repeated resizing was observed during these checks. The production bundle
  continues to use the existing AeroSpace floating rule.

The saved version-5 keymap contains Shift+S for Shuffle All and plain S for the
existing toggle. The native-output regression passes when it explicitly delivers
Shift key-down and S. Computer-use chord synthesis resolved to plain S in this
app, so a physical Shift+S press on this Mac remains unverified. This does not
affect the verified native menu command.

These GUI checks do not verify audible Gapless continuity, clipping, physical
trackpad momentum or a live DeepSeek request. The separate Apple Recent Items
system-menu crash described in `docs/PLAYBACK_RECOVERY.md` is still not proven
fixed. The original checkout's three uncommitted files remain preserved.

## Translation and thumbnail follow-up, 2026-10-14

Separate tasks implemented the original/translation layout and random video
previews in the recovery worktree. Integrated code commits are `e0681f0` and
`b8be1e6`, pushed to both `main` and `restore/reliable-playback`. The installed
bundle executable SHA-256 is
`94a077258f3409e501be47178863c4de24a3b005ba02eb330299d79b963ca3de`.

The running installed app showed all three real video previews with the compact
Videos header, 58-logical-pixel rows and an 80×45 thumbnail area. The 4:3 clip
retained its full aspect inside that area. Preview selection uses a random
per-session seed to sample 15–85% of duration and remains stable across renders
and image eviction. A silent native FFmpeg fixture verifies its selected frame
comes from the middle section and preserves rotation/SAR.

The first installed launch retained placeholders. Independent extraction of all
three files succeeded. macOS recorded a Downloads-folder permission change for
the app after that launch; a fresh process then loaded all three previews. This
supports an access-related failure, rather than a proven UI publication defect.
Failed previews are cached until library invalidation or restart. No macOS
privacy database or permissions were changed by this work.

The installed app played the real cached-translation song Deathwish: elapsed time
advanced from 0:15 to 0:23 before the user paused it. Its lyric panel was disabled,
and the user resumed control before its native visual comparison completed.
Original-plus-translation separation, wrapping, empty/missing entries, T/romaji
priority and active-line scrolling pass the software-rendered regression instead.
No new paid translation request was needed for this follow-up.

Formatting, all-target check and all-target tests pass: 296 tests passed, 24 opt-in
tests ignored. Clippy passes with the same three pre-existing lint allowances.
The adjusted video tests pass again after their two new lint warnings were fixed.
Three ignored CoreAudio playback/transport/statistics tests and the ignored native
video playback/rotation/music-handoff test were explicitly run and pass. The
optimized release build passes. Audible Gapless continuity, clipping and physical
trackpad/Shift+S behavior remain unverified; audio-core source was not changed.

## Installed T shortcut update, 2026-10-15

Installed code commit `fc935e9` after the Mac was unlocked. The replaced bundle
was retained as a backup, and the installed executable matches the verified
release SHA-256:
`290f844497666cb62d121076a0c79562b06158690ea98c2edb046632213b23c4`.
The bundle signature passed strict verification and the app relaunched with its
existing library and preferences.

The installed native interface showed Japanese originals with Chinese translations
on separate lines underneath. Pressing plain T hid translation and restored the
saved romaji annotations; a second T immediately restored cached translation and
reported "Translation ready". No new live translation request was made. Missing
translation generation, inherited-enabled preference handling, failed requests,
T-off and continuing real audio are covered by the targeted regression/native
tests. The final all-target suite passes 298 tests with 25 opt-in tests ignored;
four native audio tests, checks, Clippy and the release build passed before this
installation.

The post-install real-song check remained at 0:00. A process sample found the
audio worker waiting in `File::open`; a separate command-line read of that same
regular audio file succeeded in 20 ms. macOS TCC logs explicitly reported a
failed match against this app's previous Downloads-folder code requirement and
an outstanding Downloads permission prompt after the rebuilt app launched.
The Mac locked again before that prompt could be inspected. Renewed permission
and subsequent installed-app playback remain pending at this snapshot. No audio
engine change, privacy database edit or permission bypass was attempted.

Separately, two fresh generated-fixture transport tests timed out at their
existing three-second initial-load limit while the sampled worker was inside
CoreAudio default-device initialization. System audio enumeration reported the
built-in speakers as the default 48 kHz output; a later identical test passed
without a code or timeout change. This does not establish the cause of those
initialization delays or verify the still-blocked installed song playback.

## Post-authorization playback investigation, 2026-10-15

Classification: required macOS folder authorization, not an application bug.
The user subsequently unlocked the Mac and explicitly allowed Downloads access.
The same stalled installed process resumed real-song playback, and the user
confirmed hearing its music. Original-file transport, consecutive tracks, normal
shutdown/relaunch, and temporary fixture/real-file playback in the exact same
installed bundle then passed. No software fix was required; production code,
bundle identity and signature were unchanged. See
[NATIVE_PLAYBACK_P0.md](NATIVE_PLAYBACK_P0.md) for the TCC grant results, test
matrix, Git isolation and possible reauthorization after future ad-hoc rebuilds.
The separate intermittent CoreAudio startup delay remains unexplained.
