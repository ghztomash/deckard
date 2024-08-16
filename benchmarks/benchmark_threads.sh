#!/bin/bash

SCRIPT_DIR="$(dirname "$(realpath "$0")")"
TEST_FILES="$SCRIPT_DIR/../test_files"
BINARY="$SCRIPT_DIR/../target/release/deckard-cli"

cargo build --release

echo "Test threads"
hyperfine -N --warmup 10 "$BINARY --threads 0 $TEST_FILES" \
	"$BINARY --threads 1 $TEST_FILES" \
	"$BINARY --threads 2 $TEST_FILES" \
	"$BINARY --threads 4 $TEST_FILES" \
	"$BINARY --threads 8 $TEST_FILES" \
	"$BINARY --threads 16 $TEST_FILES" \
	"$BINARY --threads 32 $TEST_FILES"

echo "Test threads full_hash"
hyperfine -N --warmup 10 "$BINARY --full_hash --threads 0 $TEST_FILES" \
	"$BINARY --full_hash --threads 1 $TEST_FILES" \
	"$BINARY --full_hash --threads 2 $TEST_FILES" \
	"$BINARY --full_hash --threads 4 $TEST_FILES" \
	"$BINARY --full_hash --threads 8 $TEST_FILES" \
	"$BINARY --full_hash --threads 16 $TEST_FILES" \
	"$BINARY --full_hash --threads 32 $TEST_FILES"
