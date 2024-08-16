#!/bin/bash

SCRIPT_DIR="$(dirname "$(realpath "$0")")"
TEST_FILES="$SCRIPT_DIR/../test_files/images"
BINARY="$SCRIPT_DIR/../target/release/deckard-cli"

cargo build --release

echo "Test images"
hyperfine -N --warmup 10 "$BINARY --check_image --threads 0 $TEST_FILES" \
	"$BINARY --check_image --threads 1 $TEST_FILES" \
	"$BINARY --check_image --threads 2 $TEST_FILES" \
	"$BINARY --check_image --threads 4 $TEST_FILES" \
	"$BINARY --check_image --threads 8 $TEST_FILES"
