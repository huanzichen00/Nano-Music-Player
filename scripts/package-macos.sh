#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
case "${1:-}" in
  --debug) profile=debug; cargo build ;;
  "") profile=release; cargo build --release ;;
  *) echo 'Usage: scripts/package-macos.sh [--debug]' >&2; exit 1 ;;
esac
bundle="$project_dir/target/Nano Music Player.app"
mkdir -p "$bundle/Contents/MacOS"
cp "target/$profile/nano-music-player" "$bundle/Contents/MacOS/Nano Music Player.next"
# Atomic replacement leaves an already-running copy mapped to its original executable.
mv "$bundle/Contents/MacOS/Nano Music Player.next" "$bundle/Contents/MacOS/Nano Music Player"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Nano Music Player</string>
<key>CFBundleIdentifier</key><string>org.nano.music-player</string>
<key>CFBundleName</key><string>Nano Music Player</string>
<key>CFBundleDisplayName</key><string>Nano Music Player</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.1</string>
<key>CFBundleVersion</key><string>2</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict></plist>
PLIST
printf 'Built %s\n' "$bundle"
