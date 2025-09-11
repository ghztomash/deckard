#!/bin/bash

SCRIPT_DIR="$(dirname "$(realpath "$0")")"
TEST_FILES="$SCRIPT_DIR/../"
TEST_FILES_2="$HOME/Downloads/"
TEST_FILES_3="$SCRIPT_DIR/../test_files"
FLAGS="-H -a"
BINARY_1="$SCRIPT_DIR/../target/release/deckard-cli_before"
BINARY_2="$SCRIPT_DIR/../target/release/deckard-cli_hash_hash"
BINARY_3="$SCRIPT_DIR/../target/release/deckard-cli_after"

echo "Test full_hash vs quick"
hyperfine -N --warmup 10 \
    "$BINARY_1 $FLAGS $TEST_FILES $TEST_FILES_3" \
    "$BINARY_3 $FLAGS $TEST_FILES $TEST_FILES_3"
