# Rubber-band overscroll

Lists, lyrics, Genius results, playlist candidates/drafts, shortcuts and subtitle
choices support elastic travel at both ends. Home supports it before the first
page and after the last page. Content follows input while the gesture is held;
there is no wrap-around navigation. Empty and shorter-than-viewport lists return
to the origin.

## Architecture

`ui/overscroll.slint` contains two reusable Slint components:

- `ElasticAxis` owns one visual position, a velocity estimate, and bounded scroll
  coordinates. Drag distance outside the bounds is compressed with
  `0.55*d / (1 + 0.55*abs(d)/L)`, where `L` is 32% of the viewport extent. The
  inverse mapping preserves resistance across successive events and reversals.
  A held drag has no return timer. Release starts decaying pointer inertia or an
  implicit critically damped spring (22/s), preserving velocity on boundary
  entry. The spring settles exactly, without repeated oscillation.
- `ElasticFlickable` retains Slint's `Flickable` viewport, content dimensions,
  clipping and programmatic scroll coordinates. A single `SwipeGestureHandler`
  arbitrates vertical dragging and optional rightward Back. Delayed taps still
  reach the existing row controls. A small translated child container draws the
  elastic offset; callers only store the bounded `content-y`.

The pinned Slint 1.18.1 implementation clamps native pointer/fling movement before
exposing it. Supplying gesture deltas and pointer inertia is necessary to render
elastic travel. Slint remains responsible for gesture recognition, rendering,
clipping, timers and programmatic animations; no dependency patch, bitmap
snapshot, Rust animation worker, media state, or per-event collection is added.
Nested horizontal/vertical swipe recognizers block live pointer feedback in
this architecture. The live-input regression requires a single recognizer on
each scrolling viewport.

Wheel callbacks expose deltas but no gesture phase or momentum flag. Each delta
is applied directly, including OS-supplied momentum; a 90ms inactivity deadline
starts spring-back. No synthetic wheel fling is added. This phase approximation
needs physical trackpad evaluation. It intentionally changes the old fixed
180ms per-wheel interpolation to direct following. Lyric programmatic following
keeps its 180ms easing and is interruptible from the currently rendered position.

The spring timer runs at 16ms only during motion and uses elapsed animation time,
capped at 32ms per integration step to keep delayed frames stable. No timer runs
for a held stationary drag or an idle viewport. Resizing/content changes clamp
the canonical position and cancel obsolete motion. Disabling a viewport, opening
a modal, or changing navigation generation cancels pending gestures and rebound.
Generation changes also handle same-presentation navigation with Smooth Slides
off. Slider gestures and playlist reorder grips stay outside scroll recognition.

Home translates both page containers by a separate elastic offset. Existing
page-slide coordinates, navigation thresholds and page indices retain their
meaning. Boundary wheel momentum never resubmits a page request. Elastic feedback
works with Smooth Slides on or off; that setting continues to control screen and
page transitions. An inward reversal that changes pages transfers the rendered
elastic offset into the slide coordinates before clearing it. Travel and
thresholds use the display's logical scale.

## Automated verification

Run from the feature worktree:

```sh
cargo fmt --all -- --check
cargo check
cargo test --test overscroll --test overscroll_ui
cargo test --no-fail-fast
cargo clippy --all-targets -- -D warnings
```

`tests/overscroll.rs` exercises the production Slint components, rather than a
separate physics implementation: both boundaries, diminishing displacement,
held pauses, release, inertial boundary entry, repeated wheel input, rebound
interruption/reversal, invalidation, resizing, and interrupting lyric following.

`tests/overscroll_ui.rs` dispatches actual synthetic Slint pointer/wheel events
through `AppWindow`. It checks visible movement before release, no accidental row
activation, bounded persisted positions, short/empty list dragging, both Home
edges, rapid wheel bursts, reversals, same-presentation navigation, modal guards,
Home/list resizing, and both Smooth Slides settings. Device/Screen Mode at two
sizes checks displacement normalized to display scale. Software-rendered list
and Home frames are written to the system temporary `nano-qa` directory.

Existing UI suites cover keyboard/search focus, synchronized lyrics and wrapped
translations/romaji, seek/volume dragging, ratings, playlist reorder/autoscroll,
Genius, repeated screen transitions, display modes, and video orientation.

## Verification record — 2026-10-15

- Starting commit: `129f83698357db9ad4d56033d3e7b4c860217541`.
- Isolated branch: `feat/rubber-band-overscroll`; the original checkout and its
  uncommitted/untracked user work were preserved. No main-branch update or merge.
- Formatting and `cargo check`: passed.
- Focused motion, live-input, Home and screen-motion tests: 5 passed.
- Full `cargo test --no-fail-fast`: 271 passed, 0 failed, 13 ignored. Ignored
  cases require native tools/audio or opt-in profiling.
- Both opt-in `playback_runtime` tests passed separately with native macOS audio
  output: successive playback/statistics/artwork and rapid seeking/skipping,
  pause/resume/stop. This does not certify audible gaplessness or sound quality.
- Strict all-target Clippy is blocked by pre-existing lints in
  `src/app/controller.rs:2087` (`collapsible_if`), `src/library/scanner.rs:45`
  (`needless_borrow`), and, after allowing those, `tests/feature_library.rs:51`
  (`field_reassign_with_default`). These files match the starting commit.
- Scoped Clippy for the library, binary and both new tests passes with only the
  two existing source lint categories allowed:
  `cargo clippy --lib --bin nano-music-player --test overscroll --test overscroll_ui -- -D warnings -A clippy::collapsible_if -A clippy::needless_borrow`.
- Visually inspected software-rendered held list overscroll and both Home page
  boundaries. No native trackpad or native renderer interaction was performed.

Changed files: `ui/overscroll.slint`, `ui/app.slint`, `ui/home.slint`,
`ui/library.slint`, `ui/now-playing.slint`, `ui/genius-preview.slint`,
`ui/playlist-editor.slint`, `tests/overscroll.rs`, `tests/overscroll_ui.rs`,
`README.md`, `docs/OVERSCROLL.md`, and `docs/SMOOTH_SLIDES_PLAN.md`.

## Manual verification still required

- Physical macOS trackpad: slow edge pulls, fast momentum into either edge,
  delayed OS momentum tails, wheel/drag interruption and rapid reversals.
- Native renderer frame pacing and perceived spring strength at 0.75x–3x zoom,
  high-refresh displays, and during simultaneous audio playback.
- Touchscreen input and native Windows/Linux backends.
- Landscape video interaction with the native video runtime. Automated video UI
  tests check its existing geometry, while overscroll stays in the list overlays.

Synthetic macOS-hosted events and software frames are not evidence of native
trackpad behavior. No installed app bundle is replaced by this change.
