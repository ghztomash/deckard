use chksum::md5;
use chrono::prelude::*;
use colored::*;
use infer::Type;
use std::{
    fmt::{self, Display},
    fs::{self, DirEntry, File, FileType},
    os::unix::fs::MetadataExt,
    path::PathBuf,
};

use std::collections::HashSet;

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum EntryType {
    File,
    Dir,
    Symlink,
    Unknown,
}

impl EntryType {
    pub fn new(file: std::io::Result<FileType>) -> Self {
        if file.is_err() {
            return EntryType::Unknown;
        }
        let file = file.unwrap();

        if file.is_dir() {
            return EntryType::Dir;
        } else if file.is_symlink() {
            return EntryType::Symlink;
        } else if file.is_file() {
            return EntryType::File;
        }
        EntryType::Unknown
    }
}

impl Display for EntryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let symbol = match self {
            EntryType::File => "",
            EntryType::Dir => "",
            EntryType::Symlink => "󰈲",
            EntryType::Unknown => "?",
        };
        write!(f, "{}", symbol)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct FileEntry {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub prefix: String,
    pub extention: Option<String>,
    pub file_type: EntryType,
    pub created: DateTime<Local>,
    pub modified: DateTime<Local>,
    pub mime_type: Option<Type>,
    pub size: u64,
    pub depth: usize,
    pub hash: Option<String>,
    pub checked: HashSet<String>,
    pub matching: HashSet<String>,
    pub processed: bool,
}

impl FileEntry {
    pub fn new(entry: DirEntry, depth: usize) -> Self {
        let metadata = entry.metadata().unwrap();
        let path = fs::canonicalize(entry.path()).unwrap_or(entry.path());

        let digest = md5::hash(path.to_str().unwrap());

        Self {
            id: digest.to_hex_lowercase(),
            path,
            name: entry.file_name().into_string().unwrap(),
            prefix: entry
                .path()
                .file_stem()
                .and_then(|os_str| os_str.to_str())
                .map(|s| s.to_string())
                .unwrap_or_default()
                .split('.')
                .collect::<Vec<&str>>()[0]
                .to_string(),
            extention: entry
                .path()
                .extension()
                .and_then(|os_str| os_str.to_str())
                .map(|s| s.to_string()),
            file_type: EntryType::new(entry.file_type()),
            created: metadata.created().unwrap().into(),
            modified: metadata.modified().unwrap().into(),
            mime_type: None,
            size: metadata.size(),
            depth,
            hash: None,
            checked: HashSet::new(),
            matching: HashSet::new(),
            processed: false,
        }
    }

    pub fn process(&mut self) {
        self.mime_type = infer::get_from_path(&self.path).ok().flatten();
        let file = File::open(&self.path).unwrap();
        self.hash = md5::chksum(file)
            .map(|digest| digest.to_hex_lowercase())
            .ok();
        self.processed = true;
    }

    pub fn compare(&self, other: &Self) -> bool {
        let mut matching = false;

        if self.size == other.size {
            println!("{} and {} have the same size", self.name, other.name);

            //self.matching.insert(other.id.clone());
            //other.matching.insert(self.id.clone());

            if self.hash.is_some() && self.hash == other.hash && other.hash.is_some() {
                matching = true;
            }
        }

        //self.checked.insert(other.id.clone());
        //other.checked.insert(self.id.clone());

        matching
    }

    pub fn matches(&mut self, other: String) {
        self.matching.insert(other);
    }
}

impl Display for FileEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        _ = write!(f, "{}", "  ".to_string().repeat(self.depth));
        write!(
            f,
            "{} {} {} : {}",
            if self.file_type == EntryType::Dir {
                format!("{}  {}", self.file_type, self.name)
                    .bright_green()
                    .to_string()
            } else {
                format!("{}  {}", self.file_type, self.name.bold())
            },
            format!("{} B", self.size).to_string().yellow(),
            self.created
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
                .bright_blue(),
            self.id.purple(),
        )
    }
}
