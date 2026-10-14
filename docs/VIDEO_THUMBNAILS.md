# Video list previews

Videos shows a frame thumbnail beside each title and duration. Frames are
extracted on demand for visible rows, using one bounded FFmpeg process outside
the UI and playback threads. No playback sink or libmpv player is opened for a
preview. The first non-attached video stream is selected, respecting display
aspect and rotation, and scaled to at most 96×96 pixels.

Each video samples a position from the middle 70% of its duration (15–85%),
avoiding opening and closing frames. A random per-cache hash seed picks distinct
positions by path. The seed survives image eviction and library invalidation,
so revisiting or rendering a row does not reroll its frame. Unknown, non-positive
or non-finite durations use a conservative one-second seek. Selection is kept
in memory without changing video files or the metadata format.

Video rows use the shared 58-logical-pixel artwork stride even though the first
row is the Import action. A centered 80×45 thumbnail area uses
`image-fit: contain`; portrait, square and anamorphic frames retain their full
displayed aspect without clipping. Titles and durations start after that area with six
logical pixels of spacing. The row geometry scales with device/screen zoom.

The UI owns a 64-image memory cache. Imports/loads invalidate its generation;
they also cancel the pending worker, while retaining its receiver until it exits
to prevent overlapping extraction processes. An older extraction cannot
republish stale pixels. Dropping the cache cancels pending extraction as well.
Failed extractions retain a video placeholder and are not retried until library
invalidation. A five-second process deadline and a 64 KiB output limit bound
failure handling. The existing
video metadata format remains unchanged.

FFmpeg uses the existing Homebrew/PATH installation, or `NANO_FFMPEG` when
configured. Only local file/pipe protocols are allowed; previews neither decode
audio output nor modify video files.

The native video integration test captures a real 96×54 list thumbnail from a
generated 320×180 video, then verifies playback, rotation and music handoff.
Offline coverage also rejects stale generations and retains failed-image state.
Regression tests verify stable and varied frame selection, edge avoidance,
unknown-duration fallback, process cancellation/deadlines/output limits, bounded
caching, and rendered landscape and portrait geometry with row hit targets in
both display modes at 0.75×,
1.25× and 2× zoom. An additional ignored, silent FFmpeg test verifies selection
from a generated green middle section, and rotation/SAR using a two-color
anamorphic fixture. It does not open a native player or audio output.
The stream selection follows [FFmpeg's stream specifiers](https://ffmpeg.org/ffmpeg.html#Stream-specifiers).

Integration verification on 2026-10-14: formatting, all-target checks and Clippy
pass with the existing three lint allowances. The complete suite passes 296
tests with 24 opt-in tests ignored. After the thumbnail test lint adjustment,
all five video geometry/UI tests pass. Three ignored CoreAudio playback tests
and the ignored real video playback/rotation/music-handoff test were explicitly
run and pass; the optimized release build also passes. These checks do not
establish audible Gapless continuity or translation quality.
