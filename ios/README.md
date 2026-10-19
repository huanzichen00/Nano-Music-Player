# Nano Music Player for iOS

The iOS edition shares the Rust audio engine, metadata reader, lyrics parser,
queue, navigation and translation catalog with the desktop application. It uses
the original desktop nano HomeView, LibraryView, NowPlayingView and device-shell
asset, wrapped in a mobile safe-area layout. A small Objective-C bridge provides
UIKit and system audio integration.

The mobile UI embeds Noto Sans CJK SC for menus, song metadata and lyrics. This
avoids relying on iOS's private Chinese UI font, whose outlines Slint's font
loader cannot read. The full face also covers traditional Chinese, Japanese and
Korean; no network font download is needed. The build script includes its SIL
Open Font License in the app bundle's `licenses` directory. See
[`ui/fonts/noto-sans-cjk/README.md`](../ui/fonts/noto-sans-cjk/README.md).

Development scope and milestones: [iOS 开发方案](../docs/IOS_DEVELOPMENT_PLAN.md).

For a complete desktop library transfer, [USB library sync](../docs/IPHONE_SYNC.md)
copies music, playlists, lyrics/romaji/translations, ratings and listening data.
It merges records, preserves phone edits on conflicts and keeps phone settings.

## Mobile features

The default language is English. Open **Settings → Language** to choose English,
Chinese or Japanese; an existing saved choice is preserved on updates. Mobile
settings reuse the desktop catalog and persisted preferences, with window and
keyboard-shortcut settings omitted.

Playlists support creation, renaming, track selection, ordering, saving and
confirmed deletion. Lyrics support online lookup, plain-text/LRC editing,
one-tap copying, translation and romaji. DeepSeek settings and Genius reuse the
desktop services; a configured API key and network access are required for AI
requests. Ratings, listening statistics, smart playlists, ReplayGain and Gapless
playback are included. Editors avoid the software keyboard and expose Done,
Save and Cancel actions. The app icon uses the blue iPod body.

In a music list, swipe left on a row with a chevron to enter that item and swipe
right to return. Ordinary song rows play on a tap; a horizontal swipe does not
play them. Mobile list taps and pans share an immediate input handler, with
press feedback, direction locking and a rebound for incomplete swipes.

## Desktop preview

No iOS SDK is needed to compile and preview the mobile interface:

```sh
cargo run --features mobile -- --config-dir ./target/ios-preview/library
cargo test --locked --features mobile --lib
```

The preview has its own library. Imported audio is copied into its `Music`
directory. Default `cargo run` still starts the desktop application. Use `--lib`
for mobile tests because the existing integration test targets exercise the
desktop interface.

## Build for iPhone or iPad

Requirements: macOS, full Xcode with the iOS SDK, Rust 1.92+, and XcodeGen. The
initial deployment target is iOS 15.0. Install and select Xcode before building;
Command Line Tools alone is insufficient.

```sh
brew install xcodegen
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
./scripts/check-ios-env.sh
./scripts/generate-ios-project.sh
open ios/NanoMusicPlayer.xcodeproj
```

In Xcode choose the `NanoMusicPlayer` scheme and a simulator, then Run. For a
physical device, select your development team under Signing & Capabilities and
set a bundle identifier owned by your team if needed. No team identifier or
signing credentials are stored in the repository.

Unsigned simulator build from the repository root:

```sh
xcodebuild \
  -project ios/NanoMusicPlayer.xcodeproj \
  -scheme NanoMusicPlayer \
  -configuration Debug \
  -sdk iphonesimulator \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath target/ios/DerivedData \
  CODE_SIGNING_ALLOWED=NO build
```

For an Intel simulator install `x86_64-apple-ios` as well. The Xcode build phase
maps platform and architecture to the Rust target, builds the executable with
the locked dependencies, copies it into the app, and generates a dSYM when
requested. Release/Archive builds retain debug symbols. Generated `.xcodeproj`
and derived data are ignored; `project.yml` and `Info.plist` are the sources of
truth. Re-run the generator after changing project settings.

The optional **iOS simulator** GitHub Actions workflow performs an unsigned
simulator build when manually dispatched. It does not publish or install an app.

## Import and system playback

For computer imports, keep the phone and computer on the same Wi-Fi network,
open **Computer Transfer** from Music or Settings and enable it. Tap **Copy
Address**, then open that full address in a computer browser. The temporary
pairing token is included after `#`.
Drag in several audio files or use the file selector; each file shows upload
progress and an import result. The page also lists the current library.

The service starts only when enabled. You can briefly switch apps to share the
copied address. UIKit grants finite background time; once it expires, incoming
requests pause and incomplete uploads are canceled. Returning to the app resumes
the same listener, address and pairing token. Keep the app open for large uploads.
Manually stopping and re-enabling transfer creates a new pairing token. This
version supports local uploads and library browsing;
it does not synchronize deletions or publish files to a cloud service.

Use the in-app import control to choose audio from Files. The app copies selected
documents into its own `Documents/Music` directory before indexing them. The
desktop preview uses a desktop file picker and the same application-owned
storage policy. Finder file sharing is also enabled; copy audio into the app's
shared Documents area, then refresh the library or relaunch the app. Valid files
at the shared directory's root are organized into `Music` without overwriting
names; invalid source files stay in place. Files already in `Music` are indexed
on the same refresh.

USB file sharing uses Finder on modern macOS and Apple Devices on Windows. See
[Apple's file sharing instructions](https://support.apple.com/en-ie/119585).

`UIBackgroundModes` declares audio playback; the native bridge configures
`AVAudioSession` for playback and publishes Now Playing metadata and remote
commands. On-device validation is required for lock-screen controls, long
background playback, interruptions and headphone route changes.

Initial iOS scope is local music. Desktop video/libmpv, external FFmpeg tools,
folder monitoring and window management are excluded. This development version
is not an App Store release: accessibility and localization review, device
performance checks and distribution signing remain release work. The original
Home artwork still includes video, photos, fitness and radio icons; those
non-music features are outside the current mobile implementation.

## Verification

```sh
cargo check --locked
cargo test --locked --lib
cargo check --locked --features mobile
cargo test --locked --features mobile --lib
python3 -m unittest discover -s scripts/tests -p test_ios_build.py
```

For the native transfer entry point, use a booted simulator's UDID:

```sh
bash scripts/check-ios-transfer-lifecycle.sh <simulator-UDID>
```

This compiles and executes the actual Objective-C bridge for 100 Start/Stop
cycles, repeated calls and shutdown. It catches native selector/property
recursion that the Rust desktop transfer tests cannot exercise. It does not
replace testing UIKit background time or the visible app on a real device.

Without full Xcode, only desktop compilation, shared logic, UI rendering and
build-script behavior can be checked locally. A successful desktop preview is
not evidence that the app has been built or run on iOS. Current verification
results are recorded in the development plan.

Interaction/display regression coverage, screenshot matrix and device-only checks: [IOS_UI_QA.md](../docs/IOS_UI_QA.md). The pinned Slint iPhone background rendering backport is documented in [vendor/README.md](../vendor/README.md).
