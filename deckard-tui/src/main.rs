use std::{
    fs::{File, create_dir_all},
    path::PathBuf,
};

use clap::Arg;
use color_eyre::eyre::Result;
use tracing_appender::non_blocking::WorkerGuard;

mod app;
mod command;
mod constants;
mod table;
mod tree;
mod tui;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let cli = deckard::cli::commands()
        .arg(
            Arg::new("open_logs")
                .short('L')
                .long("open_logs")
                .action(clap::ArgAction::SetTrue)
                .help("Open logs file"),
        )
        .arg(
            Arg::new("no_remove_dirs")
                .short('R')
                .long("no_remove_dirs")
                .action(clap::ArgAction::SetFalse)
                .help("Do not remove empty directories"),
        )
        .arg(
            Arg::new("dry_run")
                .long("dry_run")
                .action(clap::ArgAction::SetTrue)
                .help("Don't actualy remove the files"),
        );
    let args = cli.get_matches();

    // open log file, before setting up logging because it overwrites it
    if args.get_flag("open_logs") {
        let log_path = log_path()?;
        eprintln!("Opening log file: {log_path:?}");
        open::that_detached(log_path)?;
        return Ok(());
    }

    // setup logging
    let log_level = deckard::cli::log_level(args.get_count("verbose"));
    let _guard = init_tracing(log_level)?;

    if args.get_flag("open_config") {
        deckard::config::SearchConfig::edit_config(constants::CONFIG_NAME)?;
        return Ok(());
    }

    let config = deckard::cli::augment_config(
        deckard::config::SearchConfig::load(constants::CONFIG_NAME),
        &args,
    );

    let dry_run = args.get_flag("dry_run");
    let remove_dirs = args.get_flag("no_remove_dirs");
    let disk_usage = args.get_flag("disk_usage");

    let target_dirs = match args.get_many::<String>("params") {
        Some(values) => values.map(|v| v.as_str()).collect::<Vec<&str>>(),
        None => vec!["."],
    };
    let target_paths = deckard::collect_paths(target_dirs);
    if !deckard::validate_paths(&target_paths) {
        eprintln!("No valid paths provided");
        std::process::exit(1);
    }

    let mut terminal = tui::init()?;
    let app_result = app::App::new(target_paths, config, dry_run, remove_dirs, disk_usage)
        .run(&mut terminal)
        .await;

    tui::restore()?;
    terminal.clear()?;

    if let Err(_e) = &app_result {
        // eprintln!("Error: {:?}", e);
        // kills the process without waiting for all of the threads to finish
        std::process::exit(1);
    }

    app_result
}

/// Initialize the tracing subscriber to log to a file
fn init_tracing(log_level: tracing::Level) -> Result<WorkerGuard> {
    let path = log_path()?;
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    let (non_blocking, guard) = tracing_appender::non_blocking(file);

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_writer(non_blocking)
        .init();

    Ok(guard)
}

/// Helper to get log file path
fn log_path() -> Result<PathBuf> {
    Ok(
        deckard::config::SearchConfig::get_config_folder(constants::CONFIG_NAME)?
            .join(constants::LOG_NAME),
    )
}
