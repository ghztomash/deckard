use jwalk::Parallelism;
use rayon::iter::ParallelIterator;
use rayon::iter::{IntoParallelRefIterator, ParallelBridge};
use rayon::prelude::*;
use rayon::ThreadPool;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::config::SearchConfig;
use crate::file::{EntryType, FileEntry};
use std::collections::{HashMap, HashSet};
use std::{fs, path::Path, path::PathBuf};

use log::{debug, error, trace, warn};

#[derive(Debug, Default, Clone)]
pub struct FileIndex {
    pub dirs: HashSet<PathBuf>,
    // TODO: Try BTreeMap
    pub files: HashMap<PathBuf, FileEntry>,
    pub duplicates: HashMap<PathBuf, HashSet<PathBuf>>,
    pub config: SearchConfig,
}

impl FileIndex {
    pub fn new(dirs: HashSet<PathBuf>, config: SearchConfig) -> Self {
        // Define number of threads to use
        if let Err(e) = rayon::ThreadPoolBuilder::new()
            .num_threads(config.threads)
            .build_global()
        {
            error!("error building thread pool: {:?}", e);
        }
        debug!(
            "Using thread pool with with {} threads",
            rayon::current_num_threads()
        );

        FileIndex {
            dirs,
            files: HashMap::new(),
            duplicates: HashMap::new(),
            config,
        }
    }

    pub fn index_dirs(&mut self) {
        for dir in &self.dirs {
            let index: HashMap<PathBuf, FileEntry> = jwalk::WalkDir::new(dir)
                .parallelism(Parallelism::RayonNewPool(self.config.threads))
                .sort(false)
                .skip_hidden(self.config.skip_hidden)
                .into_iter()
                .filter_map(|entry| {
                    match entry {
                        Ok(entry) => {
                            let path = entry.path();

                            if path.is_file() && !path.is_symlink() {
                                let file = FileEntry::new(
                                    path.to_owned(),
                                    entry.file_name.to_owned(),
                                    entry.metadata().unwrap(),
                                );
                                if file.file_type == EntryType::File {
                                    // Check filename filter
                                    if let Some(exclude_filter) =
                                        self.config.exclude_filter.as_ref()
                                    {
                                        if entry
                                            .file_name()
                                            .to_string_lossy()
                                            .to_lowercase()
                                            .contains(&exclude_filter.to_lowercase())
                                        {
                                            trace!(
                                                "File '{}' matches exclude filter pattern '{}'",
                                                entry.file_name().to_string_lossy(),
                                                exclude_filter
                                            );
                                            return None;
                                        }
                                    }
                                    if let Some(include_filter) =
                                        self.config.include_filter.as_ref()
                                    {
                                        if !entry
                                            .file_name()
                                            .to_string_lossy()
                                            .to_lowercase()
                                            .contains(&include_filter.to_lowercase())
                                        {
                                            return None;
                                        } else {
                                            trace!(
                                                "File '{}' matches include filter pattern '{}'",
                                                entry.file_name().to_string_lossy(),
                                                include_filter
                                            );
                                        }
                                    }
                                    // Skip empty files
                                    if self.config.skip_empty
                                        && entry.metadata().unwrap().len() == 0
                                    {
                                        trace!(
                                            "Skipping empty file {}",
                                            entry.path().to_string_lossy()
                                        );
                                        return None;
                                    }
                                    return Some((path, file));
                                }
                            }
                        }
                        Err(e) => {
                            warn!("failed reading file {}", e);
                        }
                    }
                    None
                })
                .collect();
            self.files.extend(index);
        }
    }

    pub fn process_files(&mut self, callback: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>) {
        let counter = Arc::new(AtomicUsize::new(0));
        let total = self.files_len();

        self.files.values_mut().par_bridge().for_each(|f| {
            f.process(&self.config);
            if let Some(ref callback) = callback {
                let count = counter.fetch_add(1, Ordering::SeqCst) + 1;
                callback(count, total);
            }
        });
    }

    pub fn find_duplicates(&mut self, callback: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>) {
        let vec_files: Vec<&FileEntry> = self.files.values().into_iter().collect();

        let counter = Arc::new(AtomicUsize::new(0));
        let total = vec_files.len() * (vec_files.len() - 1) / 2;

        for i in 0..vec_files.len() {
            for j in i + 1..vec_files.len() {
                let this_file = vec_files[i];
                let other_file = vec_files[j];

                // check if the files are matching
                if this_file.compare(other_file, &self.config) {
                    match self.duplicates.get_mut(&this_file.path) {
                        // file already exists, add another duplicate
                        Some(this) => {
                            this.insert(other_file.path.clone());
                        }
                        // insert a new entry
                        None => {
                            self.duplicates.insert(
                                this_file.path.clone(),
                                HashSet::from([other_file.path.clone()]),
                            );
                        }
                    };
                    // backlink this to the other file
                    match self.duplicates.get_mut(&other_file.path) {
                        // file already exists, add another duplicate
                        Some(other) => {
                            other.insert(this_file.path.clone());
                        }
                        // insert a new entry
                        None => {
                            self.duplicates.insert(
                                other_file.path.clone(),
                                HashSet::from([this_file.path.clone()]),
                            );
                        }
                    };
                }

                // Update the progress counter
                if let Some(ref callback) = callback {
                    let count = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    callback(count, total);
                }
            }
        }
    }

    pub fn files_len(&self) -> usize {
        self.files.len()
    }

    pub fn duplicates_len(&self) -> usize {
        self.duplicates.len()
    }

    pub fn file_name(&self, file: &PathBuf) -> Option<String> {
        self.files.get(file).and_then(|f| Some(f.name.clone()))
    }

    pub fn file_entry(&self, file: &PathBuf) -> Option<FileEntry> {
        self.files.get(file).and_then(|f| Some(f.clone()))
    }

    pub fn file_size(&self, file: &PathBuf) -> Option<u64> {
        self.files.get(file).and_then(|f| Some(f.size))
    }

    }
}
