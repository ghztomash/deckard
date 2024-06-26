use clap::{command, value_parser, Arg, Command};

pub fn cli() -> Command {
    command!()
        .about("Find file duplicates")
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            Arg::new("params")
                .value_name("PATH")
                .value_hint(clap::ValueHint::AnyPath)
                .value_parser(value_parser!(String))
                .help("List of paths to traverse")
                .num_args(1..),
        )
        .arg(
            Arg::new("ignore_hidden")
                .short('i')
                .long("ignore_hidden")
                .action(clap::ArgAction::SetTrue)
                .help("Do not check hidden files"),
        )
        .arg(
            Arg::new("depth")
                .short('d')
                .long("depth")
                .value_parser(value_parser!(usize))
                .help("Maximum depth to traverse")
                .num_args(1),
        )
        .arg(
            Arg::new("threads")
                .short('t')
                .long("threads")
                .value_parser(value_parser!(usize))
                .help("Number of worker threads to use")
                .num_args(1),
        )
}
