use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use deckard::collect_paths;
use deckard::config::SearchConfig;
use deckard::index::FileIndex;

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

    g.bench_function("callbacks", |b| {
        b.iter(|| {
            let mut index = black_box(FileIndex::new(
                black_box(collect_paths(vec!["../test_files/"])),
                SearchConfig::default(),
            ));
            let cancel = black_box(Arc::new(AtomicBool::new(false)));
            index.index_dirs(
                black_box(Some(Arc::new(move |x| {
                    let _s = black_box(format!("{}", x));
                }))),
                black_box(Some(cancel.clone())),
            );
            index.process_files(
                black_box(Some(Arc::new(|x, y| {
                    let _s = black_box(format!("{}, {}", x, y));
                }))),
                black_box(Some(cancel.clone())),
            );
            index.find_duplicates(
                black_box(Some(Arc::new(|x, y| {
                    let _s = black_box(format!("{}, {}", x, y));
                }))),
                black_box(Some(cancel.clone())),
            );
        })
    });

    g.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
