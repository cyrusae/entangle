//! Integration tests for `entangle set`.
//!
//! These tests spawn the compiled `entangle` binary and verify exit codes,
//! stdout/stderr content, and config file state. They complement the unit
//! tests in `src/commands/set.rs`, which test the core logic directly.
//!
//! ## What these tests cover
//!
//! **Happy-path writes**
//! - `entangle set gh-user <value>` creates/updates the config file.
//! - `entangle set tngl-user <value>` does the same for the Tangled username.
//! - `entangle set origin <value>` does the same for origin preference.
//! - Aliases (`github-user`, `tangled-user`) are accepted.
//! - Case normalisation: `CyrusAE` → `cyrusae` in the saved file.
//! - Sequential `set` calls accumulate without clobbering other fields.
//! - Success prints a confirmation message to stdout.
//!
//! **Argument-parsing edge cases (clap rejects)**
//! - `entangle set gh-user` with no VALUE → exits non-zero.
//! - `entangle set tngl-user` with no VALUE → exits non-zero.
//! - `entangle set origin` with no VALUE → exits non-zero.
//! - `entangle set` with no KEY and no VALUE → exits non-zero.
//!
//! **Validation errors**
//! - Invalid GitHub username (`-bad`) → exits non-zero with error on stderr.
//! - Invalid Tangled username (`nodot`) → same.
//! - Unknown origin value (`gitlab`) → same.
//! - Config must not be written when validation fails.
//!
//! ## Isolation
//!
//! Every test sets `ENTANGLE_CONFIG_PATH` to a per-test temp file so the
//! user's real config is never touched.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Spawn `entangle set <args>` with `ENTANGLE_CONFIG_PATH` pointing at
/// `config_path`, and return the raw [`Output`].
fn run_set(args: &[&str], config_path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_entangle"))
        .arg("set")
        .args(args)
        .env("ENTANGLE_CONFIG_PATH", config_path)
        .output()
        .expect("failed to spawn entangle set")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Create a fresh temp dir and return a config path inside it.
fn fresh_config() -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.json");
    (dir, config_path)
}

/// Read the config file as a `serde_json::Value` for field inspection.
fn read_config(path: &Path) -> serde_json::Value {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("could not read config at {}: {e}", path.display()));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("config not valid JSON: {e}\ncontent: {content}"))
}

// ---------------------------------------------------------------------------
// Happy-path: each key writes the correct field
// ---------------------------------------------------------------------------

#[test]
fn set_gh_user_creates_config_with_github_username() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["gh-user", "cyrusae"], &config_path);
    assert!(
        out.status.success(),
        "set gh-user must succeed\nstdout: {}\nstderr: {}",
        stdout(&out),
        stderr(&out)
    );
    assert!(config_path.exists(), "config file must be created by set");

    let cfg = read_config(&config_path);
    assert_eq!(
        cfg["github_username"], "cyrusae",
        "github_username must match the supplied value"
    );
    // Other fields must not be written when they weren't provided.
    assert!(
        cfg["tangled_username"].is_null(),
        "tangled_username must not be written by set gh-user"
    );
}

#[test]
fn set_tngl_user_creates_config_with_tangled_username() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["tngl-user", "atdot.fyi"], &config_path);
    assert!(
        out.status.success(),
        "set tngl-user must succeed\nstdout: {}\nstderr: {}",
        stdout(&out),
        stderr(&out)
    );

    let cfg = read_config(&config_path);
    assert_eq!(cfg["tangled_username"], "atdot.fyi");
    assert!(cfg["github_username"].is_null());
}

#[test]
fn set_origin_creates_config_with_origin_preference() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["origin", "github"], &config_path);
    assert!(
        out.status.success(),
        "set origin must succeed\nstdout: {}\nstderr: {}",
        stdout(&out),
        stderr(&out)
    );

    let cfg = read_config(&config_path);
    assert_eq!(cfg["origin_preference"], "github");
}

/// `github-user` is the long alias for `gh-user` — it must be accepted.
#[test]
fn set_github_user_long_alias_accepted() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["github-user", "cyrusae"], &config_path);
    assert!(
        out.status.success(),
        "long alias 'github-user' must be accepted\nstdout: {}\nstderr: {}",
        stdout(&out),
        stderr(&out)
    );

    let cfg = read_config(&config_path);
    assert_eq!(cfg["github_username"], "cyrusae");
}

/// `tangled-user` is the long alias for `tngl-user` — it must be accepted.
#[test]
fn set_tangled_user_long_alias_accepted() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["tangled-user", "atdot.fyi"], &config_path);
    assert!(
        out.status.success(),
        "long alias 'tangled-user' must be accepted\nstdout: {}\nstderr: {}",
        stdout(&out),
        stderr(&out)
    );

    let cfg = read_config(&config_path);
    assert_eq!(cfg["tangled_username"], "atdot.fyi");
}

/// Origin alias `gh` → stored as `"github"`.
#[test]
fn set_origin_gh_alias_stores_github() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["origin", "gh"], &config_path);
    assert!(out.status.success(), "set origin gh must succeed");

    let cfg = read_config(&config_path);
    assert_eq!(
        cfg["origin_preference"], "github",
        "alias 'gh' must be stored as canonical 'github'"
    );
}

/// Origin alias `tngl` → stored as `"tangled"`.
#[test]
fn set_origin_tngl_alias_stores_tangled() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["origin", "tngl"], &config_path);
    assert!(out.status.success(), "set origin tngl must succeed");

    let cfg = read_config(&config_path);
    assert_eq!(
        cfg["origin_preference"], "tangled",
        "alias 'tngl' must be stored as canonical 'tangled'"
    );
}

// ---------------------------------------------------------------------------
// Case normalisation
// ---------------------------------------------------------------------------

/// Mixed-case GitHub username is lowercased before saving.
///
/// This pins the sanitize-first contract at the binary level: `CyrusAE`
/// must become `cyrusae` in the config file, not produce an error.
#[test]
fn set_gh_user_normalises_mixed_case() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["gh-user", "CyrusAE"], &config_path);
    assert!(
        out.status.success(),
        "mixed-case username must succeed (sanitized before validation)\
         \nstdout: {}\nstderr: {}",
        stdout(&out),
        stderr(&out)
    );

    let cfg = read_config(&config_path);
    assert_eq!(
        cfg["github_username"], "cyrusae",
        "username must be lowercased before saving"
    );
}

/// Mixed-case Tangled username is lowercased before saving.
#[test]
fn set_tngl_user_normalises_mixed_case() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["tngl-user", "AtDot.FYI"], &config_path);
    assert!(
        out.status.success(),
        "mixed-case Tangled username must succeed"
    );

    let cfg = read_config(&config_path);
    assert_eq!(cfg["tangled_username"], "atdot.fyi");
}

/// Mixed-case origin alias is lowercased before alias matching.
#[test]
fn set_origin_normalises_mixed_case() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["origin", "GitHub"], &config_path);
    assert!(out.status.success(), "mixed-case 'GitHub' must be accepted");

    let cfg = read_config(&config_path);
    assert_eq!(cfg["origin_preference"], "github");
}

// ---------------------------------------------------------------------------
// Sequential writes accumulate — no clobber
// ---------------------------------------------------------------------------

/// Three separate `set` calls populate all three fields independently.
///
/// After all three, `entangle init` must be able to load the config. This
/// pins the non-clobber guarantee at the binary level.
#[test]
fn sequential_set_calls_accumulate_all_fields() {
    let (_dir, config_path) = fresh_config();

    run_set(&["gh-user", "cyrusae"], &config_path);
    run_set(&["tngl-user", "atdot.fyi"], &config_path);
    run_set(&["origin", "github"], &config_path);

    let cfg = read_config(&config_path);
    assert_eq!(cfg["github_username"], "cyrusae");
    assert_eq!(cfg["tangled_username"], "atdot.fyi");
    assert_eq!(cfg["origin_preference"], "github");
}

/// Updating one field must leave the others unchanged.
#[test]
fn set_gh_user_does_not_clobber_existing_tangled_username() {
    let (_dir, config_path) = fresh_config();

    // Establish both fields.
    run_set(&["gh-user", "old-name"], &config_path);
    run_set(&["tngl-user", "atdot.fyi"], &config_path);

    // Overwrite only the GitHub username.
    let out = run_set(&["gh-user", "cyrusae"], &config_path);
    assert!(out.status.success(), "update must succeed");

    let cfg = read_config(&config_path);
    assert_eq!(
        cfg["github_username"], "cyrusae",
        "github_username must be updated"
    );
    assert_eq!(
        cfg["tangled_username"], "atdot.fyi",
        "tangled_username must remain unchanged"
    );
}

// ---------------------------------------------------------------------------
// Confirmation output
// ---------------------------------------------------------------------------

/// A successful `set` must print a confirmation to stdout so the user knows
/// what was stored, without requiring them to open the config file.
#[test]
fn set_gh_user_prints_confirmation_to_stdout() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["gh-user", "cyrusae"], &config_path);
    assert!(out.status.success(), "must succeed");

    let out_str = stdout(&out);
    assert!(
        !out_str.is_empty(),
        "stdout must contain a confirmation message"
    );
    assert!(
        out_str.contains("cyrusae"),
        "confirmation must mention the set value: {out_str}"
    );
}

// ---------------------------------------------------------------------------
// Argument-parsing edge cases (clap rejects)
// ---------------------------------------------------------------------------

/// `entangle set gh-user` with no VALUE → clap must reject with a usage error.
#[test]
fn set_gh_user_without_value_exits_nonzero() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["gh-user"], &config_path);
    assert!(
        !out.status.success(),
        "set gh-user with no value must exit non-zero"
    );
    // clap writes usage errors to stderr.
    let err = stderr(&out);
    assert!(
        !err.is_empty(),
        "stderr must contain an error for missing value"
    );
}

/// `entangle set tngl-user` with no VALUE → clap must reject.
#[test]
fn set_tngl_user_without_value_exits_nonzero() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["tngl-user"], &config_path);
    assert!(
        !out.status.success(),
        "set tngl-user with no value must exit non-zero"
    );
    let err = stderr(&out);
    assert!(
        !err.is_empty(),
        "stderr must contain an error for missing value"
    );
}

/// `entangle set origin` with no VALUE → clap must reject.
#[test]
fn set_origin_without_value_exits_nonzero() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["origin"], &config_path);
    assert!(
        !out.status.success(),
        "set origin with no value must exit non-zero"
    );
    let err = stderr(&out);
    assert!(
        !err.is_empty(),
        "stderr must contain an error for missing value"
    );
}

/// `entangle set` with no KEY and no VALUE → clap must reject.
#[test]
fn set_with_no_args_exits_nonzero() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&[], &config_path);
    assert!(
        !out.status.success(),
        "set with no arguments must exit non-zero"
    );
    let err = stderr(&out);
    assert!(
        !err.is_empty(),
        "stderr must contain an error for missing key"
    );
}

/// Config must not be written when clap rejects the invocation.
#[test]
fn set_with_no_args_does_not_create_config() {
    let (_dir, config_path) = fresh_config();

    run_set(&[], &config_path);
    assert!(
        !config_path.exists(),
        "config must not be created when set receives no arguments"
    );
}

// ---------------------------------------------------------------------------
// Validation errors
// ---------------------------------------------------------------------------

/// Invalid GitHub username → exits non-zero with an actionable error on stderr.
#[test]
fn set_gh_user_invalid_username_exits_nonzero() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["gh-user", "-invalid-leading-hyphen"], &config_path);
    assert!(
        !out.status.success(),
        "invalid GitHub username must exit non-zero"
    );
    let err = stderr(&out);
    assert!(
        !err.is_empty(),
        "stderr must contain a validation error message"
    );
}

/// Config must not be written when the GitHub username is invalid.
#[test]
fn set_gh_user_invalid_username_does_not_write_config() {
    let (_dir, config_path) = fresh_config();

    run_set(&["gh-user", "-invalid-leading-hyphen"], &config_path);
    assert!(
        !config_path.exists(),
        "config must not be created when validation fails"
    );
}

/// Invalid Tangled username → exits non-zero.
#[test]
fn set_tngl_user_invalid_username_exits_nonzero() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["tngl-user", "nodot"], &config_path);
    assert!(
        !out.status.success(),
        "invalid Tangled username must exit non-zero"
    );
    let err = stderr(&out);
    assert!(
        !err.is_empty(),
        "stderr must contain a validation error for invalid Tangled username"
    );
}

/// Unrecognised origin value → exits non-zero with an error.
#[test]
fn set_origin_unknown_value_exits_nonzero() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["origin", "gitlab"], &config_path);
    assert!(
        !out.status.success(),
        "unknown origin 'gitlab' must exit non-zero"
    );
    let err = stderr(&out);
    assert!(!err.is_empty(), "stderr must describe the accepted values");
}

/// Config must not be written when the origin value is unrecognised.
#[test]
fn set_origin_unknown_value_does_not_write_config() {
    let (_dir, config_path) = fresh_config();

    run_set(&["origin", "gitlab"], &config_path);
    assert!(
        !config_path.exists(),
        "config must not be created for an unrecognised origin value"
    );
}

/// Shell metacharacter in username → rejected by sanitization (not a clap error).
#[test]
fn set_gh_user_dangerous_char_exits_nonzero() {
    let (_dir, config_path) = fresh_config();

    let out = run_set(&["gh-user", "cyrus$ae"], &config_path);
    assert!(
        !out.status.success(),
        "dangerous character in username must exit non-zero"
    );
}
