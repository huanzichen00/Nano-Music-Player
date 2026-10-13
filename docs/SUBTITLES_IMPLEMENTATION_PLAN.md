# ASS/SSA subtitle implementation plan

## Verified current behavior

The video worker renders with libmpv's software render API into an RGB0 buffer,
then copies that buffer into the latest-frame slot consumed by Slint. This is the
right place to preserve ASS styling: mpv's render output can include its subtitle
renderer (libass) when subtitle tracks are enabled. The current session explicitly
sets `sid=no` and `osd-level=0`, so current delivered frames do **not** prove
subtitle composition. `Video::render_size()` accounts for display aspect and
rotation before creating the fixed-size target. Keep subtitles in that same mpv
render pass; do not flatten ASS to plain text or add a second UI overlay renderer.

## Behavior and precedence

1. Each video has a persisted `SubtitlePreference`: Automatic, Off, Embedded track
   ID, or External sidecar path. Missing preference means Automatic for backward
   compatibility.
2. Automatic enables mpv embedded subtitle selection and adds deterministic
   matching `.ass`/`.ssa` sidecars. Matching uses the complete video stem, with
   `video.ass` preferred over language variants such as `video.ja.ass`.
3. An explicit external selection or Import/Replace is manual and outranks all
   automatically discovered files. Replacement removes the prior manual file
   association; removal selects Automatic. Off remains explicit until changed.
4. Embedded selection uses the mpv track ID from `track-list`; Off maps to `sid=no`.
   Track enumeration and selection are worker-owned and generation-scoped.
5. Failed paths, unsupported encodings, and mpv track/load failures produce a
   visible, translated notice while leaving the video playable. A stale session
   must not publish a track list, error or frame into a newer generation.

## Implementation sequence

1. **Backend model**: retain the current generation isolation. Add a subtitle
   preference to the per-video persisted association store, not `Video` identity
   metadata. Add commands/events for list tracks, choose track, set Off/Automatic,
   import/replace, and remove. Validate external paths and report precise errors.
2. **libmpv rendering**: remove the unconditional `sid=no`; configure libmpv with
   ASS/libass enabled and keep `osd-level=0` only if verified not to suppress
   subtitle bitmaps in the render API. Observe `track-list` and `sid`, expose
   selected subtitle metadata, and apply sidecars with `sub-add` or an equivalent
   documented client command. Preserve UTF-8 paths, rotation, display aspect,
   frame size, seeking, pause and audio handoffs. Verify styling in actual copied
   RGB frames, not only metadata or track-list assertions.
3. **Storage and import UX**: use a separate subtitle association file or a
   backward-compatible extension of video-library persistence. Never edit media.
   Import/Replace invokes a subtitle-only picker filtered to `.ass`/`.ssa`; store
   the chosen path per canonical video path. Missing files remain diagnosable and
   can be replaced or removed. Automatic discovery is read-only.
4. **Controls**: expose Off/Automatic, embedded tracks, matching external tracks,
   Import/Replace and Remove from the video playback menu/settings. Translate all
   labels and notices in English, Chinese and Japanese. The controller applies
   commands only to the active generation and refreshes menu state after load.
5. **Verification**: create offline generated video and ASS/SSA fixtures with
   distinct positioning, colors, outline, font size, event timing and a seek
   boundary. Inspect actual RGB frame regions and image previews. Cover embedded
   selection, automatic discovery, manual precedence, replacement/removal,
   persistence, seeking/timing, rotation/aspect, stale sessions and audio handoff.

## Current isolated implementation

`src/video/subtitles.rs` defines the preference states and deterministic sidecar
discovery. Controller, state, library persistence, Slint menus and translation
integration are owned by the primary task at present. The module therefore does
not claim end-to-end playback support. Once those files are available, connect the
preference and track commands to the existing worker and UI before describing ASS
support as complete.
