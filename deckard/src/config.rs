use image_hasher::{FilterType, HashAlg};
use log::{debug, error};
use serde::{Deserialize, Serialize};
use std::{fs::File, path::PathBuf};

#[derive(Serialize, Deserialize, Debug)]
pub struct HasherConfig {
    pub full_hash: bool,
    pub hash_algorithm: HashAlgorithm,
    pub size: u64,
    pub splits: u64,
}

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug)]
pub struct AudioConfig {
    pub compare: bool,
    pub segments_limit: u64,
    pub threshold: f64,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            compare: false,
            segments_limit: 2,
            threshold: 5.0,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SearchConfig {
    pub skip_empty: bool,
    pub skip_hidden: bool,
    pub threads: usize,
    pub include_filter: Option<String>,
    pub exclude_filter: Option<String>,
    pub hasher_config: HasherConfig,
    pub image_config: ImageConfig,
    pub audio_config: AudioConfig,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            skip_empty: false,
            skip_hidden: false,
            threads: 0,
            include_filter: None,
            exclude_filter: None,
            hasher_config: HasherConfig::default(),
            image_config: ImageConfig::default(),
            audio_config: AudioConfig::default(),
        }
    }
}

impl SearchConfig {
    pub fn load(config_name: &str) -> Self {
        debug!(
            "load config path {:?}",
            confy::get_configuration_file_path("deckard", config_name).unwrap()
        );
        match confy::load("deckard", config_name) {
            Ok(c) => {
                return c;
            }
            Err(e) => {
                match &e {
                    confy::ConfyError::BadTomlData(_) => {
                        std::fs::remove_file(
                            confy::get_configuration_file_path("deckard", config_name).unwrap(),
                        )
                        .unwrap();
                    }
                    _ => {}
                }
                error!("failed loading config {:?}", e);
                return Self::default();
            }
        }
    }

    pub fn save(&self, config_name: &str) {
        debug!(
            "save config path {:?}",
            confy::get_configuration_file_path("deckard", config_name).unwrap()
        );
        confy::store("deckard", config_name, &self).unwrap();
    }

    pub fn get_config_path(config_name: &str) -> PathBuf {
        confy::get_configuration_file_path("deckard", config_name).unwrap()
    }
}
