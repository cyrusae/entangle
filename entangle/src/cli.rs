//! CLI type definitions — all `clap` structs and enums live here.
//!
//! `main.rs` imports these and dispatches to the appropriate command handler.
//! Keeping clap definitions separate from dispatch logic makes both easier to read.

use clap::{Parser, Subcommand};

/// entangle: easy setup for mirroring GitHub repos to Tangled.org in one command.
///
/// Run `entangle <COMMAND> --help` for per-command usage.
#[derive(Debug, Parser)]
#[command(
    name = "entangle",
    about = "Easy setup for mirroring GitHub repos to Tangled.org",
    long_about = "entangle configures your git remotes so that a single `git push` \
                  sends your code to both GitHub and Tangled.org simultaneously.\n\n\
                  Quickstart:\n  \
                  1. entangle setup          # configure your usernames once\n  \
                  2. entangle init           # wire up remotes in this repo\n  \
                  3. entangle shove          # first-time push to both forges",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// The four subcommands entangle exposes.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Interactive first-time (and repeat) configuration of usernames and origin preference.
    ///
    /// Prompts for GitHub username, Tangled username, and which forge is the primary fetch
    /// remote. Stores results in your platform config directory.
    Setup,

    /// Non-interactively set individual config values.
    ///
    /// Examples:
    ///   entangle set gh-user cyrusae
    ///   entangle set tngl-user atdot.fyi
    ///   entangle set origin github
    Set {
        /// The config key to update.
        ///
        /// Valid keys: `gh-user` / `github-user`, `tngl-user` / `tangled-user`,
        /// `origin` (value: `gh`/`github` or `tngl`/`tangled`)
        #[arg(value_name = "KEY")]
        key: SetKey,

        /// The value to assign to the key.
        #[arg(value_name = "VALUE")]
        value: String,
    },

    /// Wire up GitHub and Tangled push remotes in the current git repository.
    ///
    /// With no arguments: prompts for repo name and optional Tangled alias.
    /// With arguments: `entangle init <repo-name> [alias]`
    ///
    /// Requires a valid config (run `entangle setup` first).
    Init {
        /// The repository name on GitHub (and Tangled, unless an alias is given).
        #[arg(value_name = "REPO")]
        repo: Option<String>,

        /// Optional alternate name for the repo on the non-origin forge.
        #[arg(value_name = "ALIAS")]
        alias: Option<String>,

        /// Suppress all informational output; only errors and interactive
        /// prompts are shown. Overrides `verbosity_preference` in config.
        #[arg(short = 'q', long = "quiet", conflicts_with = "debug")]
        quiet: bool,

        /// Print internal diagnostics in addition to normal output (parsed git
        /// config values, code paths taken, etc.). Overrides `verbosity_preference`
        /// in config.
        #[arg(long = "debug", conflicts_with = "quiet")]
        debug: bool,
    },

    /// Push all branches and tags to both forges in one command.
    ///
    /// Equivalent to: `git push origin --all && git push origin --tags`
    ///
    /// Because `origin` already has two push URLs configured by `entangle init`,
    /// a single push command reaches both GitHub and Tangled automatically.
    Shove,
}

/// Valid keys for `entangle set`.
///
/// Multiple aliases are accepted for each key (e.g. `gh-user` and `github-user`
/// are interchangeable) to reduce friction.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum SetKey {
    /// GitHub username
    #[value(name = "gh-user", alias = "github-user")]
    GithubUser,

    /// Tangled (ATProto) username / handle
    #[value(name = "tngl-user", alias = "tangled-user")]
    TangledUser,

    /// Which forge is the primary fetch remote (`github`/`gh` or `tangled`/`tngl`)
    #[value(name = "origin")]
    Origin,
}
