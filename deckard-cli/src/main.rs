use clap::Arg;
use color_eyre::eyre::Result;
use colored::*;
use deckard::config::SearchConfig;
use deckard::index::FileIndex;
use std::{io::stderr, time::Instant};
use tracing::info;

const CONFIG_NAME: &str = env!("CARGO_PKG_NAME");

fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = deckard::cli::commands().arg(
        Arg::new("json")
            .short('j')
            .long("json")
            .action(clap::ArgAction::SetTrue)
            .help("Output in JSON format"),
    );
    let args = cli.get_matches();

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
    info!(
        "Indexed {} files in {}",
        file_index.files_len().to_string().green(),
        format!("{elapsed:.2?}").blue()
    );

    let now = Instant::now();
    file_index.process_files(None, None);
    // file_index.process_files(Some(Arc::new(|count, total| {
    //     info!("processing file {}/{}", count, total);
    // })));
    let elapsed = now.elapsed();
    info!(
        "Processed {} files in {}",
        file_index.files_len().to_string().green(),
        format!("{elapsed:.2?}").blue()
    );

    let now = Instant::now();
    file_index.find_duplicates(None, None);
    // file_index.find_duplicates(Some(Arc::new(|count, total| {
    //     info!("comparing file {}/{}", count, total);
    // })));
    let elapsed = now.elapsed();
    info!(
        "Found {} matches in {}",
        file_index.duplicates_len().to_string().green(),
        format!("{elapsed:.2?}").blue()
    );

    if json {
        let serialized = serde_json::to_string_pretty(&file_index.duplicates)?;
        println!("{serialized}");
    } else {
        println!("\n{}", "Matches:".bold());
        for (file, file_copies) in &file_index.duplicates {
            let name = file_index.file_name(file).unwrap_or_default();
            let mut match_names = Vec::new();

            for file_copy in file_copies {
                match_names.push(file_copy.display());
            }

            println!(
                "{} matches {}",
                format!("{}", name.display()).green(),
                format!("{match_names:#?}").yellow()
            );
        }
    }

    Ok(())
}
