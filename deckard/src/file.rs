use crate::{config::SearchConfig, error::DeckardError, hasher};
use image_hasher::ImageHash;
use lofty::{
    file::{AudioFile, TaggedFileExt},
    tag::Accessor,
};
use once_cell::sync::Lazy;
use rusty_chromaprint::Configuration;
use std::{
    ffi::OsString,
    fs::{File, Metadata},
    io::{Read, Seek},
    path::{Path, PathBuf},
    time::SystemTime,
};
use tracing::{debug, warn};

static CHROMA_CFG: Lazy<Configuration> = Lazy::new(Configuration::preset_test1);
const MAGIC_SIZE: usize = 8;

#[derive(Debug, PartialEq, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: OsString,
    pub size: u64,
    pub created: Option<SystemTime>,
    pub modified: Option<SystemTime>,
    pub hash: Option<String>,
    pub image_hash: Option<ImageHash>,
    pub audio_hash: Option<Vec<u32>>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaType {
    Image,
    Audio,
    Text,
    Other,
}

impl From<&str> for MediaType {
    fn from(value: &str) -> Self {
        if value.starts_with("image") {
            Self::Image
        } else if value.starts_with("audio") {
            Self::Audio
        } else if value.starts_with("text") {
            Self::Text
        } else {
            Self::Other
        }
    }
}

impl FileEntry {
    pub fn new(path: &PathBuf, metadata: &Metadata) -> Result<Self, DeckardError> {
        Ok(Self {
            path: path.to_owned(),
            name: path
                .file_name()
                .ok_or(DeckardError::FileNameMissing)?
                .into(),
            size: metadata.len(),
            created: metadata.created().ok(),
            modified: metadata.modified().ok(),
            hash: None,
            image_hash: None,
            audio_hash: None,
        })
    }

    pub fn process(&mut self, config: &SearchConfig) -> Result<(), DeckardError> {
        let mut file = File::open(&self.path)?;

        if config.hasher_config.full_hash {
            self.hash = Some(hasher::get_full_hash(
                &config.hasher_config.hash_algorithm,
                &mut file,
            ))
        } else {
            self.hash = Some(hasher::get_quick_hash(
                &config.hasher_config.hash_algorithm,
                config.hasher_config.size,
                config.hasher_config.splits,
                &mut file,
            ))
        }

        if config.image_config.compare || config.audio_config.compare {
            match MediaType::from(get_mime_type(&self.path, &mut file).unwrap_or_default()) {
                MediaType::Image if config.image_config.compare => {
                    self.image_hash = hasher::get_image_hash(
                        &config.image_config.hash_algorithm,
                        &config.image_config.filter_algorithm,
                        config.image_config.size,
                        &self.path,
                        &mut file,
                    );
                }
                MediaType::Audio if config.audio_config.compare => {
                    self.audio_hash = hasher::get_audio_hash(&self.path, &mut file, &CHROMA_CFG);
                }
                _ => {}
            }
        }

        Ok(())
    }

    pub fn compare(&self, other: &Self, config: &SearchConfig) -> bool {
        if self.size == other.size {
            if let (Some(this_hash), Some(other_hash)) =
                (self.hash.as_deref(), other.hash.as_deref())
            {
                if this_hash == other_hash {
                    return true;
                }
            }
        }

        if config.image_config.compare
            && let (Some(this_image), Some(other_image)) =
                (self.image_hash.as_ref(), other.image_hash.as_ref())
        {
            let distance = this_image.dist(other_image);
            debug!(
                "{} and {} hamming distance: {}",
                self.name.display(),
                other.name.display(),
                distance
            );
            if distance <= config.image_config.threshold as u32 {
                return true;
            }
        }

        if config.audio_config.compare
            && let (Some(this_audio), Some(other_audio)) =
                (self.audio_hash.as_ref(), other.audio_hash.as_ref())
        {
            let chroma_config = Configuration::preset_test1();

            let segments = match rusty_chromaprint::match_fingerprints(
                this_audio,
                other_audio,
                &chroma_config,
            ) {
                Ok(s) => s,
                Err(e) => {
                    warn!("Error matching fingerprints {}", e);
                    vec![]
                }
            };

            // find average score
            let score = if segments.is_empty() {
                32.0 // is the maximum fingerprint score
            } else {
                segments.iter().map(|s| s.score).sum::<f64>() / (segments.len() as f64)
            };

            debug!(
                "{} and {} matching segments {} with score {}",
                self.name.display(),
                other.name.display(),
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

        false
    }
}

#[inline]
pub fn get_mime_type<P: AsRef<Path> + std::fmt::Debug, R: Read + Seek>(
    path: P,
    file: &mut R,
) -> Result<&'static str, DeckardError> {
    let mime = mime_guess::from_path(&path).first_raw();
    match mime {
        Some(mime_type) => Ok(mime_type),
        None => {
            let mut magic = [0; MAGIC_SIZE];
            file.rewind()?;
            let n = file.read(&mut magic)?;
            // Find the MIME type
            Ok(tree_magic::from_u8(&magic[..n]))
        }
    }
}

pub fn read_id3_tags(file: &mut File) -> Option<AudioTags> {
    let mut tags = AudioTags::default();

    let tagged_file = lofty::read_from(file).ok()?;

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

    Some(tags)
}
