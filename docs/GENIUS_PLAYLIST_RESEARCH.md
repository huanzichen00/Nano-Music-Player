# Genius playlist research and implementation handoff

Research date: 12 October 2026. Scope: Nano Music Player's local music library. This is a design and feasibility assessment; implementation and listening benchmarks are deferred. The user will have another agent build the dummy icon and interface first.

A useful Genius-style feature is feasible at moderate difficulty. The existing queue and background-worker architecture provide a good foundation. Recommendation quality, persistent playlists, and preserving generated order need new work. Reproducing Apple's historical results would also require its unavailable population data. Recommended direction: establish a metadata baseline, then add cached local audio analysis, with an optional external similarity provider later.

## What Apple actually documented

Apple's 2008 launch announcement describes a seed song, a generated playlist drawn from the user's own library, preview, refresh, and save. It explicitly says library information was combined with information from millions of users on Apple's servers, then library-specific results were returned to the computer for offline playlist creation, including on iPods. [Apple's launch announcement](https://www.apple.com/newsroom/2008/09/09Apple-Announces-iTunes-8/)

Apple's Genius disclosure identifies library information, play history, playlists, purchases, aggregated data, and third-party information as possible inputs. That establishes a broader data source than matching genre tags alone. [Apple's Genius disclosure](https://www.apple.com/legal/internet-services/itunes/us/genius.html)

The nano guide's flow is: play a song, reveal Now Playing controls, tap Genius, inspect the list, and save it under the seed artist and title. It reports failure if Genius is disabled, the seed is unrecognized, or fewer than ten similar songs exist in the library. Saved playlists sync back to iTunes; refreshing a saved list replaces it. These are documented device behaviors, not requirements for our own engine. [Apple's nano guide, printed page 24](https://cdsassets.apple.com/live/6GJYWVAV/user/locale/en-gb/ma1624_ipod_nano_2015_user_guide.pdf)

Current Mac documentation still describes Genius Playlists and Genius Shuffle, with periodic Genius-data updates. It distinguishes a chosen seed from a system-chosen seed. [Apple's Music guide](https://support.apple.com/en-ca/guide/music/musbe3694c1b/mac)

## How Apple probably built it

The strongest technical evidence is a 2009 study of iTunes 8 by Barrington, Oda, and Lanckriet. The researchers describe Genius as apparently using collaborative filtering, report Gracenote MusicID for song identification, and distinguish identification from analyzing sound for recommendation. They explicitly say Apple's exact algorithm is secret. In their 185-person study, Genius performed best overall; artist similarity explained much of its performance when names were visible. Their findings support combining musical and artist relationships, but do not establish parity for today's libraries or this player. [Original ISMIR paper](https://archives.ismir.net/ismir2009/paper/000014.pdf)

A plausible engineering reconstruction is:

1. Resolve different local files to shared song identities, using metadata and possibly identification services.
2. Learn song relationships from collections, purchases, playlists, and listening behavior across users. Normalize common-song popularity so ubiquitous tracks do not become everyone's neighbors.
3. Restrict those relationships to the songs available in one user's library and prepare a compact local cache.
4. For a seed, select compatible candidates, limit excessive artist/album repetition, vary the selection for refresh, and return an ordered list.

This is an inference, not a recovered Apple specification. Item-to-item co-occurrence, matrix factorization, and cached neighbor lists are plausible implementations; the sources do not identify the exact model, weights, sequencing rules, or cache format. The confirmed offline behavior supports precomputation, but does not prove a particular cache representation.

The distinction matters: songs that the same listeners enjoy need not sound alike. A local acoustic engine can match tempo and timbre well while missing cultural, artist, or scene relationships. Conversely, it can handle an unknown recording that a catalog-dependent system cannot identify. A single user's play history can personalize a local engine but cannot replace millions of users' collaborative data.

## What this repository already supports

The inspected working tree has these capabilities and gaps:

| Area | Current implementation | Consequence |
| --- | --- | --- |
| Track metadata | `src/library/mod.rs`: path, title, artist, album, genre, track number, duration, artwork, lyrics | Enough for a basic similarity baseline. No stored tempo, key, mood, year, recording ID, ratings, or listening history. |
| Persistence | `src/library/database.rs`: `tracks` contains path-keyed serialized metadata; `lyrics_override` is separate | Add playlist and feature records without rewriting the metadata cache. There is no persisted playlist model yet. |
| Navigation | `src/app/state.rs`, `rows()`: Playlists opens All Songs | The menu is a placeholder for a real collection, rather than an existing Genius subsystem. |
| Playback | `src/audio/queue.rs`: explicit ordered track indices; `State::start` loads one track | Generated local tracks can use the existing audio engine. No new playback backend is necessary. |
| Shuffle | `Queue::unshuffle()` sorts indices numerically | Store a canonical queue order so turning shuffle off restores the generated sequence. |
| Workers | Scanner and audio work use threads and channels; controller polls results | Add analysis/generation workers following the same pattern. Keep decoding and database work off the UI thread. |
| Playback signals | Progress, completion, error, and explicit next/previous actions exist | Useful places to collect listening events later, but no persistent event accounting exists now. |
| Identity | Runtime queue uses vector indices; imports preserve existing indices | Resolve persisted paths to current indices when loading a list. Never persist `usize` indices as track identities. |

Two additional integration issues: ordinary song rows are alphabetically sorted, so a generated playlist needs an explicit ordered view; and `State::start` resets playback position, so starting a mix from an already-playing seed should replace its queue while allowing that track to continue.

## Options and estimated difficulty

These are planning estimates for one engineer familiar with this repository, after the dummy UI is available. They include backend integration and focused verification on macOS. They exclude creating the icon/interface, multi-platform release qualification, provider approval delays, and extensive quality tuning. No implementation was compiled or benchmarked for this research.

| Approach | Feasibility and quality | Difficulty | Effort estimate |
| --- | --- | --- | --- |
| Metadata baseline | Fully offline; useful with good artist/genre tags; weak for mood, unusual tracks, or missing tags | Low to moderate | 3–7 development days for generation, refresh, saved lists, and queue integration |
| Local audio + metadata | Fully offline after indexing; better distinction within an artist and coverage of uncatalogued music; cultural affinity remains limited | Moderate | 2–4 development weeks total, including the baseline, worker/cache integration, and initial listening evaluation |
| Optional external similarity | Adds relationships learned from other listeners; local-library intersection may be sparse | Moderate | Another 1–2 development weeks for matching, caching, failures, and integration, after access/terms are settled |
| Trained audio embeddings | Potentially richer semantic similarity; requires model/runtime selection and heavier distribution/indexing | Moderate to high | Additional several-week investigation and integration; only justified if simpler analysis fails listening tests |
| Own population-scale collaborative service | Technically possible, but requires users, a shared catalog, data collection, training, service operation, and evaluation | Very high | Months of work plus a data-acquisition effort; Apple-like quality has no credible estimate from this repository alone |

The most useful first target is a reliable 25-song local playlist from one seed, refreshable and saveable. A metadata-only version is a baseline to measure improvements against; its output should not be presented as proof that the full recommendation problem is solved.

### Local analysis candidate: bliss-audio

The project is named `bliss-rs` on GitHub; the current documented Rust package inspected is `bliss-audio` 0.13.0. Its API analyzes songs into numeric features and provides distance-based playlist tools. The feature set has 23 dimensions covering tempo, spectral/timbral measures, loudness, and chroma. These are acoustic descriptors, not a trained understanding of mood or artist culture. [Versioned API](https://docs.rs/bliss-audio/0.13.0/bliss_audio/), [feature descriptors](https://docs.rs/bliss-audio/0.13.0/bliss_audio/enum.AnalysisIndex.html)

Use its Symphonia decoder configuration with default features disabled to avoid adding FFmpeg. In 0.13.0, `symphonia-all` does not include ALAC; explicitly include `symphonia-alac` to cover ALAC M4A. Do not assume that playback codec support automatically covers analysis. [Versioned feature flags](https://docs.rs/crate/bliss-audio/0.13.0/features)

The packaged manifest declares `GPL-3.0-only`. Resolve compatibility with the intended app distribution/license before adopting it. The inspected release uses Symphonia 0.6 and optional rusqlite 0.40, whereas this player declares rusqlite 0.37. Use the player's database rather than enabling bliss's optional `library` subsystem; avoid unnecessary SQLite dependency reconciliation. These observations come from the actual 0.13.0 crate archive, not just the repository README. [Crate source](https://docs.rs/crate/bliss-audio/0.13.0/source/), [upstream manifest](https://github.com/Polochon-street/bliss-rs/blob/master/Cargo.toml)

The README still describes an aubio native dependency, but the changelog records its replacement with Rust code in 0.11.3. Check the pinned release source when planning packaging. Older dependency guides and old 20-feature references are not accurate descriptions of the inspected release. [Upstream changelog](https://github.com/Polochon-street/bliss-rs/blob/master/CHANGELOG.md)

If its license does not fit, keep the same provider boundary and implement a smaller feature extractor with suitable dependencies. That requires more signal-processing work and listening evaluation; it is not a drop-in equivalent.

### External similarity and Apple API reuse

Last.fm documents `track.getSimilar` as based on listening data and offers `artist.getSimilar`. Both require an API key but no user-authentication session for these reads. Match returned tracks to local recordings; fall back to similar artists already in the library when exact-track overlap is poor. Neither endpoint's existence establishes its coverage for this user's music. [Track API](https://www.last.fm/api/show/track.getSimilar), [artist API](https://www.last.fm/api/show/artist.getSimilar)

An optional provider needs name/alias matching, live/remix-version safeguards, bounded caching, attribution, timeouts, backoff, and offline fallback. Last.fm's terms specify commercial-use arrangements, attribution, caching rules, and a data-storage cap, so an API key alone is not sufficient release planning. [Provider terms](https://www.last.fm/api/tos)

Apple documents personalized recommendations based on an Apple user's library and purchase history. That is a different contract from passing an arbitrary local seed and obtaining a Genius playlist for this application's files. No supported public Genius-generation endpoint was found in the reviewed documentation; don't make Apple's private service a dependency. [Apple Music recommendations documentation](https://developer.apple.com/documentation/applemusicapi/recommendations)

## Proposed backend architecture

Use a recommendation provider behind an application-owned generator. Start with metadata, add local audio, and let an optional online provider contribute another signal later. The generator should consume a lightweight snapshot of track identities and recommendation fields, not clone artwork and lyrics for every job.

Background preparation:

- Extract features with bounded concurrency, cancellation, progress, and per-file error handling. Indexing must not starve playback or delay importing usable tracks.
- Cache by audio path, audio-file freshness, analysis provider/version, and sampling/preprocessing version. The scanner's existing freshness stamp includes lyric sidecars; reusing it directly would reanalyze music after lyric-only changes.
- Store features separately from large metadata/artwork blobs. Keep failed/pending analyses distinguishable from valid vectors, and reject incompatible feature versions.
- Persist playlists as ordered path identities plus seed, desired size, generator version, and variation token. Paths fit today's database; a future stable track-ID/move-recovery scheme can improve rename handling.
- Use short database transactions, sensible lock handling, and application schema migrations. Analysis and import workers must coexist.

Generation:

1. Resolve a seed and exclude unavailable files and duplicate recordings. Ignore placeholder values such as `Unknown Genre` as similarity evidence.
2. Rank using available acoustic and metadata evidence. Normalize feature scales; don't let a numerous feature group or loudness dominate accidentally. Treat same artist as a useful signal with limits, rather than copying an entire album.
3. Select within a sufficiently relevant candidate pool, with soft artist/album caps and optional recent-play penalties. Reweight available signals when a feature or tag is absent; do not treat missing evidence as a zero similarity measurement.
4. Use bounded randomness for refresh. Keep the seed and relevance threshold, change the variation token, and accept that a small library may have no alternative list.
5. Anchor all candidates to the seed. If adding transition smoothing, apply it within that pool so successive nearest-neighbor steps do not drift into unrelated music.
6. Return an ordered preview and useful insufficiency/failure state. A short list is preferable to filling a quota with unrelated songs. A relaxed mode could allow fewer than ten matches; strict nano behavior can retain its threshold.

Listening history is an optional later signal. Measure actual listened time rather than the final seek position, distinguish deliberate skips from errors and queue replacement, count each playback session once, and avoid database writes on every 100 ms progress event. A session identifier also prevents stale events from a previous load of the same song affecting current accounting.

### Performance expectations to validate

With cached vectors, a seed-to-library scan costs roughly `O(N × d)` distance work, followed by selection and ordering. At 10,000 tracks and 23 dimensions that is about 230,000 feature comparisons; raw `f32` vectors occupy about 0.92 MB before metadata/database overhead. These are arithmetic estimates, not measured latency. A vector database or a full `N × N` distance matrix is unnecessary at this scale.

Decoding and analysis are the expensive first-use step. Measure songs/second, peak memory, CPU contention with playback, cache-resume behavior, and codec failures on the target Mac before promising an indexing duration. Target cached generation below 200 ms for a 10,000-track library, but verify it. Full-song PCM buffering and simultaneous analyses can use much more memory than the feature cache.

## Handoff for the dummy icon/interface agent

These names and states are a proposed integration contract, not code already present in the player.

| UI command | Intended behavior |
| --- | --- |
| `CreateGenius` | Resolve selected song in a song list, otherwise the playing song; capture that seed and request a preview. |
| `RefreshGenius` | Generate another preview using the captured seed and a new variation token. |
| `PlayGenius` | Apply the preview's ordered tracks to the queue. Continue the seed if it is already playing. |
| `SaveGenius` | Persist the current ordered snapshot, initially named from seed artist and title. |

Expose a Genius preview model with seed title/artist, ordered song rows, desired song count, generated song count, and state: `idle`, `generating`, `ready`, `insufficient`, or `error`. Reserve optional library-analysis progress separately; indexing and preview generation are different activities. A future size selector can start at 25 total songs, including the seed; the nano source inspected does not prescribe that count.

For the icon: the reference entry point is the expanded Now Playing overlay. Also allow a list action if desired. Keep the icon's visual design independent of recommendation providers. The future preview needs Refresh, Play, and Save controls, plus room for a brief unavailable/insufficient message. Browsing or refreshing a preview should allow current playback to continue. Generated rows must retain supplied order.

At the backend boundary, a request should carry `seed_path`, `limit`, `variation`, `request_id`, and `library_revision`; a result should return ordered paths and its matching request/revision. This freezes the seed even if playback changes, lets the controller discard stale results, and keeps Slint independent of transient library indices and analysis-library types. Persist the frozen seed alongside any saved list.

The dummy UI can expose these commands and display static preview states. Mark its data as a mock until the engine is connected; avoid silently using All Songs as if it were generated recommendations. No recommendation dependencies or database changes are needed for the interface-only task.

## Later implementation sequence and acceptance checks

1. Add playlist persistence, an ordered playlist screen, canonical queue order, and the proposed commands. Connect a deterministic metadata baseline with seed-preserving refresh.
2. Prototype the selected audio provider on representative MP3, AAC M4A, ALAC M4A, FLAC, and WAV music. Establish packaging compatibility, resource use, and whether its results improve the baseline before adopting it.
3. Add resumable background analysis and hybrid ranking; tune relevance, variety, and failure handling on real libraries.
4. Add listening feedback or an external provider only if evaluation demonstrates a need.

Verify seed stability, local-only membership, no duplicates, ordered save/reload, refresh variation, sparse-library handling, missing files, shuffle/unshuffle, repeat, playback continuation, restart/rescan, and cancellation/stale results. Existing tests cover queue/reducer behavior and use synthetic tone fixtures; those fixtures can verify decoding and invalid-input handling but cannot evaluate music taste.

For quality, compare metadata-only and hybrid playlists for 30–50 seeds across several genres, including atypical songs from familiar artists, multilingual/poorly tagged tracks, live/remix variants, and uncatalogued recordings. Judge both seed fit and full-list listening flow, variety, early skips, coverage, and refresh overlap. Use blinded comparisons where possible. Set quality expectations from those results, rather than claiming Apple parity from algorithm choice.

The decision is to proceed with the interface contract and plan for a local hybrid backend. The largest remaining uncertainties are subjective recommendation quality, initial indexing cost, and the selected dependency's fit with distribution requirements. Apple's proprietary data and exact algorithms remain unavailable.
