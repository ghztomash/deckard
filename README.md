# Deckard

## Install

```sh
 cargo install --path=deckard-cli 
 cargo install --path=deckard-tui 
```

## Running

```sh
cargo run --bin deckard-cli -- test_files
cargo run --bin deckard-cli -- --min_size 10M test_files
cargo run --bin deckard-cli -- --include_filter '*.wav' --include_filter '*.mp3' test_files
cargo run --bin deckard-tui -- test_files
```

`--include_filter` and `--exclude_filter` accept case-insensitive glob patterns matched against
file names. Both flags can be repeated; any matching include pattern selects a file, while any
matching exclude pattern takes precedence.

## Library progress

Long-running `FileIndex` stages accept an optional pollable `Progress` handle. Use a fresh handle
for each stage and poll `snapshot()` at the presentation layer's preferred rate. Indexing reports
only `done`; processing and duplicate comparison also publish `total` once it is known.

```rust
use deckard::progress::Progress;

let progress = Progress::new();
file_index.index_dirs(Some(&progress), cancel);
let snapshot = progress.snapshot();
```

Workers update relaxed atomics per file. Polling does not run user code or block indexing.

## To-do

- [x] Remove files from index that are not in duplicates list
- [x] Delete empty directories
- [x] Reduce memory use
- [x] Reduce CPU use
- [ ] Reduce lock time
- [ ] Better error handling
- [ ] Flatten dir cli flag
- [x] --min_size flag use humansize convention
- [x] --include_filter --exclude_filter flags use proper regex or glob
- [x] Better logging - use tracing
- [ ] Gradual comparison (process stuff only when needed?)
- [ ] optimize `get_image_hash`
- [ ] optimize `get_audio_hash`
- [ ] Hasher integration tests
- [ ] optimize `file::compare`
- [ ] File unit tests
- [ ] File integration tests
- [ ] Index unit tests
- [ ] Index integration tests
- [x] Mark all matching path
- [x] Filter view (:command mode)
- [x] Disk usage mode
