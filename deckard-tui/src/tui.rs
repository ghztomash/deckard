use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    crossterm::{
        cursor::Show,
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
};
use std::io::{self, Stdout, Write, stdout};

/// A type alias for the terminal type used in this application
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Initialize the terminal
pub fn init() -> io::Result<Tui> {
    execute!(stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    set_panic_hook();
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    Terminal::with_options(
        backend,
        TerminalOptions {
            // viewport: Viewport::Inline(8),
            viewport: Viewport::Fullscreen,
        },
    )
}

/// Restore the terminal to its original state
pub fn restore() -> io::Result<()> {
    restore_screen(stdout())?;
    disable_raw_mode()?;
    Ok(())
}

fn restore_screen<W: Write>(mut writer: W) -> io::Result<()> {
    execute!(writer, Show, LeaveAlternateScreen)
}

fn set_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore(); // ignore any errors as we are already failing
        hook(panic_info);
    }));
}

#[cfg(test)]
mod tests {
    use super::restore_screen;

    #[test]
    fn restore_screen_shows_cursor_before_leaving_alternate_screen() {
        let mut output = Vec::new();

        restore_screen(&mut output).unwrap();

        assert_eq!(output, b"\x1b[?25h\x1b[?1049l");
    }
}
