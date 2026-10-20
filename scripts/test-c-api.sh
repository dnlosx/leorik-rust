#!/bin/sh
# Builds the static library for the host and runs the C API test program against it.
set -eu
cd "$(dirname "$0")/.."
cargo build --release -p leorik-ffi
mkdir -p target/c-test
cc -std=c11 -Wall -Wextra -O2 -I leorik-ffi/include leorik-ffi/tests/c/test_api.c \
    target/release/libleorik.a -o target/c-test/test_api
./target/c-test/test_api
