use crate::{SearchConfig, config::LogLevel};
use clap::{Arg, ArgAction, ArgMatches, Command, command, value_parser};
use log::{debug, trace};

pub fn commands() -> Command {
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
                .action(ArgAction::SetTrue)
                .help("Open config file"),
        )
        .arg(
            Arg::new("skip_hidden")
                .short('H')
                .long("skip_hidden")
                .action(ArgAction::SetTrue)
                .help("Do not check hidden files"),
        )
        .arg(
            Arg::new("skip_empty")
                .short('e')
                .long("skip_empty")
                .action(ArgAction::SetTrue)
                .help("Do not check empty files"),
        )
        .arg(
            Arg::new("check_image")
                .short('i')
                .long("check_image")
                .action(ArgAction::SetTrue)
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
                .action(ArgAction::SetTrue)
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
            Arg::new("min_size")
                .short('m')
                .long("min_size")
                .value_parser(value_parser!(u64))
                .help("Filter out files that smaller in size"),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .action(ArgAction::Count)
                .help("Increase the log level verbosity"),
        )
}

pub fn augment_config(config_name: &str, args: ArgMatches) -> SearchConfig {
    let mut config = SearchConfig::load(config_name);

    trace!("loaded {:#?}", config);

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

    if args.get_flag("skip_hidden") {
        config.skip_hidden = true
    }
    if args.get_flag("skip_empty") {
        config.min_size = 1;
    }
    if let Some(s) = args.get_one::<u64>("min_size") {
        config.min_size = *s;
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

    let verbose = args.get_count("verbose");
    if verbose > 0 {
        config.log_level = LogLevel::from_count(verbose);
    }

    debug!("with arguments {:#?}", config);

    config
}
