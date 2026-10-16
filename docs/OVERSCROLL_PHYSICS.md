# Overscroll physics experiment

## Baseline and ownership

Started 2026-10-16 from `02d2acd8e64f23c573fbb1d4b78088318ea7d075`, verified
against freshly fetched `origin/main`. Original checkout has user changes in
`docs/FEATURE_PLAN.md` and untracked `tests/monitor_file_budget.rs`; both are
preserved. Integration branch: `codex/experiment-overscroll-physics`, worktree
`/Users/huanzichen/.codex/worktrees/overscroll-physics/rust-music-player`.
No installed application, main branch, or remote branch is changed.

Four agents collaborate: lead/integration (`/root`, owns integration UI),
research (`/root/physics_research`, read-only primary-source investigation),
implementation (`/root/physics_implementation`, branch
`codex/overscroll-physics-model` in `/tmp/nano-overscroll-model`, owns new physics
helper/comparison harness), and review (`/root/regression_review`, branch
`codex/overscroll-regressions` in `/tmp/nano-overscroll-regressions`, owns existing
overscroll test extensions). Only the lead edits `ui/overscroll.slint`. Worker
commits are reviewed and integrated sequentially.

## References and initial comparison

Primary sources studied:

- [Flutter BouncingScrollPhysics](https://github.com/flutter/flutter/blob/abaf9c523780a608bd46686fd5e53740a07077f8/packages/flutter/lib/src/widgets/scroll_physics.dart),
  [BouncingScrollSimulation](https://github.com/flutter/flutter/blob/abaf9c523780a608bd46686fd5e53740a07077f8/packages/flutter/lib/src/widgets/scroll_simulation.dart),
  [SpringSimulation](https://github.com/flutter/flutter/blob/abaf9c523780a608bd46686fd5e53740a07077f8/packages/flutter/lib/src/physics/spring_simulation.dart).
- [Qt Quick Flickable source](https://github.com/qt/qtdeclarative/blob/5c096a1c4f6fffe136a2e8b56d99914dfca324ee/src/quick/items/qquickflickable.cpp),
  [Flickable documentation](https://doc.qt.io/qt-6/qml-qtquick-flickable.html).
- Apple [Designing Fluid Interfaces, WWDC 2018](https://developer.apple.com/videos/play/wwdc2018/803/)
  and [Cocoa touch/scroll event handling](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/EventOverview/HandlingTouchEvents/HandlingTouchEvents.html).
  These document continuity, direct tracking, and distinct finger/momentum
  phases; they do not publish UIKit rubber-band constants or a full equation.

Flutter stable was pinned at `abaf9c5` (2026-10-08); Qt development at `5c096a1`
(2026-10-15). Qt development is a studied source snapshot, not a claim about
every released Qt version. Apple behavior is discussed only where documented.

| Aspect | Nano baseline | Flutter bouncing | Qt Quick source snapshot |
| --- | --- | --- | --- |
| Drag outside bound | Invertible rational curve; cap .32H, initial gain .55 | Incremental gain .52(1-y/H)^2 normal, .26 fast; first crossing event can be uncompressed | Ordinary touch overshoot d/2; velocity-sensitive path uses atan |
| Reversal | Inverse raw coordinate; event partition independent away from float saturation | Normal easing uses reduced overscroll fraction; fast easing may be unresisted | New input cancels/reset timelines and custom transitions |
| Return | Implicit critical spring, omega240; no crossing into list | Analytic normal spring m=.5,k=100,zeta=1.1; fast m=.3,k=75,zeta=1.3 | Default 400ms timeline: half distance in first 100ms InQuad, remaining 300ms OutExpo |
| Boundary velocity | Stored pointer velocity passed to spring; displacement itself is compressed on crossing | Computes friction boundary hit time and transfers velocity with source `min(v,5000)` policy | Timeline/inertial policies; default return is not a velocity-preserving spring |
| Momentum | Native phase ownership, no synthetic wheel fling | These physics classes are not NSEvent phase detectors; pointerScroll has a separate clamped path | Handles ScrollBegin/Update/Momentum/End and can rebound before native momentum ends |
| Timing | 16ms timer, dt capped32ms | Analytic state sampled at elapsed time | Elapsed timeline/easing; custom rebound can override |

Flutter's [activity interruption](https://github.com/flutter/flutter/blob/abaf9c523780a608bd46686fd5e53740a07077f8/packages/flutter/lib/src/widgets/scroll_position_with_single_context.dart#L210-L276)
replaces hold/drag activities at current pixels; carried momentum is framework
policy, not something added to Nano wheel input. Qt's custom rebound example
in the documentation is not its default. Qt's velocity-sensitive path uses
`S*atan(a*v/(maxV*8*S))`, with S=150*effectiveDPR; platform velocity/deceleration
settings and [compile-time behavior constants](https://github.com/qt/qtdeclarative/blob/5c096a1c4f6fffe136a2e8b56d99914dfca324ee/src/quick/items/qquickflickablebehavior_p.h#L28-L50)
also matter. Its early momentum rebound conflicts with the requested Nano
gesture ownership, so that policy is deliberately not adopted.

Nano currently uses `R(d)=a*d/(1+a*abs(d)/L)`, `a=.55`, `L=.32*extent`.
Its local outward gain is `a*(1-abs(R)/L)^2`. Flutter's normal drag gain is
`.52*(1-overscroll/viewport)^2`; it applies this to incremental input, with
different easing/reversal policy, rather than using Nano's raw-distance inverse.
Qt uses distinct drag, velocity-sensitive overshoot, and timeline rebound paths;
its default rebound must not be described as a spring simulation.

Nano's spring is backward Euler for `x''+480*x'+57600*x=0`:
`v1=(v0-57600*x0*dt)/(1+480*dt+57600*dt^2)`; `x1=x0+v1*dt`.
Timer callbacks occur at 16ms intervals; integration time is capped at 32ms.
The spring boundary crossing clamp and `.1px / 3px/s` rest thresholds terminate
motion exactly. Wheel input clears velocity and owns the visual position through
the entire native gesture. A 32ms guard protects the finger/momentum handoff.

## Unmodified production measurements

`cargo test --test overscroll --test overscroll_ui`: **6 passed** before changes.
`cargo test --test overscroll_baseline -- --nocapture` measures the original
`ElasticAxis` directly, without a test-only implementation. Extent 300px,
160px outward pointer pull, pause 128ms (zero release velocity), then release.
The mock event loop is pumped at the specified display cadence; the production
16ms timer remains in effect. These are synthetic scheduling measurements,
not physical display or trackpad observations.

| Event-loop cadence | Held offset | First changed frame | Offset on that frame | Settled at |
| --- | ---: | ---: | ---: | ---: |
| 60Hz | 45.913044px | 16.667ms | 17.012380px | 116.667ms |
| 120Hz | 45.913044px | 16.667ms | 17.012380px | 116.667ms |
| 144Hz | 45.913044px | 20.833ms | 15.638310px | 125.000ms |

With a single delayed first callback at 150ms, the original axis is still active
at **9.969651px**: only 32ms of wall time is integrated. This reproduces lost
elapsed time; it does not demonstrate subjective stiffness or trackpad quality.

Compared configurations, independently selectable in development tests:

- A: original resistance, backward Euler, frequency 240/s, original time cap.
- B: same resistance; exact critical-damping solution at frequency 100/s,
  preserving initial velocity and consuming elapsed spring time.
- C: quadratic-falloff resistance family, initial gain .52 and cap .40*extent;
  same spring as B. Inspired by Flutter's gain principle, not a Flutter clone.

For B/C, with `c=v0+omega*x0`, exact evolution is
`x1=(x0+c*dt)*exp(-omega*dt)` and
`v1=(v0-omega*c*dt)*exp(-omega*dt)`.
Keep all existing settling assertions, gesture ownership, event phases,
navigation, and follow interruption intact. Select a replacement only after
the production comparison and regression suites pass.

## Production experiment results and decision

The development exporter uses the actual production `ElasticAxis` and shared
`ElasticPhysics`, not a Rust copy of the physics. Each reset/begin/drag/release
flushes Slint's deferred timer bindings with a zero-time event-loop pump before
advancing time. Setup assertions check identical accumulated 160px pulls and
preserved release velocity. This matters: without flushing, a previous timer
can contaminate the following synthetic gesture or arm a callback one frame late.

Complete results are committed in [overscroll-physics-comparison.csv](overscroll-physics-comparison.csv):
120 combinations, three models, four extents (120/240/300/600px), paused and
moving releases, and five schedules. Cadences use cumulative rounded millisecond
deadlines; irregular intervals repeat 4/25/7/12/40/9/21ms; delayed schedule starts
with 150ms. The table below is the paused 300px case. It differs slightly from
the fractional-duration baseline above because of integer-ms scheduling.

| Model | Initial offset | First17ms offset | First17ms velocity | 60Hz settle | 120Hz settle | 144Hz settle | Irregular settle | Delayed150ms settle |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| A | 45.913044px | 16.296867px | -1742.127930px/s | 117ms | 117ms | 125ms | 147ms | 254ms |
| B | 45.913044px | 22.646402px | -1425.884521px/s | 100ms | 100ms | 104ms | 118ms | 150ms |
| C | 49.133858px | 24.235054px | -1525.910767px/s | 100ms | 100ms | 104ms | 118ms | 150ms |

B reduces the first 17ms position step from 29.616177px to 23.266642px (21.4%)
while completing sooner. At the delayed callback it satisfies the unchanged
rest thresholds immediately; A still has 9.969651px of travel. Across measured
normal display cadences the observed settle range narrows from 8ms to 4ms;
the 16ms scheduling granularity itself is unchanged. Paused cases have no
boundary crossing, and both moving and paused inputs settle and stop timers.
The solver's identity/initial derivative and semigroup tests verify preserved
initial velocity and frame-split independence before the shared snapping rules.

**Select B.** The evidence supports a softer initial sampled return, smaller
timing variation, and correct delayed-frame recovery without relaxing the
existing 128ms pointer /160ms wheel regression assertions. Resistance remains
exactly `.55`, cap `.32*extent`; spring frequency changes 240/s implicit to
100/s analytic, critical damping remains 1. Stopping thresholds stay .1px/3px/s.
For unit mass this means stiffness 10000 and damping 200, versus 57600/480 in A.
Copying omega=240 into the exact solver would sharpen the first return, so the
frequency change is calibrated together with the integrator.

Consuming a long delayed interval can jump directly to rest on the first
available frame; missing rendered frames cannot be reconstructed. The reduced
first-step claim is for ordinary cadence, not every delayed/irregular sample.

C gives 7.0% more displacement for a 160px pull and a 25% higher asymptotic cap;
short pulls start with slightly lower gain. This changes established tracking
without resolving a demonstrated resistance defect, so C remains development
only. A is explicitly selectable, has golden baseline measurements, and remains
recoverable from the initial commit and experiment commit before B selection.

For scale context, derived from the inspected equations (not framework runtime
or hardware measurements): with x0=45.913px,v0=0 and common Nano .1px/3px/s
thresholds, Flutter normal returns in about 734ms and fast in 859ms. Its own DPR 1
normal tolerance of 1px/20px/s returns around 480ms. Qt's default timeline is 400ms.
These defaults are inappropriate to copy into Nano's established 128ms contract.
Nano's original per 16ms implicit return retains 37.05% displacement on the first
step; B retains 52.49%. Exact candidate evolution composes across 7/13/17/33/80ms
steps to the same 150ms state within tested floating-point tolerances.

## Scope, performance, and limits

Changed runtime files are only `ui/overscroll.slint` and the new
`ui/overscroll-physics.slint`. The helper uses scalar arithmetic and one
exponential per active rebound tick; no dependencies, collections, locks,
background threads, new recurring timers, or per-event allocations are added.
No timer runs while held stationary; motion stops on the existing rest test,
invalidation, or new input. No native wheel phase logic, navigation thresholds,
Controller/State/Action/Effect code, audio/video/database code, installed app,
or ordinary user settings change.

Frame independence is a property of the **rebound solver**, not the entire
scroll system. Interior pointer inertia retains its capped integration and
existing displacement compression on boundary entry; position is continuous,
but its gain changes from 1 to .55 at the edge, so derivative continuity is not
claimed. New pointer input intentionally stops the previous spring and zeros
its velocity while keeping the rendered position. Native wheel deltas continue
to clear velocity, preventing a second inertia animation.

The reviewer also identified a pre-existing pointer estimator limitation by
code inspection: a tiny fresh drag event after a pause longer than 100ms may
reuse pre-pause velocity because the estimator skips that interval and then
updates its timestamp. This was not experimentally reproduced or changed in
this focused rebound refinement. Physical trackpad feel, native frame pacing,
high-refresh hardware, touchscreen/other backends, and responsiveness under
real simultaneous audio playback remain manual verification requirements.

## Collaboration and Git record

Research and regression review ran alongside implementation. The researcher
also reviewed the helper equations; the regression reviewer independently
checked the final runtime diff and preserved assertions. Both found no runtime
blockers. Integration review caught and corrected deferred-timer contamination
in the comparison harness, a missing test-only native-phase wrapper, a
fixed-size test-window resize that did not resize its viewport, and an exact-zero
assertion for a new alternating-input case whose residual was only 0.000008px.
That new case now uses its existing 0.001px inverse-roundtrip tolerance. All
assertions present in the starting commit remain unchanged.

| Role | Identity | Exclusive ownership | Worktree/branch | Worker commits |
| --- | --- | --- | --- | --- |
| Lead/integration | `/root` | `ui/overscroll.slint`, baseline, integration docs | Managed `overscroll-physics/rust-music-player`, `codex/experiment-overscroll-physics` | Integration history below |
| Physics research | `/root/physics_research` | Read-only upstream research and helper review | Read-only baseline; no file edits | None |
| Physics implementation | `/root/physics_implementation` | New helper and comparison test | `/tmp/nano-overscroll-model`, `codex/overscroll-physics-model` | `2b9b1bd`, `bb90729`, `bed9e9b`, `25c5193` |
| Regression/performance review | `/root/regression_review` | Existing overscroll test extensions; independent final review | `/tmp/nano-overscroll-regressions`, `codex/overscroll-regressions` | `4d546a7` |

Integration commits (in order):

1. `5b4605e`: research and unmodified production baseline.
2. `e194563`: isolated A/B/C experiment, shared production helper, exact A golden checks.
3. `d6ac838`: reviewed regression extensions, including integration fixes.
4. Final branch tip: select B and record verified results (see `git log -4`).

The original main checkout remains at `02d2acd`; its staged/unstaged user work
is unchanged. No reset/clean/force-push, automatic merge, remote push, or app
installation was performed. Worker branches and worktrees remain available.

## Verification

Commands run from the integration worktree, using the existing shared target
cache and two build jobs. Cargo is installed in `/Users/huanzichen/.cargo/bin`,
which was added to PATH for these commands.

```sh
cargo fmt --all -- --check
cargo check
cargo test --test overscroll --test overscroll_ui
cargo test --all-targets
cargo clippy --all-targets -- -D warnings \
  -A clippy::collapsible_if -A clippy::needless_borrow \
  -A clippy::field_reassign_with_default
NANO_OVERSCROLL_CSV=/tmp/nano-overscroll-comparison.csv \
  cargo test --test overscroll_comparison export_production_axis_comparison_csv \
  -- --ignored --nocapture
```

The three Clippy command-line allowances are the existing repository allowances
documented in `docs/CONTROLLER_REFACTOR.md`; no new lint suppression is added.
The exporter is intentionally excluded from ordinary user builds/test runs and
was also executed explicitly to produce the committed measurement table.

Results: formatting and `cargo check` pass; the required focused overscroll/UI
command passes **13 tests**. Full `cargo test --all-targets` passes **358 tests**,
with **0 failures and 26 ignored** across 38 targets. This includes Home, Back,
lyrics, screen motion, modal navigation, playlist editing/reorder, seek/volume,
and video UI coverage. The opt-in exporter passes separately with 120 rows.
Original A golden measurements and the three ordinary comparison tests also
pass within the full run. All-target Clippy also passes with warnings denied
and only the three documented baseline allowances above.

| Required behavior | Production tests |
| --- | --- |
| Both boundaries, monotone/diminishing resistance, continuity, inverse, reversal | `overscroll`, `overscroll_comparison` |
| Completion, retained initial velocity, repeated wheels, interruption, held pauses | Original `overscroll` assertions plus comparison and new hold/reversal tests |
| Native start/end/momentum handoff, terminal zero/nonzero deltas, phase-less cadence | Original native-wheel regression and `overscroll_ui` Home phase tests |
| 60/120/144Hz, irregular/delayed intervals, exact A preservation | `overscroll_baseline`, `overscroll_comparison`, new irregular-frame regression |
| Resizing, generation/disable/modal cancellation, empty/short lists | New FlickHarness/AppWindow cases plus original `overscroll_ui` |
| Home boundaries, Back ownership, Smooth Slides | `overscroll_ui`, `home_motion`, `back_scroll`, `screen_motion` |
| Lyric follow interruption and translated/romaji geometry | `flickable_receives_live_drag`, `ai_lyrics` |
| Rapid alternating gestures, reorder/autoscroll, seek/volume | New alternating-input test; existing `playlist_editing`, `ui`, `video_ui` suites |

Native-phase regressions drive the production Slint global and synthetic input;
the unchanged macOS Winit bridge was source-reviewed, not fed physical NSEvents.
They certify application semantics, not physical trackpad fidelity. Existing
ignored native/audio/profile tests retain their previous opt-in requirements.

Verification logs: `/tmp/nano-overscroll-{baseline,check,focused,all-targets,clippy}.log`.
