use crate::config::{HashAlgorithm, ImageFilterAlgorithm, ImageHashAlgorithm};
use chksum::{md5, sha1, sha2_256, sha2_512};
use image::io::Reader as ImageReader;
use image_hasher::{HasherConfig, ImageHash};
use rusty_chromaprint::{Configuration, Fingerprinter};
use std::{
    fs::File,
    io::{Read, Seek},
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

#[inline]
pub fn get_full_hash<P: AsRef<Path>>(hash: &HashAlgorithm, path: P) -> String {
    let file = File::open(path).unwrap();
    match hash {
        HashAlgorithm::MD5 => md5::chksum(file).unwrap().to_hex_lowercase(),
        HashAlgorithm::SHA1 => sha1::chksum(file).unwrap().to_hex_lowercase(),
        HashAlgorithm::SHA256 => sha2_256::chksum(file).unwrap().to_hex_lowercase(),
        HashAlgorithm::SHA512 => sha2_512::chksum(file).unwrap().to_hex_lowercase(),
    }
}

#[inline]
pub fn get_quick_hash<P: AsRef<Path>>(
    hash: &HashAlgorithm,
    size: u64,
    splits: u64,
    path: P,
) -> String {
    let mut size = size;
    let mut file = File::open(path).unwrap();
    let mut total_buffer = vec![0; 0];

    let file_len = file.metadata().unwrap().len();

    // Decide if we need to read the whole file
    let read_whole_file =
        file_len == 0 || size == 0 || splits == 0 || splits >= file_len || file_len / splits < size;

    if read_whole_file {
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

    match hash {
        HashAlgorithm::MD5 => md5::chksum(&total_buffer).unwrap().to_hex_lowercase(),
        HashAlgorithm::SHA1 => sha1::chksum(&total_buffer).unwrap().to_hex_lowercase(),
        HashAlgorithm::SHA256 => sha2_256::chksum(&total_buffer).unwrap().to_hex_lowercase(),
        HashAlgorithm::SHA512 => sha2_512::chksum(&total_buffer).unwrap().to_hex_lowercase(),
    }
}

#[inline]
pub fn get_image_hash<P: AsRef<Path> + std::fmt::Debug>(
    hash: &ImageHashAlgorithm,
    filter: &ImageFilterAlgorithm,
    size: u64,
    path: &P,
) -> Option<ImageHash> {
    match ImageReader::open(path) {
        Ok(r) => match r.decode() {
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
                warn!("Decoding image {:?} failed: {}", path, e);
            }
        },
        Err(e) => {
            warn!("Reading image {:?} failed: {}", path, e);
        }
    };
    None
}

#[inline]
pub fn get_audio_hash(
    path: impl AsRef<Path> + std::fmt::Debug,
    config: &Configuration,
) -> Option<Vec<u32>> {
    let file = std::fs::File::open(path.as_ref()).ok()?;

    let mut hint = Hint::new();
    // Provide the file extension as a hint.
    if let Some(extension) = path.as_ref().extension() {
        if let Some(extension_str) = extension.to_str() {
            hint.with_extension(extension_str);
        }
    }

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

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

    let mut printer = Fingerprinter::new(config);
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
