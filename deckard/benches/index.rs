use criterion::{black_box, criterion_group, criterion_main, Criterion};
use deckard::collect_paths;
use deckard::index::FileIndex;

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("index test_files", |b| {
        b.iter(|| {
            black_box(FileIndex::new(black_box(collect_paths(vec!["./test_files/"]))).index_dirs())
        })
    });

    c.bench_function("index doesn't_exist", |b| {
        b.iter(|| {
            black_box(
                FileIndex::new(black_box(collect_paths(vec!["./does_not_exist/"]))).index_dirs(),
            )
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
