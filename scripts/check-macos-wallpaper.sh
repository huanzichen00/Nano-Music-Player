#!/bin/sh
# Test the native bridge against a fake desktop; never changes the user's wallpaper.
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
mkdir -p target/qa/wallpaper
clang -fobjc-arc -framework AppKit tests/native_wallpaper.m -o target/qa/wallpaper/native-tests
target/qa/wallpaper/native-tests
