//! Handler for `entangle setup`.
//!
//! Interactive first-time (and repeat) configuration. Prompts for:
//!   1. GitHub username
//!   2. Tangled username
//!   3. Origin preference (GitHub or Tangled; default GitHub)
//!
//! If a field is already set in the config, the user sees:
//!   "GitHub username is already set to: cyrusae. Change? [Y/n]"
//! and can skip it by pressing Enter.
//!
//! Config is written only after *all* prompts complete — no partial saves.
//! Ctrl+C is caught cleanly; the existing config (if any) is left unchanged.
//!
//! Stub — implemented in Step 5.

use crate::config::ConfigError;

/// Entry point called by `main.rs` for the `setup` subcommand.
pub fn run() -> Result<(), ConfigError> {
    // Stub — implemented in Step 5.
    println!("entangle setup: not yet implemented");
    Ok(())
}
