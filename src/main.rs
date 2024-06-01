use deckard::*;
mod cli;
mod file;
mod index;

use colored::*;
use std::time::Instant;

use index::FileIndex;

#[tokio::main]
async fn main() {
    let args = cli::cli().get_matches();

    let target_dirs = match args.get_many::<String>("params") {
        Some(values) => values.map(|v| v.as_str()).collect::<Vec<&str>>(),
        None => vec!["."],
    };
    let depth = match args.get_one::<usize>("depth") {
        Some(v) => *v,
        None => usize::MAX,
    };
    let ignore_hidden = args.get_flag("ignore_hidden");

    let target_paths = collect_paths(target_dirs.clone());
    println!("Paths: {}", format!("{:?}", target_paths).yellow());

    let now = Instant::now();
    let mut file_index = FileIndex::new(target_paths);
    file_index.index_dirs();
    let elapsed = now.elapsed();
    println!(
        "Indexed {} files in {}",
        file_index.files_len().to_string().green(),
        format!("{:.2?}", elapsed).blue()
    );

    let now = Instant::now();
    file_index.process_files();
    let elapsed = now.elapsed();
    println!(
        "Processed {} files in {}",
        file_index.files_len().to_string().green(),
        format!("{:.2?}", elapsed).blue()
    );

    let now = Instant::now();
    file_index.find_duplicates();
    let elapsed = now.elapsed();
    println!(
        "Found {} matches in {}",
        file_index.duplicates_len().to_string().green(),
        format!("{:.2?}", elapsed).blue()
    );

    println!("\nMatches:");
    for (file, file_copies) in &file_index.duplicates {
        let name = file_index.file_name(file).unwrap();
        let mut match_names = Vec::new();

        for file_copy in file_copies {
            match_names.push(file_index.file_name(file_copy).unwrap());
        }

        println!(
            "{} matches {}",
            name.yellow(),
            format!("{:?}", match_names).yellow()
        );
    }
}
