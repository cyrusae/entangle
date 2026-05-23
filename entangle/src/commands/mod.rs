//! Command handler modules — one per subcommand.
//!
//! Each module exposes a single `run(...)` function that `main.rs` calls
//! after argument parsing. The split keeps `main.rs` minimal (just wiring)
//! and each command self-contained.

pub mod init;
pub mod set;
pub mod setup;
pub mod shove;
