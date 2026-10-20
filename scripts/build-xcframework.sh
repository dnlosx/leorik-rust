#!/bin/sh
# Builds libleorik.a for iOS devices (aarch64-apple-ios), the simulator (aarch64-apple-ios-sim and
# x86_64-apple-ios, merged into one library) and Apple-silicon Macs (aarch64-apple-darwin, so Swift packages using it can
# run `swift test` on a Mac) and packages them into build/Leorik.xcframework.
#
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios aarch64-apple-darwin
#   scripts/build-xcframework.sh
set -eu
cd "$(dirname "$0")/.."

export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-15.0}"
TARGETS="aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios aarch64-apple-darwin"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.0}"

for target in $TARGETS; do
    cargo build --release -p leorik-ffi --lib --target "$target"
done

# Debug info and local symbols of the Rust standard library make up most of the archive; apps don't need them.
for target in $TARGETS; do
    mkdir -p "build/$target"
    strip -S -x -o "build/$target/libleorik.a" "target/$target/release/libleorik.a"
done

# one simulator library for both architectures
mkdir -p build/ios-simulator
lipo -create build/aarch64-apple-ios-sim/libleorik.a build/x86_64-apple-ios/libleorik.a -output build/ios-simulator/libleorik.a

OUT=build/Leorik.xcframework
HEADERS=build/headers
rm -rf "$OUT" "$HEADERS"
mkdir -p "$HEADERS"
cp leorik-ffi/include/leorik.h leorik-ffi/include/module.modulemap "$HEADERS/"

xcodebuild -create-xcframework \
    -library build/aarch64-apple-ios/libleorik.a -headers "$HEADERS" \
    -library build/ios-simulator/libleorik.a -headers "$HEADERS" \
    -library build/aarch64-apple-darwin/libleorik.a -headers "$HEADERS" \
    -output "$OUT"

echo "Built $OUT"
