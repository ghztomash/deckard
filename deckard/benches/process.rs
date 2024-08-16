use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use deckard::collect_paths;
use deckard::config::SearchConfig;
use deckard::index::FileIndex;

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut g = c.benchmark_group("process");
    g.sample_size(10);

    g.bench_function("test_files", |b| {
        b.iter(|| {
            let mut index = black_box(FileIndex::new(
                black_box(collect_paths(vec!["../test_files/"])),
                SearchConfig::default(),
            ));
            black_box(index.index_dirs());
            index.process_files();
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
            black_box(index.index_dirs());
            index.process_files();
        })
    });

    for i in [4, 8, 16, 32, 64].iter() {
        g.bench_with_input(BenchmarkId::new("splits", i), i, |b, &i| {
            b.iter(|| {
                let mut config = SearchConfig::default();
                config.hasher_config.splits = i;

                let mut index = black_box(FileIndex::new(
                    black_box(collect_paths(vec!["../test_files/"])),
                    config,
                ));
                black_box(index.index_dirs());
                index.process_files();
            })
        });
    }

    for i in [32, 64, 128, 256, 1024, 2048].iter() {
        g.bench_with_input(BenchmarkId::new("size", i), i, |b, &i| {
            b.iter(|| {
                let mut config = SearchConfig::default();
                config.hasher_config.size = i;

                let mut index = black_box(FileIndex::new(
                    black_box(collect_paths(vec!["../test_files/"])),
                    config,
                ));
                black_box(index.index_dirs());
                index.process_files();
            })
        });
    }

    g.bench_function("dont_exist", |b| {
        b.iter(|| {
            let mut index = black_box(FileIndex::new(
                black_box(collect_paths(vec!["../dont_exist/"])),
                SearchConfig::default(),
            ));
            black_box(index.index_dirs());
            index.process_files();
        })
    });

    g.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
