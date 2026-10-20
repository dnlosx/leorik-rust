#!/bin/sh
# Builds libleorik.a for iOS devices (aarch64-apple-ios) and the Apple-silicon simulator
# (aarch64-apple-ios-sim) and packages both into build/Leorik.xcframework.
#
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim
#   scripts/build-xcframework.sh
set -eu
cd "$(dirname "$0")/.."

export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-15.0}"
TARGETS="aarch64-apple-ios aarch64-apple-ios-sim"

for target in $TARGETS; do
    cargo build --release -p leorik-ffi --lib --target "$target"
done

# Debug info and local symbols of the Rust standard library make up most of the archive; apps don't need them.
for target in $TARGETS; do
    mkdir -p "build/$target"
    strip -S -x -o "build/$target/libleorik.a" "target/$target/release/libleorik.a"
done

OUT=build/Leorik.xcframework
HEADERS=build/headers
rm -rf "$OUT" "$HEADERS"
mkdir -p "$HEADERS"
cp leorik-ffi/include/leorik.h leorik-ffi/include/module.modulemap "$HEADERS/"

xcodebuild -create-xcframework \
    -library build/aarch64-apple-ios/libleorik.a -headers "$HEADERS" \
    -library build/aarch64-apple-ios-sim/libleorik.a -headers "$HEADERS" \
    -output "$OUT"

echo "Built $OUT"
