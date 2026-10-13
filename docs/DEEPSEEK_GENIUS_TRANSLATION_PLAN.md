# DeepSeek Genius and lyric translation plan

Status: integrated locally; no live provider call has been made. Genius Create/Refresh/Play/Save now use the frozen local library snapshot, validate request-local IDs, show ordered local rows, apply a canonical ordered queue, and save through existing playlist transactions. Translation target language persists independently of UI language; explicit translate/regenerate actions fill a durable exact-source cache and paired lyric rows. Shared app/video ownership was preserved while integrating.

## Scope and product rules

Implement Genius-style local playlist generation and lyric translation using the existing DeepSeek settings, credentials, model, and JSON chat client. Requests occur only after an explicit user action. This is a local-library experience powered by a provider response; it does not reproduce or claim Apple's proprietary Genius algorithm or recommendation quality.

Preserve existing music playback, romaji, manual playlists, settings, fonts, cursor and Back gestures, window geometry, and video integration. Do not modify source lyrics, embedded metadata, or audio files. Translation language is an independent preference, not derived from the UI language.

The current nano reference flow is: start from a song, open Genius, inspect an ordered playlist, then save it under a seed-derived name. See the [Apple 2015 iPod nano guide](https://cdsassets.apple.com/live/6GJYWVAV/user/locale/en-gb/ma1624_ipod_nano_2015_user_guide.pdf) and [Genius research](GENIUS_PLAYLIST_RESEARCH.md). The player may adapt that interaction and should explain sparse-library/provider outcomes without substituting All Songs.

## Existing foundations

- `src/ai.rs` provides bounded JSON chat responses, timeout/status handling, DeepSeek model settings, environment-key precedence, a redacted `Debug`, and private (`0600`) key-file creation. Reuse this client and do not duplicate or expose credentials.
- The client already has local HTTP fixture tests; extend the pattern with deterministic fixtures. Development and UI verification must never make live provider calls.
- Tracks have path identity plus title/artist/album/genre and lyrics. The ordered queue uses runtime indices, so persisted or asynchronous results must use paths and resolve them against the current library at application time.
- Lyrics are represented as timed or plain lines. Romaji currently aligns by original lyric index; translations must use the same invariant.
- Manual playlists already have persistent ordered path storage. Genius Save should call that storage transactionally rather than introduce a second playlist format.
- The Genius preview and DeepSeek configuration UI are currently present, but Genius actions are mock states. UI labels are translated in English, Simplified Chinese, and Japanese.

## Contracts

### Frozen local-library request

At Create/Refresh, capture an immutable request containing a request ID, seed path, seed metadata, library revision, variation token, desired result limit, and a lightweight snapshot of candidate path/title/artist/album/genre/duration. Never send file contents, lyrics, artwork, or credentials in the prompt. Exclude unavailable paths before dispatch. Keep the snapshot alive until that request completes or is cancelled.

Prompt for recommendations grounded only in that snapshot and require one JSON object containing an ordered list of candidate IDs (the IDs are snapshot-local opaque values mapped to paths by the caller). Validate every ID against the exact request snapshot; reject unknown IDs, duplicates, the seed repeated as a recommendation, and paths that have become unavailable. Do not accept provider-authored paths. Reject malformed, truncated, empty, or incomplete responses as an error; do not silently fall back to All Songs or partially persist an invalid result. A valid but short result may be shown as sparse/insufficient, with an honest message.

Results carry request ID, seed path, library revision, variation, and ordered path identities. Apply only if request ID and seed still match the visible preview and the library revision remains current; otherwise discard as stale. Playback may change while generating without changing the frozen seed. Applying Play builds the queue from the preview order. If the seed is already playing, preserve its playback position while replacing the queue. Shuffle on may randomize playback; turning shuffle off restores the canonical generated order.

Refresh retains the seed and changes only the variation token. Preview rows preserve provider order. Save snapshots the current preview order and seed, derives a default name from seed artist/title, and inserts all playlist rows in one transaction through existing storage. Failed Save leaves the preview intact and retryable. Cancellation never writes.

### Translation request and cache

Persist a target-language code independently of `Settings.language`; support at least `en`, `zh-Hans`, and `ja`. Generate only from an explicit per-track Translate/Regenerate command, using the current lyrics and DeepSeek credentials/model. Request line-for-line translations, retain source ordering and one output item per source line, and do not permit provider output to replace original lyrics.

The translation key is `(track identity, exact source snapshot, target language, provider/model/prompt version)`. Source snapshot includes timed/plain mode and exact original text and timestamps. Cache validated complete results durably so they survive restart. A result includes the source fingerprint, target code, and provider/prompt version; apply it only if those still match the current track and preferences. Lyrics or target changes invalidate presentation immediately and prevent late results from being attached. Track identity should use the existing path identity for this milestone.

Render original and translated text as paired lines. Keep original timestamps and line indices authoritative for active highlighting, seek, and scroll alignment. Translation never changes `Lyrics`, sidecars, tags, or imported metadata. Romaji can remain visible with translation; row measurement must include all visible wrapped text and retain the existing measured-height scroll/highlight behavior. Show/hide is presentation state; Remove deletes the cached translation for the current key. Regenerate explicitly requests a fresh result and replaces the cache only after full validation.

## Phases and acceptance criteria

### Phase 0 — Freeze integration contracts (current delegated phase)

- Keep work inside this plan and new isolated Genius/translation backend files while shared video/UI ownership remains active.
- Do not edit Cargo manifests/lockfiles, AI client, database, controller/state/action/navigation/queue, settings, Slint UI, i18n, shared tests, or the shared implementation plan.
- Write down request identity, validation, cancellation/staleness, persistence, and UI integration decisions before integrating.

**Acceptance:** plan exists; it accurately reflects the existing DeepSeek/lyrics/playlist contracts; no protected shared file has been edited and no live API request has occurred. The isolated boundary validates ordered Genius candidate IDs against a frozen request, rejects duplicate/unknown/seed IDs and stale request/library revisions, validates complete line-for-line translations, and provides an atomic private-file cache prototype keyed by exact source, track, target, and provider version. Both domains implement the existing `DeepSeek::json` provider interface, have nonblocking bounded admission gates and cancellation tokens, and the standalone harness compiles the actual `src/ai.rs`. Its existing local TCP HTTP fixtures verify JSON mode, truncated/oversized responses, HTTP errors, and private credential handling. Eleven offline tests pass in `tests/isolated-features/`. Cancellation suppresses results before/after a blocking provider call; it cannot interrupt an already-running `reqwest::blocking` call, which remains bounded by the existing client timeout. This phase does not add application worker integration.

### Phase 1 — Isolated provider-domain logic and fixtures

- Add independently testable request/response models and validators for Genius candidate IDs and translation line counts/order.
- Define stable source fingerprints and provider/prompt version fields without changing existing data models yet.
- Build local HTTP fixtures for success, HTTP errors, invalid JSON, incomplete/truncated output, unknown IDs, duplicate IDs, short results, timeout/cancellation, and stale revision handling. Never include a real key in fixtures or logs.
- Keep network request size, concurrency, response size, and timeouts bounded. Ensure cancellation prevents late application and persistence.

**Acceptance:** all fixture cases are deterministic and offline; invalid/incomplete responses yield typed errors and no writes; credentials are redacted; no unbounded task or response path exists.

### Phase 2 — Genius backend and playlist integration

- Snapshot the local library without artwork/lyrics and generate from an explicit frozen seed.
- Validate provider IDs against the snapshot and current filesystem; deduplicate and retain ordered path identities.
- Implement Create, Refresh, ordered preview data, Play queue application, and transactional Save using existing playlist storage.
- Preserve seed position and canonical order through queue replacement and shuffle toggles. Report sparse library, cancellation, stale results, provider errors, and retry state truthfully.

**Acceptance:** tests cover empty/sparse library, missing files, duplicates, unknown IDs, seed change/playback change during requests, stale library revision, refresh variation with stable seed, ordered save/reload, failed-save retry, Play from preview order, currently playing seed position, shuffle on/off, cancellation, and retry. All Songs is never substituted.

### Phase 3 — Translation storage, preference, and display integration

- Add and validate an independent persisted language selection (`en`, `zh-Hans`, `ja`) with safe migration/default behavior.
- Persist translations by exact source snapshot, target, track identity, and provider/prompt version; use atomic/transactional writes.
- Add explicit Translate, Regenerate, Show/Hide, and Remove actions. Do not generate on track selection or startup.
- Integrate paired original/translated rows with existing lyric active index, scroll mapping, and romaji visibility. Measure complete wrapped row height.
- Translate newly added UI labels in English, Simplified Chinese, and Japanese.

**Acceptance:** tests cover language persistence independent of UI locale, timed and plain lyrics, empty lyrics, exact-source invalidation, target changes during a request, stale responses, restart/cache hits, regenerate replacement, remove, show/hide, paired-line count/order, timestamp/highlight/scroll stability, wrapped translated text, and coexistence with romaji. Source lyric data remains byte-for-byte unchanged.

### Phase 4 — Shared app integration and release verification

- Shared ownership was released after video commit `5e0904e`; the audio handoff, landscape orientation, portrait-dialog rule, and existing gestures were reviewed and retained.
- The former Genius example rows were replaced with local generated results and Create/Refresh/Play/Save wiring. Translation target selection is available in Playback independently of interface language, with Translate/Regenerate/Cancel/Show-Hide/Remove actions.
- Existing DeepSeek local HTTP fixtures and the isolated request/cache fixtures pass without a live call. The lyric layout now pairs translation and source and includes both plus romaji in measured row height.
- Full integrated tests and strict all-target Clippy pass after the feature changes. The primary chat owns packaging and normal-app relaunch; do not overwrite its bundle.

**Acceptance:** `cargo test --all-targets` and `cargo clippy --all-targets -- -D warnings` pass; native video and manual library profiles remain intentionally ignored. The UI integration test exposed and then verified the Genius open callback reentrancy fix. Cancellation prevents result application before and after the existing blocking HTTP call; in-flight socket I/O itself is bounded by the existing DeepSeek timeout and cannot be interrupted. No live provider request is part of development verification. No claim of Apple's algorithm or quality parity is made.

## Integration decisions to preserve

- Provider IDs are request-local tokens, not paths and not persistent identities.
- File paths are stable identities for this milestone; runtime indices are resolved only immediately before queue application.
- A frozen Genius seed survives playback changes, while a changed seed or library revision invalidates old results.
- Translation target and UI language are separate settings.
- Original lyrics and timestamps remain the sole source of truth; translations are a keyed overlay.
- Only complete validated provider results are cacheable/persistable. A failure or cancellation leaves the previous valid preview/translation available where applicable.
- The provider must not be described as Apple's Genius service. Recommendation quality is bounded by local metadata and provider responses and must be evaluated empirically.
