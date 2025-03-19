pub mod config;
pub mod file;
mod hasher;
pub mod index;

use config::SearchConfig;
use std::collections::{HashMap, HashSet};
use std::{env, fs, path::Path, path::PathBuf};

use log::debug;

use index::FileIndex;

pub fn find_duplicates(
    dirs: HashSet<PathBuf>,
    config: SearchConfig,
) -> HashMap<PathBuf, HashSet<PathBuf>> {
    let mut file_index = FileIndex::new(dirs, config);
    file_index.index_dirs(None);
    file_index.process_files(None);
    file_index.find_duplicates(None);
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
                debug!("{:?} is part of {:?}", path, p);
                to_insert = false;
            }
        }
        if to_insert {
            paths.insert(path);
        }
    }

    paths
}

pub fn find_common_path(target_paths: &HashSet<PathBuf>) -> Option<PathBuf> {
    let paths: Vec<&Path> = target_paths.iter().map(|p| p.as_path()).collect();
    common_path::common_path_all(paths)
}

pub fn to_relative_path(path: &PathBuf) -> PathBuf {
    let current_dir = env::current_dir().expect("failed getting current directory");
    pathdiff::diff_paths(path, current_dir).expect("failed getting relative path")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_common_path() {}

    #[test]
    fn collect_different_path() {}

    #[test]
    fn common_path() {
        let paths: HashSet<PathBuf> = [
            PathBuf::from("/home/user/tmp/coverage/test"),
            PathBuf::from("/home/user/tmp/covert/operator"),
            PathBuf::from("/home/user/tmp/coven/members"),
        ]
        .iter()
        .cloned()
        .collect();

        let common = find_common_path(&paths);
        assert_eq!(common, Some(PathBuf::from("/home/user/tmp")));
    }

    #[test]
    fn no_common_path() {
        let paths: HashSet<PathBuf> = [
            PathBuf::from("/home/user/tmp/covert/operator"),
            PathBuf::from("./coven/members"),
        ]
        .iter()
        .cloned()
        .collect();

        let common = find_common_path(&paths);
        assert_eq!(common, None);
    }
}
