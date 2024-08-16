use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use deckard::collect_paths;
use deckard::config::SearchConfig;
use deckard::index::FileIndex;
use env_logger;

pub fn criterion_benchmark(c: &mut Criterion) {
    // env_logger::init();

    let mut g = c.benchmark_group("image");
    g.sample_size(50);

    for i in [8, 16, 32, 64, 128].iter() {
        g.bench_with_input(BenchmarkId::new("size", i), i, |b, &i| {
            b.iter(|| {
                let mut config = black_box(SearchConfig::default());
                config.image_config.check_image = true;
                config.image_config.size = i as u64;

                let mut index = black_box(FileIndex::new(
                    black_box(collect_paths(vec!["../test_files/images"])),
                    config,
                ));
                black_box(index.index_dirs());
                index.process_files();
            })
        });
    }

    for i in [
        "mean",
        "median",
        "gradient",
        "vert_gradient",
        "double_gradient",
        "blockhash",
    ]
    .iter()
    {
        g.bench_with_input(BenchmarkId::new("hash", i), i, |b, &i| {
            b.iter(|| {
                let mut config = black_box(SearchConfig::default());
                config.image_config.check_image = true;
                config.image_config.hash_algorithm = black_box(i.to_string());

                let mut index = black_box(FileIndex::new(
                    black_box(collect_paths(vec!["../test_files/images"])),
                    config,
                ));
                black_box(index.index_dirs());
                index.process_files();
            })
        });
    }

    for i in ["nearest", "triangle", "catmull", "gaussian", "lanczos"].iter() {
        g.bench_with_input(BenchmarkId::new("filter", i), i, |b, &i| {
            b.iter(|| {
                let mut config = black_box(SearchConfig::default());
                config.image_config.check_image = true;
                config.image_config.filter_algorithm = black_box(i.to_string());

                let mut index = black_box(FileIndex::new(
                    black_box(collect_paths(vec!["../test_files/images"])),
                    config,
                ));
                black_box(index.index_dirs());
                index.process_files();
            })
        });
    }

    g.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
