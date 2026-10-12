# Library cover performance

Albums and Artists use the existing 58-unit rows and representative-cover rules.
Opening these screens must not decode source artwork on the UI thread.

`app::thumbnails` wraps the library scanner's background load/import results.
Before publishing tracks, it prepares 96-pixel previews, preserving aspect ratio,
and stores them in `cover-previews-v1` beside `library.sqlite3`. These generated
files are PNGs capped at 64 KiB. Original artwork and music remain untouched.
Identical embedded covers are decoded once and share one Slint image. Corrupt,
oversized, or missing preview files regenerate; cache write failures still allow
an in-memory preview. The first launch prepares this cache in the background;
later launches read the small previews.

The controller seeds previews by track path after library merging, so stable
playback indexes survive imports. All decoded library covers are ready on the
first Albums/Artists frame, and Now Playing can reuse a song's preview immediately.
Unreadable source art continues to use the existing music-note fallback.

Two workers prepare sharper thumbnails only for the visible rows and one row of
overscan. New viewports replace obsolete queued work. Decode results carry a
library generation, so an import invalidates stale results. Sharpening updates
individual rows in the existing model and preserves selection and scroll position.
The larger-image cache holds at most 64 entries / 32 MiB. The small ready-preview
set uses at most 36 KiB of pixels per unique cover, shared across matching tracks.

Group metadata is collected in one traversal rather than rescanning the library
for every album or artist. Ordering remains case folded with original-text ties;
representative covers keep the album/title/path ordering and skip missing art.

## Verification

`cargo test --test album_entry_latency` checks that startup publishes immediate
covers, sharpening preserves the model and navigation, and a first song reuses
its preview. Thumbnail unit tests cover worker scheduling, bounds, invalidation,
shared previews, path remapping, and corrupt-cache recovery. Existing library
ordering and keyboard/UI tests verify representative artwork and geometry.

The ignored profile uses a read-only source database and a temporary working
library. It measures grouping, screen entry, and software-rendered first frame;
it does not start playback or change the user's database:

```sh
NANO_PROFILE_LIBRARY=/path/to/library.sqlite3 cargo test --test album_entry_latency -- --ignored --nocapture
```

Debug-build measurements on the development library (42 songs, six albums), with
a headless software renderer:

| Measurement | Before | After |
| --- | ---: | ---: |
| Cold screen entry | 8,465 ms | 0.13–0.41 ms |
| Cold first-frame rendering | 55.83 ms | 44.60–45.63 ms |
| Warm screen entry | 0.17 ms | 0.08–0.09 ms |

All six covers are present before the first frame. Their generated files total
132.7 KiB. The startup fixture (including writing a temporary database, refreshing
source metadata, and preparing its first preview cache) took about 12.3 seconds
on this library; that work happens in the background before publishing tracks.
These timings measure the test renderer rather than live display presentation.

The ignored large-library profile builds 10,000 album rows from 10,000 tracks in
25.44 ms, 100 artist rows in 3.04 ms, and one artist's 100 albums in 0.32 ms.
