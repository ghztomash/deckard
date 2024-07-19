use deckard::*;

use colored::*;
use std::time::Instant;

use deckard::index::FileIndex;

fn main() {
    env_logger::init();
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

    let config = deckard::config::SearchConfig::default();

    let now = Instant::now();
    let mut file_index = FileIndex::new(target_paths, config);
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

mod cli {
    use clap::{command, value_parser, Arg, Command};

    pub fn cli() -> Command {
        command!()
            .about("Find file duplicates")
            .version(env!("CARGO_PKG_VERSION"))
            .arg(
                Arg::new("params")
                    .value_name("PATH")
                    .value_hint(clap::ValueHint::AnyPath)
                    .value_parser(value_parser!(String))
                    .help("List of paths to traverse")
                    .num_args(1..),
            )
            .arg(
                Arg::new("ignore_hidden")
                    .short('i')
                    .long("ignore_hidden")
                    .action(clap::ArgAction::SetTrue)
                    .help("Do not check hidden files"),
            )
            .arg(
                Arg::new("depth")
                    .short('d')
                    .long("depth")
                    .value_parser(value_parser!(usize))
                    .help("Maximum depth to traverse")
                    .num_args(1),
            )
            .arg(
                Arg::new("threads")
                    .short('t')
                    .long("threads")
                    .value_parser(value_parser!(usize))
                    .help("Number of worker threads to use")
                    .num_args(1),
            )
    }
}
