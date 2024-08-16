#!/bin/bash

SCRIPT_DIR="$(dirname "$(realpath "$0")")"
FILE_FOLDER="$SCRIPT_DIR/big_files"
SIZE="1000"

echo "Creating big_files folder"
mkdir -p $FILE_FOLDER

echo "Creating zero files"
for i in {1..3}; do
  echo "Number: $i"
  dd if=/dev/zero of=$FILE_FOLDER/zero_file_$i bs=1M count=$SIZE
done

echo "Creating random files"
for i in {1..3}; do
  echo "Number: $i"
  dd if=/dev/urandom of=$FILE_FOLDER/random_file_$i bs=1M count=$SIZE
done
