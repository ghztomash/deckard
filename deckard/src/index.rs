use chrono::{DateTime, Local};
use dashmap::DashMap;
use jwalk::Parallelism;
use rayon::iter::ParallelIterator;
use rayon::iter::{IntoParallelRefIterator, ParallelBridge};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use crate::config::SearchConfig;
use crate::file::{EntryType, FileEntry};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use tracing::{debug, error, trace, warn};

#[derive(Debug, Default, Clone)]
pub struct FileIndex {
    pub dirs: HashSet<PathBuf>,
    // TODO: Try BTreeMap
    pub files: HashMap<PathBuf, FileEntry>,
    pub duplicates: HashMap<PathBuf, HashSet<PathBuf>>,
    pub config: SearchConfig,
    pub pool: Option<Arc<ThreadPool>>,
}

impl FileIndex {
    pub fn new(dirs: HashSet<PathBuf>, config: SearchConfig) -> Self {
        // Build a local thread pool
        debug!(
            "Building local Rayon thread pool with {} threads",
            rayon::current_num_threads()
        );
        let pool = ThreadPoolBuilder::new()
            .num_threads(config.threads)
            .thread_name(|i| format!("deckard-pool-{i}"))
            .build()
            .or_else(|e| {
                error!("Error building local thread pool: {:?}", e);
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
            let index: HashMap<PathBuf, FileEntry> = jwalk::WalkDir::new(dir)
                .parallelism(parallelism.to_owned())
                .sort(false)
                .skip_hidden(self.config.skip_hidden)
                .into_iter()
                .filter_map(|entry| {
                    if let Some(cancel) = cancel.as_ref() {
                        if cancel.load(Ordering::Relaxed) {
                            // TODO: this doesn't really short circuit the parallel iterator
                            return None;
                        }
                    }
                    match entry {
                        Ok(entry) => {
                            let path = entry.path();

                            if path.is_file() && !path.is_symlink() {
                                let metadata = entry.metadata().ok()?;
                                let file = FileEntry::new(
                                    path.to_owned(),
                                    entry.file_name.to_owned(),
                                    metadata.to_owned(),
                                );
                                if file.file_type == EntryType::File {
                                    // Check filename filter
                                    let file_name = entry.file_name().to_string_lossy();
                                    if let Some(exclude_filter) =
                                        self.config.exclude_filter.as_ref()
                                    {
                                        if file_name
                                            .to_lowercase()
                                            .contains(&exclude_filter.to_lowercase())
                                        {
                                            trace!(
                                                "File '{}' matches exclude filter pattern '{}'",
                                                file_name, exclude_filter
                                            );
                                            return None;
                                        }
                                    }
                                    if let Some(include_filter) =
                                        self.config.include_filter.as_ref()
                                    {
                                        if !file_name
                                            .to_lowercase()
                                            .contains(&include_filter.to_lowercase())
                                        {
                                            return None;
                                        } else {
                                            trace!(
                                                "File '{}' matches include filter pattern '{}'",
                                                file_name, include_filter
                                            );
                                        }
                                    }

                                    // Skip files that are smaller in size
                                    let file_size = metadata.len();
                                    if file_size < self.config.min_size {
                                        trace!(
                                            "Skipping file {}, size {} smaller than {}",
                                            file_name, file_size, self.config.min_size,
                                        );
                                        return None;
                                    }

                                    // Update the progress counter
                                    if let Some(ref callback) = callback {
                                        let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
                                        callback(count);
                                    }
                                    return Some((path, file));
                                }
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
                if let Some(cancel) = cancel.as_ref() {
                    if cancel.load(Ordering::Relaxed) {
                        // short circit the parallel iterator
                        // TODO: this still doesn't cancel ongoing processing
                        return Err(());
                    }
                }
                f.process(&self.config);
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
                    if let Some(cancel) = cancel.as_ref() {
                        if cancel.load(Ordering::Relaxed) {
                            // short circit the parallel iterator
                            return Err(());
                        }
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

    pub fn file_name(&self, file: &PathBuf) -> Option<String> {
        self.files.get(file).map(|f| f.name.to_owned())
    }

    pub fn file_entry(&self, file: &PathBuf) -> Option<FileEntry> {
        self.files.get(file).cloned()
    }

    pub fn file_size(&self, file: &PathBuf) -> Option<u64> {
        self.files.get(file).map(|f| f.size)
    }

    pub fn file_date_modified(&self, file: &PathBuf) -> Option<DateTime<Local>> {
        self.files.get(file).map(|f| f.modified)
    }

    pub fn file_date_created(&self, file: &PathBuf) -> Option<DateTime<Local>> {
        self.files.get(file).map(|f| f.created)
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
