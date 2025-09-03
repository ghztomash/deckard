#!/bin/bash

SCRIPT_DIR="$(dirname "$(realpath "$0")")"
TEST_FILES="${1:-"$SCRIPT_DIR/../test_files"}"
BINARY="$SCRIPT_DIR/../target/release/deckard-cli"

cargo build --release

echo "Test deckard vs fclones"
hyperfine -N --warmup 10 "$BINARY --disk_usage --lines_number 20 $TEST_FILES" \
  "dust --no-progress --no-percent-bars --number-of-lines 20 --skip-total --full-paths --only-file $TEST_FILES"
