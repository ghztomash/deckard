pub mod file;
mod hasher;
pub mod index;

use file::{EntryType, FileEntry};
use std::collections::{HashMap, HashSet};
use std::{fs, path::Path, path::PathBuf};

use index::FileIndex;

pub fn find_duplicates(dirs: HashSet<PathBuf>) -> HashMap<PathBuf, HashSet<PathBuf>> {
    let mut file_index = FileIndex::new(dirs);
    file_index.index_dirs();
    file_index.process_files();
    file_index.find_duplicates();
    file_index.duplicates
}

pub fn collect_paths<P: AsRef<Path>>(target_paths: Vec<P>) -> HashSet<PathBuf> {
    let mut paths: HashSet<PathBuf> = HashSet::with_capacity(target_paths.len());

    for path in target_paths {
        let path: PathBuf = path.as_ref().components().collect();
        let path = fs::canonicalize(&path).unwrap_or(path);

        let mut to_insert = true;

        // don't insert subfolders like
        // path/ path/sub_path
        for p in &paths {
            if path.starts_with(p) {
                println!("{:?} is part of {:?}", path, p);
                to_insert = false;
            }
        }
        if to_insert {
            paths.insert(path);
        }
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        // visit_dir(Path::new("."));
        assert!(true);
    }
}
