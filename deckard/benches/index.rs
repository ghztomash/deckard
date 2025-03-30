use criterion::{black_box, criterion_group, criterion_main, Criterion};
use deckard::collect_paths;
use deckard::config::SearchConfig;
use deckard::index::FileIndex;

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("index test_files", |b| {
        b.iter(|| {
            black_box(
                FileIndex::new(
                    black_box(collect_paths(vec!["../test_files/"])),
                    SearchConfig::default(),
                )
                .index_dirs(None, None),
            )
        })
    });

    c.bench_function("index doesn't exist", |b| {
        b.iter(|| {
            black_box(
                FileIndex::new(
                    black_box(collect_paths(vec!["../does_not_exist/"])),
                    SearchConfig::default(),
                )
                .index_dirs(None, None),
            )
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
