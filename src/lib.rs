use std::{fs, path::Path};

pub fn visit_dirs(dir: &Path, indent: usize) -> std::io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let metadata = entry.metadata().unwrap();
            let filetype = entry.file_type();

            let filetype = if metadata.is_dir() {
                "d"
            } else if metadata.is_file() {
                "f"
            } else if metadata.is_symlink() {
                "l"
            } else {
                "?"
            };
            print!("{}", " ".to_string().repeat(indent));
            println!(
                "{} {} B {}",
                filetype,
                metadata.len(),
                path.to_str().unwrap()
            );

            if path.is_dir() {
                visit_dirs(&path, indent + 1).unwrap();
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        visit_dirs(Path::new("."), 0).unwrap();
        assert!(true);
    }
}
