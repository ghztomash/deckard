use clap::Arg;
use color_eyre::eyre::Result;
use colored::*;
use deckard::config::SearchConfig;
use deckard::index::FileIndex;
use std::{io::stderr, time::Instant};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

const CONFIG_NAME: &str = env!("CARGO_PKG_NAME");

fn main() -> Result<()> {
    color_eyre::install()?;
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_writer(stderr)
        .without_time()
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let cli = deckard::cli::commands()
        .arg(
            Arg::new("json")
                .short('j')
                .long("json")
                .action(clap::ArgAction::SetTrue)
                .help("Output in JSON format"),
        )
        .arg(
            Arg::new("delete")
                .long("DELETE")
                .action(clap::ArgAction::SetTrue)
                .help(format!(
                    "{} {}",
                    "Delete duplicate files",
                    "(No way to undo!)".bold()
                )),
        )
        .arg(
            Arg::new("trash")
                .long("trash")
                .action(clap::ArgAction::SetTrue)
                .help("Move duplicate files to trash"),
        );
    let args = cli.get_matches();

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

    let config = deckard::cli::augment_config(CONFIG_NAME, args);

    if !json {
        println!("Paths: {}", format!("{:?}", target_paths).yellow());
    }

    let now = Instant::now();
    let mut file_index = FileIndex::new(target_paths, config);
    file_index.index_dirs(None, None);
    let elapsed = now.elapsed();
    info!(
        "Indexed {} files in {}",
        file_index.files_len().to_string().green(),
        format!("{:.2?}", elapsed).blue()
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
        format!("{:.2?}", elapsed).blue()
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
        format!("{:.2?}", elapsed).blue()
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
                match_names.push(file_copy.to_string_lossy());
            }

            println!(
                "{} matches {}",
                name.green(),
                format!("{:#?}", match_names).yellow()
            );
        }
    }

    Ok(())
}
