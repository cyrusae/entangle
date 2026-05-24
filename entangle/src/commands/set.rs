//! Handler for `entangle set`.
//!
//! Non-interactively sets individual config values:
//!   entangle set gh-user <username>
//!   entangle set tngl-user <username>
//!   entangle set origin <github|tangled>
//!
//! Flow for each invocation:
//!   1. Sanitize and validate the value for the given key.
//!   2. Load whatever config fields are already on disk (missing file → blank slate).
//!   3. Overwrite the target field; leave all others untouched.
//!   4. Write back to disk.
//!   5. Print a confirmation line.
//!
//! Config is updated field-by-field via [`PartialConfig`] so that running
//! `entangle set gh-user cyrusae` never clobbers a previously-set tangled username.
//!
//! Always prints what was set — silent success is hard to debug.

use std::path::Path;

use crate::cli::SetKey;
use crate::config::{OriginPreference, PartialConfig, config_path};
use crate::output;
use crate::validate::{validate_github_username, validate_tangled_username};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Entry point called by `main.rs` for the `set` subcommand.
///
/// Resolves the platform config path and delegates to [`run_with_config_path`].
pub fn run(key: SetKey, value: String) -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path()?;
    run_with_config_path(key, value, &path)
}

// ---------------------------------------------------------------------------
// Testable core
// ---------------------------------------------------------------------------

/// Set a single config field, reading from and writing to `config_path`.
///
/// Separated from [`run`] so tests can pass a [`tempfile`] path without
/// touching the real platform config directory.
pub fn run_with_config_path(
    key: SetKey,
    value: String,
    config_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // ── 1. Validate the value ────────────────────────────────────────────────
    // Each key has its own validation rules. We validate *before* touching the
    // config file so an invalid value never results in a partial write.
    let (validated_value, confirmation) = validate_for_key(&key, &value)?;

    // ── 2. Load current config (or start from a blank slate) ─────────────────
    let mut partial = PartialConfig::load_from_path(config_path)?;

    // ── 3. Update exactly the requested field ────────────────────────────────
    apply_to_partial(&mut partial, &key, validated_value);

    // ── 4. Save ──────────────────────────────────────────────────────────────
    partial.save_to_path(config_path)?;

    // ── 5. Confirm ───────────────────────────────────────────────────────────
    println!("{}", output::success(&confirmation));

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Validate `value` according to the rules for `key`.
///
/// Returns `(validated_value_string, confirmation_message)` on success, or an
/// error if the value fails validation.
///
/// The validated value is always a `String` — for the `origin` key it's the
/// canonical name ("github" or "tangled") rather than any alias the user typed.
fn validate_for_key(
    key: &SetKey,
    value: &str,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    match key {
        SetKey::GithubUser => {
            let validated = validate_github_username(value)?;
            let msg = format!("GitHub username set to: {validated}");
            Ok((validated, msg))
        }

        SetKey::TangledUser => {
            let validated = validate_tangled_username(value)?;
            let msg = format!("Tangled username set to: {validated}");
            Ok((validated, msg))
        }

        SetKey::Origin => {
            // The origin value isn't a username, so we don't use the username
            // validators. We do sanitize (lowercase + strip quotes), then check
            // against the known aliases. An unknown alias is a hard error.
            let sanitized = crate::validate::sanitize(value)?;
            let pref = OriginPreference::from_alias(&sanitized).ok_or_else(|| {
                format!(
                    "'{sanitized}' is not a valid origin preference. \
                     Use 'github' (or 'gh') or 'tangled' (or 'tngl')."
                )
            })?;
            let canonical = pref.to_string();
            let msg = format!("Origin preference set to: {canonical}");
            // We store the canonical string; apply_to_partial re-parses it.
            Ok((canonical, msg))
        }
    }
}

/// Write the validated value into the correct field of `partial`.
fn apply_to_partial(partial: &mut PartialConfig, key: &SetKey, validated: String) {
    match key {
        SetKey::GithubUser => {
            partial.github_username = Some(validated);
        }
        SetKey::TangledUser => {
            partial.tangled_username = Some(validated);
        }
        SetKey::Origin => {
            // `validated` is already the canonical string ("github"/"tangled"),
            // so from_alias is guaranteed to succeed here.
            partial.origin_preference = OriginPreference::from_alias(&validated);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, OriginPreference};
    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Create a (TempDir, PathBuf) pair for a config file that doesn't yet
    /// exist on disk.  The TempDir must be kept alive for the lifetime of the
    /// test; the PathBuf is the path to pass to save/load helpers.
    ///
    /// We deliberately do NOT use NamedTempFile as a write target: on Windows
    /// NamedTempFile holds the file open with an exclusive lock, and
    /// atomic_write_config's rename-into-place fails with "Access is denied."
    /// Using a path inside a TempDir avoids that because no handle is open on
    /// the destination file.
    fn new_temp_config() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        (dir, path)
    }

    /// Run `set` against a fresh temp path and return the dir + path + result.
    /// Caller must keep `_dir` alive for the duration of the test.
    fn set_at(
        key: SetKey,
        value: &str,
    ) -> (
        tempfile::TempDir,
        std::path::PathBuf,
        Result<(), Box<dyn std::error::Error>>,
    ) {
        let (dir, path) = new_temp_config();
        let result = run_with_config_path(key, value.to_string(), &path);
        (dir, path, result)
    }

    /// Read back a PartialConfig from a path.
    fn read_partial(path: &std::path::Path) -> PartialConfig {
        PartialConfig::load_from_path(path).unwrap()
    }

    // ── Setting each key ─────────────────────────────────────────────────────

    #[test]
    fn set_gh_user_writes_github_username() {
        let (_dir, path, result) = set_at(SetKey::GithubUser, "cyrusae");
        result.unwrap();
        let partial = read_partial(&path);
        assert_eq!(partial.github_username, Some("cyrusae".to_string()));
        assert_eq!(partial.tangled_username, None);
        assert_eq!(partial.origin_preference, None);
    }

    #[test]
    fn set_tngl_user_writes_tangled_username() {
        let (_dir, path, result) = set_at(SetKey::TangledUser, "atdot.fyi");
        result.unwrap();
        let partial = read_partial(&path);
        assert_eq!(partial.tangled_username, Some("atdot.fyi".to_string()));
        assert_eq!(partial.github_username, None);
        assert_eq!(partial.origin_preference, None);
    }

    #[test]
    fn set_origin_github_full_word() {
        let (_dir, path, result) = set_at(SetKey::Origin, "github");
        result.unwrap();
        let partial = read_partial(&path);
        assert_eq!(partial.origin_preference, Some(OriginPreference::Github));
    }

    #[test]
    fn set_origin_gh_alias() {
        let (_dir, path, result) = set_at(SetKey::Origin, "gh");
        result.unwrap();
        let partial = read_partial(&path);
        assert_eq!(partial.origin_preference, Some(OriginPreference::Github));
    }

    #[test]
    fn set_origin_tangled_full_word() {
        let (_dir, path, result) = set_at(SetKey::Origin, "tangled");
        result.unwrap();
        let partial = read_partial(&path);
        assert_eq!(partial.origin_preference, Some(OriginPreference::Tangled));
    }

    #[test]
    fn set_origin_tngl_alias() {
        let (_dir, path, result) = set_at(SetKey::Origin, "tngl");
        result.unwrap();
        let partial = read_partial(&path);
        assert_eq!(partial.origin_preference, Some(OriginPreference::Tangled));
    }

    // ── Other fields are not clobbered ───────────────────────────────────────

    #[test]
    fn set_gh_user_leaves_existing_fields_untouched() {
        let (_dir, path) = new_temp_config();

        // Prime with two fields already set.
        let initial = PartialConfig {
            github_username: Some("old-name".to_string()),
            tangled_username: Some("atdot.fyi".to_string()),
            origin_preference: Some(OriginPreference::Tangled),
        };
        initial.save_to_path(&path).unwrap();

        // Overwrite just the GitHub username.
        run_with_config_path(SetKey::GithubUser, "cyrusae".to_string(), &path).unwrap();

        let updated = read_partial(&path);
        assert_eq!(updated.github_username, Some("cyrusae".to_string()));
        // Other fields must be unchanged.
        assert_eq!(updated.tangled_username, Some("atdot.fyi".to_string()));
        assert_eq!(updated.origin_preference, Some(OriginPreference::Tangled));
    }

    #[test]
    fn set_on_missing_config_creates_file_with_just_that_field() {
        // Point at a path that doesn't exist yet — set() must create it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("entangle").join("config.json");

        run_with_config_path(SetKey::GithubUser, "cyrusae".to_string(), &path).unwrap();

        let partial = PartialConfig::load_from_path(&path).unwrap();
        assert_eq!(partial.github_username, Some("cyrusae".to_string()));
        assert_eq!(partial.tangled_username, None);
        assert_eq!(partial.origin_preference, None);
    }

    // ── Validation errors prevent writes ─────────────────────────────────────

    #[test]
    fn invalid_github_username_does_not_write_config() {
        let (_dir, path) = new_temp_config();
        let result = run_with_config_path(
            SetKey::GithubUser,
            "-invalid-leading-hyphen".to_string(),
            &path,
        );
        assert!(result.is_err());
        // File was never written; PartialConfig::load_from_path returns default on missing.
        let partial = read_partial(&path);
        assert_eq!(partial.github_username, None);
    }

    #[test]
    fn invalid_tangled_username_does_not_write_config() {
        let (_dir, path) = new_temp_config();
        let result = run_with_config_path(
            SetKey::TangledUser,
            "nodot".to_string(), // missing TLD dot
            &path,
        );
        assert!(result.is_err());
        let partial = read_partial(&path);
        assert_eq!(partial.tangled_username, None);
    }

    #[test]
    fn invalid_origin_value_does_not_write_config() {
        let (_dir, path) = new_temp_config();
        let result = run_with_config_path(SetKey::Origin, "gitlab".to_string(), &path);
        assert!(result.is_err());
        let partial = read_partial(&path);
        assert_eq!(partial.origin_preference, None);
    }

    #[test]
    fn dangerous_char_in_username_does_not_write_config() {
        let (_dir, path) = new_temp_config();
        let result = run_with_config_path(SetKey::GithubUser, "cyrus$ae".to_string(), &path);
        assert!(result.is_err());
        let partial = read_partial(&path);
        assert_eq!(partial.github_username, None);
    }

    // ── Sanitization flows through ────────────────────────────────────────────

    #[test]
    fn set_gh_user_normalises_case() {
        let (_dir, path, result) = set_at(SetKey::GithubUser, "CyrusAE");
        result.unwrap();
        assert_eq!(
            read_partial(&path).github_username,
            Some("cyrusae".to_string())
        );
    }

    #[test]
    fn set_gh_user_strips_quotes() {
        let (_dir, path, result) = set_at(SetKey::GithubUser, r#""cyrusae""#);
        result.unwrap();
        assert_eq!(
            read_partial(&path).github_username,
            Some("cyrusae".to_string())
        );
    }

    #[test]
    fn set_origin_normalises_case() {
        let (_dir, path, result) = set_at(SetKey::Origin, "GitHub");
        result.unwrap();
        assert_eq!(
            read_partial(&path).origin_preference,
            Some(OriginPreference::Github)
        );
    }

    // ── Integration: full round-trip produces a loadable Config ──────────────

    #[test]
    fn three_sets_produce_valid_full_config() {
        let (_dir, path) = new_temp_config();

        run_with_config_path(SetKey::GithubUser, "cyrusae".to_string(), &path).unwrap();
        run_with_config_path(SetKey::TangledUser, "atdot.fyi".to_string(), &path).unwrap();
        run_with_config_path(SetKey::Origin, "github".to_string(), &path).unwrap();

        // After all three fields are set, Config::load_from_path must succeed.
        let cfg = Config::load_from_path(&path).unwrap();
        assert_eq!(cfg.github_username, "cyrusae");
        assert_eq!(cfg.tangled_username, "atdot.fyi");
        assert_eq!(cfg.origin_preference, OriginPreference::Github);
    }
}
