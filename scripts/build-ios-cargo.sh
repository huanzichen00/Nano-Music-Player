#!/bin/bash
# Invoked by the Xcode target after its normal compilation phase.
set -euo pipefail
export PATH="${HOME}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:${PATH}"
repo_dir="$(cd "$(dirname "$0")/.." && pwd)"

for setting in ARCHS PLATFORM_NAME CONFIGURATION TARGET_BUILD_DIR EXECUTABLE_PATH DERIVED_FILE_DIR; do
    if [[ -z "${!setting:-}" ]]; then
        printf 'Missing Xcode build setting %s. Run this script through the NanoMusicPlayer Xcode scheme.\n' "$setting" >&2
        exit 1
    fi
done

case "$PLATFORM_NAME" in
    iphoneos|iphonesimulator) ;;
    *) printf 'Unsupported iOS platform: %s\n' "$PLATFORM_NAME" >&2; exit 1 ;;
esac
if ! command -v cargo >/dev/null 2>&1; then
    printf 'Cargo was not found. Install Rust with rustup.\n' >&2
    exit 1
fi

export SDKROOT="$(xcrun --sdk "$PLATFORM_NAME" --show-sdk-path)"
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-15.0}"
export CARGO_TARGET_DIR="${NANO_IOS_CARGO_TARGET_DIR:-${DERIVED_FILE_DIR}/cargo}"
# Keep release symbols so Archive can produce a matching dSYM.
export CARGO_PROFILE_RELEASE_DEBUG="${CARGO_PROFILE_RELEASE_DEBUG:-1}"

profile=debug
profile_args=(--profile dev)
if [[ "$CONFIGURATION" != Debug ]]; then
    profile=release
    profile_args=(--release)
else
    # The device runs the Rust UI and image decoders too. Unoptimized dependency
    # code makes touch-driven layout expensive even on a fast iPhone.
    export CARGO_PROFILE_DEV_OPT_LEVEL="${CARGO_PROFILE_DEV_OPT_LEVEL:-2}"
fi

executables=()
for arch in $ARCHS; do
    case "$PLATFORM_NAME/$arch" in
        iphoneos/arm64) rust_target=aarch64-apple-ios ;;
        iphonesimulator/arm64) rust_target=aarch64-apple-ios-sim ;;
        iphonesimulator/x86_64) rust_target=x86_64-apple-ios ;;
        *) printf 'Unsupported iOS platform/architecture: %s/%s\n' "$PLATFORM_NAME" "$arch" >&2; exit 1 ;;
    esac
    # Cargo automatically selects the mobile interface for iOS targets.
    cargo build --manifest-path "$repo_dir/Cargo.toml" --locked \
        --target "$rust_target" --bin nano-music-player "${profile_args[@]}"
    executables+=("${CARGO_TARGET_DIR}/${rust_target}/${profile}/nano-music-player")
done

binary_path="${TARGET_BUILD_DIR}/${EXECUTABLE_PATH}"
mkdir -p "$(dirname "$binary_path")"
xcrun lipo -create -output "$binary_path" "${executables[@]}"

# The mobile UI embeds Noto Sans CJK. Keep its redistribution license with the app.
mkdir -p "$(dirname "$binary_path")/licenses"
cp "$repo_dir/ui/fonts/noto-sans-cjk/LICENSE" \
    "$(dirname "$binary_path")/licenses/NotoSansCJK-OFL.txt"

if [[ -n "${DWARF_DSYM_FOLDER_PATH:-}" && -n "${DWARF_DSYM_FILE_NAME:-}" ]]; then
    mkdir -p "$DWARF_DSYM_FOLDER_PATH"
    xcrun dsymutil "$binary_path" -o "${DWARF_DSYM_FOLDER_PATH}/${DWARF_DSYM_FILE_NAME}"
fi

if [[ "$PLATFORM_NAME" == iphoneos && "${CODE_SIGNING_ALLOWED:-YES}" != NO && -n "${EXPANDED_CODE_SIGN_IDENTITY:-}" ]]; then
    sign_args=(--force --sign "$EXPANDED_CODE_SIGN_IDENTITY")
    entitlements="${TARGET_TEMP_DIR:-}/${PRODUCT_NAME:-NanoMusicPlayer}.app.xcent"
    if [[ -s "$entitlements" ]]; then
        sign_args+=(--entitlements "$entitlements")
    fi
    /usr/bin/codesign "${sign_args[@]}" "$binary_path"
fi
