# Smooth Slides implementation plan

## Scope and design decisions

Add fluid directional transitions between app screens and make Home-page swipes follow their direction. Forward navigation enters from the right and moves the current screen left; Back enters from the left and moves the current screen right. Preserve the existing keyboard, pointer, and horizontal-wheel meanings. Keep vertical library and lyric scrolling and seek/volume dragging independent from horizontal navigation.

Transitions use Slint's UI animation mechanism and logical device coordinates. The display's scaled frame is the clipping boundary, so zoom and Device/Screen Mode geometry do not alter travel distance. Keep the current and destination views alive only for one transition, without image snapshots. A new navigation request retargets the active transition from its current offsets. Turning the setting off commits the destination immediately and clears animation state.

Settings default to enabled when the preference is absent, persist with the existing settings format, and expose translated title/help in English, Simplified Chinese, and Japanese. Reduced-motion support must be evaluated against an actual platform signal available to this app; do not infer or claim OS integration without a supported API.

## Phases

### 1. Isolated motion prototype

Build a standalone Slint prototype showing both directions, an interruptible progress target, clipping, and logical-size travel. Keep the prototype out of the production build until shared-file ownership is released.

Acceptance: motion is directionally correct, bounded to the viewport, can be retargeted during motion, and has no frame capture or timer-driven Rust work.

### 2. Gesture and navigation audit

Map screen-changing actions, Home page swipes, Now Playing's rightward Back swipe, horizontal-wheel accumulation, and shortcut dispatch. Define gesture arbitration so text editing, vertical scroll, sliders, and modal overlays retain priority where appropriate. Inspect the user manual and existing reference photos as design evidence.

Acceptance: a documented event matrix retains h/l, j/k, arrows, pointer clicks, horizontal wheel, lyric/library scroll, and seek/volume drag behavior without duplicate or accidental navigation.

Event matrix for integration:

| Input | Existing meaning | Transition / arbitration rule |
| --- | --- | --- |
| h / Left / Back control | Back | Navigate one level; slide right |
| l / Enter / pointer activation | Open selected item | Navigate forward; slide left |
| j / k / Up / Down | Move selection or scroll | No screen transition |
| Horizontal wheel | Existing horizontal navigation; Now Playing accumulates rightward deltas for Back | Preserve thresholds and timeout; a fired action is delivered once |
| Now Playing rightward swipe | Back | Require horizontal dominance and minimum travel; keep seek/volume regions outside the recognizer |
| Home horizontal swipe | Change Home page | Move the page in the swipe direction |
| Vertical library or lyric drag | Scroll | Never trigger Back or Home page change |
| Seek / volume drag | Adjust playback | Never trigger navigation |
| Text input focused | Edit text | Do not capture h/l/j/k or arrow editing keys as navigation |
| Modal/editor open | Modal-specific behavior | Do not start or retarget underlying screen transitions |

Navigation state policy: each accepted request has a monotonically increasing transition generation. A subsequent request first resolves the in-flight destination as the source of truth, invalidates the old completion callback, then starts the next transition. Completion only commits when its generation is current. Disabling motion increments the generation and commits immediately. Keep view state in the screen model across the short visual overlap; the transition host owns no media or playback state.

### 3. Settings and transition integration

After shared ownership is released, integrate the preference, translations, and transition host. Capture source/destination screen identity and direction once per navigation request; preserve screen selection, scroll offsets, lyrics/highlighting, playback position, and media ownership. Handle rapid sequences, Back during motion, Home, and video orientation changes without stale content.

Acceptance: setting persistence and safe migration pass; off means immediate navigation and cancels current motion. Animation uses the shared logical coordinate scale in both display modes and all zoom levels.

### 4. Verification and delivery

Run formatting, build, tests, strict lint, rendered UI checks, and actual pointer/keyboard behavior checks after integration. Inspect visual output in both display modes, at multiple zooms, and around video portrait/landscape changes. Update this plan with measured evidence and limitations.

Acceptance: no regression in library entry latency, no accumulated frame snapshots, no duplicate action on interruption, and no changed playback/lyrics state. Reduced-motion limitations are recorded accurately.

## Current status

- Phase 1: isolated Slint prototype compiles with 1.18.1 and its software-renderer runtime harness passes forward/back direction, rapid request, immediate disable, and post-disable stale-animation checks. Inspected a captured mid-forward frame: the outgoing screen is left of the entering screen and viewport clipping is correct. Runtime evidence is under `tests/isolated-motion`; those files are not part of the app build.
- The supplied manual PDF has malformed cross-reference entries but remains readable through Poppler recovery. Its interaction diagrams inform screen and gesture interpretation; the app's existing behavior remains the source for shortcut compatibility.
- Reduced-motion behavior is not yet verified against a platform API.

## Review notes for the in-progress Genius/translation integration

- `800adc5` adds a real Genius preview modal and translation target menu actions. Keep these modal views above the screen-transition host; opening/closing them should not be interpreted as app-screen navigation.
- Now Playing now lays out each lyric row from the preferred heights of the original, translated, and romaji text. A transition must preserve `lyric-scroll-y` and let those live measured rows render; do not flatten them into a snapshot.
- The preview consumes live seed, song, busy, and stage properties. It must not be recreated or have its stage reset when an unrelated screen transition is interrupted.
- The isolated prototype parses and compiles with Slint 1.18.1. Its runtime harness verifies directional offsets, rapid retarget requests, disabling motion during flight, and no later movement from the canceled transition. A software-rendered mid-flight image was visually inspected. Touch-driven swipes and native backend timing remain unverified.
