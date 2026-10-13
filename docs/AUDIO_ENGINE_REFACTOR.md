# Audio engine stability refactor

Audit baseline: `dedf482`, 2026-10-14. All audio-core changes are owned by the
primary agent; two independent agents performed read-only audits. The user's
uncommitted `docs/FEATURE_PLAN.md` is outside this change.

## Audit and plan

The existing generation and distinct source serials protect repeated track IDs
and rapid reloads. Successor starts are committed at the first consumed sample;
already-started successors must survive later repeat/shuffle/gain cancellation.
Controller handoffs and listening-session timestamps depend on these semantics.

Concrete risks found by reading the engine, controller, tests, recent audio
commits (`f4817ab`, `b8857ec`, `dc10a68`), and the installed Rodio 0.21.1 sources:

- `Sink::try_seek` blocks on an unbounded feedback receive. EOF between Rodio's
  5 ms control checks can leave the request queued forever. A boundary seek can
  also reach a canceled successor instead of the intended active source.
- First-sample decoding and notification run while holding the intent mutex.
  A slow decoder can delay command submission and event filtering.
- Runtime output errors are only printed by Rodio; playback state receives no
  failure notification.
- ReplayGain Off still opens and parses tags. Gain parsing also computes file
  properties it does not use. Per-sample gain itself allocates and locks nothing.

Incremental milestones:

1. Extract synchronous intent, source lifecycle, worker playback state, and
   Rodio/command ownership; retain public commands/events and existing tests.
2. Eliminate callback-dependent seek feedback, tighten lifecycle synchronization,
   and add deterministic regressions for cancellation, EOF and failures.
3. Remove unnecessary metadata work, verify gain/sample equivalence, run all
   required checks and native smoke tests, and record remaining limitations.

## Baseline verification

- Existing 21 audio tests pass before and after mechanical extraction.
- Full baseline suite passes outside the sandbox; inside the sandbox two AI
  mock-server tests cannot bind localhost (`Operation not permitted`).
- Native smoke inside the sandbox fails opening CoreAudio. A run outside the
  sandbox is required before drawing conclusions about native output.

## Backend limits to preserve in the verification record

Rodio 0.21.1 performs packet reads, decoding, sample-buffer allocation and source
destruction on its output callback. Its public decoder iterator represents fatal
packet errors as EOF, so the engine cannot reliably distinguish midstream
corruption from natural completion. Repeated canceled successors remain in the
Rodio queue until consumption reaches them. Full-stream lazy `buffered()` would
not remove callback decoding and would add unbounded retention; no such change
is planned without performance evidence.

Device-free sample tests cover raw same-sink WAV/FLAC/MP3/M4A boundaries, not
native resampling across different channel counts/sample rates. The native
`tests/playback_runtime.rs` smoke checks ordinary consecutive WAV playback,
completion, statistics and artwork. It does not measure output gaps or clipping.
Audible continuity, mixed-format hardware boundaries, physical media keys,
Control Center and native Windows/Linux behavior require separate verification.
