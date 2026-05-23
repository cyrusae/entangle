//! Input sanitization and validation.
//!
//! All user-facing input passes through this module before being used anywhere.
//! Functions here are pure (no I/O, no side effects) so they are fully unit-testable offline.
//!
//! **Order of operations (always):** sanitize first, then validate.
//! This means `"CyrusAE"` becomes `"cyrusae"` before the GitHub username regex runs —
//! the user gets a clean success, not a confusing "uppercase not allowed" error.
//!
//! Stub module — validation logic is implemented in Step 3.
