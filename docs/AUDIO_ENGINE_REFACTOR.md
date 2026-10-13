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

## Lifecycle milestone

- Playback reduction now consumes consistent intent snapshots and returns event
  envelopes without locks, channels, decoders or device calls. Active, ended,
  finished and failed phases are explicit.
- Seeks prepare a fresh decoder on the worker, preserve the resolved gain, volume
  and pause state, then retire the old source and replace its sink. Source serials
  distinguish old EOF from the seeked source. Progress adds the prepared seek
  offset, resetting it at the next genuine track transition.
- Failed seeks leave the old decoder usable and rebase its EOF epoch. EOF seeks
  can reopen an already-drained source; newer seek epochs supersede older work.
- First-sample decoding is outside the intent mutex. Start notification stays
  inside the short latch transaction, preserving ordering against seek/cancel.
  Explicit retirement prevents old samples overlapping a replacement before
  Rodio observes its asynchronous Stop flag.
- Output callbacks report stream-identified failures to the worker. One failure
  retires that output, suppresses progress/finish and permits a subsequent load;
  errors from older output instances are ignored.
- 34 non-ignored audio tests pass, plus the opt-in source-opening profile.
  The slow-decoder cancellation test fails on the previous implementation and
  passes after the lock change. Existing format/sample/gain tests remain intact. The complete suite also passes.
- `cargo check` and formatting pass. All-target Clippy passes with only the three
  existing lint classes suppressed: `collapsible_if` in `src/app/controller.rs`,
  `needless_borrow` in `src/library/scanner.rs`, and `field_reassign_with_default`
  in `tests/feature_library.rs`. Unmodified strict Clippy reports those lints.
- Both native tests in `tests/playback_runtime.rs` pass outside the sandbox,
  including the new seek/skip/pause/resume/Stop regression. This establishes
  native control responsiveness in these scenarios, not audible gaplessness.

Seek tradeoff: reopening adds worker-side file/decoder setup to each applied
seek. If the path disappears or becomes unreadable after loading, seek reports
that error while existing decoded playback can continue. This avoids holding
the command worker indefinitely on an output-callback feedback channel. Seeks
still require ordinary file I/O to complete; no hard wall-clock I/O bound is
claimed. Buffered/resampled or hardware-queued samples already generated before
a control change cannot be recalled.

## Metadata and final review

ReplayGain Off now bypasses Lofty entirely. Enabled modes read tags without
calculating unused file properties. The gain formula, album-to-track fallback,
peak limiting, optional final clamp and per-sample multiplication are unchanged.
Tagged WAV/FLAC/MP3/M4A decoder samples remain identical in Off mode. A seek
regression also changes file tags after loading and verifies that the original
resolved gain is retained.

The opt-in `profile_source_opening` test performs 1,000 warmed fixture opens per
format without a pass/fail timing threshold. Example debug measurements from
this host (milliseconds per 1,000 opens):

| ReplayGain Off | Before tag-probe removal | After |
| --- | ---: | ---: |
| WAV | 90.9 | 82.1 |
| FLAC | 228.9 | 222.4 |
| MP3 | 564.0 | 537.1 |
| M4A | 731.6 | 686.6 |

These are small cached synthetic files and single profile runs, not a controlled
throughput or callback-latency benchmark. The concrete saving is one metadata
probe/file open per Off load or prefetch. No steady-state speedup or underrun
improvement is claimed.

Final independent review caught output failure arriving after an unread
successor start. The backend now stops output and drains the completed callback
notifications before marking failure, preserving the transition and reporting
the successor's ID. An injected failure regression checks the precise
Transitioned/Error ordering and absence of further progress/completion.

Native video verification also passed:
`cargo test --test video_ui actual_video_import_frames_rotation_and_music_handoff -- --ignored`.
It uses generated temporary media and exercises video import, transport and the
acknowledged return to music. The two playback runtime tests use real macOS
output; physical output-device disconnection is tested only by event injection,
not by unplugging or disabling a device.

## Completed verification

- `cargo fmt --all` and `cargo fmt --all -- --check`: pass.
- `cargo check`: pass.
- `cargo test --no-fail-fast --quiet`: **268 passed, 0 failed, 13 ignored**,
  including 153 non-ignored library tests. Audio coverage grew from 21 to
  **38 non-ignored tests**, plus the manual source-opening profile.
- `cargo test --test playback_runtime -- --ignored --test-threads=1`:
  **2 passed** on real macOS output after the final core change.
- The generated-media native video handoff test: **1 passed** separately.
- `cargo clippy --all-targets`: completes with only the three existing warnings
  outside audio listed above. `-D warnings` fails on those existing lints;
  all-target strict checking passes when only those three lint classes are
  allowed. No audio warnings, dependency changes or UI/controller changes.
- `git diff --check`: pass. The user's original `FEATURE_PLAN.md` matches its
  initial SHA-256 byte-for-byte and is excluded from every commit.

The original sample-boundary, repeat-ID, gain/tag and queue/state tests continue
to pass. Added tests cover slow first-sample cancellation and retirement,
replacement overlap, EOF and boundary seeking, latest-seek ordering, successor
intent after seek, failed seek/prefetch recovery, stale output errors, duplicate
EOF and authoritative transition/error ordering. The public Engine command and
event interfaces remain unchanged.

Remaining unverified behavior: audible output gaps/clipping/artifacts; hardware
continuity across differing sample rates/channel layouts; physical output loss;
physical media keys and Control Center; native Windows/Linux builds. The Rodio
callback I/O/decoder-error/queued-cancellation limitations above remain explicit.
