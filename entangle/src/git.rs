//! `gix` wrappers for local git operations.
//!
//! Covers: repo detection, `git init`, remote inspection, and adding/replacing
//! push URLs. All functions here operate on the local filesystem only —
//! network operations live in `remote.rs`.
//!
//! **Design note**: we use `gix` rather than shelling out to `git` for all
//! production code. Integration tests may call `git` directly to verify the
//! observable state of the repository (e.g., `git remote -v`), but that is
//! the only sanctioned use of the `git` binary.
//!
//! Stub module — implemented in Steps 8–10.
