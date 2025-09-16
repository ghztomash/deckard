pub const HELP_LOGO: &str = r#"
    ___          _                 _
   /   \___  ___| | ____ _ _ __ __| |
  / /\ / _ \/ __| |/ / _` | '__/ _` |
 / /_//  __/ (__|   < (_| | | | (_| |
/___,' \___|\___|_|\_\__,_|_|  \__,_|
"#;

/// Help text to show.
pub const HELP_TEXT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    " v",
    env!("CARGO_PKG_VERSION"),
    "\n",
    env!("CARGO_PKG_REPOSITORY"),
    "\n",
    "written by ",
    env!("CARGO_PKG_AUTHORS"),
    "\n",
    env!("CARGO_PKG_DESCRIPTION"),
    "\n",
    "\n",
    "Commands:\n",
    "filter <str> - show only files containing <str> in paht\n",
    "clear_filter - remove filter\n",
    "mark_filter <str> - mark files containing <str> in paht\n",
    "mark_all - mark all displayed files\n",
    "clear_marked - unmark all files\n",
    "help - show this window\n",
    "quit - quit deckard\n",
);

pub const CONFIG_NAME: &str = env!("CARGO_PKG_NAME");
pub const LOG_NAME: &str = "deckard-tui.log";
