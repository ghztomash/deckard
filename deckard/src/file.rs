use chrono::prelude::*;
use lofty::{
    file::{AudioFile, TaggedFileExt},
    tag::Accessor,
};
use rusty_chromaprint::Configuration;
use std::{
    ffi::OsString,
    fmt::{self, Display},
    fs::{DirEntry, File, FileType, Metadata},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use image_hasher::ImageHash;

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
            EntryType::File => "file",
            EntryType::Dir => "dir",
            EntryType::Symlink => "link",
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
    pub image_hash: Option<ImageHash>,
    pub audio_hash: Option<Vec<u32>>,
    pub audio_tags: Option<AudioTags>,
    pub processed: bool,
}

#[derive(Debug, PartialEq, Clone, Default)]
pub struct AudioTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub duration: Option<f32>,
    pub bitrate: Option<String>,
    pub sample_rate: Option<String>,
    pub bpm: Option<String>,
    pub rating: Option<String>,
    pub comment: Option<String>,
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
            audio_hash: None,
            audio_tags: None,
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
            audio_hash: None,
            audio_tags: None,
            processed: false,
        }
    }

    pub fn process(&mut self, config: &SearchConfig) {
        if self.file_type != EntryType::File {
            warn!("process: {} is not a file!", self.path.to_string_lossy());
            return;
        }

        self.mime_type = Some(get_mime_type(&self.path));
        trace!("{} found mime type {:?}", self.name, self.mime_type);

        self.hash = Some(hasher::get_quick_hash(
            &config.hasher_config.hash_algorithm,
            config.hasher_config.size,
            config.hasher_config.splits,
            &self.path,
        ));

        if config.hasher_config.full_hash {
            self.full_hash = Some(hasher::get_full_hash(
                &config.hasher_config.hash_algorithm,
                &self.path,
            ))
        }

        if let Some(mime) = self.mime_type.as_ref() {
            if mime.contains("image") && config.image_config.compare {
                self.image_hash = hasher::get_image_hash(
                    &config.image_config.hash_algorithm,
                    &config.image_config.filter_algorithm,
                    config.image_config.size,
                    &self.path,
                );
            }
        } else {
            warn!("No MIME type for file {}", self.path.to_string_lossy())
        }

        if let Some(mime) = self.mime_type.as_ref() {
            if mime.contains("audio") {
                if config.audio_config.read_tags {
                    self.audio_tags = get_id3_tags(&self.path);
                }
                if config.audio_config.compare {
                    let chroma_config = Configuration::preset_test1();
                    self.audio_hash = hasher::get_audio_hash(&self.path, &chroma_config);
                }
            }
        } else {
            warn!("No MIME type for file {}", self.path.to_string_lossy())
        }

        self.processed = true;
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

        if self.size == other.size {
            if self.hash.is_some() && self.hash == other.hash && other.hash.is_some() {
                // check the full file
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

        if config.image_config.compare && self.mime_type.is_some() && other.mime_type.is_some() {
            if self.mime_type.as_ref().unwrap().contains("image")
                && other.mime_type.as_ref().unwrap().contains("image")
                && self.image_hash.is_some()
                && other.image_hash.is_some()
            {
                let this_image = self.image_hash.as_ref().unwrap();
                let other_image = other.image_hash.as_ref().unwrap();

                let distance = this_image.dist(other_image);
                debug!(
                    "{} and {} hamming distance: {}",
                    self.name, other.name, distance
                );
                if distance <= config.image_config.threshold as u32 {
                    return true;
                }
            }
        }

        if config.audio_config.compare && self.mime_type.is_some() && other.mime_type.is_some() {
            if self.mime_type.as_ref().unwrap().contains("audio")
                && other.mime_type.as_ref().unwrap().contains("audio")
                && self.audio_hash.is_some()
                && other.audio_hash.is_some()
            {
                let this_audio = self.audio_hash.clone().unwrap();
                let other_audio = other.audio_hash.clone().unwrap();
                let chroma_config = Configuration::preset_test1();

                let segments = rusty_chromaprint::match_fingerprints(
                    &this_audio,
                    &other_audio,
                    &chroma_config,
                )
                .unwrap();

                // find average score
                let score = if !segments.is_empty() {
                    segments.iter().map(|s| s.score).sum::<f64>() / segments.len() as f64
                } else {
                    32.0 // is the maximum fingerprint score
                };

                debug!(
                    "{} and {} matching segments {} with score {}",
                    self.name,
                    other.name,
                    segments.len(),
                    score
                );

                if !segments.is_empty()
                    && segments.len() <= config.audio_config.segments_limit as usize
                    && score <= config.audio_config.threshold
                {
                    return true;
                }
            }
        }

        false
    }
}

impl Display for FileEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}  {} {} {} B : {}",
            self.file_type,
            self.name,
            self.size,
            self.created.format("%Y-%m-%d %H:%M:%S"),
            self.path.to_string_lossy(),
        )
    }
}

#[inline]
pub fn get_mime_type<P: AsRef<Path> + std::fmt::Debug>(path: P) -> String {
    let mime = mime_guess::from_path(&path).first();
    match mime {
        Some(mime_type) => mime_type.to_string(),
        None => {
            let mut file = File::open(&path).unwrap();

            let mut magic = [0; MAGIC_SIZE];
            if file.metadata().unwrap().size() >= MAGIC_SIZE as u64 {
                file.read_exact(&mut magic)
                    .unwrap_or_else(|e| warn!("read magic: {:?} for {:?}", e, path));
            }
            // Find the MIME type
            tree_magic::from_u8(&magic)
        }
    }
}

#[inline]
pub fn get_id3_tags<P: AsRef<Path> + std::fmt::Debug>(path: P) -> Option<AudioTags> {
    let mut tags = AudioTags::default();

    trace!("Reading id3 tags for file {:?}", path);

    let mut file = File::open(&path).ok()?;
    let tagged_file = lofty::read_from(&mut file).ok()?;

    let file_tag = match tagged_file.primary_tag() {
        Some(primary_tag) => primary_tag,
        // If the "primary" tag doesn't exist, just grab the first tag we can find.
        None => tagged_file.first_tag()?,
    };

    tags.title = file_tag.title().map(|b| b.to_string());
    tags.artist = file_tag.artist().map(|b| b.to_string());
    tags.album = file_tag.album().map(|b| b.to_string());
    tags.genre = file_tag.genre().map(|b| b.to_string());
    tags.comment = file_tag.comment().map(|b| b.to_string());

    tags.bpm = file_tag
        .get_string(&lofty::tag::ItemKey::Bpm)
        .map(|b| b.to_string());

    let properties = tagged_file.properties();
    tags.bitrate = properties.overall_bitrate().map(|b| b.to_string());
    tags.sample_rate = properties.sample_rate().map(|b| b.to_string());
    tags.duration = Some(properties.duration().as_secs_f32());

    trace!("{:?}", tags);

    Some(tags)
}
