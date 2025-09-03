use clap::{Arg, value_parser};
use color_eyre::eyre::Result;
use colored::*;
use deckard::config::SearchConfig;
use deckard::index::FileIndex;
use std::{io::stderr, path::PathBuf, time::Instant};
use tracing::Level;

const CONFIG_NAME: &str = env!("CARGO_PKG_NAME");

fn collect_sorted_files<'a>(
    entries: impl Iterator<Item = (&'a PathBuf, u64)>,
    reverse: bool,
    limit: Option<&usize>,
) -> Vec<(&'a PathBuf, u64)> {
    let mut vec: Vec<_> = entries.collect();
    vec.sort_unstable_by(|a, b| b.1.cmp(&a.1));

    if let Some(limit) = limit {
        vec.truncate(*limit);
    }

    if reverse {
        vec.reverse();
    }
    vec
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = deckard::cli::commands()
        .arg(
            Arg::new("json")
                .short('j')
                .long("json")
                .action(clap::ArgAction::SetTrue)
                .help("Output in JSON format"),
        )
        .arg(
            Arg::new("lines_number")
                .short('n')
                .long("lines_number")
                .value_parser(value_parser!(usize))
                .help("Number of lines of output to show")
                .num_args(1),
        )
        .arg(
            Arg::new("reverse")
                .short('r')
                .long("reverse")
                .action(clap::ArgAction::SetTrue)
                .help("Display the biggest directories at the top in descending order"),
        );
    let args = cli.get_matches();
    let disk_usage_mode = args.get_flag("disk_usage");

    // setup logging
    let log_level = deckard::cli::log_level(args.get_count("verbose"));
    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_writer(stderr)
        .without_time()
        .init();

    let config = deckard::cli::augment_config(SearchConfig::load(CONFIG_NAME), &args);

    if args.get_flag("open_config") {
        SearchConfig::edit_config(CONFIG_NAME)?;
        return Ok(());
    }

    let json = args.get_flag("json");

    let target_dirs = match args.get_many::<String>("params") {
        Some(values) => values.map(|v| v.as_str()).collect::<Vec<&str>>(),
        None => vec!["."],
    };
    let target_paths = deckard::collect_paths(target_dirs);
    if !deckard::validate_paths(&target_paths) {
        eprintln!("No valid paths provided");
        std::process::exit(1);
    }

    if !json {
        eprintln!("Paths: {}", format!("{target_paths:?}").yellow());
    }

    let now = Instant::now();
    let mut file_index = FileIndex::new(target_paths, config);
    file_index.index_dirs(None, None);
    let elapsed = now.elapsed();

    if log_level >= Level::INFO {
        eprintln!(
            "Indexed {} files in {}",
            file_index.files_len().to_string().green(),
            format!("{elapsed:.2?}").blue()
        );
    }

    // Only display the size
    if disk_usage_mode {
        // Sort indexed files by size
        let now = Instant::now();
        let files = collect_sorted_files(
            file_index
                .files
                .iter()
                .map(|(path, info)| (path, info.size)),
            args.get_flag("reverse"),
            args.get_one::<usize>("lines_number"),
        );

        let elapsed = now.elapsed();
        if log_level >= Level::INFO {
            eprintln!(
                "Sorted {} files in {}",
                file_index.files_len().to_string().green(),
                format!("{elapsed:.2?}").blue()
            );
        }

        if json {
            let serialized = serde_json::to_string_pretty(&files)?;
            println!("{serialized}");
        } else {
            println!("\n{}", "Size:".bold());

            for (name, size) in files.iter().rev() {
                println!(
                    "{}: {}",
                    name.display(),
                    humansize::format_size(*size, humansize::DECIMAL).blue()
                );
            }
        }
    } else {
        // perform normal comparison
        let now = Instant::now();
        file_index.process_files(None, None);

        let elapsed = now.elapsed();
        if log_level >= Level::INFO {
            eprintln!(
                "Processed {} files in {}",
                file_index.files_len().to_string().green(),
                format!("{elapsed:.2?}").blue()
            );
        }

        let now = Instant::now();
        file_index.find_duplicates(None, None);

        let elapsed = now.elapsed();
        if log_level >= Level::INFO {
            eprintln!(
                "Found {} matches in {}",
                file_index.duplicates_len().to_string().green(),
                format!("{elapsed:.2?}").blue()
            );
        }

        // Sort duplicate files by size
        let now = Instant::now();
        let duplicates = collect_sorted_files(
            file_index
                .duplicates
                .keys()
                .map(|path| (path, file_index.files[path].size)),
            args.get_flag("reverse"),
            args.get_one::<usize>("lines_number"),
        );

        let elapsed = now.elapsed();
        if log_level >= Level::INFO {
            eprintln!(
                "Sorted {} files in {}",
                file_index.duplicates_len().to_string().green(),
                format!("{elapsed:.2?}").blue()
            );
        }

        if json {
            let serialized = serde_json::to_string_pretty(&file_index.duplicates)?;
            println!("{serialized}");
        } else {
            println!("\n{}", "Matches:".bold());
            for (file, size) in duplicates.iter().rev() {
                let file_copies = &file_index.duplicates[*file];
                let name = file_index.file_name(file).unwrap_or_default();
                let mut match_names = Vec::with_capacity(file_copies.len());

                for file_copy in file_copies {
                    match_names.push(file_copy.display());
                }

                println!(
                    "{}: {} matches {}",
                    format!("{}", name.display()).green(),
                    humansize::format_size(*size, humansize::DECIMAL).blue(),
                    format!("{match_names:#?}").yellow()
                );
            }
        }
    }

    Ok(())
}
