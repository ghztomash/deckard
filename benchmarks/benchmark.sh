#!/bin/bash

SCRIPT_DIR="$(dirname "$(realpath "$0")")"
TEST_FILES="$SCRIPT_DIR/../test_files"
BINARY="$SCRIPT_DIR/../target/release/deckard-cli"

cargo build --release

echo "Test full_hash vs quick"
hyperfine -N --warmup 10 "$BINARY $TEST_FILES" \
	"$BINARY --full_hash $TEST_FILES"
