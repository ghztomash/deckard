pub mod cli;
pub mod config;
pub mod error;
pub mod file;
mod hasher;
pub mod index;

use config::SearchConfig;
use std::collections::HashSet;
use std::{env, fs, path::Path, path::PathBuf};
use tracing::{error, warn};

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
                warn!("{:?} is part of {:?}", path, p);
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

/// Validate at least one of the provided paths exist
pub fn validate_paths(target_paths: &HashSet<PathBuf>) -> bool {
    if target_paths.is_empty() {
        return true;
    }

    let mut atleast_one_path_valid = false;
    for path in target_paths {
        if path.exists() {
            atleast_one_path_valid = true;
        }
    }

    atleast_one_path_valid
}

pub fn to_relative_path(path: &PathBuf) -> PathBuf {
    env::current_dir()
        .map_err(|e| {
            error!("failed getting current_dir: {e}");
            e
        })
        .ok()
        .and_then(|current_dir| pathdiff::diff_paths(path, current_dir))
        .unwrap_or_else(|| path.to_owned())
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
