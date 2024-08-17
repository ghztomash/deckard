use std::path::PathBuf;

use log::{debug, error};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct HasherConfig {
    pub full_hash: bool,
    pub hash_algorithm: String,
    pub size: u64,
    pub splits: u64,
}

impl Default for HasherConfig {
    fn default() -> Self {
        Self {
            full_hash: false,
            hash_algorithm: "sha1".to_string(),
            size: 1024,
            splits: 8,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ImageConfig {
    pub check_image: bool,
    pub hash_algorithm: String,
    pub filter_algorithm: String,
    pub size: u64,
    pub threshold: u64,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            check_image: false,
            hash_algorithm: "mean".to_string(),
            filter_algorithm: "nearest".to_string(),
            size: 16,
            threshold: 40,
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
