use std::path::PathBuf;

use log::debug;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct SearchConfig {
    pub skip_empty: bool,
    pub skip_hidden: bool,
    pub full_hash: bool,
    pub check_image: bool,
    pub include_filter: Option<String>,
    pub exclude_filter: Option<String>,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            skip_empty: false,
            skip_hidden: false,
            full_hash: false,
            check_image: false,
            include_filter: None,
            exclude_filter: None,
        }
    }
}

impl SearchConfig {
    pub fn load(config_name: &str) -> Self {
        debug!(
            "load config path {:?}",
            confy::get_configuration_file_path("deckard", config_name).unwrap()
        );
        confy::load("deckard", config_name).unwrap()
    }

    pub fn save(&self, config_name: &str) {
        debug!(
            "save config path {:?}",
            confy::get_configuration_file_path("deckard", config_name).unwrap()
        );
        confy::store("deckard", config_name, &self).unwrap();
    }

    pub fn get_config_path() -> PathBuf {
        confy::get_configuration_file_path("deckard", None).unwrap()
    }
}
