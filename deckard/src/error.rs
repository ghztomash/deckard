use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DeckardError {
    #[error(transparent)]
    IoError(#[from] io::Error),
    #[error(transparent)]
    ConfigError(#[from] confy::ConfyError),
    #[error("Config path does not exist")]
    ConfigPathNotFound(),
}
