use chrono::prelude::*;
use infer::Type;
use std::{
    fmt::{self, Display},
    fs::{self, DirEntry, FileType},
    os::unix::fs::MetadataExt,
    path::PathBuf,
    time::SystemTime,
};

#[derive(Debug)]
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
            EntryType::Symlink => "l",
            EntryType::Unknown => "?",
        };
        write!(f, "{}", symbol)
    }
}

#[derive(Debug)]
pub struct FileEntry {
    pub id: u64,
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
    pub hash: Option<u64>,
    pub processed: bool,
}

impl FileEntry {
    pub fn new(entry: DirEntry, depth: usize) -> Self {
        let metadata = entry.metadata().unwrap();
        Self {
            id: 0,
            path: fs::canonicalize(&entry.path()).unwrap_or(entry.path()),
            name: entry.file_name().into_string().unwrap_or_default(),
            prefix: entry
                .path()
                .file_stem()
                .and_then(|os_str| os_str.to_str())
                .map(|s| s.to_string())
                .unwrap_or_default()
                .split(".")
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
            // mime_type: infer::get_from_path(entry.path()).unwrap(),
            mime_type: None,
            size: metadata.size(),
            depth,
            hash: None,
            processed: false,
        }
    }
}

impl Display for FileEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        _ = write!(f, "{}", "  ".to_string().repeat(self.depth));
        write!(
            f,
            "{}  {} {}B",
            self.file_type,
            self.name,
            self.size,
        )
    }
}
