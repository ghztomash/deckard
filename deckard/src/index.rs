use dashmap::DashMap;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use jwalk::Parallelism;
use rayon::iter::ParallelIterator;
use rayon::iter::{IntoParallelRefIterator, ParallelBridge};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};

use std::ffi::OsString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use crate::config::SearchConfig;
use crate::file::FileEntry;
use crate::progress::Progress;
use std::collections::{HashMap, HashSet, hash_map::Entry};
use std::path::PathBuf;

use tracing::{debug, error, trace, warn};

#[derive(Debug, Default, Clone)]
pub struct FileIndex {
    pub dirs: HashSet<PathBuf>,
    pub files: HashMap<Arc<PathBuf>, FileEntry>,
    pub duplicates: HashMap<Arc<PathBuf>, HashSet<Arc<PathBuf>>>,
    pub config: SearchConfig,
    pub pool: Option<Arc<ThreadPool>>,
}

impl FileIndex {
    pub fn new(dirs: HashSet<PathBuf>, config: SearchConfig) -> Self {
        let target_threads = if config.threads == 0 {
            rayon::current_num_threads()
        } else {
            config.threads
        };
        debug!("Building local Rayon thread pool with {target_threads} threads",);

        // Build a local thread pool
        let pool = ThreadPoolBuilder::new()
            .num_threads(config.threads)
            .thread_name(|i| format!("deckard-pool-{i}"))
            .build()
            .or_else(|e| {
                error!("Error building local thread pool: {e}");
                // fallback to default
                ThreadPoolBuilder::default().build()
            })
            .map(Arc::new)
            .ok();

        FileIndex {
            dirs,
            files: HashMap::new(),
            duplicates: HashMap::new(),
            config,
            pool,
        }
    }

    /// Replaces the indexed files and clears previous duplicate results on completion.
    ///
    /// Progress counts accepted files without publishing a total. Cancellation preserves
    /// the previous index and duplicate results.
    pub fn index_dirs(&mut self, progress: Option<&Progress>, cancel: Option<Arc<AtomicBool>>) {
        let include_filter = build_glob_set(self.config.include_filter.as_deref());
        let exclude_filter = build_glob_set(self.config.exclude_filter.as_deref());
        let parallelism = if let Some(pool) = self.pool.as_ref() {
            Parallelism::RayonExistingPool {
                pool: pool.clone(),
                busy_timeout: Some(Duration::from_secs(1)),
            }
        } else {
            Parallelism::RayonDefaultPool {
                busy_timeout: Duration::from_secs(1),
            }
        };
        let mut index = HashMap::new();
        let cancelled = || {
            cancel
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Relaxed))
        };
        for dir in &self.dirs {
            if cancelled() {
                return;
            }
            let mut walker = jwalk::WalkDir::new(dir)
                .parallelism(parallelism.to_owned())
                .sort(false)
                .skip_hidden(self.config.skip_hidden);
            if let Some(depth) = self.config.depth {
                walker = walker.max_depth(depth);
            }
            for entry in walker {
                if cancelled() {
                    return;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        warn!("failed indexing file {}", error);
                        continue;
                    }
                };
                let path = entry.path();
                if !path.is_file() || path.is_symlink() {
                    continue;
                }

                let file_name = entry.file_name().to_string_lossy();
                if exclude_filter
                    .as_ref()
                    .is_some_and(|filter| filter.is_match(file_name.as_ref()))
                {
                    trace!("File '{}' matches an exclude filter", file_name);
                    continue;
                }
                if let Some(filter) = &include_filter {
                    if !filter.is_match(file_name.as_ref()) {
                        continue;
                    }
                    trace!("File '{}' matches an include filter", file_name);
                }

                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        error!("Error reading metadata {}", error);
                        continue;
                    }
                };
                let file_size = metadata.len();
                if file_size < self.config.min_size {
                    trace!(
                        "Skipping file {}, size {} smaller than {}",
                        file_name, file_size, self.config.min_size
                    );
                    continue;
                }
                if let Some(max_size) = self.config.max_size
                    && file_size > max_size
                {
                    trace!(
                        "Skipping file {}, size {} larger than {}",
                        file_name, file_size, max_size
                    );
                    continue;
                }

                let path = Arc::new(path);
                let file = match FileEntry::new(&path, &metadata) {
                    Ok(file) => file,
                    Err(error) => {
                        error!("Error reading File Entry {}", error);
                        continue;
                    }
                };
                if let Entry::Vacant(entry) = index.entry(path) {
                    entry.insert(file);
                    if let Some(progress) = progress {
                        progress.advance();
                    }
                }
            }
        }
        if cancelled() {
            return;
        }
        self.files = index;
        self.duplicates.clear();
    }

    /// Processes indexed files and publishes their count as the progress total.
    pub fn process_files(&mut self, progress: Option<&Progress>, cancel: Option<Arc<AtomicBool>>) {
        let total = self.files_len();
        if let Some(progress) = progress {
            progress.publish_total(total);
        }

        let mut process_op = || {
            let _ = self.files.values_mut().par_bridge().try_for_each(|f| {
                if let Some(cancel) = cancel.as_ref()
                    && cancel.load(Ordering::Relaxed)
                {
                    // short circit the parallel iterator
                    // TODO: this still doesn't cancel ongoing processing
                    return Err(());
                }

                if let Err(e) = f.process(&self.config) {
                    error!("Error processing file {}", e);
                }
                if let Some(progress) = progress {
                    progress.advance();
                }
                Ok(())
            });
        };

        if let Some(pool) = self.pool.as_ref() {
            pool.install(process_op);
        } else {
            process_op();
        }
    }

    /// Compares indexed files and publishes their count as the progress total.
    pub fn find_duplicates(
        &mut self,
        progress: Option<&Progress>,
        cancel: Option<Arc<AtomicBool>>,
    ) {
        let vec_files: Vec<&FileEntry> = self.files.values().collect();

        let total = vec_files.len();
        if let Some(progress) = progress {
            progress.publish_total(total);
        }

        // Use DashMap for concurrent access to the duplicates map
        let duplicates = DashMap::new();

        let min_len = if self.config.image_config.compare
            || self.config.audio_config.compare
            || total > 10000
        {
            // Each parallel iterator will have at least one item.
            1
        } else {
            // Make parallel iterator behave sequentially, it's faster when we do short comparisons
            vec_files.len()
        };
        // Parallelize the outer loop using rayon
        let compare_op = || {
            let _ = vec_files
                .par_iter()
                .with_min_len(min_len)
                .enumerate()
                .try_for_each(|(i, this_file)| {
                    if let Some(cancel) = cancel.as_ref()
                        && cancel.load(Ordering::Relaxed)
                    {
                        // short circit the parallel iterator
                        return Err(());
                    }

                    for other_file in vec_files.iter().skip(i + 1) {
                        // Check if the files are matching
                        if this_file.compare(other_file, &self.config) {
                            // Insert into the duplicates map
                            duplicates
                                .entry(this_file.path.clone())
                                .or_insert(HashSet::new())
                                .insert(other_file.path.clone());

                            // Insert the reverse mapping as well
                            duplicates
                                .entry(other_file.path.clone())
                                .or_insert(HashSet::new())
                                .insert(this_file.path.clone());
                        }
                    }
                    if let Some(progress) = progress {
                        progress.advance();
                    }
                    Ok(())
                });
        };

        if let Some(pool) = self.pool.as_ref() {
            pool.install(compare_op);
        } else {
            compare_op();
        }

        // Collect the results from the DashMap back into the `self.duplicates` HashMap
        self.duplicates = duplicates.into_iter().collect();
    }

    pub fn files_len(&self) -> usize {
        self.files.len()
    }

    pub fn duplicates_len(&self) -> usize {
        self.duplicates.len()
    }

    pub fn file_name(&self, file: &PathBuf) -> Option<OsString> {
        self.files.get(file).map(|f| f.name())?
    }

    pub fn file_entry(&self, file: &PathBuf) -> Option<FileEntry> {
        self.files.get(file).cloned()
    }

    pub fn file_size(&self, file: &PathBuf) -> Option<u64> {
        self.files.get(file).map(|f| f.size)
    }

    pub fn file_date_modified(&self, file: &PathBuf) -> Option<SystemTime> {
        self.files.get(file).map(|f| f.modified)?
    }

    pub fn file_date_created(&self, file: &PathBuf) -> Option<SystemTime> {
        self.files.get(file).map(|f| f.created)?
    }

    /// Get number of duplicates a file has
    pub fn file_duplicates_len(&self, file: &PathBuf) -> Option<usize> {
        Some(self.duplicates.get(file)?.len())
    }

    pub fn remove_from_index(&mut self, file: &PathBuf) {
        // get the given file
        if let Some(clones) = self.duplicates.remove(file) {
            // check the clones of the file
            for clone in &clones {
                if let Some(set) = self.duplicates.get_mut(clone) {
                    // remove all the backlinks
                    set.remove(file);
                    if set.is_empty() {
                        self.duplicates.remove(clone);
                        self.files.remove(clone);
                    }
                }
            }
            self.files.remove(file);
        }
    }

    pub fn cleanup_index(&mut self) {
        // Clean index from files without duplicates
        self.files
            .retain(|path, _entry| self.duplicates.contains_key(path));
        self.files.shrink_to_fit();
    }
}

fn build_glob_set(patterns: Option<&[String]>) -> Option<GlobSet> {
    let patterns = patterns?;
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        match GlobBuilder::new(pattern).case_insensitive(true).build() {
            Ok(glob) => {
                builder.add(glob);
            }
            Err(error) => error!(pattern, %error, "Invalid file filter glob"),
        }
    }
    builder
        .build()
        .inspect_err(|error| error!(%error, "Failed building file filters"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::{Progress, ProgressSnapshot};
    use std::fs;
    use std::path::Path;
    use std::time::UNIX_EPOCH;

    #[test]
    fn scan_depth_limits_index() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test_files/folder");
        let top = "another.txt";
        let mid = "subfolder/midfile";
        let deep = "subfolder/deepfolder/deepfile";
        let file = root.join(top);
        let cases: &[(&Path, Option<usize>, &[&str])] = &[
            (&root, Some(0), &[]),
            (&root, Some(1), &[top]),
            (&root, Some(2), &[top, mid]),
            (&root, None, &[top, mid, deep]),
            (&file, Some(0), &[top]),
        ];
        for &(path, depth, expected) in cases {
            let config = SearchConfig {
                depth,
                threads: 1,
                ..SearchConfig::default()
            };
            let mut index = FileIndex::new(HashSet::from([path.to_owned()]), config);
            index.index_dirs(None, None);
            let paths: HashSet<_> = index
                .files
                .keys()
                .map(|path| path.strip_prefix(&root).unwrap())
                .collect();
            assert_eq!(
                paths,
                expected.iter().map(Path::new).collect(),
                "root {path:?}, depth {depth:?}"
            );
        }
    }

    #[test]
    fn scan_depth_preserves_nested_roots() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../test_files/folder")
            .canonicalize()
            .unwrap();
        let subfolder = root.join("subfolder");
        let top = root.join("another.txt");
        let mid = subfolder.join("midfile");
        let deep = subfolder.join("deepfolder/deepfile");
        let cases: &[([&Path; 2], usize, &[&Path])] = &[
            ([&root, &subfolder], 1, &[&top, &mid]),
            ([&subfolder, &root], 1, &[&top, &mid]),
            ([&root, &subfolder], 2, &[&top, &mid, &deep]),
        ];
        for &(roots, depth, expected) in cases {
            let config = SearchConfig {
                depth: Some(depth),
                threads: 1,
                ..SearchConfig::default()
            };
            let progress = Progress::new();
            let mut index = FileIndex::new(
                crate::collect_paths_with_depth(&roots, config.depth),
                config,
            );
            index.index_dirs(Some(&progress), None);
            assert_eq!(
                index
                    .files
                    .keys()
                    .map(|path| path.as_path())
                    .collect::<HashSet<_>>(),
                expected.iter().copied().collect(),
                "roots {roots:?}, depth {depth}"
            );
            assert_eq!(
                progress.snapshot().done,
                expected.len(),
                "roots {roots:?}, depth {depth}"
            );
        }
    }

    #[test]
    fn rescan_replaces_files_and_invalidates_duplicates() {
        let dir = TestDir::new();
        let nested = dir.0.join("nested");
        fs::create_dir(&nested).unwrap();
        let top = dir.0.join("original");
        fs::write(&top, b"identical contents").unwrap();
        fs::write(nested.join("copy"), b"identical contents").unwrap();
        let mut index = FileIndex::new(HashSet::from([dir.0.clone()]), SearchConfig::default());
        index.index_dirs(None, None);
        index.process_files(None, None);
        index.find_duplicates(None, None);
        assert_eq!(index.duplicates_len(), 2);

        index.config.depth = Some(1);
        index.index_dirs(None, Some(Arc::new(AtomicBool::new(true))));
        assert_eq!(index.files_len(), 2);
        assert_eq!(index.duplicates_len(), 2);

        index.index_dirs(None, None);
        assert_eq!(index.files_len(), 1);
        assert!(index.files.contains_key(&top));
        assert!(index.duplicates.is_empty());
    }

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should follow the Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("deckard-filter-{nonce}"));
            fs::create_dir(&path).expect("test directory should be created");
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("test directory should be removed");
        }
    }

    #[test]
    fn include_globs_match_any_file_name_pattern() {
        let dir = TestDir::new();
        for name in ["kick.wav", "snare.MP3", "notes.txt"] {
            fs::write(dir.0.join(name), b"audio").expect("test file should be written");
        }
        let config = SearchConfig {
            include_filter: Some(vec!["*.wav".into(), "*.mp3".into()]),
            ..SearchConfig::default()
        };
        let mut index = FileIndex::new(HashSet::from([dir.0.clone()]), config);

        index.index_dirs(None, None);

        let names: HashSet<_> = index
            .files
            .keys()
            .filter_map(|path| path.file_name())
            .collect();
        assert_eq!(
            names,
            HashSet::from(["kick.wav".as_ref(), "snare.MP3".as_ref()])
        );
    }

    #[test]
    fn exclude_globs_take_precedence() {
        let dir = TestDir::new();
        for name in ["kick.wav", "kick-preview.wav"] {
            fs::write(dir.0.join(name), b"audio").expect("test file should be written");
        }
        let config = SearchConfig {
            include_filter: Some(vec!["*.wav".into()]),
            exclude_filter: Some(vec!["*-preview.*".into()]),
            ..SearchConfig::default()
        };
        let mut index = FileIndex::new(HashSet::from([dir.0.clone()]), config);

        index.index_dirs(None, None);

        let names: HashSet<_> = index
            .files
            .keys()
            .filter_map(|path| path.file_name())
            .collect();
        assert_eq!(names, HashSet::from(["kick.wav".as_ref()]));
    }

    #[test]
    fn max_size_keeps_boundary_and_smaller_files() {
        let dir = TestDir::new();
        for (name, contents) in [
            ("smaller.dat", b"four".as_slice()),
            ("boundary.dat", b"fives".as_slice()),
            ("larger.dat", b"sixes!".as_slice()),
        ] {
            fs::write(dir.0.join(name), contents).expect("test file should be written");
        }
        let matches = crate::cli::commands()
            .try_get_matches_from(["app", "--max_size", "5 B"])
            .expect("valid maximum size should parse");
        let config = crate::cli::augment_config(SearchConfig::default(), &matches);
        let mut index = FileIndex::new(HashSet::from([dir.0.clone()]), config);

        index.index_dirs(None, None);

        let names: HashSet<_> = index
            .files
            .keys()
            .filter_map(|path| path.file_name())
            .collect();
        assert_eq!(
            names,
            HashSet::from(["smaller.dat".as_ref(), "boundary.dat".as_ref()])
        );
    }

    #[test]
    fn indexing_progress_counts_accepted_files() {
        let dir = TestDir::new();
        for name in ["kick.wav", "snare.wav", "notes.txt"] {
            fs::write(dir.0.join(name), b"audio").expect("test file should be written");
        }
        let config = SearchConfig {
            include_filter: Some(vec!["*.wav".into()]),
            ..SearchConfig::default()
        };
        let progress = Progress::new();
        let mut index = FileIndex::new(HashSet::from([dir.0.clone()]), config);

        index.index_dirs(Some(&progress), None);

        assert_eq!(
            progress.snapshot(),
            ProgressSnapshot {
                done: 2,
                total: None,
            }
        );
    }

    #[test]
    fn processing_progress_reports_file_total() {
        let dir = TestDir::new();
        for name in ["first.dat", "second.dat"] {
            fs::write(dir.0.join(name), b"same").expect("test file should be written");
        }
        let progress = Progress::new();
        let mut index = FileIndex::new(HashSet::from([dir.0.clone()]), SearchConfig::default());
        index.index_dirs(None, None);

        index.process_files(Some(&progress), None);

        assert_eq!(
            progress.snapshot(),
            ProgressSnapshot {
                done: 2,
                total: Some(2),
            }
        );
    }

    #[test]
    fn comparison_progress_reports_file_total() {
        let dir = TestDir::new();
        for name in ["first.dat", "second.dat"] {
            fs::write(dir.0.join(name), b"same").expect("test file should be written");
        }
        let progress = Progress::new();
        let mut index = FileIndex::new(HashSet::from([dir.0.clone()]), SearchConfig::default());
        index.index_dirs(None, None);
        index.process_files(None, None);

        index.find_duplicates(Some(&progress), None);

        assert_eq!(
            progress.snapshot(),
            ProgressSnapshot {
                done: 2,
                total: Some(2),
            }
        );
    }
}
