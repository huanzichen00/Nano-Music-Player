# Video list previews

Videos shows a frame thumbnail beside each title and duration. Frames are
extracted on demand for visible rows, using one bounded FFmpeg process outside
the UI and playback threads. No playback sink or libmpv player is opened for a
preview. The first non-attached video stream is selected, respecting display
aspect and rotation, and scaled to at most 96×96 pixels.

The UI owns a 64-image memory cache. Imports/loads invalidate its generation;
an older extraction cannot republish stale pixels. Failed extractions retain a
video placeholder and are not retried until library invalidation. A five-second
process deadline and a 64 KiB output limit bound failure handling. The existing
video metadata format remains unchanged.

FFmpeg uses the existing Homebrew/PATH installation, or `NANO_FFMPEG` when
configured. Only local file/pipe protocols are allowed; previews neither decode
audio output nor modify video files.

The native video integration test captures a real 96×54 list thumbnail from a
generated 320×180 video, then verifies playback, rotation and music handoff.
Offline coverage also rejects stale generations and retains failed-image state.
The stream selection follows [FFmpeg's stream specifiers](https://ffmpeg.org/ffmpeg.html#Stream-specifiers).
