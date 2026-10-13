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

## Implemented support and verification

Per-video preferences are stored atomically in `video-subtitles.json`, keyed by
video path. Automatic discovery follows mpv `sub-auto=all` and `sid=auto`; an
explicit external path is added with `sub-add ... select`, while embedded IDs,
Off and Automatic map to mpv subtitle selection commands. Manual paths are
canonicalized and restricted to existing `.ass`/`.ssa` files before persistence.
Track-list events expose ID, title, language, external status and external path.
Subtitle command and load errors are reported without stopping video playback.

The playback menu offers Automatic, Off, embedded/external track selection,
Import/Replace and Remove. Preferences survive controller recreation. The native
picker captures the active video path and generation; a delayed result is ignored
if either changed. Its native-dialog state blocks shortcuts until the picker
closes. The menu handles configured J/K and H/L navigation, Enter, Back and Home,
and its track list scrolls within a fixed footer. Labels and errors are localized
in English, Chinese and Japanese.

Generated native fixtures verified styled ASS color/bold composition in copied
RGB frames, event timing after seek, manual replacement precedence over a
matching discovered sidecar, Off, embedded track enumeration/selection, and a
nonfatal missing subtitle. UI tests cover pointer selection, keyboard selection
and Back, persisted replacement/removal without touching the original subtitle,
invalid path rejection, and many-track landscape rendering in Device and Screen
Mode. Rotation/aspect and audio handoff retain the existing video pipeline and
have their existing independent video tests. Re-run the full suite and strict
lint after any native platform/runtime upgrade.
