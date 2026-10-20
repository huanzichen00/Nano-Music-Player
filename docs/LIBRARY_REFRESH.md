# Music library refresh

The desktop player uses finite background scans rather than persistent filesystem
watchers. Both former monitoring modules, their controller queues, the isolated
watcher prototype and the `notify` dependency have been removed.

## User behavior

- Startup first publishes cached metadata and cover previews. It then refreshes
  imported folders in the background, discovering new files and metadata changes.
- **File → Refresh Library** and **Settings → Refresh Library** run the same scan.
  No folder picker is needed. Import and refresh cannot run concurrently, and
  repeated refresh requests do not start overlapping jobs.
- Between scans, changes made outside the player remain unseen until the next
  explicit refresh or restart. Playback still opens and validates sources on its
  audio worker, reporting a playback error if a requested file cannot be opened.
- Unchanged files reuse cached metadata using audio/lyric-sidecar modification
  stamps. No per-file monitoring handles or subscriptions survive the scan.
- Missing files are hidden and evicted only when a root traversal is complete and
  the imported root remains available. Disconnected volumes and incomplete scans
  retain cached songs. Restoring a same-path file makes it discoverable again.
- Refresh retains stable queue slots and selections. A valid active track keeps
  playing; confirmed absence of that active file stops it. Playlist path order,
  manual lyric associations, ratings and listening history remain stored.
- Moves to a different path are handled as an old-path removal and new-path
  addition. Automatic rename detection/history migration has been retired.

Imported roots, including empty folders, persist as `library_roots` in settings.
Older `monitor_roots` values are accepted as an alias. The obsolete
`monitor_deleted_files` setting is ignored on input and omitted on save. Legacy
libraries without saved roots use cached track parent folders on refresh.

Videos retain their separate behavior: a missing video row is removed only when
playback is requested, while watch progress and subtitle preferences are retained.

## Verification

Coverage includes startup discovery, settings/menu refresh, reentry/import guards,
unchanged active playback, stable queue/selection, manual lyric edits during a
scan, deletion/restoration, and unavailable-root safety. The macOS descriptor
regression refreshes 512 generated fixture paths with a 256-descriptor process
limit, then opens 32 additional files and decodes audio successfully.

Final verification on 2026-10-20: `cargo test --all-targets --no-fail-fast`
passed 406 tests with 27 intentionally ignored; `cargo fmt --all -- --check`,
`cargo clippy --all-targets -- -D warnings` and `cargo check --features mobile`
also passed. Refresh completion preserves newer user-operation errors, and
removing previous music cannot cancel the active podcast/video handoff.
