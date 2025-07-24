# Deckard

## Running

```sh
cargo run --bin deckard-cli -- test_files
cargo run --bin deckard-tui -- test_files
```

## To-do

- [ ] Remove files from index that are not in duplicates list
- [ ] Delete empty directories
- [ ] Better error handling
- [ ] Better logging - use tracing
- [ ] Gradual comparison (process stuff only when needed?)
