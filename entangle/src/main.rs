//! entangle — easy setup for mirroring GitHub repos to Tangled.org.
//!
//! This file is intentionally thin: parse args, dispatch to the right command
//! handler, surface any errors. All real logic lives in the modules below.
//!
//! Module layout:
//!   cli.rs          — clap type definitions (Cli, Commands, SetKey)
//!   config.rs       — Config struct, load/save, ConfigError
//!   validate.rs     — sanitization and validation (pure functions, no I/O)
//!   urls.rs         — SSH URL construction (pure functions)
//!   git.rs          — gix wrappers: repo detection, remotes, push
//!   remote.rs       — SSH ls-refs, RemoteCheckResult
//!   commands/
//!     setup.rs      — `entangle setup` handler
//!     set.rs        — `entangle set` handler
//!     init.rs       — `entangle init` handler
//!     shove.rs      — `entangle shove` handler

mod cli;
mod commands;
mod config;
mod git;
mod remote;
mod urls;
mod validate;

use clap::Parser;
use cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    // All handlers return Result<(), Box<dyn std::error::Error>> so that each
    // command module can use ? freely across ConfigError, ValidationError, gix
    // errors, and anything else that arises without a central error-enum wrapper.
    let result: Result<(), Box<dyn std::error::Error>> = match cli.command {
        // No subcommand: print available commands (not an error).
        None => {
            print_available_commands();
            return;
        }

        Some(Commands::Setup) => commands::setup::run(),
        Some(Commands::Set { key, value }) => commands::set::run(key, value),
        Some(Commands::Init { repo, alias, quiet, debug }) => {
            commands::init::run(repo, alias, quiet, debug)
        }
        Some(Commands::Shove) => commands::shove::run(),
    };

    // Surface errors to stderr with a clean message, then exit non-zero.
    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

/// Print a brief command listing when `entangle` is run with no subcommand.
///
/// This is friendlier than an error — the user just needs a nudge, not a scolding.
fn print_available_commands() {
    println!("entangle — easy setup for mirroring GitHub repos to Tangled.org");
    println!();
    println!("Commands:");
    println!("  setup   Interactive configuration of usernames and origin preference");
    println!("  set     Non-interactively set a single config value");
    println!("  init    Wire up GitHub and Tangled push remotes in this repo");
    println!("  shove   Push all branches and tags to both forges");
    println!();
    println!("Run `entangle <COMMAND> --help` for per-command usage.");
}
