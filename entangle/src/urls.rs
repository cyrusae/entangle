//! URL construction for GitHub and Tangled SSH remotes.
//!
//! Pure functions — no network calls, no I/O. Given a username and repo name,
//! return the SSH URL string. Also resolves which URL is `origin` vs. mirror
//! based on the user's `OriginPreference`.
//!
//! Stub module — implemented in Step 6.
