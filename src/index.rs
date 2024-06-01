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
        let vec_files: Vec<&FileEntry> = self.files.values().into_iter().collect();

        for i in 0..vec_files.len() {
            for j in i + 1..vec_files.len() {
                let this_file = vec_files[i];
                let other_file = vec_files[j];

                // check if the files are matching
                if this_file.compare(other_file) {
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

    pub fn file(&self, file: &PathBuf) -> Option<FileEntry> {
        self.files.get(file).and_then(|f| Some(f.clone()))
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
