#!/bin/sh
# Runs the C API test inside a booted iOS simulator, linked against the XCFramework's simulator slice.
# Usage: scripts/test-ios-sim.sh [simulator-udid]   (default: the first booted simulator)
set -eu
cd "$(dirname "$0")/.."
[ -d build/Leorik.xcframework ] || scripts/build-xcframework.sh
DEVICE="${1:-$(xcrun simctl list devices booted | grep -m1 -oE '[0-9A-F]{8}-([0-9A-F]{4}-){3}[0-9A-F]{12}')}"
[ -n "$DEVICE" ] || { echo "No booted simulator; boot one or pass a UDID" >&2; exit 1; }
SDK=$(xcrun --sdk iphonesimulator --show-sdk-path)
mkdir -p target/c-test
xcrun --sdk iphonesimulator clang -target arm64-apple-ios15.0-simulator -isysroot "$SDK" \
    -I leorik-ffi/include leorik-ffi/tests/c/test_api.c \
    build/Leorik.xcframework/ios-arm64-simulator/libleorik.a -o target/c-test/test_api_sim
xcrun simctl spawn "$DEVICE" "$PWD/target/c-test/test_api_sim"
