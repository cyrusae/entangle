//! Handler for `entangle set`.
//!
//! Non-interactively sets individual config values:
//!   entangle set gh-user <username>
//!   entangle set tngl-user <username>
//!   entangle set origin <github|tangled>
//!
//! Flow: sanitize/validate the value → load config (or start empty) →
//! update the single field → save → print confirmation.
//!
//! Always prints what was set, e.g.:
//!   "GitHub username set to: cyrusae"
//! Silent success is hard to debug, so we always confirm.
//!
//! Stub — implemented in Step 4.

use crate::cli::SetKey;
use crate::config::ConfigError;

/// Entry point called by `main.rs` for the `set` subcommand.
pub fn run(key: SetKey, value: String) -> Result<(), ConfigError> {
    // Stub — implemented in Step 4.
    println!("entangle set {key:?} {value}: not yet implemented");
    Ok(())
}
