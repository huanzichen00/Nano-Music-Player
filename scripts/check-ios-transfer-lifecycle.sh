#!/bin/bash
set -euo pipefail
repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
if [[ $# -ne 1 ]]; then
    printf 'Usage: %s <booted simulator UDID>\n' "$0" >&2
    exit 1
fi
simulator_id="$1"
sdk_dir="$(xcrun --sdk iphonesimulator --show-sdk-path)"
binary_dir="$repo_dir/target/ios-preview"
mkdir -p "$binary_dir"
case "$(uname -m)" in
    arm64) test_target=arm64-apple-ios15.0-simulator ;;
    x86_64) test_target=x86_64-apple-ios15.0-simulator ;;
    *) printf 'Unsupported simulator host architecture\n' >&2; exit 1 ;;
esac
xcrun --sdk iphonesimulator clang -target "$test_target" -isysroot "$sdk_dir" \
    -fobjc-arc -fmodules -Wall -Wextra -Werror \
    -Wno-unused-parameter \
    "$repo_dir/ios/tests/TransferLifecycleRegression.m" \
    -framework UIKit -framework Foundation -framework AVFoundation \
    -framework MediaPlayer -framework UniformTypeIdentifiers \
    -o "$binary_dir/transfer-lifecycle-regression"
xcrun simctl spawn "$simulator_id" "$binary_dir/transfer-lifecycle-regression"
