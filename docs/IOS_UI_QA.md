# iOS interaction and display checks (2026-10-17)

## Changes verified in portable controller/input tests

- Albums and Artists now bind representative track thumbnails. The mobile app
  uses the desktop thumbnail workers, viewport scheduling and bounded cache;
  completion changes existing rows without replacing the list/scroll model.
- Synced covers also have a durable content-addressed store. Older completed
  packages recover cover assets without recopying audio or overwriting phone
  lyrics, ratings, playlists, play counts or history. Existing phone artwork wins.
- Queue scopes are shared between rows rather than copied N times. Rating and
  lyric polling borrow songs rather than copying their embedded cover every tick.
- Device Debug builds optimize Rust UI/decoder dependencies (`opt-level=2`).
- API settings have an explicit Paste button backed by UIPasteboard. Reads occur
  only on that action. Both fields use their full touch height; Paste doesn't
  persist a credential until Save. No real credential/clipboard was read in QA.
- Bottom notices dismiss after six seconds, can be tapped away, can reappear for
  a repeated failure, and clear on Back/Home. Field validation stays in its editor.
- Timed lyrics follow the active row; manual touch/scroll pauses following,
  inertia stops before the three-second resume timer, and new songs reset the
  position. Tests use `Lyrics::active` at 15s before a 31.310s first timestamp,
  and 134s within a later timed line; no inferred timestamps are fabricated.
- Phone empty-library instructions point to Music → Import Music. The lyric
  editor reserves message space only when it has a message.

## Display matrix

`mobile::visual_tests` renders 96 actual software-rendered screenshots:

- 320×568, 390×844, 440×956, 844×390; safe insets for portrait and landscape.
- Both iPod body and screen-only modes.
- Both Home pages, album list with long mixed-script metadata, empty library,
  search, playing with wrapped translations, loading, error notice, active
  computer transfer, API/lyric editors above the keyboard, and delete confirmation.

Modal bounds and content fitting are asserted. Touch regression tests cover
left-swipe into the touched menu, Back, vertical scroll with horizontal drift,
repeated taps, sliders, multiline editing, clipboard actions, playlist editing,
and lyric follow/resume. These are offline tests, not a claim that a human has
checked every state on a physical phone.

## Physical iPhone GPU corruption

The screenshots reported by the user resemble the confirmed Slint 1.18.1 iOS
background glyph-atlas issue [#14086](https://github.com/slint-ui/slint/issues/14086).
A source backport of the upstream draft fix
[#14094](https://github.com/slint-ui/slint/pull/14094) is pinned in `vendor/`.
It guards the display-link draw path while UIKit is in the background, discards
GPU caches on foreground return, and requests the skipped frame. See
[the precise provenance and patch](../vendor/README.md).

The updated signed iPhone build was installed over USB and launched on 2026-10-17.
The phone returned a successful cover recovery receipt for 142 tracks. The user
then reported “已经十分流畅了” after trying the update. This confirms subjective
responsiveness for that real-device session; no measured frame-rate improvement
is claimed. Lock/background/return glyph recovery with music and lock-screen
controls still requires a dedicated physical-device run. Do not mark that row
complete from software snapshots or the responsiveness report.

## Evidence

Ignored local artifacts are in `target/ios-preview/`:
`mobile-ui-audit-tests.log`, `desktop-ui-audit-tests.log`,
`desktop-shared-ui-audit-tests.log`, `ios-ui-audit-python.log`,
`mobile-ui-audit-clippy.log`, `ios-background-render-fix-build.log`,
`matrix/`, `lyrics-before-first-timestamp.png`, `lyrics-follow-active.png`,
`lyrics-follow-resumed.png`.

Debug-only `--ui-audit player|list|home|api` displays a fixed native-rendering
fixture without opening the user's library, audio device, credentials or network.
It can be used for GPU/renderer comparisons; return to a normal launch afterward.

## Playback stall after interruption (2026-10-17)

The user reported all songs opening the player without advancing and confirmed
that restarting recovered playback. The saved device inventory matches all 142
audio paths, readable flags and file sizes (2,613,831,201 bytes). The device crash
list contains four reports at 12:48–12:49; the latest still has the previously
fixed `setTransferActive:` recursion. No matching new crash report was found.
The old build had no persisted playback trace, and host `log collect --device`
requires administrator privileges unavailable in this session. These findings
do not establish that playlist creation or lyric lookup caused the stall.

Code review found a reproducible missing-Ended deadlock: `interruption` stayed
latched, all selected songs were deferred, and Play never attempted activation.
Apple explicitly warns that interruption Began need not have a corresponding
[Ended](https://developer.apple.com/library/archive/documentation/Audio/Conceptual/AudioSessionProgrammingGuide/HandlingAudioInterruptions/HandlingAudioInterruptions.html).
Explicit Play/selection now tries activation and only clears an old latch after
success. Automatic resume still requires native permission and retained user
intent. The next load reopens the audio device, including when a newer song
supersedes a queued recovery load. A disconnected worker reports an error and
can be recreated on a subsequent user request.

The new build writes bounded asynchronous `Documents/.nano/playback.jsonl` and
`playback.previous.jsonl` (256 KiB each), surviving app restarts. Records identify
playlist/lyric actions, native interruptions, activation, decode, sink creation,
first progress/status changes and worker exit. No API keys, clipboard contents,
lyrics or user-entered playlist names are recorded. This instrumentation enables
correlation if the original issue returns; it is not historical evidence of it.

The signed recovery build was installed and launched over USB. The user then
confirmed “播放正常，时间会走”. Retrieved device diagnostics show successful native
activation, decoder preparation and sink creation; position advances past 101s
with working pause/resume acknowledgements. The trace contains no interruption
for that session, so it verifies healthy playback, not the original trigger.
Playlist Name now accepts taps over the full field and padding; actual touch
tests cover its center/lower edge and a 260px keyboard at 320×568.

Validation for this update: 229 mobile tests passed (one optional test ignored),
245 desktop library tests passed (eight optional native-video tests ignored),
three additional sync process/UI tests, 54 core/settings integration tests,
55 Python checks, bridge checks, and mobile/desktop Clippy with warnings denied.
The device SDK build and application signature verification passed. Desktop sync
was checked with the connected iPhone and a read-only plan of 142 songs/2.61 GB;
the new GUI did not initiate another full transfer during QA.

The installed desktop File menu opens the new window. Live accessibility checks
confirm the connected iPhone, 142 songs, one playlist and 2.61 GB, and the Preview
button transitions to “Preview ready” with Sync enabled. The user confirmed
“界面正常显示” on the computer. The white computer-use capture is therefore a
capture limitation, not an observed blank application window. Software-rendered layouts are saved as
`iphone-sync-summary.png`, `iphone-sync-music.png`, `iphone-sync-small-result.png`
and `iphone-sync-small-busy.png`.
