use clone_hunter::*;
use std::env;
use std::path::Path;

fn main() {
    // Prints each argument on a separate line
    for argument in env::args() {
        println!("{argument}");
    }

    println!("Files:");
    visit_dirs(Path::new("test_files"), 0).unwrap();
}
