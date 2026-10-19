# Desktop podcasts

Implemented on 2026-10-19. The original scope and follow-up options are in [the desktop plan](PODCAST_DESKTOP_PLAN.md).

## User flow

Home → Podcasts opens Add Subscription, Continue Listening, Latest Episodes, Downloaded Episodes and subscribed shows. Paste a show's HTTP(S) RSS URL, fetch a preview, and explicitly subscribe. Previewing alone does not create a subscription or download audio. Public feeds need no directory-service credentials. Private URL query parameters are preserved and HTTP errors omit their URLs.

Shows expose manual refresh and unsubscribe. Shows and episodes use the native cover/blue unread-dot list. Episodes are ordered by release date; tapping an episode plays it, and the information button opens its inert, scrollable description and download/played-status actions. Download and Play waits for a complete validated file. Selecting another music, video or podcast playback target invalidates pending automatic playback. Unsubscribing retains local downloads and listening state.

The podcast player reuses the same NowPlayingView component as songs: three-line title strip, full-width cover and fixed dark transport/volume panel. Tapping the cover reveals position/date, repeat, 30-second rewind, speed (1X/2X/half) and episode-list controls. Previous restarts the current episode after its first three seconds; Next selects the next episode. Natural completion marks the episode played and selects the next unplayed/partly played episode in the same show; the last episode stops. Seeking directly to the end does not mark it played or start the next episode. An explicit replay starts at zero. Progress is saved about every five seconds during playback, and on pause, seek, media changes and orderly exit.

## Native UI reference

The supplied [Apple iPod nano seventh-generation guide](/Users/huanzichen/Downloads/Study/References/46034.pdf), printed pages 20-21, was extracted and visually rendered with Poppler. Page 20 supplies show/episode hierarchy, chronological playback, unread blue dots and automatic continuation; page 21 supplies the song-like playback layout and tap-to-reveal controls. Page 10 is the existing thumbnail-row geometry reference. The supplied PDF has malformed xref warnings but these pages render successfully.

Desktop RSS management is an addition to the original synced-content workflow. The playback screen shares the song component rather than introducing a separate visual skin. Chapter navigation and pitch-preserving time stretching remain deferred.

Review images: [show list](previews/podcast-home.png), [RSS subscription](previews/podcast-subscription.png), [playback](previews/podcast-playing.png), and [playback controls](previews/podcast-playing-controls.png).

## Implementation

- `src/podcast/feed.rs`: RSS 2.0/iTunes metadata parsing, URL validation and plain descriptions. Missing metadata and unsupported audio enclosures remain browsable.
- `src/podcast/store.rs`: independent `podcasts.sqlite3`, stable local IDs, feed URL aliases, GUID matching and GUID-less enclosure aliases. Feed updates preserve progress, completion and downloads. A GUID that appears later can upgrade a matching GUID-less episode. Ambiguous identity changes are not merged.
- `src/podcast/service.rs`: a storage coordinator and at most two HTTP jobs; feed validation headers/304 responses, bounded RSS and artwork responses, cancellation, streamed temporary files, audio validation and atomic publication. RSS size is capped at 8 MiB, images at 4 MiB/4096 pixels per axis, downloads at 4 GiB. Download failure or cancellation restarts from the beginning on retry.
- `src/app/podcast_state.rs`: desktop navigation, rows and transport actions. Music state and queue indices remain separate.
- `src/app/controller/podcasts.rs`: background events, pending play intentions, periodic persistence, artwork and macOS media metadata, plus the file-release barrier for deleting a playing download. Native speed controls use Rodio rate changes; pitch changes with speed in this version.
- `AudioCommand::LoadAt`: opens and seeks a local decoder before output starts. Original music/mobile load commands retain their existing behavior. Existing generation/epoch filtering rejects previous-session events, including repeat plays of the same episode.
- `ui/podcasts.slint`: subscription preview. Podcast playback uses spoken-mode controls in the existing `ui/now-playing.slint`. Existing list/navigation components provide show and episode browsing; English, Simplified Chinese and Japanese labels are included.

Media ownership switches use the existing Stop/Stopped handshake. Podcast events cannot index music tracks; podcast playback disables music prefetch, statistics sampling and ReplayGain, and uses its own repeat setting independently of music's shuffle/repeat. Deleting a playing download waits for file release before removal, and subsequent loads wait for that barrier. macOS media commands target the active media owner; other platforms retain the existing no-op native bridge.

Completed media lives in `podcasts/<episode-id>.audio`, with artwork in `podcasts/artwork/`. Temporary transfers are never played. Startup discards unfinished transfers, clears missing-file associations and recovers files renamed just before a database acknowledgement was interrupted. These paths are separate from music import and iPhone synchronization.

Only the desktop frontend exports the podcast module and screens. iOS and `--features mobile` keep their placeholder entry.

## Verification

Tests use original fixture audio and local HTTP servers; no user subscriptions or media are modified.

- Podcast unit tests cover namespace parsing, metadata and query parameters, stable GUID upgrades, changed enclosure URLs, duplicate GUID separation, persistent progress, conditional refresh, invalid downloaded audio, cancellation and FIFO shutdown writes.
- Controller tests cover music/podcast event isolation, pending handoffs, natural EOF versus seek-to-end, cancellation of download autoplay when switching media, and deletion waiting for a matching release acknowledgement.
- `tests/podcasts.rs` drives the actual desktop subscription preview, explicit subscribe, show and episode navigation, Chinese labels and the player under Slint's software backend. It writes review images to the system temporary `nano-qa` directory. Transport assertions also check separate music position and paused seeking after EOF.
- Audio engine regression tests include loading directly at a saved position, preserving pause/volume, stale-event rejection and existing gapless/seek behavior.
- Existing desktop core, reference UI, screen motion and settings navigation tests verify the surrounding UI.
- A `mobile` library check verifies that desktop-only podcast screens and service do not enter the mobile frontend.

The desktop all-target check and scoped strict Clippy (`--lib --test podcasts`) pass. Full test-target Clippy also reports existing `bool_assert_comparison` and `field_reassign_with_default` lints in `tests/ui.rs` and `tests/feature_library.rs`. The pre-existing untracked `tests/monitor_file_budget.rs` also reproduces the documented macOS kqueue file-descriptor limit; automatic library monitoring remains disabled as described in [Playback recovery](PLAYBACK_RECOVERY.md).

This validation is on macOS. Windows/Linux native output and file deletion need platform QA; automated fixture playback/decoding is not a claim that every public feed or codec has been tested. The feature does not depend on libmpv.

## Deferred

Directory search/share-link conversion, Atom, OPML, background scheduled refresh, HTTP download resumption, streaming, pitch-preserving speed changes, chapters and transcripts remain outside this first version. Feed URLs that change episode identity without a stable GUID can produce a separate item rather than transfer listening history speculatively.
