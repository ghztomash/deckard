use chksum::{md5, sha2_256};
use chrono::prelude::*;
use colored::*;
use infer::Type;
use std::{
    fmt::{self, Display},
    fs::{self, DirEntry, File, FileType},
    io::Read,
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
    pub path: PathBuf,
    pub name: String,
    pub prefix: String,
    pub extention: Option<String>,
    pub file_type: EntryType,
    pub created: DateTime<Local>,
    pub modified: DateTime<Local>,
    pub mime_type: Option<String>,
    pub size: u64,
    pub hash: Option<String>,
    pub matching: HashSet<PathBuf>,
    pub processed: bool,
}

impl FileEntry {
    pub fn new(entry: DirEntry) -> Self {
        let metadata = entry.metadata().unwrap();

        Self {
            path: entry.path(),
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
            hash: None,
            matching: HashSet::new(),
            processed: false,
        }
    }

    pub fn full_path(&self) -> PathBuf {
        fs::canonicalize(self.path.clone()).unwrap_or(self.path.clone())
    }

    pub fn process(&mut self) {
        let mut file = File::open(&self.path).unwrap();

        let mut magic = [0; 32];
        _ = file.read_exact(&mut magic);

        // TODO: Configurable process

        // let infer = infer::get_from_path(&self.path).ok().flatten();
        // println!({:?},infer);

        // Find the MIME type
        // self.mime_type = Some(tree_magic::from_u8(&magic));
        // println!("{:?}", self.mime_type);

        self.hash = md5::chksum(magic)
            // self.hash = md5::chksum(file)
            // self.hash = sha2_256::chksum(file)
            .map(|digest| digest.to_hex_lowercase())
            .ok();
        self.processed = true;

        // println!("{:?}", self.hash);
    }

    pub fn compare(&self, other: &Self) -> bool {
        let mut matching = false;

        if self.size == other.size {
            // println!("{} and {} have the same size", self.name, other.name);

            // self.matching.insert(other.id.clone());
            // other.matching.insert(self.id.clone());

            if self.hash.is_some() && self.hash == other.hash && other.hash.is_some() {
                matching = true;
            }
        }

        //self.checked.insert(other.id.clone());
        //other.checked.insert(self.id.clone());

        matching
    }

    pub fn matches(&mut self, other: &PathBuf) {
        self.matching.insert(other.clone());
    }
}

impl Display for FileEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
            self.path.to_string_lossy().purple(),
        )
    }
}
