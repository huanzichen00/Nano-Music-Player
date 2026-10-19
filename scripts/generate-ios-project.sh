#!/bin/bash
set -euo pipefail
export PATH="${HOME}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:${PATH}"
repo_dir="$(cd "$(dirname "$0")/.." && pwd)"

if ! command -v xcodegen >/dev/null 2>&1; then
    printf 'XcodeGen is required. Install it with: brew install xcodegen\n' >&2
    exit 1
fi
xcodegen generate --spec "$repo_dir/ios/project.yml" --project "$repo_dir/ios"
printf '\nOpen %s/ios/NanoMusicPlayer.xcodeproj in Xcode. Select your signing team for a physical device.\n' "$repo_dir"
