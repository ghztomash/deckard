use color_eyre::eyre::Result;

mod app;
mod cli;
mod tui;

fn main() -> Result<()> {
    color_eyre::install()?;
    env_logger::init();

    let args = cli::cli().get_matches();
    let config = cli::get_config();

    let mut terminal = tui::init()?;

    let app_result = app::App::default().run(&mut terminal);

    tui::restore()?;
    terminal.clear()?;
    app_result
}
