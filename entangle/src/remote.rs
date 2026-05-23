//! SSH remote validation via `gix` ls-refs.
//!
//! Distinguishes three error cases that represent fundamentally different user problems:
//!
//! - [`RemoteCheckResult::NotFound`]: the repo URL resolved but returned no refs —
//!   the repo doesn't exist or hasn't been initialized. User action: check the repo
//!   name / create the repo on that forge.
//!
//! - [`RemoteCheckResult::AuthFailure`]: SSH handshake failed — the user's key is
//!   not set up for that forge. User action: add their SSH key to GitHub or Tangled.
//!   This is a *different* problem from a typo and needs a different error message.
//!
//! - [`RemoteCheckResult::NetworkError`]: timeout or no route to host — the forge
//!   is unreachable right now. User action: check connectivity, or accept the
//!   override prompt to proceed offline.
//!
//! Stub module — implemented in Step 7.
