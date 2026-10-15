# Nano Music Player 0.1.1 integration

Integration date: 2026-10-15 (Asia/Shanghai). Branch:
`codex/integrated-player`, based on `3f9e9d9`. This version combines the current
player and its authorization verification with rubber-band scrolling. Existing
worktrees and uncommitted work are preserved; main is not updated automatically.

## Included branches

| Branch | Included state |
| --- | --- |
| `main`, `restore/reliable-playback` | Their current committed work is already in the base, including the stable playback recovery, settings hierarchy, library icons, ratings, translation controls and video previews |
| `codex/nano-feature-refinement` | `dc10a68` is already an ancestor of the base |
| `codex/nano-audio-refinement` | `bf92250` is patch-equivalent to the integrated `b8857ec`; later engine stability work is retained |
| `codex/macos-media-refinement` | `8f48bb0` has exactly the same media implementation blob as the current base |
| `codex/fix-slide-home-lifecycle` | `dedd757` is patch-equivalent to the integrated `462dfc9`; subsequent gesture and animation changes are retained |
| `codex/native-playback-p0` | Includes the real installed-app verification and classification as required macOS folder authorization |
| `feat/rubber-band-overscroll` | `9a0e98d` is merged, with compatibility changes described below |

Commit `97d09b9` records ancestry for the three previously integrated branches
after verifying patch equivalence or identical file contents. It deliberately
leaves the current tree unchanged. Commit `1c5a67e` merges the new overscroll work
and resolves its overlaps with the newer UI. All existing local branch tips are
ancestors of the resulting integration; no old engine implementation is restored.

## Merge compatibility

- Keep the 80ms Back quiet interval and 140ms screen animation. Elastic list and
  lyric viewports share the existing cross-view gesture owner with headers/Home.
- Arm that quiet timer when a Back is consumed, including a gesture with only one
  event. The added regression fails before this integration call and passes after
  it: a second deliberate gesture must reach Music instead of staying on Albums.
- Keep originals above their secondary lyric line. Translation takes priority
  over romaji; T, generation and existing translation preferences are retained.
- Preserve the current settings hierarchy, Music/playlist icons, black rating
  screen, shuffle-row rules and random video thumbnail geometry.
- Audio-core, controller, media integration, monitoring recovery and translation
  worker source remain unchanged from the verified base.

The Cargo package version is 0.1.1. The macOS package script uses the same short
version and build number 2; the bundle identifier remains `org.nano.music-player`.

## Verification

Initial focused checks pass for elastic physics/live input, Home/screen motion,
the continuous Back burst and wrapped translated lyrics. The additional short
gesture regression also passes after the compatibility fix.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo check` | PASS |
| `cargo test --all-targets` | PASS: 301 passed, 0 failed, 25 opt-in tests ignored |
| `cargo test --test playback_runtime -- --ignored --test-threads=1 --nocapture` | PASS: 4 native audio tests |
| `cargo test --test playback_comparison -- --ignored --test-threads=1 --nocapture` | PASS: 8 native comparison tests, including the established 53-file manifest and isolated database snapshot |
| Native generated-video/music handoff test | PASS: 1 passed, 4 filtered out |
| All-target Clippy with warnings denied and the three existing lint categories allowed | PASS: no additional warnings |
| `cargo build --release` | PASS |
| Package script syntax, candidate plist and strict code-signature verification | PASS |

Clippy uses the existing allowances for `collapsible_if`, `needless_borrow` and
`field_reassign_with_default`; it is not an unconditional warnings-denied pass.
The native video command is
`cargo test --test video_ui actual_video_import_frames_rotation_and_music_handoff -- --ignored --test-threads=1 --nocapture`.
Software-rendered held-list and both Home boundary frames were visually checked.

The local candidate is `target/Nano Music Player.app`, version 0.1.1, build 2,
with the existing bundle identifier and local ad-hoc signing type. Its executable
SHA-256 is `707d1a75ed2e3f5eb2b63e43ce07b6d8d93959bd2b34295db8c0967b213f3dd5`.
The currently installed bundle was not replaced and still matches its verified
starting executable hash. The candidate's native GUI and original-library access
were not tested; the native audio results above come from the integration's test
executables.

## Native verification scope

The previously installed application was verified with original library files,
temporary media, transport, consecutive tracks, user-confirmed audible output,
quit and relaunch; see [the authorization record](NATIVE_PLAYBACK_P0.md).
Those installed-bundle observations apply to that exact earlier executable.
They are not automatically attributed to a newly built integration executable.

Synthetic wheel/drag tests and software renders do not certify physical trackpad
feel, native frame pacing or audible gapless quality. The separately reported
intermittent CoreAudio initialization delay remains unexplained. A rebuilt local
ad-hoc-signed app may require the user's folder approval again; this is required
macOS authorization, not a confirmed application bug.
