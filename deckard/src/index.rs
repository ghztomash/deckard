use dashmap::DashMap;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use jwalk::Parallelism;
use rayon::iter::ParallelIterator;
use rayon::iter::{IntoParallelRefIterator, ParallelBridge};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};

use std::ffi::OsString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use crate::config::SearchConfig;
use crate::file::FileEntry;
use std::collections::{HashMap, HashSet};
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

    pub fn index_dirs(
        &mut self,
        callback: Option<Arc<dyn Fn(usize) + Send + Sync>>,
        cancel: Option<Arc<AtomicBool>>,
    ) {
        let include_filter = build_glob_set(self.config.include_filter.as_deref());
        let exclude_filter = build_glob_set(self.config.exclude_filter.as_deref());
        let counter = Arc::new(AtomicUsize::new(0));
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
        for dir in &self.dirs {
            let index: HashMap<Arc<PathBuf>, FileEntry> = jwalk::WalkDir::new(dir)
                .parallelism(parallelism.to_owned())
                .sort(false)
                .skip_hidden(self.config.skip_hidden)
                .into_iter()
                .filter_map(|entry| {
                    if let Some(cancel) = cancel.as_ref()
                        && cancel.load(Ordering::Relaxed)
                    {
                        // TODO: this doesn't really short circuit the parallel iterator
                        return None;
                    }
                    match entry {
                        Ok(entry) => {
                            let path = entry.path();

                            if path.is_file() && !path.is_symlink() {
                                // Check filename filter
                                let file_name = entry.file_name().to_string_lossy();
                                if let Some(exclude_filter) = exclude_filter.as_ref()
                                    && exclude_filter.is_match(file_name.as_ref())
                                {
                                    trace!("File '{}' matches an exclude filter", file_name);
                                    return None;
                                }
                                if let Some(include_filter) = include_filter.as_ref() {
                                    if !include_filter.is_match(file_name.as_ref()) {
                                        return None;
                                    } else {
                                        trace!("File '{}' matches an include filter", file_name);
                                    }
                                }

                                // Skip files that are smaller in size
                                let metadata = entry
                                    .metadata()
                                    .inspect_err(|e| error!("Error reading metadata {}", e))
                                    .ok()?;
                                let file_size = metadata.len();
                                if file_size < self.config.min_size {
                                    trace!(
                                        "Skipping file {}, size {} smaller than {}",
                                        file_name, file_size, self.config.min_size,
                                    );
                                    return None;
                                }

                                let path = Arc::new(path);
                                let file = FileEntry::new(&path, &metadata)
                                    .inspect_err(|e| error!("Error reading File Entry {}", e))
                                    .ok()?;

                                // Update the progress counter
                                if let Some(ref callback) = callback {
                                    let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
                                    callback(count);
                                }
                                return Some((path, file));
                            }
                        }
                        Err(e) => {
                            warn!("failed indexing file {}", e);
                        }
                    }
                    None
                })
                .collect();
            self.files.extend(index);
        }
    }

    pub fn process_files(
        &mut self,
        callback: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
        cancel: Option<Arc<AtomicBool>>,
    ) {
        let counter = Arc::new(AtomicUsize::new(0));
        let total = self.files_len();

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
                if let Some(ref callback) = callback {
                    let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
                    callback(count, total);
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

    pub fn find_duplicates(
        &mut self,
        callback: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
        cancel: Option<Arc<AtomicBool>>,
    ) {
        let vec_files: Vec<&FileEntry> = self.files.values().collect();

        let counter = Arc::new(AtomicUsize::new(0));
        let total = vec_files.len();

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
                    // Update the progress counter
                    if let Some(ref callback) = callback {
                        let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
                        callback(count, total);
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
    use std::fs;
    use std::time::UNIX_EPOCH;

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
}
