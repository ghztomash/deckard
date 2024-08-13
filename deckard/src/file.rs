use base64::prelude::*;
use chksum::{md5, sha2_256};
use chrono::prelude::*;
use image_hasher::{FilterType, HashAlg, HasherConfig};
use infer::Type;
use std::{
    ffi::OsString,
    fmt::{self, Display},
    fs::{self, read, DirEntry, File, FileType, Metadata},
    io::{Read, Seek},
    os::unix::fs::MetadataExt,
    path::PathBuf,
    u32, u8, usize,
};

use image::io::Reader as ImageReader;
use image_hasher::ImageHash;
use std::io::Cursor;

use log::{debug, trace, warn};

use crate::{config::SearchConfig, hasher};

const MAGIC_SIZE: usize = 8;

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
    pub extension: Option<String>,
    pub file_type: EntryType,
    pub created: DateTime<Local>,
    pub modified: DateTime<Local>,
    pub mime_type: Option<String>,
    pub size: u64,
    pub hash: Option<String>,
    pub full_hash: Option<String>,
    pub image_hash: Option<String>,
    pub processed: bool,
}

impl FileEntry {
    pub fn new(path: PathBuf, name: OsString, metadata: Metadata) -> Self {
        Self {
            path: path.to_owned(),
            name: name.into_string().unwrap(),
            prefix: path
                .file_stem()
                .and_then(|os_str| os_str.to_str())
                .map(|s| s.to_string())
                .unwrap_or_default()
                .split('.')
                .collect::<Vec<&str>>()[0]
                .to_string(),
            extension: path
                .extension()
                .and_then(|os_str| os_str.to_str())
                .map(|s| s.to_string()),
            file_type: EntryType::new(Ok(metadata.file_type())),
            created: metadata.created().unwrap().into(),
            modified: metadata.modified().unwrap().into(),
            mime_type: None,
            size: metadata.size(),
            hash: None,
            full_hash: None,
            image_hash: None,
            processed: false,
        }
    }

    pub fn from_dir_entry(entry: DirEntry) -> Self {
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
            extension: entry
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
            full_hash: None,
            image_hash: None,
            processed: false,
        }
    }

    pub fn process(&mut self, config: &SearchConfig) {
        if self.file_type != EntryType::File {
            warn!("process: {} is not a file!", self.path.to_string_lossy());
            return;
        }
        let mut file = File::open(&self.path).unwrap();

        let mut magic = [0; MAGIC_SIZE];
        let result = file.read_exact(&mut magic);
        if result.is_err() {
            warn!("read magic: {:?} for {:?}", result, self.path);
        }

        // .expect(format!("reading {:?} size: {}, data:  {:?}", file, self.size, magic).as_str());

        // TODO: Configurable process

        // let infer = infer::get_from_path(&self.path).ok().flatten();
        // println!({:?},infer);

        // Find the MIME type
        self.mime_type = Some(tree_magic::from_u8(&magic));
        // println!("{:?}", self.mime_type);

        // self.hash = md5::chksum(&magic)
        //     // self.hash = md5::chksum(file)
        //     .map(|digest| digest.to_hex_lowercase())
        //     .ok();
        self.hash = Some(hasher::get_quick_hash(
            config.hasher_config.hash_algorithm.as_ref(),
            config.hasher_config.size,
            config.hasher_config.splits,
            &self.path,
        ));

        // self.full_hash = sha2_256::chksum(&buffer)
        //     .map(|digest| digest.to_hex_lowercase())
        //     .ok();
        if config.hasher_config.full_hash {
            self.full_hash = Some(hasher::get_full_hash(
                config.hasher_config.hash_algorithm.as_ref(),
                &self.path,
            ))
        }

        if config.image_config.check_image {
            if self.mime_type.as_ref().unwrap().contains("image") {
                // read the whole file
                let mut buffer = Vec::new();
                let result = file.rewind();
                trace!("image rewind: {:?}", result);
                let result = file.read_to_end(&mut buffer);
                trace!("image read: {:?}", result);
                match ImageReader::new(Cursor::new(&buffer)).with_guessed_format() {
                    Ok(r) => match r.decode() {
                        Ok(img) => {
                            let hasher = HasherConfig::new()
                                .hash_size(
                                    config.image_config.size as u32,
                                    config.image_config.size as u32,
                                )
                                .resize_filter(FilterType::Triangle)
                                .hash_alg(HashAlg::Gradient)
                                .to_hasher();
                            let hash = hasher.hash_image(&img);
                            self.image_hash = Some(hash.to_base64());
                            debug!(
                                "{} Image hash: {}",
                                self.path.to_string_lossy(),
                                hash.to_base64()
                            );
                        }
                        Err(e) => {
                            warn!(
                                "{} decoding image failed: {}",
                                self.path.to_string_lossy(),
                                e
                            );
                        }
                    },
                    Err(e) => {
                        warn!(
                            "{} reading image failed: {}",
                            self.path.to_string_lossy(),
                            e
                        );
                    }
                };
            }
        }

        self.processed = true;

        // println!("{:?}", self.hash);
    }

    pub fn compare(&self, other: &Self, config: &SearchConfig) -> bool {
        if self.file_type != EntryType::File {
            warn!(
                "compare self: {} is not a file!",
                self.path.to_string_lossy()
            );
            return false;
        }

        if other.file_type != EntryType::File {
            warn!(
                "compare other: {} is not a file!",
                other.path.to_string_lossy()
            );
            return false;
        }

        if config.image_config.check_image {
            if self.mime_type.as_ref().unwrap().contains("image")
                && other.mime_type.as_ref().unwrap().contains("image")
            {
                let img1: ImageHash<Vec<u8>> =
                    ImageHash::from_base64(self.image_hash.as_ref().unwrap().as_str()).unwrap();
                let img2 =
                    ImageHash::from_base64(other.image_hash.as_ref().unwrap().as_str()).unwrap();

                let distance = img1.dist(&img2);
                debug!(
                    "{} and {} Hamming Distance: {}",
                    self.path.to_string_lossy(),
                    other.path.to_string_lossy(),
                    img1.dist(&img2)
                );
                if distance <= config.image_config.threshold as u32 {
                    return true;
                }
            }
        }

        if self.size == other.size {
            // println!("{} and {} have the same size", self.name, other.name);
            if self.hash.is_some() && self.hash == other.hash && other.hash.is_some() {
                // check the full file size
                // TODO calculate full checksum hash for the files

                // let this_file = File::open(&self.path).unwrap();
                // let this_hash = sha2_256::chksum(this_file)
                //     .map(|digest| digest.to_hex_lowercase())
                //     .unwrap();

                // let other_file = File::open(&other.path).unwrap();
                // let other_hash = sha2_256::chksum(other_file)
                //     .map(|digest| digest.to_hex_lowercase())
                //     .unwrap();

                // if this_hash == other_hash {
                //     matching = true;
                // }
                if config.hasher_config.full_hash {
                    if self.full_hash.is_some()
                        && other.full_hash.is_some()
                        && self.full_hash == other.full_hash
                    {
                        return true;
                    }
                } else {
                    return true;
                }
            }
        }

        false
    }

    fn read_data() {}
}

impl Display for FileEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} : {}",
            if self.file_type == EntryType::Dir {
                format!("{}  {}", self.file_type, self.name)
            } else {
                format!("{}  {}", self.file_type, self.name)
            },
            format!("{} B", self.size),
            self.created.format("%Y-%m-%d %H:%M:%S"),
            self.path.to_string_lossy(),
        )
    }
}
