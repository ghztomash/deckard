use std::{
    fmt,
    fmt::Display,
    fs::{self, DirEntry, FileType, Metadata},
    os::unix::fs::MetadataExt,
    path::PathBuf,
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
    pub path: PathBuf,
    pub full_path: PathBuf,
    pub name: String,
    pub stem: String,
    pub extention: String,
    pub file_type: EntryType,
    pub metadata: Metadata,
    pub size: u64,
    pub depth: usize,
}

impl FileEntry {
    pub fn new(entry: DirEntry, depth: usize) -> Self {
        let metadata = entry.metadata().unwrap();
        Self {
            path: entry.path(),
            full_path: fs::canonicalize(&entry.path()).unwrap_or(entry.path()),
            name: entry.file_name().into_string().unwrap_or_default(),
            stem: entry
                .path()
                .file_stem()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default()
                .to_string(),
            extention: entry
                .path()
                .extension()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default()
                .to_string(),
            file_type: EntryType::new(entry.file_type()),
            metadata: metadata.clone(),
            size: metadata.size(),
            depth,
        }
    }
}

impl Display for FileEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        _ = write!(f, "{}", " ".to_string().repeat(self.depth));
        write!(
            f,
            "{} {}B {}",
            self.path.to_str().unwrap_or_default(),
            self.size,
            self.file_type
        )
    }
}
