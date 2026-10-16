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

Primary sources under investigation:

- [Flutter BouncingScrollPhysics](https://github.com/flutter/flutter/blob/abaf9c523780a608bd46686fd5e53740a07077f8/packages/flutter/lib/src/widgets/scroll_physics.dart),
  [BouncingScrollSimulation](https://github.com/flutter/flutter/blob/abaf9c523780a608bd46686fd5e53740a07077f8/packages/flutter/lib/src/widgets/scroll_simulation.dart),
  [SpringSimulation](https://github.com/flutter/flutter/blob/abaf9c523780a608bd46686fd5e53740a07077f8/packages/flutter/lib/src/physics/spring_simulation.dart).
- [Qt Quick Flickable source](https://github.com/qt/qtdeclarative/blob/5c096a1c4f6fffe136a2e8b56d99914dfca324ee/src/quick/items/qquickflickable.cpp),
  [Flickable documentation](https://doc.qt.io/qt-6/qml-qtquick-flickable.html).

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

Planned alternatives, independently selectable in development tests:

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
