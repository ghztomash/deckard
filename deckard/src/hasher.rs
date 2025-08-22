use crate::{
    config::{HashAlgorithm, ImageFilterAlgorithm, ImageHashAlgorithm},
    error::DeckardError,
};
use chksum::{md5, sha1, sha2_256, sha2_512};
use image::{ImageFormat, io::Reader as ImageReader};
use image_hasher::{HasherConfig, ImageHash};
use rusty_chromaprint::{Configuration, Fingerprinter};
use std::{
    fmt::Display,
    fs::File,
    io::{BufReader, Read, Seek},
    path::Path,
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_NULL, DecoderOptions},
    errors::Error,
    io::MediaSourceStream,
    probe::Hint,
};
use tracing::{error, trace, warn};

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
    size: u64,
    splits: u64,
    file: &mut File,
) -> Result<Hash, DeckardError> {
    let mut size = size;
    let mut total_buffer = vec![0; 0];

    let file_len = file.metadata().unwrap().len();

    // Decide if we need to read the whole file
    let read_whole_file =
        file_len == 0 || size == 0 || splits == 0 || splits >= file_len || file_len / splits < size;

    if read_whole_file {
        file.rewind().unwrap();
        file.read_to_end(&mut total_buffer).unwrap();
    } else {
        let mut index_step = file_len / splits;
        if index_step == 0 {
            // println!("index_step too small {}", index_step);
            index_step = 1;
        }

        // println!("index_step {}", index_step);

        if (index_step * (splits - 1) + size) > file_len {
            let remaining = file_len - index_step * (splits - 1);
            // println!("file is too small {}", file_len);
            // println!("remaining {} b", remaining);
            size = remaining;
        }

        for i in 0..splits {
            let mut buffer = vec![0; size as usize];
            let index = i * index_step;
            // println!("reading {} bytes at {} of {}", size, index, file_len);

            file.seek(std::io::SeekFrom::Start(index)).unwrap();
            file.read_exact(&mut buffer).unwrap();
            total_buffer.append(&mut buffer);
        }
        // append size to the hash, otherwise files that start with the same bytes match
        total_buffer.append(&mut file_len.to_le_bytes().to_vec());
    }

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
) -> Option<ImageHash> {
    file.rewind().unwrap();
    let reader = BufReader::new(file);
    let reader = match ImageFormat::from_path(path) {
        Ok(format) => ImageReader::with_format(reader, format),
        Err(e) => {
            warn!("Failed reading image format: {}", e);
            ImageReader::new(reader)
        }
    };
    match reader.decode() {
        Ok(img) => {
            let hasher = HasherConfig::new()
                .hash_size(size as u32, size as u32)
                .resize_filter(filter.into_filter_type())
                .hash_alg(hash.into_hash_alg())
                .to_hasher();
            let hash = hasher.hash_image(&img);
            trace!("Image {:?} hash: {}", path, hash.to_base64());
            return Some(hash);
        }
        Err(e) => {
            error!("Decoding image {:?} failed: {}", path, e);
        }
    }
    None
}

#[inline]
pub fn get_audio_hash<P: AsRef<Path> + std::fmt::Debug>(
    path: P,
    file: &mut File,
) -> Option<Vec<u32>> {
    // let file = std::fs::File::open(path.as_ref()).ok()?;

    let mut hint = Hint::new();
    // Provide the file extension as a hint.
    if let Some(extension) = path.as_ref().extension()
        && let Some(extension_str) = extension.to_str()
    {
        hint.with_extension(extension_str);
    }

    file.rewind().unwrap();
    let mss = MediaSourceStream::new(Box::new(file.try_clone().ok()?), Default::default());

    // guess the format
    let probe = match symphonia::default::get_probe().format(
        &hint,
        mss,
        &Default::default(),
        &Default::default(),
    ) {
        Ok(br) => br,
        Err(e) => {
            error!("failed to prove audio format for file {:?}: {:?}", path, e);
            return None;
        }
    };
    let mut format = probe.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .expect("no supported audio tracks");

    let dec_opts: DecoderOptions = Default::default();
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &dec_opts)
        .expect("unsupported codec");

    let track_id = track.id;

    let sample_rate = 11025;
    let channels = track
        .codec_params
        .channels
        .expect("missing audio channels")
        .count() as u32;

    let mut printer = Fingerprinter::new(&Configuration::preset_test1());
    printer
        .start(sample_rate, channels)
        .expect("initializing audio fingerprinter");

    let mut sample_buf = None;

    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                if sample_buf.is_none() {
                    let spec = *audio_buf.spec();
                    let duration = audio_buf.capacity() as u64;
                    sample_buf = Some(SampleBuffer::<i16>::new(duration, spec));
                }

                if let Some(buf) = &mut sample_buf {
                    buf.copy_interleaved_ref(audio_buf);
                    printer.consume(buf.samples());
                }
            }
            Err(Error::DecodeError(_)) => (),
            Err(_) => break,
        }
    }

    printer.finish();

    Some(printer.fingerprint().to_vec())
}
