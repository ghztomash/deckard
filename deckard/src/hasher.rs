use crate::{
    config::{
        HashAlgorithm, HasherConfig as FileHasherConfig, ImageFilterAlgorithm, ImageHashAlgorithm,
    },
    error::DeckardError,
};
use chksum::{md5, sha1, sha2_256, sha2_512};
use image::{ImageFormat, ImageReader};
use image_hasher::{HasherConfig, ImageHash};
use rusty_chromaprint::{Configuration, Fingerprinter};
use std::{
    fmt::Display,
    fs::File,
    io::{BufReader, Read, Seek},
    path::Path,
};
use symphonia::core::{
    codecs::audio::AudioDecoderOptions,
    errors::Error,
    formats::{FormatOptions, TrackType, probe::Hint},
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
};
use tracing::warn;

/// Content digest produced by the configured file hash algorithm.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Hash(DigestBytes);

// Keep the default SHA-1 digest inline without making every file entry carry 64 bytes.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum DigestBytes {
    Md5([u8; 16]),
    Sha1([u8; 20]),
    Long(Box<[u8]>),
}

impl From<md5::Digest> for Hash {
    fn from(d: md5::Digest) -> Self {
        Hash(DigestBytes::Md5(d.into_inner()))
    }
}

impl From<sha1::Digest> for Hash {
    fn from(d: sha1::Digest) -> Self {
        Hash(DigestBytes::Sha1(d.into_inner()))
    }
}

impl From<sha2_256::Digest> for Hash {
    fn from(d: sha2_256::Digest) -> Self {
        Hash(DigestBytes::Long(Box::new(d.into_inner())))
    }
}

impl From<sha2_512::Digest> for Hash {
    fn from(d: sha2_512::Digest) -> Self {
        Hash(DigestBytes::Long(Box::new(d.into_inner())))
    }
}

impl Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytes: &[u8] = match &self.0 {
            DigestBytes::Md5(bytes) => bytes,
            DigestBytes::Sha1(bytes) => bytes,
            DigestBytes::Long(bytes) => bytes,
        };
        for b in bytes {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl FileHasherConfig {
    /// Computes the configured full or sampled content digest of an open file.
    #[inline]
    pub fn hash_file(&self, file: &mut File) -> Result<Hash, DeckardError> {
        if self.full_hash {
            get_full_hash(&self.hash_algorithm, file)
        } else {
            get_quick_hash(&self.hash_algorithm, self.size, self.splits, file)
        }
    }
}

#[inline]
pub fn get_full_hash(hash: &HashAlgorithm, file: &mut File) -> Result<Hash, DeckardError> {
    file.rewind()?;
    Ok(match hash {
        HashAlgorithm::MD5 => md5::chksum(file).map(Hash::from)?,
        HashAlgorithm::SHA1 => sha1::chksum(file).map(Hash::from)?,
        HashAlgorithm::SHA256 => sha2_256::chksum(file).map(Hash::from)?,
        HashAlgorithm::SHA512 => sha2_512::chksum(file).map(Hash::from)?,
    })
}

#[inline]
pub fn get_quick_hash(
    hash: &HashAlgorithm,
    chunk_size: u64,
    splits: u64,
    file: &mut File,
) -> Result<Hash, DeckardError> {
    let file_len = file.metadata()?.len();
    // Decide if we need to read the whole file
    let read_whole_file = file_len == 0
        || chunk_size == 0
        || splits == 0
        || splits >= file_len
        || (file_len / splits) < chunk_size;

    if read_whole_file {
        return get_full_hash(hash, file);
    }

    match hash {
        HashAlgorithm::MD5 => hash_samples::<md5::MD5>(file, file_len, chunk_size, splits),
        HashAlgorithm::SHA1 => hash_samples::<sha1::SHA1>(file, file_len, chunk_size, splits),
        HashAlgorithm::SHA256 => {
            hash_samples::<sha2_256::SHA2_256>(file, file_len, chunk_size, splits)
        }
        HashAlgorithm::SHA512 => {
            hash_samples::<sha2_512::SHA2_512>(file, file_len, chunk_size, splits)
        }
    }
}

fn hash_samples<H: chksum::Hash>(
    file: &mut File,
    file_len: u64,
    chunk_size: u64,
    splits: u64,
) -> Result<Hash, DeckardError>
where
    H::Digest: Into<Hash>,
{
    let mut hash = H::default();
    let mut buffer = [0; 8192];
    // The whole-file fallback guarantees that every sample fits, including the last.
    let step = file_len / splits;
    for i in 0..splits {
        if i == 0 || step != chunk_size {
            file.seek(std::io::SeekFrom::Start(i * step))?;
        }
        let mut remaining = chunk_size;
        while remaining > 0 {
            let n = remaining.min(buffer.len() as u64) as usize;
            file.read_exact(&mut buffer[..n])?;
            hash.update(&buffer[..n]);
            remaining -= n as u64;
        }
    }
    hash.update(file_len.to_le_bytes());
    Ok(hash.digest().into())
}

#[inline]
pub fn get_image_hash<P: AsRef<Path> + std::fmt::Debug, R: Read + Seek>(
    hash: &ImageHashAlgorithm,
    filter: &ImageFilterAlgorithm,
    size: u64,
    path: &P,
    file: &mut R,
) -> Result<ImageHash, DeckardError> {
    file.rewind()?;
    let reader = BufReader::new(file);
    let reader = match ImageFormat::from_path(path) {
        Ok(format) => ImageReader::with_format(reader, format),
        Err(e) => {
            warn!("Failed reading image format: {}", e);
            ImageReader::new(reader)
        }
    };
    reader.decode().map(|img| {
        let hasher = HasherConfig::new()
            .hash_size(size as u32, size as u32)
            .resize_filter(filter.into_filter_type())
            .hash_alg(hash.into_hash_alg())
            .to_hasher();
        Ok(hasher.hash_image(&img))
    })?
}

#[inline]
pub fn get_audio_hash<P: AsRef<Path> + std::fmt::Debug>(
    path: P,
    file: &mut File,
) -> Result<Vec<u32>, DeckardError> {
    let mut hint = Hint::new();
    // Provide the file extension as a hint.
    if let Some(extension) = path.as_ref().extension()
        && let Some(extension_str) = extension.to_str()
    {
        hint.with_extension(extension_str);
    }

    file.rewind()?;
    let mss = MediaSourceStream::new(
        Box::new(file.try_clone()?),
        MediaSourceStreamOptions::default(),
    );

    // guess the format
    let mut format = symphonia::default::get_probe().probe(
        &hint,
        mss,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or(DeckardError::AudioTrackMissing)?;
    let track_id = track.id;

    let audio_params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or(DeckardError::AudioTrackMissing)?;

    let dec_opts = AudioDecoderOptions::default();
    let mut decoder =
        symphonia::default::get_codecs().make_audio_decoder(audio_params, &dec_opts)?;

    let mut printer = Fingerprinter::new(&Configuration::preset_test1());
    let mut printer_started = false;
    let mut samples = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) | Err(Error::ResetRequired) => break,
            Err(err) => return Err(err.into()),
        };

        if packet.track_id != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                if !printer_started {
                    let spec = audio_buf.spec();
                    printer.start(spec.rate(), spec.channels().count() as u32)?;
                    printer_started = true;
                }
                audio_buf.copy_to_vec_interleaved::<i16>(&mut samples);
                printer.consume(&samples);
            }
            Err(Error::DecodeError(_)) | Err(Error::IoError(_)) => (),
            Err(Error::ResetRequired) => break,
            Err(err) => return Err(err.into()),
        }
    }

    if !printer_started {
        return Err(DeckardError::AudioTrackMissing);
    }

    printer.finish();

    Ok(printer.fingerprint().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn sampled_digests_match_concatenated_bytes() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test_files/images/Sapeck.jpg");
        let bytes = std::fs::read(&path).unwrap();
        let mut file = File::open(path).unwrap();
        let size = bytes.len();
        for (chunk, splits) in [(1024, 8), (10_000, 3), (size / 4, 4), (0, 8), (1, 0)] {
            let mut sampled = Vec::new();
            let input = if chunk == 0 || splits == 0 {
                bytes.as_slice()
            } else {
                for i in 0..splits {
                    let offset = i * (size / splits);
                    sampled.extend_from_slice(&bytes[offset..offset + chunk]);
                }
                sampled.extend_from_slice(&(size as u64).to_le_bytes());
                sampled.as_slice()
            };
            for (algorithm, expected) in [
                (HashAlgorithm::MD5, Hash::from(md5::hash(input))),
                (HashAlgorithm::SHA1, Hash::from(sha1::hash(input))),
                (HashAlgorithm::SHA256, Hash::from(sha2_256::hash(input))),
                (HashAlgorithm::SHA512, Hash::from(sha2_512::hash(input))),
            ] {
                let actual =
                    get_quick_hash(&algorithm, chunk as u64, splits as u64, &mut file).unwrap();
                assert_eq!(actual, expected);
            }
        }
    }

    fn audio_fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("test_files")
            .join("audio")
            .join(name)
    }

    #[test]
    fn audio_hash_matches_equivalent_flac_fixtures() {
        let file_a_path = audio_fixture("440_44khz_16b_mono.flac");
        let file_b_path = audio_fixture("440_44khz_24b_mono.flac");
        let mut file_a = File::open(&file_a_path).unwrap();
        let mut file_b = File::open(&file_b_path).unwrap();

        let hash_a = get_audio_hash(&file_a_path, &mut file_a).unwrap();
        let hash_b = get_audio_hash(&file_b_path, &mut file_b).unwrap();

        assert!(!hash_a.is_empty());
        assert!(!hash_b.is_empty());

        let segments =
            rusty_chromaprint::match_fingerprints(&hash_a, &hash_b, &Configuration::preset_test1())
                .unwrap();

        assert!(!segments.is_empty());
    }
}
