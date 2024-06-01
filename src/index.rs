use crate::file::{EntryType, FileEntry};
use std::collections::{HashMap, HashSet};
use std::{fs, path::Path, path::PathBuf};

pub struct FileIndex {
    pub dirs: HashSet<PathBuf>,
    pub files: HashMap<PathBuf, FileEntry>,
    pub duplicates: HashMap<PathBuf, HashSet<PathBuf>>,
}

impl FileIndex {
    pub fn new(dirs: HashSet<PathBuf>) -> Self {
        FileIndex {
            dirs,
            files: HashMap::new(),
            duplicates: HashMap::new(),
        }
    }

    pub fn index_dirs(&mut self) {
        let mut dirs: HashSet<PathBuf> = self.dirs.clone();

        while !dirs.is_empty() {
            for dir in dirs.clone() {
                let (f, d) = visit_dir(&dir);
                dirs.remove(&dir);

                self.files.extend(f);
                dirs.extend(d);
            }
        }
    }

    pub fn process_files(&mut self) {
        for (_, f) in &mut self.files {
            f.process();
        }
    }

    pub fn find_duplicates(&mut self) {
        let mut vec_files = Vec::with_capacity(self.files.len());
        // let mut vec_files: Vec<&mut FileEntry> = files_index.values_mut().into_iter().collect();

        for (_, f) in &self.files {
            vec_files.push(f);
        }

        for i in 0..vec_files.len() {
            for j in i + 1..vec_files.len() {
                let f = &vec_files[i];
                let ff = &vec_files[j];
                let matching = f.compare(ff);

                if matching {
                    match self.duplicates.get_mut(&f.path) {
                        Some(ref mut v) => {
                            v.insert(ff.path.clone());
                        }
                        None => {
                            self.duplicates
                                .insert(f.path.clone(), HashSet::from([ff.path.clone()]));
                        }
                    };
                    match self.duplicates.get_mut(&ff.path) {
                        Some(ref mut v) => {
                            v.insert(f.path.clone());
                        }
                        None => {
                            self.duplicates
                                .insert(ff.path.clone(), HashSet::from([f.path.clone()]));
                        }
                    };
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
}

fn visit_dir(dir: &Path) -> (HashMap<PathBuf, FileEntry>, HashSet<PathBuf>) {
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
