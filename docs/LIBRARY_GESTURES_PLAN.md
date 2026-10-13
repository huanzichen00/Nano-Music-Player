# Library back gestures

## Reference behavior

The iPod nano manual (`/Users/huanzichen/Downloads/Study/References/46034.pdf`, printed page 11) says to swipe right to go to the previous screen and swipe left to go to the next screen when available. The library browser should apply the right-swipe Back behavior to Albums, Artists, Songs, playlists, and nested library levels.

## Recognition

- Feed horizontal trackpad scroll events into a short-lived accumulator. Positive/rightward horizontal movement can invoke the existing Back action after a deliberate threshold.
- Reset accumulation after a long pause, a vertical-dominant gesture, completion, or a guard that blocks navigation.
- For pointer/touch, track press through successive move frames and inspect the final displacement on release. Require clearly dominant rightward travel; cancel on interruption.
- Preserve ordinary vertical list scrolling, selection, playback state, queue and position. Ignore gestures while text editing, modal UI, or outgoing navigation animation owns input.
- Route through the existing navigation Back action so each level restores its parent selection and scroll position consistently.

## Independent recognizer harness

`tests/isolated-library-gestures` exercises accumulated deltas, timeout/reset, direction and diagonal rejection, small-motion rejection, and multi-frame pointer lifecycle without changing the parent's active UI/controller work. Run it with a separate target directory, for example:

```sh
CARGO_TARGET_DIR=/tmp/rust-music-player-gesture-target cargo test --manifest-path tests/isolated-library-gestures/Cargo.toml
```

## Integration follow-up

Once the parent releases the shared UI/controller files, wire this recognition into the library root and list levels, reusing the existing Now Playing navigation guards and actions. Add Slint UI coverage for Albums, Artists, Songs, playlists, parent selection restoration, and vertical-scroll preservation. Run formatting, the isolated and relevant project tests, strict lint, and a native visual check without launching/repackaging the user's app.
