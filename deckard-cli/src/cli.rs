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
            Arg::new("open_config")
                .short('O')
                .long("open_config")
                .action(clap::ArgAction::SetTrue)
                .help("Open config file"),
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
                .help("Compare image files similarities"),
        )
        .arg(
            Arg::new("check_audio")
                .short('a')
                .long("check_audio")
                .action(clap::ArgAction::SetTrue)
                .help("Compare audio files similarities"),
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
        .arg(
            Arg::new("json")
                .short('j')
                .long("json")
                .action(clap::ArgAction::SetTrue)
                .help("Output in JSON format"),
        )
}

pub fn get_config() -> SearchConfig {
    let args = cli().get_matches();
    let mut config = deckard::config::SearchConfig::load("deckard-cli");

    debug!("loaded {:#?}", config);

    let include_filter = args
        .get_one::<String>("include_filter")
        .map(|v| v.to_owned());
    if include_filter.is_some() {
        config.include_filter = include_filter
    }

    let exclude_filter = args
        .get_one::<String>("exclude_filter")
        .map(|v| v.to_owned());
    if exclude_filter.is_some() {
        config.exclude_filter = exclude_filter
    }

    let skip_hidden = args.get_flag("skip_hidden");
    if skip_hidden {
        config.skip_hidden = skip_hidden
    }

    let skip_empty = args.get_flag("skip_empty");
    if skip_empty {
        config.skip_empty = skip_empty
    }

    let check_image = args.get_flag("check_image");
    if check_image {
        config.image_config.compare = check_image
    }

    let check_audio = args.get_flag("check_audio");
    if check_audio {
        config.audio_config.compare = check_audio
    }

    let full_hash = args.get_flag("full_hash");
    if full_hash {
        config.hasher_config.full_hash = full_hash
    }

    if let Some(t) = args.get_one::<usize>("threads") {
        config.threads = *t;
    }

    debug!("with arguments {:#?}", config);

    config
}
