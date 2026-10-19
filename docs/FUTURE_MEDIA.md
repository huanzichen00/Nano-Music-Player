# Future video and podcast support

This is the original architecture evaluation. Current desktop implementations are documented in [Video implementation](VIDEO_IMPLEMENTATION.md) and [Podcast implementation](PODCAST_IMPLEMENTATION.md); the proposals below are historical context.

Both are feasible while preserving the reducer/controller architecture. Podcasts can extend the current audio worker and SQLite cache incrementally. Video has a larger integration cost because it requires a multimedia backend, an audio/video clock and a frame presentation path; prototype those boundaries before committing to a backend.

## Video playback

The current playback path is audio-only: the controller applies semantic actions to the reducer, emits `AudioCommand`s, and a worker owns the Rodio output stream, sink, decoder and progress events. Slint/FemtoVG renders the application UI. The library scanner currently recognizes audio extensions and stores audio metadata and artwork in the `tracks` SQLite table.

Video needs a separate media backend, not just another audio extension. The backend must demux audio and video, decode both streams, schedule video frames against an audio clock, handle seek/pause/rate changes, and report end/error state. Audio is typically the stable playback clock; decoded frames need to be dropped or delayed to remain synchronized after seeking and under load. The current 100 ms progress polling is adequate for a position indicator but does not provide a frame presentation clock.

The app currently uses Symphonia 0.5 through Rodio for audio. Symphonia 0.6.1 documents video codec support as experimental and warns against production use. A video implementation should first prototype a maintained multimedia backend with demuxing, decoding, audio output and seeking (for example, a native platform backend or GStreamer/libmpv integration), then confirm packaging, licensing and codec availability on each OS. [Symphonia 0.6.1 feature status](https://docs.rs/symphonia/0.6.1/symphonia/).

Rendering is a second major decision. Slint's `Image` can present pixel buffers or textures, but the current FemtoVG path does not establish a video-frame import/presentation pipeline. A CPU path can copy decoded frames into owned RGBA buffers and update a Slint image on the UI event loop; this is simpler but risks expensive copies and UI stalls at video rates. A GPU/native texture path avoids copies but requires renderer/backend interop, texture lifetime rules, synchronization, and a supported fallback. Slint documents its image and texture APIs, but integration must be validated against this app's pinned Slint 1.18.1 and renderer configuration. [Slint `Image`](https://docs.rs/slint/1.18.1/slint/struct.Image.html).

Preserve the existing reducer and semantic `Action` model as the source of playback state. Introduce a media-worker interface behind commands and events; decoder and rendering callbacks must not mutate `State`. Keep probing and decoding off the UI thread. Deliver owned frame data or retained texture handles through a bounded latest-frame channel so a slow UI drops stale frames. Make shutdown and resource lifetimes explicit. The audio engine currently checks track IDs; add a monotonically increasing playback generation to every command, frame and event so reopening the same item also rejects events from a previous playback session.

Suggested first milestone: play one local MP4 with synchronized audio, pause/resume, seek, stop, and clean shutdown; test seek during playback, rapid source changes, decode errors, window resize, and repeated open/close. Defer playlists, subtitles, hardware acceleration, and broad codec claims until the baseline works on macOS and another target.

## Podcasts

For the desktop-only scope, see [Podcast desktop plan](PODCAST_DESKTOP_PLAN.md); the first implementation and validation are recorded in [Podcast implementation](PODCAST_IMPLEMENTATION.md).

Podcasts fit the existing music player more naturally, but need feed and episode domain objects rather than treating each episode as an ordinary local song. The import pipeline already performs filesystem work on a background thread and caches metadata in SQLite; it can remain the model for background feed refresh and download work, while the controller/reducer continues to own visible state and user actions.

An incremental design could add tables like:

```sql
CREATE TABLE podcast_feeds (
  id TEXT PRIMARY KEY,
  feed_url TEXT NOT NULL UNIQUE,
  title TEXT NOT NULL,
  image_url TEXT,
  last_checked INTEGER
);

CREATE TABLE podcast_episodes (
  id TEXT PRIMARY KEY,
  feed_id TEXT NOT NULL REFERENCES podcast_feeds(id),
  guid TEXT NOT NULL,
  title TEXT NOT NULL,
  media_url TEXT,
  local_path TEXT,
  duration REAL,
  published INTEGER,
  position REAL NOT NULL DEFAULT 0,
  completed INTEGER NOT NULL DEFAULT 0,
  UNIQUE(feed_id, guid)
);
```

Use normalized feed URLs as initial feed identity and feed-scoped RSS GUIDs as episode identity. RSS GUIDs are optional; define and persist a fallback identity derived from enclosure URL, publication time and title, with an alias/migration policy if a GUID later appears. Enclosure URLs can change, so a URL alone is a poor durable key. [RSS GUID and enclosure specification](https://www.rssboard.org/rss-specification).

Store download paths separately from remote enclosure URLs; do not overload the track path key. Persist playback position periodically and on pause/stop, then restore it when an episode is selected. Define completion behavior (for example, mark complete near the end and reset only on explicit replay); avoid a database update on every 100 ms progress event.

RSS parsing and network transfer should run off the UI thread. A feed refresh should parse RSS/Atom metadata, apply updates transactionally, preserve local playback position/completion, and report malformed feeds or unavailable enclosures without discarding the prior cache. Downloads need cancellation, temporary-file plus atomic rename, disk-space/error reporting, and a clear policy for duplicate enclosures. Network requests should have timeouts and should not block the app's controller loop. Keep feed refresh separate from playback so an offline or slow feed cannot interrupt local music.

Expose podcast actions through the existing reducer/controller path: add or refresh feed, select episode, download/delete download, resume, mark played/unplayed, and skip a configurable intro/outro. Reuse the audio worker initially for downloaded or streamed audio only if it can report stable seek positions for the chosen source; remote streaming adds buffering and reconnect semantics that the current file-backed `File::open` decoder does not provide. Preserve stable episode IDs across feed refreshes and reject stale worker events using playback IDs, as the audio engine already does for tracks.

Suggested stages are local episode records and resume state first, then RSS refresh, then downloads and optional streaming. This separates core episode navigation and durable resume behavior from network and buffering complexity. No RSS parser or HTTP dependency is selected here; choose them during implementation based on current maintenance, async/runtime compatibility, TLS support, and license review.

## Existing boundaries to preserve

- `State` remains the authoritative reducer; UI callbacks dispatch semantic actions and effects.
- Controller owns the connection from reducer effects to background workers and Slint updates.
- Scanning, network work, database operations, and decoding stay off the UI thread.
- Worker events identify the active media item so delayed events cannot update a newer selection.
- Existing music-library track indices and queue behavior remain stable while adding separate media entities.
