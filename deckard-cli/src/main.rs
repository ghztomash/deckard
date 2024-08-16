use deckard::*;

use colored::*;
use std::time::Instant;

use deckard::index::FileIndex;

mod cli;

use log::info;

fn main() {
    env_logger::init();
    let args = cli::cli().get_matches();
    let config = cli::get_config();

    let target_dirs = match args.get_many::<String>("params") {
        Some(values) => values.map(|v| v.as_str()).collect::<Vec<&str>>(),
        None => vec!["."],
    };

    let target_paths = collect_paths(target_dirs.clone());
    println!("Paths: {}", format!("{:?}", target_paths).yellow());

    let now = Instant::now();
    let mut file_index = FileIndex::new(target_paths, config);
    file_index.index_dirs();
    let elapsed = now.elapsed();
    info!(
        "Indexed {} files in {}",
        file_index.files_len().to_string().green(),
        format!("{:.2?}", elapsed).blue()
    );

    let now = Instant::now();
    file_index.process_files();
    let elapsed = now.elapsed();
    info!(
        "Processed {} files in {}",
        file_index.files_len().to_string().green(),
        format!("{:.2?}", elapsed).blue()
    );

    let now = Instant::now();
    file_index.find_duplicates();
    let elapsed = now.elapsed();
    info!(
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
            name.green(),
            format!("{:#?}", match_names).yellow()
        );
    }
}
