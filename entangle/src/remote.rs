//! SSH remote validation via `gix` ls-refs.
//!
//! Distinguishes three error cases that represent fundamentally different user problems:
//!
//! - [`RemoteCheckResult::NotFound`]: the repo URL resolved but the server-side
//!   `git-upload-pack` reported the repository does not exist. User action: check
//!   the repo name, or create the repo on that forge.
//!
//! - [`RemoteCheckResult::AuthFailure`]: SSH handshake failed — the user's key is
//!   not set up for that forge. User action: add their SSH key to GitHub or Tangled.
//!   This is a *different* problem from a typo and needs a different error message.
//!
//! - [`RemoteCheckResult::NetworkError`]: timeout or no route to host — the forge
//!   is unreachable right now. User action: check connectivity, or accept the
//!   override prompt to proceed offline.
//!
//! ## Error mapping (gix → RemoteCheckResult)
//!
//! gix wraps SSH subprocess output in its error chain. We classify by inspecting
//! the `Display` of the full error chain:
//!
//! | Substring in error chain          | Classification        |
//! |-----------------------------------|-----------------------|
//! | "permission denied", "publickey"  | `AuthFailure`         |
//! | "authentication failed"           | `AuthFailure`         |
//! | "repository not found"            | `NotFound`            |
//! | "repository does not exist"       | `NotFound`            |
//! | Timeout (30 s with no response)   | `NetworkError`        |
//! | Anything else                     | `NetworkError`        |
//!
//! The classification is intentionally conservative: when in doubt we report
//! `NetworkError` and prompt the user rather than incorrectly claiming a hard
//! `NotFound` or `AuthFailure`.
//!
//! ## Timeout
//!
//! `check_remote` spawns the gix call in a separate thread and waits at most
//! [`REMOTE_CHECK_TIMEOUT`] seconds. If the thread does not respond in time,
//! `NetworkError("connection timed out …")` is returned and the thread is
//! detached (it will eventually be cleaned up when the process exits).
//!
//! ## Repo context requirement
//!
//! gix's remote API requires a local `Repository` as a protocol context. The
//! function calls `gix::discover(".")`, which succeeds when `check_remote` is
//! called from within a git repository — as is always the case for `entangle init`.
//! If called outside a git repository, a `NetworkError` describing the discovery
//! failure is returned instead of panicking.

use std::sync::mpsc;
use std::time::Duration;

/// How long `check_remote` waits for the gix thread before reporting a timeout.
const REMOTE_CHECK_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Outcome of a single remote reachability check.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RemoteCheckResult {
    /// The remote exists and is accessible via SSH.
    Ok,
    /// SSH connected but the server reported the repository does not exist.
    NotFound,
    /// SSH connection was rejected — key not set up for this forge.
    AuthFailure,
    /// Network-level failure: host unreachable, DNS failure, timeout, etc.
    NetworkError(String),
}

/// Errors returned by [`validate_remotes`] / [`validate_remotes_with_checker`].
#[derive(Debug)]
pub enum RemoteError {
    /// The origin remote repository does not exist.
    OriginNotFound { url: String },
    /// SSH auth failed for the origin remote.
    OriginAuthFailure { url: String },
    /// The mirror remote repository does not exist.
    MirrorNotFound { url: String },
    /// SSH auth failed for the mirror remote.
    MirrorAuthFailure { url: String },
    /// User declined the offline-override prompt; setup was cleanly aborted.
    OfflineAborted,
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteError::OriginNotFound { url } => write!(
                f,
                "origin repository '{url}' does not exist. \
                 Create it on the forge, then re-run `entangle init`."
            ),
            RemoteError::OriginAuthFailure { url } => write!(
                f,
                "SSH authentication failed for origin '{url}'. \
                 Add your SSH public key to the forge, then re-run `entangle init`."
            ),
            RemoteError::MirrorNotFound { url } => write!(
                f,
                "mirror repository '{url}' does not exist. \
                 Create it on the forge, then re-run `entangle init`."
            ),
            RemoteError::MirrorAuthFailure { url } => write!(
                f,
                "SSH authentication failed for mirror '{url}'. \
                 Add your SSH public key to the forge, then re-run `entangle init`."
            ),
            RemoteError::OfflineAborted => write!(
                f,
                "Setup cancelled: could not verify remote accessibility. \
                 Check your network connection and re-run `entangle init`, \
                 or re-run and accept the offline-override prompt."
            ),
        }
    }
}

impl std::error::Error for RemoteError {}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check a single remote URL by attempting an SSH ls-refs via gix.
///
/// Runs the gix call in a background thread and returns a [`NetworkError`]
/// if no response arrives within [`REMOTE_CHECK_TIMEOUT`].
///
/// Requires the current directory to be inside a git repository so that
/// gix has a local context for its remote API. When called outside a repo,
/// returns `NetworkError` describing the discovery failure.
///
/// [`NetworkError`]: RemoteCheckResult::NetworkError
pub fn check_remote(url: &str) -> RemoteCheckResult {
    let url_owned = url.to_string();
    let (tx, rx) = mpsc::channel::<RemoteCheckResult>();

    // Spawn the gix call so we can apply a hard timeout.
    // All gix types are created inside the thread to avoid Send constraints.
    std::thread::spawn(move || {
        let result = do_ls_refs_blocking(&url_owned);
        let _ = tx.send(result); // ignore send error if receiver already timed out
    });

    match rx.recv_timeout(REMOTE_CHECK_TIMEOUT) {
        Ok(result) => result,
        Err(_) => RemoteCheckResult::NetworkError(format!(
            "connection timed out after {} seconds",
            REMOTE_CHECK_TIMEOUT.as_secs()
        )),
    }
}

/// Check both the origin and mirror remotes in order, prompting on network errors.
///
/// Calls `check_remote` for each URL and uses `dialoguer` for the offline-override
/// prompt. Testable logic lives in [`validate_remotes_with_checker`].
pub fn validate_remotes(
    origin_url: &str,
    mirror_url: &str,
) -> Result<(), RemoteError> {
    validate_remotes_with_checker(origin_url, mirror_url, check_remote, |url| {
        use dialoguer::{theme::ColorfulTheme, Confirm};
        eprintln!("Warning: couldn't reach '{url}'.");
        Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Accept anyway and proceed with setup?")
            .default(false) // default No — safe choice
            .interact()
            .unwrap_or(false)
    })
}

/// Testable core of remote validation.
///
/// Accepts an injectable `checker` (replaces `check_remote` in tests) and an
/// injectable `offline_override` prompt (replaces the dialoguer `Confirm` in tests).
///
/// ## Flow
///
/// 1. Check origin with `checker`.
///    - `Ok` → continue to step 2.
///    - `NotFound` / `AuthFailure` → hard error, return immediately without
///      checking mirror.
///    - `NetworkError` → call `offline_override(origin_url)`.
///      - Returns `true` → treat as OK, continue to step 2.
///      - Returns `false` → return `OfflineAborted`.
///
/// 2. Check mirror with `checker`.
///    - `Ok` → return `Ok(())`.
///    - `NotFound` / `AuthFailure` → hard error.
///    - `NetworkError` → call `offline_override(mirror_url)`.
///      - Returns `true` → return `Ok(())`.
///      - Returns `false` → return `OfflineAborted`.
pub fn validate_remotes_with_checker(
    origin_url: &str,
    mirror_url: &str,
    checker: impl Fn(&str) -> RemoteCheckResult,
    offline_override: impl Fn(&str) -> bool,
) -> Result<(), RemoteError> {
    // ── 1. Check origin ──────────────────────────────────────────────────────
    match checker(origin_url) {
        RemoteCheckResult::Ok => {}
        RemoteCheckResult::NotFound => {
            return Err(RemoteError::OriginNotFound {
                url: origin_url.to_string(),
            })
        }
        RemoteCheckResult::AuthFailure => {
            return Err(RemoteError::OriginAuthFailure {
                url: origin_url.to_string(),
            })
        }
        RemoteCheckResult::NetworkError(_) => {
            if !offline_override(origin_url) {
                return Err(RemoteError::OfflineAborted);
            }
            // User accepted — treat as reachable and continue to mirror check.
        }
    }

    // ── 2. Check mirror ──────────────────────────────────────────────────────
    match checker(mirror_url) {
        RemoteCheckResult::Ok => {}
        RemoteCheckResult::NotFound => {
            return Err(RemoteError::MirrorNotFound {
                url: mirror_url.to_string(),
            })
        }
        RemoteCheckResult::AuthFailure => {
            return Err(RemoteError::MirrorAuthFailure {
                url: mirror_url.to_string(),
            })
        }
        RemoteCheckResult::NetworkError(_) => {
            if !offline_override(mirror_url) {
                return Err(RemoteError::OfflineAborted);
            }
            // User accepted — treat as reachable.
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// gix implementation
// ---------------------------------------------------------------------------

/// Perform an SSH ls-refs against `url_str` using gix and return a classified result.
///
/// This is the only function that touches gix and the network. It is always
/// called inside a spawned thread by [`check_remote`].
fn do_ls_refs_blocking(url_str: &str) -> RemoteCheckResult {
    // gix's remote API requires a local repository as a protocol context.
    // entangle init always runs inside a git repo, so discovery should succeed.
    let repo = match gix::discover(".") {
        Ok(r) => r,
        Err(e) => {
            return RemoteCheckResult::NetworkError(format!(
                "not in a git repository (required for remote checks): {e}"
            ))
        }
    };

    // Parse the URL first so we can give a useful error if it's malformed.
    let url = match gix::url::parse(url_str.as_bytes().into()) {
        Ok(u) => u,
        Err(e) => {
            return RemoteCheckResult::NetworkError(format!("invalid remote URL '{url_str}': {e}"))
        }
    };

    // Create a transient remote from the URL — not saved to the repo's config.
    let remote = match repo.remote_at(url) {
        Ok(r) => r,
        Err(e) => return classify_error_chain(&e),
    };

    // Connect to the remote. This is where SSH auth failures and network errors
    // typically surface (SSH subprocess is spawned here).
    let connection = match remote.connect(gix::remote::Direction::Fetch) {
        Ok(c) => c,
        Err(e) => return classify_error_chain(&e),
    };

    // Perform the handshake and list refs. "Repository not found" errors from
    // the server side (git-upload-pack) surface here.
    match connection.ref_map(gix::progress::Discard, Default::default()) {
        Ok(_) => RemoteCheckResult::Ok,
        Err(e) => classify_error_chain(&e),
    }
}

/// Walk the full error chain and classify based on known substrings.
fn classify_error_chain(e: &dyn std::error::Error) -> RemoteCheckResult {
    let full_chain = full_error_chain(e);
    classify_error_str(&full_chain)
}

/// Collect the full `source()` chain of an error into a single lowercase string.
fn full_error_chain(e: &dyn std::error::Error) -> String {
    let mut parts = vec![e.to_string()];
    let mut current = e.source();
    while let Some(src) = current {
        parts.push(src.to_string());
        current = src.source();
    }
    parts.join(": ").to_lowercase()
}

/// Classify a lowercased error chain string into a [`RemoteCheckResult`].
///
/// See the module-level doc table for the full mapping rationale.
fn classify_error_str(lower: &str) -> RemoteCheckResult {
    if lower.contains("permission denied")
        || lower.contains("publickey")
        || lower.contains("authentication failed")
    {
        RemoteCheckResult::AuthFailure
    } else if lower.contains("repository not found")
        || lower.contains("repository does not exist")
        || lower.contains("no such repository")
    {
        RemoteCheckResult::NotFound
    } else {
        // Conservative: unknown errors are network errors (prompt the user,
        // don't incorrectly hard-stop with a "not found" or "auth" message).
        RemoteCheckResult::NetworkError(lower.to_string())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── RemoteCheckResult — basic properties ─────────────────────────────────

    #[test]
    fn remote_check_result_ok_is_not_error() {
        assert_eq!(RemoteCheckResult::Ok, RemoteCheckResult::Ok);
    }

    #[test]
    fn remote_check_result_variants_are_distinct() {
        assert_ne!(RemoteCheckResult::Ok, RemoteCheckResult::NotFound);
        assert_ne!(RemoteCheckResult::Ok, RemoteCheckResult::AuthFailure);
        assert_ne!(
            RemoteCheckResult::Ok,
            RemoteCheckResult::NetworkError("x".to_string())
        );
        assert_ne!(RemoteCheckResult::NotFound, RemoteCheckResult::AuthFailure);
    }

    // ── classify_error_str ────────────────────────────────────────────────────

    #[test]
    fn classify_permission_denied_is_auth_failure() {
        assert_eq!(
            classify_error_str("permission denied (publickey)"),
            RemoteCheckResult::AuthFailure
        );
    }

    #[test]
    fn classify_publickey_alone_is_auth_failure() {
        assert_eq!(
            classify_error_str("publickey"),
            RemoteCheckResult::AuthFailure
        );
    }

    #[test]
    fn classify_authentication_failed_is_auth_failure() {
        assert_eq!(
            classify_error_str("authentication failed"),
            RemoteCheckResult::AuthFailure
        );
    }

    #[test]
    fn classify_repository_not_found_is_not_found() {
        assert_eq!(
            classify_error_str("error: repository not found"),
            RemoteCheckResult::NotFound
        );
    }

    #[test]
    fn classify_repository_does_not_exist_is_not_found() {
        assert_eq!(
            classify_error_str("repository does not exist"),
            RemoteCheckResult::NotFound
        );
    }

    #[test]
    fn classify_no_such_repository_is_not_found() {
        assert_eq!(
            classify_error_str("no such repository"),
            RemoteCheckResult::NotFound
        );
    }

    #[test]
    fn classify_connection_refused_is_network_error() {
        let r = classify_error_str("connection refused");
        assert!(matches!(r, RemoteCheckResult::NetworkError(_)));
    }

    #[test]
    fn classify_no_route_to_host_is_network_error() {
        let r = classify_error_str("no route to host");
        assert!(matches!(r, RemoteCheckResult::NetworkError(_)));
    }

    #[test]
    fn classify_unknown_error_is_network_error() {
        let r = classify_error_str("something completely unexpected happened");
        assert!(matches!(r, RemoteCheckResult::NetworkError(_)));
    }

    // ── RemoteError Display ───────────────────────────────────────────────────

    #[test]
    fn remote_error_origin_not_found_mentions_url_and_action() {
        let e = RemoteError::OriginNotFound {
            url: "git@github.com:user/repo.git".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("git@github.com:user/repo.git"), "must mention URL: {s}");
        assert!(s.contains("does not exist"), "must describe problem: {s}");
    }

    #[test]
    fn remote_error_origin_auth_failure_mentions_url_and_action() {
        let e = RemoteError::OriginAuthFailure {
            url: "git@github.com:user/repo.git".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("git@github.com:user/repo.git"), "must mention URL: {s}");
        assert!(s.contains("SSH authentication"), "must describe problem: {s}");
    }

    #[test]
    fn remote_error_mirror_not_found_mentions_url_and_action() {
        let e = RemoteError::MirrorNotFound {
            url: "git@tangled.org:user/repo".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("git@tangled.org:user/repo"), "must mention URL: {s}");
        assert!(s.contains("does not exist"), "must describe problem: {s}");
    }

    #[test]
    fn remote_error_offline_aborted_mentions_network_and_retry() {
        let s = RemoteError::OfflineAborted.to_string();
        assert!(s.contains("cancelled") || s.contains("aborted"), "must say it was cancelled: {s}");
        assert!(s.contains("entangle init"), "must suggest retry: {s}");
    }

    // ── validate_remotes_with_checker — ordering ──────────────────────────────

    /// Verifies that origin is checked before mirror and that a hard error on
    /// origin prevents mirror from being checked at all.
    #[test]
    fn origin_not_found_fails_before_mirror_is_checked() {
        let mirror_checked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mirror_flag = mirror_checked.clone();

        let result = validate_remotes_with_checker(
            "git@github.com:user/origin.git",
            "git@tangled.org:user/mirror",
            move |url| {
                if url.contains("tangled") {
                    mirror_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    RemoteCheckResult::Ok
                } else {
                    RemoteCheckResult::NotFound
                }
            },
            |_| false, // offline prompt: not expected to be called
        );

        assert!(
            result.is_err(),
            "NotFound on origin must be an error"
        );
        assert!(
            !mirror_checked.load(std::sync::atomic::Ordering::SeqCst),
            "mirror must not be checked when origin fails with NotFound"
        );
        assert!(
            matches!(result.unwrap_err(), RemoteError::OriginNotFound { .. }),
            "error variant must be OriginNotFound"
        );
    }

    #[test]
    fn origin_auth_failure_fails_before_mirror_is_checked() {
        let mirror_checked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mirror_flag = mirror_checked.clone();

        let result = validate_remotes_with_checker(
            "git@github.com:user/origin.git",
            "git@tangled.org:user/mirror",
            move |url| {
                if url.contains("tangled") {
                    mirror_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    RemoteCheckResult::Ok
                } else {
                    RemoteCheckResult::AuthFailure
                }
            },
            |_| false,
        );

        assert!(result.is_err());
        assert!(!mirror_checked.load(std::sync::atomic::Ordering::SeqCst));
        assert!(matches!(result.unwrap_err(), RemoteError::OriginAuthFailure { .. }));
    }

    // ── validate_remotes_with_checker — happy paths ───────────────────────────

    #[test]
    fn both_ok_returns_ok() {
        let result = validate_remotes_with_checker(
            "git@github.com:user/repo.git",
            "git@tangled.org:user/repo",
            |_| RemoteCheckResult::Ok,
            |_| false,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn mirror_not_found_returns_mirror_error() {
        let result = validate_remotes_with_checker(
            "git@github.com:user/repo.git",
            "git@tangled.org:user/repo",
            |url| {
                if url.contains("github") {
                    RemoteCheckResult::Ok
                } else {
                    RemoteCheckResult::NotFound
                }
            },
            |_| false,
        );
        assert!(matches!(result.unwrap_err(), RemoteError::MirrorNotFound { .. }));
    }

    #[test]
    fn mirror_auth_failure_returns_mirror_auth_error() {
        let result = validate_remotes_with_checker(
            "git@github.com:user/repo.git",
            "git@tangled.org:user/repo",
            |url| {
                if url.contains("github") {
                    RemoteCheckResult::Ok
                } else {
                    RemoteCheckResult::AuthFailure
                }
            },
            |_| false,
        );
        assert!(matches!(result.unwrap_err(), RemoteError::MirrorAuthFailure { .. }));
    }

    // ── validate_remotes_with_checker — NetworkError / offline override ───────

    #[test]
    fn origin_network_error_calls_offline_override() {
        let override_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = override_called.clone();

        let _ = validate_remotes_with_checker(
            "git@github.com:user/repo.git",
            "git@tangled.org:user/repo",
            |_| RemoteCheckResult::NetworkError("no route to host".to_string()),
            move |_url| {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                false // decline
            },
        );

        assert!(
            override_called.load(std::sync::atomic::Ordering::SeqCst),
            "offline override must be called for NetworkError on origin"
        );
    }

    #[test]
    fn origin_network_error_accepted_continues_to_mirror() {
        let mirror_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = mirror_called.clone();

        let result = validate_remotes_with_checker(
            "git@github.com:user/repo.git",
            "git@tangled.org:user/repo",
            move |url| {
                if url.contains("tangled") {
                    flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    RemoteCheckResult::Ok
                } else {
                    RemoteCheckResult::NetworkError("offline".to_string())
                }
            },
            |_| true, // accept
        );

        assert!(result.is_ok());
        assert!(
            mirror_called.load(std::sync::atomic::Ordering::SeqCst),
            "mirror must still be checked when origin NetworkError is accepted"
        );
    }

    #[test]
    fn origin_network_error_declined_returns_offline_aborted() {
        let result = validate_remotes_with_checker(
            "git@github.com:user/repo.git",
            "git@tangled.org:user/repo",
            |_| RemoteCheckResult::NetworkError("no route to host".to_string()),
            |_| false, // decline
        );
        assert!(matches!(result.unwrap_err(), RemoteError::OfflineAborted));
    }

    #[test]
    fn mirror_network_error_accepted_returns_ok() {
        let result = validate_remotes_with_checker(
            "git@github.com:user/repo.git",
            "git@tangled.org:user/repo",
            |url| {
                if url.contains("github") {
                    RemoteCheckResult::Ok
                } else {
                    RemoteCheckResult::NetworkError("offline".to_string())
                }
            },
            |_| true, // accept
        );
        assert!(result.is_ok());
    }

    #[test]
    fn mirror_network_error_declined_returns_offline_aborted() {
        let result = validate_remotes_with_checker(
            "git@github.com:user/repo.git",
            "git@tangled.org:user/repo",
            |url| {
                if url.contains("github") {
                    RemoteCheckResult::Ok
                } else {
                    RemoteCheckResult::NetworkError("offline".to_string())
                }
            },
            |_| false, // decline
        );
        assert!(matches!(result.unwrap_err(), RemoteError::OfflineAborted));
    }

    // ── online integration tests (#[ignore] — require network + SSH keys) ─────

    /// Verifies that a real GitHub repo returns Ok.
    ///
    /// Requires: network access and an SSH key configured for github.com.
    /// Run with: cargo test -- --ignored check_remote_real_github_ok
    #[test]
    #[ignore]
    fn check_remote_real_github_ok() {
        let result = check_remote("git@github.com:cyrusae/entangle.git");
        assert_eq!(
            result,
            RemoteCheckResult::Ok,
            "expected Ok for a real GitHub repo"
        );
    }

    /// Verifies that a non-routable IP address yields NetworkError.
    ///
    /// Uses TEST-NET-1 (192.0.2.0/24, RFC 5737), which is reserved for
    /// documentation and must not route on any real network.
    ///
    /// Requires: no route to 192.0.2.1 (should be true on any normal network).
    /// Note: this test will take up to REMOTE_CHECK_TIMEOUT seconds to complete.
    /// Run with: cargo test -- --ignored check_remote_non_routable_is_network_error
    #[test]
    #[ignore]
    fn check_remote_non_routable_is_network_error() {
        let result = check_remote("git@192.0.2.1:user/repo.git");
        assert!(
            matches!(result, RemoteCheckResult::NetworkError(_)),
            "expected NetworkError for non-routable address, got: {result:?}"
        );
    }
}
