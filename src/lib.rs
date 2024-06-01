pub mod file;

use file::{EntryType, FileEntry};
use std::collections::{HashMap, HashSet};
use std::{fs, path::Path, path::PathBuf};

use colored::*;

pub fn index_dirs(dirs: HashSet<PathBuf>) -> HashMap<PathBuf, FileEntry> {
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
                if file.file_type == EntryType::File {
                    files.insert(file.path.clone(), file);
                }
            } else if path.is_dir() {
                dirs.insert(path);
            }
        }
    }
    (files, dirs)
}

pub fn process_files(files_index: &mut HashMap<PathBuf, FileEntry>) {
    for (_, f) in files_index {
        f.process();
    }
}

pub fn find_matches(
    files_index: &HashMap<PathBuf, FileEntry>,
) -> HashMap<PathBuf, HashSet<PathBuf>> {
    let mut file_matches: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();
    let mut vec_files = Vec::with_capacity(files_index.capacity());

    // let mut vec_files: Vec<&mut FileEntry> = files_index.values_mut().into_iter().collect();

    for (_, f) in files_index {
        vec_files.push(f);
    }

    for i in 0..vec_files.len() {
        for j in i + 1..vec_files.len() {
            let f = &vec_files[i];
            let ff = &vec_files[j];
            let matching = f.compare(ff);
            if matching {
                match file_matches.get_mut(&f.path) {
                    Some(ref mut v) => {
                        v.insert(ff.path.clone());
                    }
                    None => {
                        file_matches.insert(f.path.clone(), HashSet::from([ff.path.clone()]));
                    }
                };
                match file_matches.get_mut(&ff.path) {
                    Some(ref mut v) => {
                        v.insert(f.path.clone());
                    }
                    None => {
                        file_matches.insert(ff.path.clone(), HashSet::from([f.path.clone()]));
                    }
                };
                //f.matches(ff.id);
                //ff.matches(f.id);
            }
        }
    }

    file_matches
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
