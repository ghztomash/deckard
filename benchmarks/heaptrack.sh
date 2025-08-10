#!/bin/bash

SCRIPT_DIR="$(dirname "$(realpath "$0")")"
TEST_FILES="$SCRIPT_DIR/../test_files"
BINARY="$SCRIPT_DIR/../target/release/deckard-cli"

cargo build --release

heaptrack "$BINARY" "$TEST_FILES"
