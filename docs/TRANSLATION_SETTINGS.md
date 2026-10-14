# iPod translation settings and online lyrics

Settings → Lyrics → Translation exposes language selection, Translate, Regenerate,
Show, Cancel, Remove, DeepSeek credentials and Find Lyrics Online. The native menu
and device rows dispatch the same typed commands. Translation language is saved
independently of interface language. Translate reuses a valid exact-source cache;
Regenerate makes a new request. Show Translation and plain T control the same
persisted, optional display mode (Off by default). When On, the lyrics area shows
only the translated lines; original text and romaji are hidden. Turning it Off
restores original lyrics and the existing R/romaji preference. Switching modes
never generates a network request. A pending result cannot override a later T-off
choice. T uses a free default chord; migration preserves custom T assignments and
current-format explicit unassignment.

Settings → Lyrics → Find Lyrics opens a song chooser and a dedicated lookup page.
Choosing a song does not start it or change the current queue. The page shows the
selected title/artist, persistent request status, Find/Cancel, lyric management
and DeepSeek settings. It also opens from Lyrics for Track. Explicit Find can
operate on a song that is not playing; the Translate continuation stays tied to
the playing song.

Use Automatic Lyrics clears an imported/downloaded lyric association or removal
and rediscovers embedded audio tags or nearby `.lrc`/`.txt` files. It does not make
an online request; its row now explains these file sources.

Existing lyrics always take precedence. If Translate is explicitly requested for a
song without lyrics, it runs the following stages off the UI/audio path:

1. Ask DeepSeek to search for the song's lyrics using its native
   `web_search_20250305` tool, limited to one search. This uses the separate
   Anthropic-compatible Messages endpoint and existing masked-key configuration.
   Model prose never counts as evidence that a search occurred.
2. Retrieve lyrics from LRCLIB using a structured LRCLIB search-result ID when
   present, otherwise its exact song-signature API. Verify title, artist, album
   and duration (within two seconds), reject instrumental/empty/mismatched records,
   and preserve actual synchronized lyrics. Plain text is never given invented
   timestamps. URLs on arbitrary search-result hosts are never fetched.
3. Recheck the requested song, metadata, empty source and cancellation state before
   saving (and the playing song for a Translate continuation). The existing SQLite lyric association stores content and source URL on
   the existing lyric-edit worker. An atomic insert cannot replace a newer manual
   import or a removal. The UI publishes lyrics only after persistence succeeds.
4. If this was Translate, invoke the existing validated translation path after
   the lyrics update. Find Lyrics Online stops after storing the lyrics.

Cancel and target changes stop the pending lookup/translation continuation.
Manual lyric edits cancel a pending lookup. A currently executing network request
can finish within its timeout (DeepSeek 120 seconds, LRCLIB 20 seconds), but a
cancelled or stale response cannot start persistence or translation. Only one
lookup is admitted at a time, including while its cancelled HTTP call is exiting.
Already accepted lyric persistence can finish after cancellation; Cancel stops
translation of that result. No network request starts on app launch or track
selection, and no audio files are modified.

The provider and client contracts were checked against
[DeepSeek's native search implementation](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/web/web-search-deepseek/src/provider.ts)
and [LRCLIB's API specification](https://lrclib.net/docs).

## Verification, 2026-10-14

Offline coverage includes actual device-row dispatch, independent language
persistence, shared menu commands, private credential UI, T priority/persistence and late-result behavior, cached translation
without HTTP, native-search response validation, HTTP metadata/User-Agent and
credential isolation, response limits, real timestamps/plain fallback, recording
matching, stale/cancelled results and durable storage that preserves manual edits.

The opt-in `tests/online_lyrics_runtime.rs` test made a real configured DeepSeek
native-search request and fetched a matching LRCLIB recording for the supplied
song metadata. It passed and retrieved 28 lines with source record 15918. That test
does not edit the normal library or print credentials/lyric content. Development
tests never automatically use saved production credentials.

Lookup availability depends on DeepSeek search and LRCLIB's recording coverage;
a matching source may not exist. Lyrics on unsupported websites are not scraped,
and model-recalled or generated lyrics are not substituted. This pass does not
claim a new live DeepSeek translation-quality check or audible Gapless check.
Release build, native playback and installed interface results are recorded after
final verification below.

- Formatting/checks and all-target tests pass: 288 passed, 23 intentionally ignored.
- All-target Clippy passes with warnings denied and the same three pre-existing
  lint classes allowed as in the recovery pass.
- Native CoreAudio playback/transport/statistics tests: three passed.
- The native video import/frame/rotation/music-handoff test also passes with a
  real 96×54 thumbnail assertion from an FFmpeg-generated video.
- Focused final tests verify asynchronous lyric persistence leaves playback keys
  available and empty synchronized records fall back to plain lyrics or fail.
- The optimized release build passes. Production audio-core source is unchanged.
