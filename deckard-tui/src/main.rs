use clap::Arg;
use color_eyre::eyre::Result;

mod app;
mod table;
mod tui;

const CONFIG_NAME: &str = env!("CARGO_PKG_NAME");

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    env_logger::init();

    let cli = deckard::cli::commands()
        .arg(
            Arg::new("remove_dirs")
                .short('E')
                .long("remove_dirs")
                .action(clap::ArgAction::SetTrue)
                .help("Remove empty directories"),
        )
        .arg(
            Arg::new("dry_run")
                .long("dry_run")
                .action(clap::ArgAction::SetTrue)
                .help("Don't actualy remove the files"),
        );
    let args = cli.get_matches();

    if args.get_flag("open_config") {
        deckard::config::SearchConfig::edit_config(CONFIG_NAME)?;
        return Ok(());
    }

    let dry_run = args.get_flag("dry_run");
    let remove_dirs = args.get_flag("remove_dirs");

    let mut terminal = tui::init()?;

    let target_dirs = match args.get_many::<String>("params") {
        Some(values) => values.map(|v| v.as_str()).collect::<Vec<&str>>(),
        None => vec!["."],
    };
    let target_paths = deckard::collect_paths(target_dirs);

    let config = deckard::cli::augment_config(CONFIG_NAME, args);
    let app_result = app::App::new(target_paths, config, dry_run, remove_dirs)
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
