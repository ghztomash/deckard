use clap::{command, value_parser, Arg, Command};
use deckard::config::SearchConfig;
use log::debug;

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
            Arg::new("skip_hidden")
                .short('H')
                .long("skip_hidden")
                .action(clap::ArgAction::SetTrue)
                .help("Do not check hidden files"),
        )
        .arg(
            Arg::new("skip_empty")
                .short('e')
                .long("skip_empty")
                .action(clap::ArgAction::SetTrue)
                .help("Do not check empty files"),
        )
        .arg(
            Arg::new("check_image")
                .short('i')
                .long("check_image")
                .action(clap::ArgAction::SetTrue)
                .help("Compare image files"),
        )
        .arg(
            Arg::new("full_hash")
                .long("full_hash")
                .action(clap::ArgAction::SetTrue)
                .help("Compare every byte of the file"),
        )
        .arg(
            Arg::new("include_filter")
                .short('f')
                .long("include_filter")
                .value_parser(value_parser!(String))
                .help("Include files that contain filter in their file name"),
        )
        .arg(
            Arg::new("exclude_filter")
                .short('x')
                .long("exclude_filter")
                .value_parser(value_parser!(String))
                .help("Exclude files that contain filter in their file name"),
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

pub fn get_config() -> SearchConfig {
    let args = cli().get_matches();
    let mut config = deckard::config::SearchConfig::load("deckard-cli");

    debug!("loaded {:#?}", config);

    let include_filter = match args.get_one::<String>("include_filter") {
        Some(v) => Some(v.to_owned()),
        None => None,
    };
    if include_filter.is_some() {
        config.include_filter = include_filter
    }

    let exclude_filter = match args.get_one::<String>("exclude_filter") {
        Some(v) => Some(v.to_owned()),
        None => None,
    };
    if exclude_filter.is_some() {
        config.exclude_filter = exclude_filter
    }

    let skip_hidden = args.get_flag("skip_hidden");
    if skip_hidden == true {
        config.skip_hidden = skip_hidden
    }

    let skip_empty = args.get_flag("skip_empty");
    if skip_empty == true {
        config.skip_empty = skip_empty
    }

    let check_image = args.get_flag("check_image");
    if check_image == true {
        config.image_config.check_image = check_image
    }

    let full_hash = args.get_flag("full_hash");
    if full_hash == true {
        config.hasher_config.full_hash = full_hash
    }

    match args.get_one::<usize>("threads") {
        Some(v) => {
            config.threads = v.to_owned();
        }
        None => {}
    };

    debug!("with arguments {:#?}", config);

    config
}
