use deckard::*;
mod cli;
mod file;

use colored::*;
use std::time::Instant;

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
    let mut files_index = index_dirs(target_paths);
    let elapsed = now.elapsed();
    println!(
        "Indexed {} files in {}",
        &files_index.len().to_string().green(),
        format!("{:.2?}", elapsed).blue()
    );

    let now = Instant::now();
    process_files(&mut files_index);
    let elapsed = now.elapsed();
    println!(
        "Processed {} files in {}",
        &files_index.len().to_string().green(),
        format!("{:.2?}", elapsed).blue()
    );

    let now = Instant::now();
    let file_matches = find_matches(&files_index);
    let elapsed = now.elapsed();
    println!(
        "Found {} matches in {}",
        file_matches.len().to_string().green(),
        format!("{:.2?}", elapsed).blue()
    );

    println!("\nMatches:");
    for (file, file_copies) in file_matches {
        let name = files_index.get(&file).unwrap().name.clone();
        let mut match_names = Vec::new();

        for fc in file_copies {
            match_names.push(files_index.get(&fc).unwrap().name.clone());
        }

        println!("{} matches {:?}", name.yellow(), match_names);
    }
}
