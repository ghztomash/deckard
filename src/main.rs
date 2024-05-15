use clone_hunter::*;
use std::path::Path;

fn main() {
    println!("Hello World!");
    visit_dirs(Path::new("."), 0).unwrap();
}
