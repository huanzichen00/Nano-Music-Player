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
as regular rows and appear only in song lists (including playlists) and artist
album views. Search and the Albums category have no Shuffle row.

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

## Installation state

The optimized candidate was copied into the production bundle, signed ad hoc,
and installed with executable-hash and plist checks plus a recoverable previous
bundle backup. The Mac locked before the final release-candidate GUI smoke check.
The running production process has not been restarted by this pass; the installed
files take effect on the next launch. Final native Music/Albums/Playlists/rating
GUI verification and the restart remain pending until the Mac is unlocked.
The earlier native side-by-side font fixture and all audio/video tests above did
run; this limitation is specific to the final production GUI check.
