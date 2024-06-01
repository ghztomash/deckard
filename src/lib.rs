mod files;

use files::FileEntry;
use std::collections::{HashMap, HashSet};
use std::{fs, path::Path, path::PathBuf};

pub fn visit_dirs(dirs: HashSet<PathBuf>) -> HashMap<PathBuf, FileEntry> {
    let mut files: HashMap<PathBuf, FileEntry> = HashMap::new();
    let mut dirs: HashSet<PathBuf> = HashSet::from(dirs);

    while !dirs.is_empty() {
        for dir in dirs.clone() {
            let (f, d) = visit_dir(&dir);
            dirs.remove(&dir);

            files.extend(f);
            dirs.extend(d);
        }
    }

    //println!("{:#?}", files);
    files
}

pub fn visit_dir(dir: &Path) -> (HashMap<PathBuf, FileEntry>, HashSet<PathBuf>) {
    let mut files: HashMap<PathBuf, FileEntry> = HashMap::new();
    let mut dirs: HashSet<PathBuf> = HashSet::new();

    if dir.is_dir() {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();

            if path.is_file() {
                let file = FileEntry::new(entry);
                //println!("{:#?}", file);
                //println!("{}", file);
                files.insert(file.path.clone(), file);
            } else if path.is_dir() {
                dirs.insert(path);
            }
        }
    }
    (files, dirs)
}

pub fn collect_paths<P: AsRef<Path>>(target_paths: Vec<P>) -> HashSet<PathBuf> {
    let mut paths: HashSet<PathBuf> = HashSet::with_capacity(target_paths.len());

    for path in target_paths {
        let path: PathBuf = path.as_ref().components().collect();
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
        visit_dir(Path::new("."));
        assert!(true);
    }
}
