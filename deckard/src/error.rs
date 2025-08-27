use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DeckardError {
    #[error(transparent)]
    IoError(#[from] io::Error),
    #[error(transparent)]
    ConfigError(#[from] confy::ConfyError),
    #[error("Config path does not exist")]
    ConfigPathNotFound,
    #[error("File name missing")]
    FileNameMissing,
    #[error(transparent)]
    HashingFailed(#[from] chksum::Error),
    #[error(transparent)]
    ImageError(#[from] image::ImageError),
    #[error(transparent)]
    AudioError(#[from] symphonia::core::errors::Error),
    #[error("Audio track missing")]
    AudioTrackMissing,
    #[error(transparent)]
    AudioFingerprintError(#[from] rusty_chromaprint::ResetError),
    #[error("No valid paths provided")]
    NoValidPaths,
}
