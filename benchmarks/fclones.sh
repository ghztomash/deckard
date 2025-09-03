#!/bin/bash

SCRIPT_DIR="$(dirname "$(realpath "$0")")"
TEST_FILES="${1:-"$SCRIPT_DIR/../test_files"}"
BINARY="$SCRIPT_DIR/../target/release/deckard-cli"

cargo build --release

echo "Test deckard vs fclones"
hyperfine -N --warmup 10 "$BINARY --min_size 2 --skip_hidden $TEST_FILES" \
  "fclones group $TEST_FILES"
