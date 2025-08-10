#!/bin/bash

SCRIPT_DIR="$(dirname "$(realpath "$0")")"
TEST_FILES="$SCRIPT_DIR/../test_files/images"
BINARY="$SCRIPT_DIR/../target/release/deckard-cli"

cargo build --release

echo "Test images"
hyperfine -N --warmup 10 "$BINARY --check_audio --threads 0 $TEST_FILES" \
	"$BINARY --check_audio --threads 1 $TEST_FILES" \
	"$BINARY --check_audio --threads 2 $TEST_FILES" \
	"$BINARY --check_audio --threads 4 $TEST_FILES" \
	"$BINARY --check_audio --threads 8 $TEST_FILES"
