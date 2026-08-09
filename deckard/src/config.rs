use image_hasher::{FilterType, HashAlg};
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, path::PathBuf};
use tracing::{debug, error, warn};

use crate::error::DeckardError;

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct HasherConfig {
    pub full_hash: bool,
    pub hash_algorithm: HashAlgorithm,
    pub size: u64,
    pub splits: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    MD5,
    SHA1,
    SHA256,
    SHA512,
}

impl Default for HasherConfig {
    fn default() -> Self {
        Self {
            full_hash: false,
            hash_algorithm: HashAlgorithm::SHA1,
            size: 1024,
            splits: 8,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct ImageConfig {
    pub compare: bool,
    pub hash_algorithm: ImageHashAlgorithm,
    pub filter_algorithm: ImageFilterAlgorithm,
    pub size: u64,
    pub threshold: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ImageHashAlgorithm {
    Mean,
    Median,
    Gradient,
    VertGradient,
    DoubleGradient,
    Blockhash,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ImageFilterAlgorithm {
    Nearest,
    Triangle,
    CatmullRom,
    Gaussian,
    Lanczos3,
}

impl ImageHashAlgorithm {
    pub fn into_hash_alg(&self) -> HashAlg {
        match self {
            ImageHashAlgorithm::Mean => HashAlg::Mean,
            ImageHashAlgorithm::Median => HashAlg::Median,
            ImageHashAlgorithm::Gradient => HashAlg::Gradient,
            ImageHashAlgorithm::VertGradient => HashAlg::VertGradient,
            ImageHashAlgorithm::DoubleGradient => HashAlg::DoubleGradient,
            ImageHashAlgorithm::Blockhash => HashAlg::Blockhash,
        }
    }
}

impl ImageFilterAlgorithm {
    pub fn into_filter_type(&self) -> FilterType {
        match self {
            ImageFilterAlgorithm::Nearest => FilterType::Nearest,
            ImageFilterAlgorithm::Triangle => FilterType::Triangle,
            ImageFilterAlgorithm::CatmullRom => FilterType::CatmullRom,
            ImageFilterAlgorithm::Gaussian => FilterType::Gaussian,
            ImageFilterAlgorithm::Lanczos3 => FilterType::Lanczos3,
        }
    }
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            compare: false,
            hash_algorithm: ImageHashAlgorithm::Mean,
            filter_algorithm: ImageFilterAlgorithm::Nearest,
            size: 16,
            threshold: 40,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct AudioConfig {
    pub compare: bool,
    pub read_tags: bool,
    pub segments_limit: u64,
    pub threshold: f64,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            compare: false,
            read_tags: false,
            segments_limit: 2,
            threshold: 5.0,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SearchConfig {
    pub skip_hidden: bool,
    pub threads: usize,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "optional_patterns"
    )]
    pub include_filter: Option<Vec<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "optional_patterns"
    )]
    pub exclude_filter: Option<Vec<String>>,
    pub min_size: u64,
    pub hasher_config: HasherConfig,
    pub image_config: ImageConfig,
    pub audio_config: AudioConfig,
}

mod optional_patterns {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Patterns {
        One(String),
        Many(Vec<String>),
    }

    pub fn serialize<S>(patterns: &Option<Vec<String>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match patterns {
            Some(patterns) if patterns.len() == 1 => patterns[0].serialize(serializer),
            Some(patterns) => patterns.serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match Patterns::deserialize(deserializer)? {
            Patterns::One(pattern) => Some(vec![pattern]),
            Patterns::Many(patterns) => Some(patterns),
        })
    }
}

#[allow(clippy::derivable_impls)]
impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            skip_hidden: false,
            threads: 0,
            include_filter: None,
            exclude_filter: None,
            min_size: 0,
            hasher_config: HasherConfig::default(),
            image_config: ImageConfig::default(),
            audio_config: AudioConfig::default(),
        }
    }
}

impl SearchConfig {
    pub fn load(config_name: &str) -> Self {
        let config_path = match Self::get_config_path(config_name) {
            Ok(p) => p,
            Err(e) => {
                error!("failed getting config file path: {e}");
                return Self::default();
            }
        };

        debug!("load config path {:?}", config_path);
        match confy::load("deckard", config_name) {
            Ok(c) => c,
            Err(e) => {
                error!("failed loading config {e}");
                if let confy::ConfyError::BadTomlData(ee) = &e {
                    error!("{ee}");
                    warn!("deleting bad config");
                    if let Err(eee) = std::fs::remove_file(config_path) {
                        error!("failed deleting bad config {eee}");
                    }
                }
                Self::default()
            }
        }
    }

    pub fn save(&self, config_name: &str) -> Result<(), DeckardError> {
        debug!("save config path {:?}", Self::get_config_path(config_name)?);
        confy::store("deckard", config_name, self)?;
        Ok(())
    }

    pub fn get_config_path(config_name: &str) -> Result<PathBuf, DeckardError> {
        Ok(confy::get_configuration_file_path("deckard", config_name)?)
    }

    pub fn get_config_folder(config_name: &str) -> Result<PathBuf, DeckardError> {
        Ok(confy::get_configuration_file_path("deckard", config_name)?
            .parent()
            .ok_or(DeckardError::ConfigPathNotFound)?
            .to_path_buf())
    }

    pub fn edit_config(config_name: &str) -> Result<(), DeckardError> {
        let config_path = Self::get_config_path(config_name)?;
        // Check if the file actually exists
        if !config_path.is_file() {
            // You might return a custom error variant or a generic IO error, etc.
            warn!("config file didn't exist");
            Self::default().save(config_name)?;
        }

        eprintln!("Opening configuration file: {config_path:?}");
        open::that_detached(config_path)?;
        Ok(())
    }
}
