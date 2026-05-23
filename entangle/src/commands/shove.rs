//! Handler for `entangle shove`.
//!
//! Convenience alias for:
//!   git push origin --all
//!   git push origin --tags
//!
//! Because `entangle init` configures `origin` with two push URLs (GitHub and
//! Tangled), a single push command reaches both forges. `shove` is intended as
//! a one-time "push the whole thing" helper for the first sync after `init`,
//! when you want to make sure all branches and tags land on both forges.
//!
//! Errors from the push operations are surfaced with informative messages rather
//! than raw `gix` output.
//!
//! Stub — implemented in Step 11.

use crate::config::ConfigError;

/// Entry point called by `main.rs` for the `shove` subcommand.
pub fn run() -> Result<(), ConfigError> {
    // Stub — implemented in Step 11.
    println!("entangle shove: not yet implemented");
    Ok(())
}
