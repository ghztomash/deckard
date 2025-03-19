use color_eyre::eyre::Result;

mod app;
mod cli;
mod table;
mod tui;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    env_logger::init();

    let args = cli::cli().get_matches();
    let config = cli::get_config();

    let mut terminal = tui::init()?;

    let target_dirs = match args.get_many::<String>("params") {
        Some(values) => values.map(|v| v.as_str()).collect::<Vec<&str>>(),
        None => vec!["."],
    };

    let target_paths = deckard::collect_paths(target_dirs);

    let app_result = app::App::new(target_paths, config).run(&mut terminal).await;

    tui::restore()?;
    terminal.clear()?;
    app_result
}
