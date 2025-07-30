use clap::Arg;
use color_eyre::eyre::Result;
use tracing::Level;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::FmtSubscriber;

mod app;
mod table;
mod tui;

const CONFIG_NAME: &str = env!("CARGO_PKG_NAME");

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    // setup logging
    let file_appender = RollingFileAppender::new(
        Rotation::NEVER,
        deckard::config::SearchConfig::get_config_folder(CONFIG_NAME)?,
        "deckard-tui.log",
    );
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_writer(non_blocking)
        .without_time()
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

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

/// Initialize the tracing subscriber to log to a file
fn init_tracing(log_level: u8) -> Result<WorkerGuard> {
    let file = File::create("eos-term.log").wrap_err("failed to create eos-term.log")?;
    let (non_blocking, guard) = non_blocking(file);

    // By default, the subscriber is configured to log all events with a level of `DEBUG` or higher,
    // but this can be changed by setting the `RUST_LOG` environment variable.
    let env_filter = EnvFilter::builder()
        .with_default_directive(log_level.into())
        .from_env_lossy();

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(env_filter)
        .init();
    Ok(guard)
}
