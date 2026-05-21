use crate::{
    config::{HashAlgorithm, ImageFilterAlgorithm, ImageHashAlgorithm},
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hash(Vec<u8>);

impl From<md5::Digest> for Hash {
    fn from(d: md5::Digest) -> Self {
        Hash(d.as_bytes().to_owned())
    }
}

impl From<sha1::Digest> for Hash {
    fn from(d: sha1::Digest) -> Self {
        Hash(d.as_bytes().to_owned())
    }
}

impl From<sha2_256::Digest> for Hash {
    fn from(d: sha2_256::Digest) -> Self {
        Hash(d.as_bytes().to_owned())
    }
}

impl From<sha2_512::Digest> for Hash {
    fn from(d: sha2_512::Digest) -> Self {
        Hash(d.as_bytes().to_owned())
    }
}

impl Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in self.0.as_slice() {
            write!(f, "{b:02x}")?;
        }
        Ok(())
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

    let step = std::cmp::max(1, file_len / splits);
    // println!("step {}", step);

    let mut total_buffer = Vec::with_capacity((chunk_size * splits) as usize + 8);
    let mut temp = vec![0u8; chunk_size as usize];

    for i in 0..splits {
        let offset = i * step;
        if offset >= file_len {
            break;
        }

        let to_read = std::cmp::min(chunk_size, file_len - offset) as usize;
        // println!("reading {} bytes at {} of {}", to_read, offset, file_len);

        file.seek(std::io::SeekFrom::Start(offset))?;
        file.read_exact(&mut temp[..to_read])?;
        total_buffer.extend_from_slice(&temp[..to_read]);
    }
    // append size to the hash, otherwise files that start with the same bytes match
    total_buffer.extend_from_slice(&file_len.to_le_bytes());

    Ok(match hash {
        HashAlgorithm::MD5 => md5::chksum(&total_buffer).map(Hash::from)?,
        HashAlgorithm::SHA1 => sha1::chksum(&total_buffer).map(Hash::from)?,
        HashAlgorithm::SHA256 => sha2_256::chksum(&total_buffer).map(Hash::from)?,
        HashAlgorithm::SHA512 => sha2_512::chksum(&total_buffer).map(Hash::from)?,
    })
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
