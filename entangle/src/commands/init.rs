//! Handler for `entangle init`.
//!
//! The main event. Full flow (see DESIGN.md for rationale on each step):
//!
//!  1. Check config is valid; refer to `entangle setup` if not.
//!  2. Collect repo name and optional alias (CLI args or interactive prompts).
//!  3. Validate repo name(s).
//!  4. Build prospective GitHub and Tangled SSH URLs.
//!  5. Validate URLs — local regex first (fast fail), then `gix` SSH ls-refs.
//!  6. Detect git repo status of the current directory; `git init` if needed.
//!  7. Check for `.gitignore` and `README.md`; suggest adding them if absent.
//!  8. Inspect existing remotes; handle the overwrite/proceed prompts.
//!  9. Add non-origin push URL, then origin push URL (order matters).
//! 10. Print final remote state and suggest `entangle shove`.
//!
//! Stub — implemented in Steps 8–10.

/// Entry point called by `main.rs` for the `init` subcommand.
pub fn run(repo: Option<String>, alias: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    // Stub — implemented in Steps 8–10.
    println!("entangle init (repo={repo:?}, alias={alias:?}): not yet implemented");
    Ok(())
}
