# Local video implementation

## Current milestone

The isolated `src/video` backend imports local video metadata, stores a separate
`videos.json` library, and plays through dynamically loaded libmpv. `ui/video.slint`
provides the playback view. Shared application integration is pending the other
chat's playlist integration commit; these files alone do not activate Videos in
the normal app. No music table, playlist schema or shared manifest was changed.

The interface follows chapter 5, pages 27–28 of the supplied iPod nano seventh
generation manual (`/Users/huanzichen/Downloads/Study/References/46034.pdf`): select
from Videos; tap the image to show/hide Done, position and transport controls.
Desktop volume is also visible. Frames fit the available display without cropping.
Chapter selection, rentals, subtitles, video playlists and rotation of the entire
device UI are outside this milestone.

## Runtime dependencies and formats

On macOS, install `brew install mpv` (includes FFmpeg/ffprobe). The development
machine was exercised with Homebrew mpv 0.41.0 and FFmpeg 9.0.2 on Apple Silicon.
No native libraries are linked into the Rust binary at build time. `libloading`
0.8.9 is the only additional Rust dependency needed at integration.

- Playback loads libmpv client ABI **2** (mpv 0.35 or newer), tries Apple Silicon
  and Intel Homebrew locations, then platform library names. `NANO_MPV_LIBRARY`
  supplies an additional library candidate. Missing/incompatible libraries return
  a playback error, without preventing music or app startup.
- Import runs ffprobe, preferring `NANO_FFPROBE`, then Homebrew paths, then PATH.
  Missing ffprobe produces import errors. Probe execution is limited to 15 seconds
  per file, with bounded retained output. Sources must be ordinary local files.
- The importer accepts **MP4, M4V, MOV, MKV, WebM, AVI** extensions (case insensitive),
  then requires an actual video stream with finite duration and nonzero dimensions.
  An attached cover image does not qualify. Metadata probing does not guarantee
  that the installed mpv build can decode a particular codec.
- Verified generated clips (320×180, 24 fps): **MP4/M4V/MKV with H.264 + AAC**,
  **MOV with H.264 + PCM S16LE**, **WebM with VP9 + Opus**, **AVI with MPEG-4 Part 2
  + MP3**, and **silent MP4/H.264**. Other codec/container combinations depend on
  the installed FFmpeg/mpv build and remain unverified. Encrypted/DRM files are unsupported.
- Decoder output is SDR 8-bit RGB. Hardware decoding, HDR fidelity and GPU texture
  import are not implemented. Windows/Linux library candidates are provided but
  native compilation, audio output and packaging have not been verified there.

libmpv's software renderer first appeared in mpv 0.33, but this wrapper deliberately
requires ABI 2 and does not attempt compatibility with old ABI 1 installations.
The Homebrew runtime is installed separately. It is not copied into the app bundle.
Redistributing libmpv/FFmpeg requires reviewing the exact build's licenses and
dependencies; mpv's default build is GPL, with an optional LGPL build configuration.

## Worker and library design

`Engine` owns one worker, which owns every mpv handle and callback lifetime. A load
destroys the previous session before creating another, even for the same path.
Every transport command, progress snapshot, frame, completion and error carries a
monotonically increasing generation supplied by the reducer. Stale transport
commands are ignored. Native events cannot cross session boundaries.

The worker uses asynchronous mpv commands and property observations after render
context creation, following the renderer's threading restrictions. mpv owns
demuxing, decoding, audio output, synchronization, seek and pause. The callback
only sets an atomic update flag. Rendering and copying happen on the worker.
Software targets have 64-byte aligned pointers/strides and a longest dimension of
640 pixels; display aspect accounts for sample aspect ratio and rotation metadata.
The RGB0 byte is explicitly converted to opaque alpha before reaching Slint.

`take_frame` and `take_progress` consume a single latest-value slot each. A slow UI
replaces old frames instead of accumulating a decode backlog. The controller must
convert owned RGBA bytes to `slint::SharedPixelBuffer<Rgba8Pixel>` / `Image` on the UI
thread and reject generations that are no longer current. UI resizing uses Slint
`image-fit: contain`, preserving aspect. The controller should consume frames on a
separate approximately 16 ms timer; the existing 50 ms app poll is too slow for
smooth 24/30 fps playback. Native shutdown frees the render context before mpv,
then joins the worker without holding the frame mutex.

Library import runs on a separate worker. It recursively scans supported
extensions, canonicalizes paths, merges duplicates by path, sorts titles, and
atomically replaces `videos.json`. Probe failures retain existing entries and
report per-file errors. A corrupt existing library is preserved and reported,
rather than overwritten. Original media is never modified or copied.

## Integration contract

After playlist ownership is released:

1. Export `pub mod video` from `src/lib.rs` and add `libloading = "0.8.9"`.
2. Add Videos and VideoPlaying navigation, a video-library row target and Import
   Videos action. Keep music indices/queue separate from video entries. Reducer
   state owns the current video path/index, generation, progress/status and overlay.
3. Load/import the video library on workers; file-picker cancellation clears the
   busy state. Persist only successfully imported results. Display source metadata
   without translating titles. Error/status text uses the existing i18n mechanism.
4. Add explicit audio-ownership handoff acknowledgements: suspend/stop the music
   sink before sending video Load; wait for video Stop completion before music Load
   or resume (`Command::Stop { handoff }` replies `Event::Stopped { handoff }`
   only after native destruction). Ignore music progress/completion while video owns the output. Simply
   sending commands to both workers concurrently is insufficient to guarantee no
   brief overlap. Keep the suspended music queue/selection for later playback.
5. Route Space, global volume/mute, seek, previous/next and Now Playing to the active
   media owner. Videos uses list navigation; playback uses Now Playing's seek/volume
   surfaces. Back/Done pauses video and returns to its list; Home preserves the
   owner's transport semantics. Selecting music switches ownership explicitly.
6. Bind `VideoView` to its frame/status/controls and add the native File menu import
   item. Keep the existing shortcut editor, music controls and settings functioning.
7. Run full formatting/check/tests/strict lint and headless UI regressions; verify
   the native app with a generated clip only after playlist bundle delivery.

## Verification

During isolation, `/tmp/nano-video-prototype/Cargo.toml` exports the module by path
with independent `libloading`, `serde`, `serde_json`, `walkdir` and `tempfile`
dependencies. It does not build or restart the normal app. `src/video/tests.rs`
contains explicit ignored native tests so dependency-free default test runs remain
usable. Run native checks with `cargo test video:: -- --include-ignored` once exported.

Five module tests (including three explicit native tests) and the separate Slint
preview render test pass. The preview's visible and hidden control states were
inspected as PNGs without opening the normal app. Tests exercise original generated media, import/reload/deduplication, damaged file
handling, actual RGB frames and alpha, pause, paused seek, volume commands, stale
generations, reopening the same file, EOF, stop, missing sources and default audio
output (observing a non-null native audio driver), the codec/container matrix and
silent video. Metadata tests cover anamorphic aspect, rotation and attached-picture
rejection. Current native tests and prototype strict lint pass; application/UI
integration remains unverified until the shared-file handoff.

## Primary references

- [libmpv C client ABI](https://github.com/mpv-player/mpv/blob/v0.41.0/include/mpv/client.h)
- [Render ABI, software format/alignment and threading rules](https://github.com/mpv-player/mpv/blob/v0.41.0/include/mpv/render.h)
- [Official software-rendering example](https://github.com/mpv-player/mpv-examples/blob/master/libmpv/sdl/main_sw.c)
- [mpv client API changes](https://github.com/mpv-player/mpv/blob/v0.41.0/DOCS/client-api-changes.rst)
- [mpv commands/properties](https://mpv.io/manual/stable/)
- [FFprobe machine-readable metadata](https://ffmpeg.org/ffprobe.html)
- [Homebrew mpv runtime formula](https://formulae.brew.sh/formula/mpv)
- [mpv licensing/build exceptions](https://github.com/mpv-player/mpv/blob/v0.41.0/Copyright)
