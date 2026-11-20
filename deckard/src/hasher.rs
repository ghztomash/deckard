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
    cell::RefCell,
    fmt::Display,
    fs::File,
    io::{BufReader, ErrorKind, IsTerminal, Read, Seek, SeekFrom},
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
    /// Computes the configured digest from the start of an open file.
    pub fn hash_file(&self, file: &mut File) -> Result<Hash, DeckardError> {
        let sampled_len = if self.full_hash || self.size == 0 || self.splits == 0 {
            None
        } else {
            let len = file.metadata()?.len();
            (self.splits < len && self.size <= len / self.splits).then_some(len)
        };
        self.hash_content(file, sampled_len)
    }

    fn hash_content(
        &self,
        file: &mut File,
        sampled_len: Option<u64>,
    ) -> Result<Hash, DeckardError> {
        // A worker initializes only the algorithms it uses. All share one read buffer.
        thread_local! {
            static BUFFER: RefCell<[u8; 8192]> = const { RefCell::new([0; 8192]) };
            static MD5: RefCell<md5::MD5> = RefCell::default();
            static SHA1: RefCell<sha1::SHA1> = RefCell::default();
            static SHA256: RefCell<sha2_256::SHA2_256> = RefCell::default();
            static SHA512: RefCell<sha2_512::SHA2_512> = RefCell::default();
        }

        BUFFER.with_borrow_mut(|buffer| match self.hash_algorithm {
            HashAlgorithm::MD5 => {
                MD5.with_borrow_mut(|hash| self.hash_with(hash, buffer, file, sampled_len))
            }
            HashAlgorithm::SHA1 => {
                SHA1.with_borrow_mut(|hash| self.hash_with(hash, buffer, file, sampled_len))
            }
            HashAlgorithm::SHA256 => {
                SHA256.with_borrow_mut(|hash| self.hash_with(hash, buffer, file, sampled_len))
            }
            HashAlgorithm::SHA512 => {
                SHA512.with_borrow_mut(|hash| self.hash_with(hash, buffer, file, sampled_len))
            }
        })
    }

    fn hash_with<H: chksum::Hash>(
        &self,
        hash: &mut H,
        buffer: &mut [u8],
        file: &mut File,
        sampled_len: Option<u64>,
    ) -> Result<Hash, DeckardError>
    where
        H::Digest: Into<Hash>,
    {
        hash.reset();
        file.rewind()?;
        if let Some(len) = sampled_len {
            let step = len / self.splits;
            for i in 0..self.splits {
                if i > 0 && step != self.size {
                    file.seek(SeekFrom::Start(i * step))?;
                }
                let mut remaining = self.size;
                while remaining > 0 {
                    let n = remaining.min(buffer.len() as u64) as usize;
                    file.read_exact(&mut buffer[..n])?;
                    hash.update(&buffer[..n]);
                    remaining -= n as u64;
                }
            }
            hash.update(len.to_le_bytes());
        } else {
            if file.is_terminal() {
                return Err(chksum::Error::IsTerminal.into());
            }
            loop {
                match file.read(buffer) {
                    Ok(0) => break,
                    Ok(n) => hash.update(&buffer[..n]),
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(e) => return Err(chksum::Error::Io(e).into()),
                }
            }
        }
        Ok(hash.digest().into())
    }
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
                let config = FileHasherConfig {
                    hash_algorithm: algorithm,
                    size: chunk as u64,
                    splits: splits as u64,
                    full_hash: false,
                };
                let actual = config.hash_file(&mut file).unwrap();
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn digest_recovers_after_incomplete_sample() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test_files");
        let short = root.join("file.txt");
        let len = short.metadata().unwrap().len();
        assert!(len > 0);
        let next = root.join("images/Sapeck.jpg");
        let bytes = std::fs::read(&next).unwrap();
        for (algorithm, expected) in [
            (HashAlgorithm::MD5, Hash::from(md5::hash(&bytes))),
            (HashAlgorithm::SHA1, Hash::from(sha1::hash(&bytes))),
            (HashAlgorithm::SHA256, Hash::from(sha2_256::hash(&bytes))),
            (HashAlgorithm::SHA512, Hash::from(sha2_512::hash(&bytes))),
        ] {
            let mut config = FileHasherConfig {
                hash_algorithm: algorithm,
                size: len,
                splits: 2,
                full_hash: false,
            };
            // Simulate a file shortened after metadata was read. The first sample succeeds.
            let err = config
                .hash_content(&mut File::open(&short).unwrap(), Some(2 * len))
                .unwrap_err();
            assert!(
                matches!(err, DeckardError::IoError(e) if e.kind() == ErrorKind::UnexpectedEof)
            );

            config.full_hash = true;
            assert_eq!(
                config.hash_file(&mut File::open(&next).unwrap()).unwrap(),
                expected
            );
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
