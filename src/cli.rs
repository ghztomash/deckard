use clap::{arg, command, value_parser, Arg, ArgAction, Command};
use std::path::PathBuf;

pub fn cli() -> Command {
    command!()
        .about("Find file duplicates")
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            Arg::new("params")
                .value_name("PATH")
                .value_hint(clap::ValueHint::AnyPath)
                .value_parser(value_parser!(String))
                .num_args(1..),
        )
}
