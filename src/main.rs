use clone_hunter::*;
use std::env;
use std::path::Path;

mod cli;

fn main() {
    let args = cli::cli().get_matches();

    let target_dirs = match args.get_many::<String>("params") {
        Some(values) => values.map(|v| v.as_str()).collect::<Vec<&str>>(),
        None => vec!["."],
    };

    let mut path = "test_files";

    println!("Files:");
    visit_dirs(Path::new(target_dirs[0]), 0).unwrap();
}
