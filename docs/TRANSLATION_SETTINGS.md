# iPod translation settings and online lyrics

## Automatic resolution of conflicting lookup results, 2026-10-21

Conflicting search results now continue through LRCLIB's exact recording
signature and, if still needed and configured, DeepSeek discovery. The previous
ambiguity error stopped both stages and told the user to import lyrics manually.
Candidate conflicts now respect the same album, synchronization and guest-credit
priorities as selection. A duration match more than half a second closer also
outranks a weaker candidate; smaller differences remain tied to allow for rounded
metadata. All candidates still require compatible recording versions and a
duration within two seconds. Only unresolved conflicts after the fallback stages
report that the recording's lyrics could not be verified; those misses use the
existing short negative cache.

The empty-lyric overlay shows no find-key hint while searching or saving. After
a failed lookup it offers a localized retry hint instead of repeating the
initial find prompt. English, Chinese and Japanese overlay snapshots are covered
by the desktop translation-settings regression.

Offline HTTP regressions cover conflicting plain/timed results, meaningful versus
rounded duration differences, exact-signature resolution without a model call,
discovery after an unresolved conflict, rejection of a discovered live substitute,
and caching after fallback failure. These checks use synthetic lyrics and do not
make live provider requests or modify the user's library.

## Existing timestamps and recording versions, 2026-10-19

The latest Firefly download was LRCLIB record 12038413: its synchronized field
is null and its 20 plain lines have no timing. The file metadata includes the
title, Blackfield, Blackfield IV and 166.909 seconds; nothing was missing from
those matching fields. LRCLIB also has record 4038489 with 22 timed lines, the
same album and a 167-second duration, but without the featured credit in its
title. Same-album candidates now prefer real timing before guest-credit
completeness, while conflicting guest identities remain rejected.

When metadata lookup yields only plain text, configured DeepSeek discovery now
continues searching for an existing synchronized LRCLIB source. Its query
includes the title, artist, album and precise duration and explicitly asks it to
verify studio/live, venue/date and other recording versions. It never supplies
generated lyric timing. Only native search source URLs are accepted; retrieved
records still pass local recording/version and duration validation. Live venue
or year mismatches and English/Chinese/Japanese/Korean live-album labels are
covered by offline regressions. Unlabelled or incorrect local metadata cannot
identify an actual live recording by listening; matching cannot guarantee that
case from text alone.

F can retry an already downloaded plain-text association and upgrade it to a
verified timed source. The request captures the original lyrics; the storage
worker compares that snapshot and the remote association before its conditional
update. Synchronized lyrics and local manual imports/removals are protected.
If no timed source is found, the existing plain text remains. Discovery service
errors also preserve usable text retrieved earlier in the same lookup.

The offline lookup, storage and desktop regressions pass. A metadata-only live
check selected record 4038489 and parsed 22 timed lines without DeepSeek. Later
live verification received LRCLIB HTTP 503, so the user's saved Firefly override
was not replaced during this pass. No live DeepSeek request or audible alignment
verification is claimed. After updating/restarting the desktop app, F can retry
that downloaded plain-text song when the source service is available.

The optimized desktop build and the shared mobile-library compatibility check
passed. The signed desktop bundle is installed at
`~/Applications/Nano Music Player.app`; its previous version is retained at
`~/Applications/.nano-lyric-timing-backup-2xmhjjw_/Nano Music Player.app`.
The running session was left open and needs a restart to load this update.

## Desktop shortcuts, 2026-10-19

Desktop Settings also has a direct Translate Current Song entry. Plain F on
Now Playing (including its volume selection) invokes Find Lyrics Online for the
playing song without enabling or generating translation. Missing-lyric pages
show the lookup status persistently and a localized F hint when F has its default
binding. Existing lyrics, custom F assignments, list letter jumps, text editing
and modal keyboard guards are preserved. Find Lyrics Online is independently
customizable in Keyboard Shortcuts.

Desktop verification, 2026-10-19: 318 tests passed with 8 intentional opt-in
ignores across the library unit tests and the core, translation settings, settings
navigation, reference library/UI, modal navigation, lyrics associations, paired
lyric rendering and main UI regressions. Actual F key events verify the playing
song target, unchanged queue and translation visibility, existing-lyric
preservation, and opening the lyric overlay from the cover. Dispatcher coverage
checks repeat suppression, custom F assignments, list prefixes, search modifiers,
text inputs and modal guards. Lookup failures use deliberately invalid test
credentials; successful lookup/storage uses existing offline fixtures. This pass
does not make a live provider request. Rendered missing-lyric/error pages and
English/Chinese/Japanese Settings menus were inspected; all seven desktop root
entries fit the viewport. Formatting and whitespace checks pass.

The optimized desktop build passed and was installed at
`~/Applications/Nano Music Player.app` with a verified ad-hoc bundle signature.
The previous app is retained at
`~/Applications/.nano-desktop-lyrics-backup-18aj5axe/Nano Music Player.app`.
The existing running session remains open; restarting loads this update.

Settings → Lyrics exposes Show Translation, Translate To and Japanese Readings.
Translate Current Song opens Translate, Regenerate, Cancel and Remove for the
playing song; AI credentials live in Settings → AI Services. The native menu
and device rows dispatch the same typed commands. Translation language is saved
independently of interface language. Translate reuses a valid exact-source cache;
Regenerate makes a new request. Show Translation and plain T control the same
persisted, optional display mode (Off by default). Original lyrics always remain
visible. When On, each translation appears on a separate line underneath its
original, and romaji is hidden. Turning it Off restores the existing R/romaji
preference. Enabling with T reuses an exact-source cache, or starts translation
when it is missing (first finding source lyrics online if necessary). The first
T press on a new song without translation also starts generation if the display
preference was inherited from another song; the next press hides it, including
after a failed request. Show
Translation in Settings remains a display-only toggle. Turning T off never
starts a request, and a pending result cannot override a later T-off
choice. T uses a free default chord; migration preserves custom T assignments and
current-format explicit unassignment.

Settings → Lyrics → Find / Edit Lyrics opens a song chooser and a dedicated lookup page.
Choosing a song does not start it or change the current queue. The page shows the
selected title/artist, persistent request status, Find/Cancel, lyric management
and DeepSeek settings. It also opens from Lyrics for Track. Explicit Find can
operate on a song that is not playing; the Translate continuation stays tied to
the playing song.

Use Automatic Lyrics clears an imported/downloaded lyric association or removal
and rediscovers embedded audio tags or nearby `.lrc`/`.txt` files. It does not make
an online request; its row now explains these file sources.

Manual imports and synchronized lyrics take precedence. If Translate is explicitly requested for a
song without lyrics, it runs the following stages off the UI/audio path:

1. Search LRCLIB directly using normalized title and artist fields. Compare all
   returned candidates, preserving recording words such as Live, Acoustic, Remix,
   Clean and specific mix years. Normalize Unicode typography, punctuation and
   guest credits in either title or artist. Album release-format/encoding/edition
   labels can differ; matching albums outrank other releases. A different live or
   acoustic album is rejected. Title, primary artist and duration (within two
   seconds) remain required. Conflicting lyric text among equally useful candidates
   continues to the exact recording signature and optional discovery instead of
   ending the lookup. Only unresolved conflicts produce a verification failure.
2. If no usable candidate exists, try LRCLIB's exact signature endpoint. Only then
   attempt native DeepSeek discovery when credentials are configured. This uses
   `web_search_20250305` with one tool use at the Anthropic-compatible Messages
   endpoint. Retrieve at most three structured LRCLIB record IDs and apply the
   same validation; model prose and arbitrary search-result hosts are never fetched.
   Finding lyrics itself needs no DeepSeek key. Instrumental, empty and invalidly
   timed records are rejected. Preserve real synchronized lyrics or plain text
   without invented timestamps. Search arrays are bounded to 2 MiB/200 entries;
   individual records retain the 256 KiB limit.
3. Recheck the requested song, metadata, empty source and cancellation state before
   saving (and the playing song for a Translate continuation). The existing SQLite lyric association stores content and source URL on
   the existing lyric-edit worker. An atomic insert cannot replace a newer manual
   import or a removal. The UI publishes lyrics only after persistence succeeds.
4. If this was Translate, invoke the existing validated translation path after
   the lyrics update. Find Lyrics Online stops after storing the lyrics.

Cancel and target changes stop the pending lookup/translation continuation.
Manual lyric edits cancel a pending lookup. A currently executing network request
can finish within its timeout (DeepSeek 120 seconds, each LRCLIB request 20 seconds), but a
cancelled or stale response cannot start persistence or translation. Only one
lookup is admitted at a time, including while its cancelled HTTP call is exiting.
Already accepted lyric persistence can finish after cancellation; Cancel stops
translation of that result. No network request starts on app launch or track
selection, and no audio files are modified.

LRCLIB requests share a 1.5-second minimum spacing across songs and endpoints.
Waiting occurs on the lyric worker and remains cancellable. LRCLIB permits one
retry per lookup only when a 429 requests a cooldown of at most five seconds;
longer or repeated limits end the lookup. Both providers share cooldowns across their own
endpoints: honor `Retry-After` seconds or HTTP dates, default to 30 seconds when
absent, and reject new requests locally during that interval. DeepSeek cooldowns
also cover translation, romaji and Genius. Provider origins and cooldown seconds
are logged without credentials or lyric text. Repeated unsuccessful metadata
lookups are cached for 60 seconds (at most 128 entries); edits bypass that cache.
DeepSeek's `max_uses_exceeded` is reported as a per-request tool budget, separately
from HTTP 429 or `rate_limit_exceeded`. Valid native results remain usable if the
model subsequently exhausts its tool or output-token budget.

The rate-limit investigation reproduced an LRCLIB HTTP 429 with `Retry-After: 1`.
This is separate from DeepSeek's model-account concurrency limits or native
tool-use budget. No DeepSeek HTTP 429 was reproduced. A six-song metadata-only
live check passed after this correction, including recovery from the short
LRCLIB cooldown: Blackfield's `Firefly (feat. Brett Anderson)` and `Pain (Live)`,
Fishmans' `Shiawasemono`, Capstan's `The Death of an Illusion` and `Wax Poetic`,
and Genesis' `The Battle of Epping Forest (2007 Stereo Mix)`. This check made no
DeepSeek requests and wrote no lyrics to the user's library.

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

Lookup availability depends on LRCLIB's recording coverage; DeepSeek search failure
does not block its verified metadata lookup. A matching source may not exist.
Lyrics on unsupported websites are not scraped,
and model-recalled or generated lyrics are not substituted. This pass does not
claim a new live DeepSeek translation-quality check or audible Gapless check.
Release build, native playback and installed interface results are recorded after
final verification below.

- Formatting/checks and all-target tests pass: 298 passed, 25 intentionally ignored
  after the layout, thumbnail and T-generation corrections.
- All-target Clippy passes with warnings denied and the same three pre-existing
  lint classes allowed as in the recovery pass.
- Native CoreAudio playback/transport/statistics/translation-shortcut tests:
  four passed. The new test starts a real worker with deliberately invalid test
  credentials, observes its failure and T-off, and verifies native audio continues.
- The native video import/frame/rotation/music-handoff test also passes with a
  real 96×54 thumbnail assertion from an FFmpeg-generated video.
- Focused final tests verify asynchronous lyric persistence leaves playback keys
  available and empty synchronized records fall back to plain lyrics or fail.
- The optimized release build passes. Production audio-core source is unchanged.
- A rendered regression checks that original and translated glyphs occupy separate
  vertical regions, wrapped text contributes its full height, missing translations
  preserve originals, and T restores both enabled and disabled romaji preferences.
  On 2026-10-15 the installed app also displayed paired Japanese originals and
  Chinese translations. T-off restored saved romaji; T-on restored the cached
  translation with "Translation ready". See
  [installed verification](UI_REFERENCE_VERIFICATION.md) for the actual GUI results.

The T follow-up also checks both initially disabled and inherited-enabled display
preferences, cached reuse without HTTP, the missing-source lookup path, unchanged
romaji preference, single-worker admission and late-result visibility. It does
not make a new live provider request or claim a new translation-quality check.

## Wax Poetic lookup correction, 2026-10-16

Reproduced the failure for Capstan's `Wax Poetic`, tagged as `Cultural Divide - EP`
with a duration of 222 seconds. LRCLIB's album-filtered request returned 404,
while the metadata request without the album filter returned record 10460169
under `Cultural Divide`. A separate live attempt also encountered a native
DeepSeek web-search failure. The lookup now tolerates EP/Single album suffixes,
retries missing or mismatched search-result records, and continues with verified
LRCLIB metadata lookup if DeepSeek search fails.

The opt-in live lookup passed with the unchanged track metadata, retrieving 30
synchronized lines from record 10460169. It did not modify the user's library.
Offline tests cover both causes, wrong-record rejection, cancellation between
requests, and immediate stopping on rate limits and service errors. Library,
lyric-association and translation-settings checks passed: 226 tests, 8 ignored.
The all-target run encountered the existing untracked `monitor_file_budget` test's
file-descriptor exhaustion failure, outside this lyric change.
Formatting and whitespace checks passed. All-target Clippy passed with the same
three documented baseline lint allowances (`collapsible_if`, `needless_borrow`,
`field_reassign_with_default`). The optimized macOS bundle was built and its
executable installed at `~/Applications/Nano Music Player.app`; the installed and
built SHA-256 hashes match. The previous executable is retained in
`~/Applications/.nano-lyrics-fix-backup-bz2jrwmn/`. The running session was left
open; restarting the app loads this fix.

## General lookup and rate-limit verification, 2026-10-16

The general lookup policy above supersedes the initial Wax Poetic-only fallback.
The metadata-only live test passed all six representative songs without a
DeepSeek request, including recovery after an actual LRCLIB HTTP 429 with a
one-second `Retry-After`. Normalization, candidate ranking, conflicting-version
rejection, cancellation, cached misses, distinct source IDs and bounded cooldown
retries have offline regressions.

The all-target regression run passed 374 tests with 27 intentionally ignored;
only the pre-existing untracked file-monitor budget regression was filtered out.
The final focused lookup run passed 16 tests, and the isolated provider/domain
harness passed 19. Formatting, whitespace checks and all-target Clippy pass with
the same three documented baseline lint allowances. The optimized release was
installed at `~/Applications/Nano Music Player.app`, with matching built/installed
SHA-256 `e9beacddd376e587cfeaa6cca732b4c0844834c954e2cb446633108a1b0650e4`.
The previous executable is retained in
`~/Applications/.nano-lyrics-general-backup-4nek9pvv/`. The running session remains
open and needs a restart to load this update.
