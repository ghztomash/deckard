use crate::SearchConfig;
use clap::{Arg, ArgAction, ArgMatches, Command, command, value_parser};
use tracing::{Level, trace};

fn parse_glob(value: &str) -> Result<String, String> {
    globset::Glob::new(value)
        .map(|_| value.to_owned())
        .map_err(|error| error.to_string())
}

fn parse_size(value: &str) -> Result<u64, String> {
    let value = value.trim();
    let number_end = value
        .find(|character: char| !character.is_ascii_digit() && character != '.')
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(number_end);
    let unit = unit.trim();

    let multiplier = if unit.is_empty() || unit.eq_ignore_ascii_case("B") {
        1
    } else if unit.eq_ignore_ascii_case("K") || unit.eq_ignore_ascii_case("kB") {
        1_000
    } else if unit.eq_ignore_ascii_case("M") || unit.eq_ignore_ascii_case("MB") {
        1_000_000
    } else if unit.eq_ignore_ascii_case("G") || unit.eq_ignore_ascii_case("GB") {
        1_000_000_000
    } else {
        return Err(format!("unknown size unit `{unit}`"));
    };

    let mut parts = number.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(|digits| {
            digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit())
        })
        || parts.next().is_some()
    {
        return Err(format!("invalid size `{value}`"));
    }

    let fraction = fraction.unwrap_or_default();
    let digits = format!("{whole}{fraction}");
    let magnitude = digits
        .parse::<u128>()
        .map_err(|_| format!("size `{value}` is too large"))?;
    let divisor = 10_u128
        .checked_pow(
            fraction
                .len()
                .try_into()
                .map_err(|_| format!("size `{value}` is too precise"))?,
        )
        .ok_or_else(|| format!("size `{value}` is too precise"))?;
    let scaled = magnitude
        .checked_mul(multiplier)
        .ok_or_else(|| format!("size `{value}` is too large"))?;
    if scaled % divisor != 0 {
        return Err(format!("size `{value}` is not a whole number of bytes"));
    }

    u64::try_from(scaled / divisor).map_err(|_| format!("size `{value}` is too large"))
}

/// Common CLI arguments
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
            Arg::new("disk_usage")
                .short('d')
                .long("disk_usage")
                .action(ArgAction::SetTrue)
                .help("Index the files only and display the size"),
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
                .action(ArgAction::Append)
                .value_parser(parse_glob)
                .value_name("GLOB")
                .help("Include files whose names match GLOB (repeatable)"),
        )
        .arg(
            Arg::new("exclude_filter")
                .short('x')
                .long("exclude_filter")
                .action(ArgAction::Append)
                .value_parser(parse_glob)
                .value_name("GLOB")
                .help("Exclude files whose names match GLOB (repeatable)"),
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
                .value_parser(parse_size)
                .value_name("SIZE")
                .help("Filter out files smaller than SIZE"),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .action(ArgAction::Count)
                .help("Increase the log level verbosity"),
        )
}

/// Update provided `SearchConfig` based on CLI arguments from `ArgMatches`.
pub fn augment_config(mut config: SearchConfig, args: &ArgMatches) -> SearchConfig {
    let include_filter = args
        .get_many::<String>("include_filter")
        .map(|values| values.cloned().collect());
    if include_filter.is_some() {
        config.include_filter = include_filter
    }

    let exclude_filter = args
        .get_many::<String>("exclude_filter")
        .map(|values| values.cloned().collect());
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

    trace!("with arguments {:#?}", config);

    config
}

/// Convert verbose counts from cli (`-v`, `-vv`, etc.) to a log `tracing::Level`
pub fn log_level(count: u8) -> Level {
    match count {
        0 => Level::ERROR,
        1 => Level::INFO,
        2 => Level::DEBUG,
        _ => Level::TRACE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_conversion() {
        assert_eq!(log_level(0), Level::ERROR);
        assert_eq!(log_level(1), Level::INFO);
        assert_eq!(log_level(2), Level::DEBUG);
        assert_eq!(log_level(3), Level::TRACE);
        assert_eq!(log_level(100), Level::TRACE);
    }

    #[test]
    fn test_augment_config_sets_expected_fields() {
        let cmd = commands();
        let matches = cmd.get_matches_from(vec![
            "app",
            "--include_filter",
            "*.wav",
            "--include_filter",
            "*.mp3",
            "--exclude_filter",
            "bar",
            "--skip_hidden",
            "--check_image",
            "--check_audio",
            "--full_hash",
            "--threads",
            "4",
            "--min_size",
            "1024",
        ]);

        let config = augment_config(SearchConfig::default(), &matches);

        assert_eq!(
            config.include_filter,
            Some(vec!["*.wav".into(), "*.mp3".into()])
        );
        assert_eq!(config.exclude_filter, Some(vec!["bar".into()]));
        assert!(config.skip_hidden);
        assert_eq!(config.min_size, 1024);
        assert_eq!(config.threads, 4);
        assert!(config.image_config.compare);
        assert!(config.audio_config.compare);
        assert!(config.hasher_config.full_hash);
    }

    #[test]
    fn min_size_accepts_decimal_units() {
        for (value, expected) in [
            ("1024", 1_024),
            ("1 kB", 1_000),
            ("1K", 1_000),
            ("1 K", 1_000),
            ("1.5MB", 1_500_000),
            ("1M", 1_000_000),
            ("1 M", 1_000_000),
            ("2 GB", 2_000_000_000),
            ("1G", 1_000_000_000),
            ("1 G", 1_000_000_000),
        ] {
            let matches = commands()
                .try_get_matches_from(["app", "--min_size", value])
                .expect("valid minimum size should parse");
            let config = augment_config(SearchConfig::default(), &matches);

            assert_eq!(config.min_size, expected);
        }
    }

    #[test]
    fn min_size_rejects_invalid_values() {
        for value in ["0.5 B", "1 XB", "18446744073709551616"] {
            assert!(
                commands()
                    .try_get_matches_from(["app", "--min_size", value])
                    .is_err(),
                "{value} should be rejected"
            );
        }
    }

    #[test]
    fn filters_reject_invalid_globs() {
        assert!(
            commands()
                .try_get_matches_from(["app", "--include_filter", "["])
                .is_err()
        );
        assert!(
            commands()
                .try_get_matches_from(["app", "--exclude_filter", "{"])
                .is_err()
        );
    }
}
