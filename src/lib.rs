mod files;

use files::FileEntry;
use std::{fs, path::Path};

pub fn visit_dirs(dir: &Path, depth: usize) -> std::io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                let file = FileEntry::new(entry, depth);
                //println!("{:#?}", file);
                println!("{}", file);
            } else if path.is_dir() {
                visit_dirs(&path, depth + 1).unwrap();
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
