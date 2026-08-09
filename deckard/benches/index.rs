use criterion::{Criterion, criterion_group, criterion_main};
use deckard::config::SearchConfig;
use deckard::index::FileIndex;
use deckard::progress::Progress;
use std::collections::HashSet;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const DIRECTORY_COUNT: usize = 64;
const FILE_COUNT: usize = 4_096;

struct BenchmarkFixture {
    root: PathBuf,
    dirs: HashSet<PathBuf>,
}

impl BenchmarkFixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "deckard-index-benchmark-{}-{nonce}",
            std::process::id()
        ));
        let files_per_directory = FILE_COUNT / DIRECTORY_COUNT;
        let mut dirs = HashSet::with_capacity(DIRECTORY_COUNT);

        for directory_index in 0..DIRECTORY_COUNT {
            let directory = root.join(format!("dir-{directory_index:02}"));
            fs::create_dir_all(&directory).expect("benchmark directory should be created");
            dirs.insert(directory.clone());

            for file_index in 0..files_per_directory {
                fs::write(directory.join(format!("file-{file_index:02}.dat")), b"x")
                    .expect("benchmark file should be written");
            }
        }

        Self { root, dirs }
    }
}

impl Drop for BenchmarkFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("benchmark fixture should be removed");
    }
}

pub fn criterion_benchmark(c: &mut Criterion) {
    let fixture = BenchmarkFixture::new();
    let mut group = c.benchmark_group("index");

    group.bench_function("no_progress", |b| {
        b.iter(|| {
            let mut index = FileIndex::new(fixture.dirs.clone(), SearchConfig::default());
            index.index_dirs(None, None);
            black_box(index.files_len());
        });
    });

    group.bench_function("progress_enabled", |b| {
        b.iter(|| {
            let progress = Progress::new();
            let mut index = FileIndex::new(fixture.dirs.clone(), SearchConfig::default());
            index.index_dirs(Some(&progress), None);
            black_box(index.files_len());
            black_box(progress.snapshot());
        });
    });

    group.finish();

    c.bench_function("index doesn't exist", |b| {
        b.iter(|| {
            let dirs = HashSet::from([PathBuf::from("../does_not_exist/")]);
            let mut index = FileIndex::new(dirs, SearchConfig::default());
            index.index_dirs(None, None);
            black_box(index.files_len());
        });
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
