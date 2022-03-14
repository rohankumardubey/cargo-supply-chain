//! Gather author, contributor, publisher data on crates in your dependency graph.
//!
//! There are some use cases:
//!
//! * Find people and groups worth supporting.
//! * An analysis of all the contributors you implicitly trust by building their software. This
//!   might have both a sobering and humbling effect.
//! * Identify risks in your dependency graph.

#![forbid(unsafe_code)]
#![allow(dead_code)] // TODO

use std::{error::Error, ffi::OsString, path::PathBuf, time::Duration};

use bpaf::*;
use pico_args::Arguments;

mod api_client;
mod common;
mod crates_cache;
mod publishers;
mod subcommands;

/* TODO:
Support these `cargo metadata` flags:
        --features <FEATURES>...         Space or comma separated list of features to activate
        --all-features                   Activate all available features
        --no-default-features            Do not activate the `default` feature
        --target <TRIPLE>...             Only include resolve dependencies matching the given target-triple
        --manifest-path <PATH>           Path to Cargo.toml
and maybe also
        --config <KEY=VALUE>...          Override a configuration value (unstable)
 */

fn main() -> Result<(), std::io::Error> {
    let args = args_parser().run();
    println!("{:?}", args);
    dispatch_command(args)
}

fn args_parser() -> OptionParser<ValidatedArgs> {
    let diffable = short('d')
        .long("diffable")
        .switch()
        .help("Make output more friendly towards tools such as `diff`");
    let cache_max_age_parser = long("cache-max-age")
        .argument("AGE")
        .parse(|text| humantime::parse_duration(&text))
        .help(
            "\
The cache will be considered valid while younger than specified.
The format is a human readable duration such as `1w` or `1d 6h`.
If not specified, the cache is considered valid for 48 hours.",
        )
        .fallback(Duration::from_secs(48 * 3600));
    let metadata_args = short('m').argument("ARGS").many();
    let cache_max_age = cache_max_age_parser.clone();
    let args_parser = construct!(QueryCommandArgs {
        cache_max_age,
        diffable,
        metadata_args
    });

    let all_features = long("all-features").switch()
    .help("Activate all available features");
    let no_default_features = long("no-default-features").switch().help("Do not activate the `default` feature");
    let features = long("features").argument("FEATURES").optional().help("Space or comma separated list of features to activate");
    let target = long("target").argument("TRIPLE").optional().help("Only include dependencies matching the given target-triple");
    let manifest_path = long("manifest-path").argument_os("PATH").map(|s| PathBuf::from(s)).optional().help("Path to Cargo.toml");

    let metadata_args_parser = construct!( MetadataArgs {
        all_features,
        no_default_features,
        features,
        target,
        manifest_path,
    });

    fn subcommand_with_common_args(
        command_name: &'static str,
        args: Parser<QueryCommandArgs>,
        meta_args: Parser<MetadataArgs>,
        descr: &'static str,
    ) -> bpaf::Parser<ValidatedArgs> {
        let parser = match command_name {
            "publishers" => construct!(ValidatedArgs::Publishers { args, meta_args }),
            "crates" => construct!(ValidatedArgs::Crates { args, meta_args }),
            "json" => construct!(ValidatedArgs::Json { args, meta_args }),
            _ => unreachable!(),
        };
        let parser = Info::default().descr(descr).for_parser(parser);
        command(command_name, Some(descr), parser)
    }

    let publishers = subcommand_with_common_args(
        "publishers",
        args_parser.clone(),
        metadata_args_parser.clone(),
        "List all crates.io publishers in the depedency graph",
    );
    let crates = subcommand_with_common_args(
        "crates",
        args_parser.clone(),
        metadata_args_parser.clone(),
        "List all crates in dependency graph and crates.io publishers for each",
    );
    let json = subcommand_with_common_args(
        "json",
        args_parser.clone(),
        metadata_args_parser.clone(),
        "Like 'crates', but in JSON and with more fields for each publisher",
    );

    let cache_max_age = cache_max_age_parser.clone();
    let update = construct!(ValidatedArgs::Update { cache_max_age });
    let update = Info::default()
        .descr("Download the latest daily dump from crates.io to speed up other commands")
        .for_parser(update);
    let update = command(
        "update",
        Some("Download the latest daily dump from crates.io to speed up other commands"),
        update,
    );

    //let help =            construct!(ValidatedArgs::Help { command });
    let parser = publishers.or_else(crates).or_else(json).or_else(update);

    Info::default()
        .version(env!("CARGO_PKG_VERSION"))
        .descr("Gather author, contributor and publisher data on crates in your dependency graph")
        .for_parser(parser)
}

fn dispatch_command(args: ValidatedArgs) -> Result<(), std::io::Error> {
    match args {
        ValidatedArgs::Publishers { args, meta_args } => {
            subcommands::publishers(args.metadata_args, args.diffable, args.cache_max_age)?
        }
        ValidatedArgs::Crates { args, meta_args } => {
            subcommands::crates(args.metadata_args, args.diffable, args.cache_max_age)?
        }
        ValidatedArgs::Json { args, meta_args } => {
            subcommands::json(args.metadata_args, args.diffable, args.cache_max_age)?
        }
        ValidatedArgs::Update { cache_max_age } => subcommands::update(cache_max_age),
        ValidatedArgs::Help { command } => subcommands::help(command.as_deref()),
    }

    Ok(())
}

fn parse_max_age(text: &str) -> Result<Duration, humantime::DurationError> {
    humantime::parse_duration(&text)
}

/// Arguments for typical querying commands - crates, publishers, json
#[derive(Clone, Debug)]
struct QueryCommandArgs {
    cache_max_age: Duration,
    diffable: bool,
    metadata_args: Vec<String>,
}

#[derive(Clone, Debug)]
enum ValidatedArgs {
    Publishers { args: QueryCommandArgs, meta_args: MetadataArgs },
    Crates { args: QueryCommandArgs, meta_args: MetadataArgs },
    Json { args: QueryCommandArgs, meta_args: MetadataArgs },
    Update { cache_max_age: Duration },
    Help { command: Option<String> },
}

/// Arguments to be passed to `cargo metadata`
#[derive(Clone, Debug)]
struct MetadataArgs {
    // `all_features` and `no_default_features` are not mutually exclusive in `cargo metadata`,
    // in the sense that it will not error out when encontering them; it just follows `all_features`
    all_features: bool,
    no_default_features: bool,
    // This is a `String` because we don't parse the value, just pass it on to `cargo metadata` blindly
    features: Option<String>,
    target: Option<String>,
    manifest_path: Option<PathBuf>,
}

/*  -- Everything below this line is going to be removed and replaced with bpaf --

#[derive(Clone, Debug)]
struct Args {
    help: bool,
    command: String,
    diffable: bool,
    cache_max_age: Duration,
    metadata_args: Vec<String>,
    free: Vec<String>,
}

/// CLI-focused help message for displaying to the user
pub(crate) const CLI_HELP: &str =
    "Usage: cargo supply-chain COMMAND [OPTIONS...] [-- CARGO_METADATA_OPTIONS...]

Commands:
  publishers   List all crates.io publishers in the depedency graph
  crates       List all crates in dependency graph and crates.io publishers for each
  json         Like 'crates', but in JSON and with more fields for each publisher
  update       Download the latest daily dump from crates.io to speed up other commands

See 'cargo supply-chain help <command>' for more information on a specific command.

Arguments:
  --cache-max-age  The cache will be considered valid while younger than specified.
                   The format is a human readable duration such as `1w` or `1d 6h`.
                   If not specified, the cache is considered valid for 48 hours.
  -d, --diffable   Make output more friendly towards tools such as `diff`

Any arguments after the `--` will be passed to `cargo metadata`, for example:
  cargo supply-chain crates -- --filter-platform=x86_64-unknown-linux-gnu
See `cargo metadata --help` for a list of flags it supports.";

fn get_args() -> Result<ValidatedArgs, Box<dyn Error>> {
    let args = parse_args()?;
    let valid_args = validate_args(args)?;
    Ok(valid_args)
}

fn validate_args(args: Args) -> Result<ValidatedArgs, std::io::Error> {
    if args.help {
        return Ok(ValidatedArgs::Help {
            command: Some(args.command),
        });
    } else {
        if args.command != "help" && !args.free.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Unrecognized argument: {}", args.free[0]),
            ));
        }
        match args.command.as_str() {
            "publishers" => {
                return Ok(ValidatedArgs::Publishers {
                    cache_max_age: args.cache_max_age,
                    diffable: args.diffable,
                    metadata_args: args.metadata_args,
                })
            }
            "crates" => {
                return Ok(ValidatedArgs::Crates {
                    cache_max_age: args.cache_max_age,
                    diffable: args.diffable,
                    metadata_args: args.metadata_args,
                })
            }
            "json" => {
                return Ok(ValidatedArgs::Json {
                    cache_max_age: args.cache_max_age,
                    diffable: args.diffable,
                    metadata_args: args.metadata_args,
                })
            }
            "update" => {
                return Ok(ValidatedArgs::Update {
                    cache_max_age: args.cache_max_age,
                })
            }
            "help" => {
                return Ok(ValidatedArgs::Help {
                    command: args.free.get(0).map(String::to_owned),
                })
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Unrecognized argument: {}", args.command.as_str()),
                ))
            }
        }
    }
}

/// Separates arguments intended for us and for cargo-metadata
fn separate_metadata_args() -> (Vec<OsString>, Vec<String>) {
    // Everything before "--" should be parsed, and everything after it should be passed to cargo-metadata
    let mut supply_args: Vec<OsString> = std::env::args_os()
        .skip(1) // skip argv[0], the name of the binary
        .take_while(|x| x != "--")
        .collect();
    let metadata_args = std::env::args()
        .skip(1) // skip argv[0], the name of the binary
        .skip_while(|x| x != "--")
        .skip(1) // skips "--" itself
        .collect();
    // When invoked via `cargo supply-chain update`, Cargo passes the arguments it receives verbatim.
    // So instead of "update" our binary receives "supply-chain update".
    // We ignore the "supply-chain" in the beginning if it's present.
    if supply_args.get(0) == Some(&OsString::from("supply-chain")) {
        supply_args.remove(0);
    }

    (supply_args, metadata_args)
}

/// Converts all recognized arguments into a struct.
/// Does not check whether the argument is valid for the given subcommand.
fn parse_args() -> Result<Args, pico_args::Error> {
    let (supply_args, metadata_args) = separate_metadata_args();
    let default_cache_max_age = Duration::from_secs(48 * 3600);
    let mut args = Arguments::from_vec(supply_args);
    if let Some(command) = args.subcommand()? {
        let args = Args {
            help: args.contains(["-h", "--help"]),
            command,
            diffable: args.contains(["-d", "--diffable"]),
            metadata_args,
            cache_max_age: args
                .opt_value_from_fn("--cache-max-age", parse_max_age)?
                .unwrap_or(default_cache_max_age),
            free: args.free()?,
        };
        Ok(args)
    } else {
        Err(pico_args::Error::ArgumentParsingFailed {
            cause: "No subcommand given".to_string(),
        })
    }
}

fn eprint_help() {
    eprintln!("{}", CLI_HELP);
}

*/

// TODO: remove all uses of this and return error from the function instead
pub(crate) fn err_exit(msg: &str) -> ! {
    match msg.into() {
        Some(v) => eprintln!("{}", v),
        None => (),
    };

    std::process::exit(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_max_age_parser() {
        let _ = args_parser()
            .run_inner(Args::from(&["crates", "--cache-max-age", "7d"]))
            .unwrap();
        let _ = args_parser()
            .run_inner(Args::from(&["crates", "--cache-max-age=7d"]))
            .unwrap();
        let _ = args_parser()
            .run_inner(Args::from(&["crates", "--cache-max-age=1w"]))
            .unwrap();
        let _ = args_parser()
            .run_inner(Args::from(&["crates", "--cache-max-age=1m"]))
            .unwrap();
        let _ = args_parser()
            .run_inner(Args::from(&["crates", "--cache-max-age=1s"]))
            .unwrap();
        // erroneous invocations that must be rejected
        assert!(args_parser()
            .run_inner(Args::from(&["crates", "--cache-max-age"]))
            .is_err());
        assert!(args_parser()
            .run_inner(Args::from(&["crates", "--cache-max-age=5"]))
            .is_err());
    }

    #[test]
    fn test_accepted_query_options() {
        for command in ["crates", "publishers", "json"] {
            let _ = args_parser().run_inner(Args::from(&[command])).unwrap();
            let _ = args_parser()
                .run_inner(Args::from(&[command, "-d"]))
                .unwrap();
            let _ = args_parser()
                .run_inner(Args::from(&[command, "--diffable"]))
                .unwrap();
            let _ = args_parser()
                .run_inner(Args::from(&[command, "--cache-max-age=7d"]))
                .unwrap();
            let _ = args_parser()
                .run_inner(Args::from(&[command, "-d", "--cache-max-age=7d"]))
                .unwrap();
            let _ = args_parser()
                .run_inner(Args::from(&[command, "--diffable", "--cache-max-age=7d"]))
                .unwrap();
        }
    }

    #[test]
    fn test_accepted_update_options() {
        let _ = args_parser().run_inner(Args::from(&["update"])).unwrap();
        let _ = args_parser()
            .run_inner(Args::from(&["update", "--cache-max-age=7d"]))
            .unwrap();
        // erroneous invocations that must be rejected
        assert!(args_parser()
            .run_inner(Args::from(&["update", "-d"]))
            .is_err());
        assert!(args_parser()
            .run_inner(Args::from(&["update", "--diffable"]))
            .is_err());
        assert!(args_parser()
            .run_inner(Args::from(&["update", "-d", "--cache-max-age=7d"]))
            .is_err());
        assert!(args_parser()
            .run_inner(Args::from(&["update", "--diffable", "--cache-max-age=7d"]))
            .is_err());
    }
}
