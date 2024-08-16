use chksum::md5;
use chksum::sha1;
use chksum::sha2_256;
use core::panic;
use image::io::Reader as ImageReader;
use image_hasher::{FilterType, HashAlg, HasherConfig};
use log::{debug, error, trace, warn};
use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::path::Path;

#[inline]
pub fn get_full_hash<P: AsRef<Path>>(hash: &str, path: P) -> String {
    let file = File::open(path).unwrap();
    let digest = match hash {
        "md5" => md5::chksum(file).unwrap().to_hex_lowercase(),
        "sha1" => sha1::chksum(file).unwrap().to_hex_lowercase(),
        "sha256" => sha2_256::chksum(file).unwrap().to_hex_lowercase(),
        _ => panic!("wrong hashing algorithm"),
    };
    digest
}

#[inline]
pub fn get_quick_hash<P: AsRef<Path>>(hash: &str, size: u64, splits: u64, path: P) -> String {
    let mut size = size;
    let mut file = File::open(path).unwrap();
    let mut total_buffer = vec![0; 0];

    let file_len = file.metadata().unwrap().len();
    let mut read_whole_file = false;

    if file_len == 0 || size == 0 || splits == 0 {
        read_whole_file = true;
    } else if splits >= file_len || file_len / splits < size {
        read_whole_file = true;
    }

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
            let index = i as u64 * index_step;
            // println!("reading {} bytes at {} of {}", size, index, file_len);

            file.seek(std::io::SeekFrom::Start(index)).unwrap();
            file.read_exact(&mut buffer).unwrap();
            total_buffer.append(&mut buffer);
        }
        // append size to the hash, otherwise files that start with the same bytes match
        total_buffer.append(&mut file_len.to_le_bytes().to_vec());
    }

    // TODO: Move parsing to config
    let digest = match hash {
        "md5" => md5::chksum(&total_buffer).unwrap().to_hex_lowercase(),
        "sha1" => sha1::chksum(&total_buffer).unwrap().to_hex_lowercase(),
        "sha256" => sha2_256::chksum(&total_buffer).unwrap().to_hex_lowercase(),
        _ => panic!("wrong hashing algorithm"),
    };
    digest
}

#[inline]
pub fn get_image_hash<P: AsRef<Path> + std::fmt::Debug>(
    hash: &str,
    filter: &str,
    size: u64,
    path: &P,
) -> Option<String> {
    // TODO: Move parsing to config
    let hash = match hash.to_lowercase().as_ref() {
        "mean" => HashAlg::Mean,
        "median" => HashAlg::Median,
        "gradient" => HashAlg::Gradient,
        "vert_gradient" => HashAlg::VertGradient,
        "double_gradient" => HashAlg::DoubleGradient,
        "blockhash" => HashAlg::Blockhash,
        _ => {
            error!("wrong hash algorithm {}", hash);
            HashAlg::Gradient
        }
    };
    let filter = match filter.to_lowercase().as_ref() {
        "nearest" => FilterType::Nearest,
        "triangle" => FilterType::Triangle,
        "catmull" => FilterType::CatmullRom,
        "gaussian" => FilterType::Gaussian,
        "lanczos" => FilterType::Lanczos3,
        _ => {
            error!("wrong filter algorithm {}", filter);
            FilterType::Triangle
        }
    };

    match ImageReader::open(path) {
        Ok(r) => match r.decode() {
            Ok(img) => {
                let hasher = HasherConfig::new()
                    .hash_size(size as u32, size as u32)
                    .resize_filter(filter)
                    .hash_alg(hash)
                    .to_hasher();
                let hash = hasher.hash_image(&img).to_base64();
                trace!("Image hash: {}", hash);
                return Some(hash);
            }
            Err(e) => {
                error!("Decoding image {:?} failed: {}", path, e);
            }
        },
        Err(e) => {
            warn!("Reading image {:?} failed: {}", path, e);
        }
    };
    None
}
