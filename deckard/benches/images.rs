use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use deckard::collect_paths;
use deckard::config::{ImageFilterAlgorithm, ImageHashAlgorithm, SearchConfig};
use deckard::index::FileIndex;

pub fn criterion_benchmark(c: &mut Criterion) {
    // env_logger::init();

    let mut g = c.benchmark_group("image");
    g.sample_size(50);

    for i in [8, 16, 32, 64, 128].iter() {
        g.bench_with_input(BenchmarkId::new("size", i), i, |b, &i| {
            b.iter(|| {
                let mut config = black_box(SearchConfig::default());
                config.image_config.compare = true;
                config.image_config.size = i as u64;

                let mut index = black_box(FileIndex::new(
                    black_box(collect_paths(vec!["../test_files/images"])),
                    config,
                ));
                black_box(index.index_dirs(None, None));
                index.process_files(None, None);
            })
        });
    }

    for i in [
        ImageHashAlgorithm::Mean,
        ImageHashAlgorithm::Median,
        ImageHashAlgorithm::Gradient,
        ImageHashAlgorithm::VertGradient,
        ImageHashAlgorithm::DoubleGradient,
        ImageHashAlgorithm::Blockhash,
    ]
    .iter()
    {
        g.bench_with_input(BenchmarkId::new("hash", format!("{:?}", i)), i, |b, &i| {
            b.iter(|| {
                let mut config = black_box(SearchConfig::default());
                config.image_config.compare = true;
                config.image_config.hash_algorithm = black_box(i);

                let mut index = black_box(FileIndex::new(
                    black_box(collect_paths(vec!["../test_files/images"])),
                    config,
                ));
                black_box(index.index_dirs(None, None));
                index.process_files(None, None);
            })
        });
    }

    for i in [
        ImageFilterAlgorithm::Nearest,
        ImageFilterAlgorithm::Triangle,
        ImageFilterAlgorithm::CatmullRom,
        ImageFilterAlgorithm::Gaussian,
        ImageFilterAlgorithm::Lanczos3,
    ]
    .iter()
    {
        g.bench_with_input(
            BenchmarkId::new("filter", format!("{:?}", i)),
            i,
            |b, &i| {
                b.iter(|| {
                    let mut config = black_box(SearchConfig::default());
                    config.image_config.compare = true;
                    config.image_config.filter_algorithm = black_box(i);

                    let mut index = black_box(FileIndex::new(
                        black_box(collect_paths(vec!["../test_files/images"])),
                        config,
                    ));
                    black_box(index.index_dirs(None, None));
                    index.process_files(None, None);
                })
            },
        );
    }

    g.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
