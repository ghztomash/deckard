# Deckard

## Install

```sh
 cargo install --path=deckard-cli 
 cargo install --path=deckard-tui 
```

## Running

```sh
cargo run --bin deckard-cli -- test_files
cargo run --bin deckard-tui -- test_files
```

## To-do

- [x] Remove files from index that are not in duplicates list
- [x] Delete empty directories
- [x] Reduce memory use
- [x] Reduce CPU use
- [ ] Reduce lock time
- [ ] Better error handling
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
