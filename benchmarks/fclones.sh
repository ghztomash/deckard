#!/bin/bash

SCRIPT_DIR="$(dirname "$(realpath "$0")")"
TEST_FILES="$SCRIPT_DIR/../test_files"
BINARY="$SCRIPT_DIR/../target/release/deckard-cli"

cargo build --release

echo "Test deckard vs fclones"
hyperfine -N --warmup 10 "$BINARY -m 2 -H $TEST_FILES" \
	"fclones group $TEST_FILES"
