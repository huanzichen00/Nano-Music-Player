#!/bin/bash
# Read-only checks. Installing Xcode, SDKs and Rust targets is left to the developer.
set -euo pipefail
export PATH="${HOME}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:${PATH}"

issues=0
report_missing() {
    printf 'Missing: %s\n' "$1" >&2
    issues=$((issues + 1))
}

if [[ "$(uname -s)" != Darwin ]]; then
    report_missing 'macOS is required for iOS builds.'
fi

if xcodebuild -version >/dev/null 2>&1; then
    xcodebuild -version
    for sdk in iphoneos iphonesimulator; do
        if ! xcrun --sdk "$sdk" --show-sdk-path >/dev/null 2>&1; then
            report_missing "$sdk SDK; install the platform in Xcode Settings > Components."
        fi
    done
else
    report_missing 'full Xcode; Command Line Tools alone does not include the iOS SDK. Select Xcode in Settings > Locations > Command Line Tools, or set DEVELOPER_DIR.'
fi

if command -v cargo >/dev/null 2>&1 && command -v rustup >/dev/null 2>&1; then
    cargo --version
    installed_targets="$(rustup target list --installed)"
    for target in aarch64-apple-ios aarch64-apple-ios-sim; do
        if ! printf '%s\n' "$installed_targets" | /usr/bin/grep -qx "$target"; then
            report_missing "Rust target $target; run: rustup target add $target"
        fi
    done
else
    report_missing 'Rust/Cargo managed by rustup (https://rustup.rs).'
fi

if command -v xcodegen >/dev/null 2>&1; then
    xcodegen --version
else
    report_missing 'XcodeGen; run: brew install xcodegen'
fi

if [[ "$issues" -ne 0 ]]; then
    printf '\niOS environment has %s missing requirement(s). Desktop mobile preview remains available with cargo run --features mobile.\n' "$issues" >&2
    exit 1
fi
printf '\niOS build requirements are available. Device signing and simulator runtimes are configured separately in Xcode.\n'
