use criterion::{black_box, criterion_group, criterion_main, Criterion};
use deckard::collect_paths;
use deckard::index::FileIndex;

pub fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("process test_files", |b| {
        b.iter(|| {
            let mut index = black_box(FileIndex::new(black_box(collect_paths(vec![
                "./test_files/",
            ]))));
            black_box(index.index_dirs());
            index.process_files();
        })
    });

    c.bench_function("process dont_exist", |b| {
        b.iter(|| {
            let mut index = black_box(FileIndex::new(black_box(collect_paths(vec![
                "./dont_exist/",
            ]))));
            black_box(index.index_dirs());
            index.process_files();
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
