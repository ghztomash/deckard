use chksum::md5;
use chksum::sha1;
use chksum::sha2_256;
use core::panic;
use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::path::Path;

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

    // println!("{:?}", total_buffer);
    let digest = match hash {
        "md5" => md5::chksum(&total_buffer).unwrap().to_hex_lowercase(),
        "sha1" => sha1::chksum(&total_buffer).unwrap().to_hex_lowercase(),
        "sha256" => sha2_256::chksum(&total_buffer).unwrap().to_hex_lowercase(),
        _ => panic!("wrong hashing algorithm"),
    };
    digest
}
