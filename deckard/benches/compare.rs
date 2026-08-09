use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use criterion::{Criterion, criterion_group, criterion_main};
use deckard::collect_paths;
use deckard::config::SearchConfig;
use deckard::index::FileIndex;
use deckard::progress::Progress;
use std::hint::black_box;

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut g = c.benchmark_group("compare");
    g.sample_size(10);

    g.bench_function("test_files", |b| {
        b.iter(|| {
            let mut index = black_box(FileIndex::new(
                black_box(collect_paths(vec!["../test_files/"])),
                SearchConfig::default(),
            ));
            index.index_dirs(None, None);
            index.process_files(None, None);
            index.find_duplicates(None, None);
        })
    });

    g.bench_function("full_hash", |b| {
        b.iter(|| {
            let mut config = SearchConfig::default();
            config.hasher_config.full_hash = true;

            let mut index = black_box(FileIndex::new(
                black_box(collect_paths(vec!["../test_files/"])),
                config,
            ));
            index.index_dirs(None, None);
            index.process_files(None, None);
            index.find_duplicates(None, None);
        })
    });

    g.bench_function("progress", |b| {
        b.iter(|| {
            let mut index = black_box(FileIndex::new(
                black_box(collect_paths(vec!["../test_files/"])),
                SearchConfig::default(),
            ));
            let cancel = black_box(Arc::new(AtomicBool::new(false)));
            let indexing = Progress::new();
            index.index_dirs(Some(&indexing), black_box(Some(cancel.clone())));
            black_box(indexing.snapshot());
            let processing = Progress::new();
            index.process_files(Some(&processing), black_box(Some(cancel.clone())));
            black_box(processing.snapshot());
            let comparing = Progress::new();
            index.find_duplicates(Some(&comparing), black_box(Some(cancel.clone())));
            black_box(comparing.snapshot());
        })
    });

    g.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
