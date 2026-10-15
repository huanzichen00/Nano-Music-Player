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

Implemented in the production UI. The controller compares semantic `Screen` identity, so navigation between views that share the same Slint presentation code (for example Music and Albums) still starts a transition. For each request it latches the outgoing screen title, rows, selection, Home page, list scroll, and lyric scroll. The source and destination remain live Slint views during the overlap; no bitmap snapshots are retained. A new request captures the latest destination as its source and restarts the motion. Turning the setting off calls the public cancellation function and commits immediately.

The host animates both live views with Slint position animations inside the scaled display clip. Home uses two live icon pages and animates both incoming and outgoing pages; direction is derived from the old and new Home page so keyboard navigation matches swipes. Video playback entry, exit, and orientation handoffs bypass the slide host because its display has a separate rotated view. Modal overlays remain above it. Previous library rows are released when a transition completes or is canceled.

Acceptance: settings persistence and the missing-key default are covered by core tests; UI regression coverage checks forward, Back, same-presentation-class library navigation, an interrupted library transition, Home in both directions, and disabling motion mid-flight. Logical-coordinate scaling is implemented through the display's dimensions and clip. Native backend timing, multiple zoom levels, and every video orientation change have not been visually verified.

### 4. Verification and delivery

Production integration is present. Run formatting, focused tests, strict lint, and rendered UI checks before delivery. The UI test checks transition offsets for Home, Settings, and library navigation. The software-renderer snapshot helper did not capture the in-flight composition even when those offsets were nonzero, so it is not evidence of the production animation frame. The isolated prototype's separate software-rendered mid-forward image was inspected. Device/Screen mode and video orientation checks remain limited to the existing broader UI regression; native backend animation timing has not been measured.

Acceptance: no bitmap snapshots, no duplicate navigation observed during the tested interruption, and Smooth Slides off cancels active motion. No album-list entry latency benchmark has been run, so the plan makes no measured latency claim. Reduced-motion integration is not provided because no supported platform signal has been verified.

## Current status

- Phase 1: isolated Slint prototype compiles with 1.18.1 and its software-renderer runtime harness passes forward/back direction, rapid request, immediate disable, and post-disable stale-animation checks. Inspected a captured mid-forward frame: the outgoing screen is left of the entering screen and viewport clipping is correct. Runtime evidence is under `tests/isolated-motion`; those files are not part of the app build.
- Phases 2–3: production settings, translations, live view transition host, semantic navigation generation, and symmetric Home-page motion are implemented. UI regression coverage includes same-class library navigation, Back during motion, and cancellation when disabling the setting. UI regression snapshots are generated under the system temporary `nano-qa` directory, but the snapshot helper currently does not capture the active production transition composition.
- The source and destination are duplicated live Slint views only during the short overlap. Playback state stays in the application model; no media or playback state is copied into the transition host.
- The supplied manual PDF has malformed cross-reference entries but remains readable through Poppler recovery. Its interaction diagrams inform screen and gesture interpretation; the app's existing behavior remains the source for shortcut compatibility.
- Reduced-motion behavior is not integrated or verified against a platform API. Native backend timing, zoom variants, and a dedicated album-list latency benchmark remain unverified.

## Related UI constraints

- `800adc5` adds a real Genius preview modal and translation target menu actions. Keep these modal views above the screen-transition host; opening/closing them should not be interpreted as app-screen navigation.
- Now Playing now lays out each lyric row from the preferred heights of the original, translated, and romaji text. A transition must preserve `lyric-scroll-y` and let those live measured rows render; do not flatten them into a snapshot.
- The preview consumes live seed, song, busy, and stage properties. It must not be recreated or have its stage reset when an unrelated screen transition is interrupted.
- The isolated prototype parses and compiles with Slint 1.18.1. Its runtime harness verifies directional offsets, rapid retarget requests, disabling motion during flight, and no later movement from the canceled transition. A software-rendered mid-flight image was visually inspected. Touch-driven swipes and native backend timing remain unverified.

## Repeated navigation repair — 2026-10-13

The album/list transition leak and hidden Home page motion were reproduced and repaired in a separate project chat, integrated as `462dfc9`. Completion now checks the active navigation generation, and an interrupted slide explicitly restarts its timer deadline. Settling clears both screen offsets and releases outgoing rows. Screen transitions reset both Home page hosts; outgoing Home stays static and current Home gestures wait until the screen settles.

`tests/screen_motion.rs` covers repeated album/list navigation, near-deadline interruptions, pointer row activation after settling, restoring Home after page-two navigation, horizontal-wheel accumulation, Smooth Slides toggling, and Device/Screen modes. Its snapshots render different source/intermediate/destination frames and the settled Home matches its original frame. An intermediate album composition and settled Home were visually inspected. This supersedes the earlier limitation about production intermediate frames above; native animation timing and OS reduced-motion integration remain unverified.

## Elastic scrolling integration — 2026-10-15

[Rubber-band overscroll](OVERSCROLL.md) uses a separate visual offset from the
screen and Home page slides. Only bounded list/lyric coordinates are retained in
navigation state. A navigation generation cancels pending elastic motion even
when navigation reuses a library presentation with Smooth Slides disabled. Home
page boundary input never wraps or repeats navigation. Its elastic feedback works
with either Smooth Slides setting. Library and lyric viewports arbitrate vertical
dragging and horizontal Back in one recognizer; seek/volume controls remain outside
it. Existing motion regressions and the new live-input overscroll tests exercise
these interactions. Native trackpad feel and frame pacing still require manual
verification.
