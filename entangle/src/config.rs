//! Config struct, load/save, and error types.
//!
//! The config file lives at `{config_dir}/entangle/config.json`.
//! On Unix this is `~/.config/entangle/config.json`.
//! On Windows this is `%APPDATA%\entangle\config.json` (i.e., `AppData\Roaming`).
//!
//! NOTE (Windows): `dirs::config_dir()` returns `AppData\Roaming` on Windows,
//! not `AppData\Local`. This matches the XDG convention of "user-specific
//! non-cache data that should roam with the profile". No code changes needed
//! here, but it's worth knowing when debugging Windows paths.
//!
//! ## Testability
//!
//! The public `load()` / `save()` functions call the real platform config path.
//! Their `_from_path` / `_to_path` counterparts accept an explicit `&Path` and
//! are used by tests, which pass a `tempfile` location to avoid touching the
//! user's real config.

use dirs::config_dir;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// How much output `entangle` produces.
///
/// Levels are ordered: `Quiet < Verbose < Debug`. Code that emits output
/// checks `if verbosity >= RequiredLevel` so that each level is a superset of
/// the one below it.
///
/// ## Serialization
///
/// Stored as a lowercase string in the config file:
/// `"quiet"`, `"verbose"`, or `"debug"`.
///
/// The `verbose` level is the default, so it is **omitted** from the config
/// file when serializing (via `skip_serializing_if`) to keep existing config
/// files clean. If the field is absent when loading, the default applies.
///
/// ## Override precedence
///
/// CLI flags override the config file:
/// - `-q` / `--quiet` → [`VerbosityLevel::Quiet`], regardless of config
/// - `--debug`        → [`VerbosityLevel::Debug`],  regardless of config
/// - (no flag)        → use `config.verbosity_preference`, which defaults to
///   [`VerbosityLevel::Verbose`]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum VerbosityLevel {
    /// Only errors and interactive prompts reach the user.
    ///
    /// All informational `stdout` is suppressed. `stderr` (errors, dialoguer
    /// prompts) is unaffected.
    Quiet,

    /// Full informational output: status messages, tips, URL preview, etc.
    ///
    /// This is the default level.
    #[default]
    Verbose,

    /// Everything in `Verbose` plus internal diagnostics: parsed git config
    /// values, code paths taken during remote inspection, etc.
    Debug,
}

impl VerbosityLevel {
    /// Returns `true` when this is the default level (`Verbose`).
    ///
    /// Used by `#[serde(skip_serializing_if)]` so that the default level is
    /// omitted from the config file — existing configs without
    /// `verbosity_preference` remain valid and unchanged.
    fn is_default_verbosity(&self) -> bool {
        matches!(self, VerbosityLevel::Verbose)
    }
}

// ---------------------------------------------------------------------------

/// Which forge is the primary fetch remote — i.e., where `git fetch` pulls from.
///
/// The non-origin forge is configured as a push-only URL on the same `origin` remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OriginPreference {
    /// GitHub is the fetch remote (default).
    Github,
    /// Tangled.org is the fetch remote.
    Tangled,
}

impl std::fmt::Display for OriginPreference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OriginPreference::Github => write!(f, "github"),
            OriginPreference::Tangled => write!(f, "tangled"),
        }
    }
}

impl OriginPreference {
    /// Parse an origin preference from a user-supplied string alias.
    ///
    /// Accepts the following (case-insensitive after sanitization):
    /// - `"github"` or `"gh"` → [`OriginPreference::Github`]
    /// - `"tangled"` or `"tngl"` → [`OriginPreference::Tangled`]
    ///
    /// Returns `None` for anything else so callers can produce their own error message.
    pub fn from_alias(s: &str) -> Option<Self> {
        match s {
            "github" | "gh" => Some(OriginPreference::Github),
            "tangled" | "tngl" => Some(OriginPreference::Tangled),
            _ => None,
        }
    }
}

/// Persisted user configuration.
///
/// Stored as JSON at `{config_dir}/entangle/config.json`.
/// `github_username`, `tangled_username`, and `origin_preference` are required;
/// missing required fields produce actionable errors. `verbosity_preference` is
/// optional and defaults to [`VerbosityLevel::Verbose`] when absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// GitHub username (≤39 chars, alphanumeric + hyphens).
    pub github_username: String,

    /// Tangled username — a valid ATProto handle (e.g. `atdot.fyi`).
    pub tangled_username: String,

    /// Which forge is the primary fetch remote. Defaults to `github`.
    pub origin_preference: OriginPreference,

    /// How much output `entangle` produces. Defaults to [`VerbosityLevel::Verbose`].
    ///
    /// Omitted from the config file when set to the default so that existing
    /// configs without this field remain valid and unchanged.
    ///
    /// Override at runtime with `-q` (quiet) or `--debug`; the CLI flag takes
    /// precedence over this stored preference.
    #[serde(default, skip_serializing_if = "VerbosityLevel::is_default_verbosity")]
    pub verbosity_preference: VerbosityLevel,
}

impl Config {
    /// Resolve the effective verbosity level for a command invocation.
    ///
    /// Applies the CLI-override precedence rule:
    /// - `quiet` flag → [`VerbosityLevel::Quiet`]
    /// - `debug` flag → [`VerbosityLevel::Debug`]
    /// - neither     → the stored `verbosity_preference` (defaulting to
    ///   [`VerbosityLevel::Verbose`] if never set)
    ///
    /// `quiet` and `debug` are mutually exclusive at the CLI layer (enforced by
    /// clap's `conflicts_with`), so this function does not need to handle the
    /// case where both are `true`.
    pub fn effective_verbosity(&self, quiet: bool, debug: bool) -> VerbosityLevel {
        if quiet {
            VerbosityLevel::Quiet
        } else if debug {
            VerbosityLevel::Debug
        } else {
            self.verbosity_preference
        }
    }
}

// ---------------------------------------------------------------------------
// PartialConfig — for incremental field-by-field updates via `entangle set`
// ---------------------------------------------------------------------------

/// A version of [`Config`] where every field is optional.
///
/// `entangle set` uses this so it can update a single field without requiring
/// the other fields to already be present. The file format is identical to
/// `Config` — missing fields simply aren't written (via `skip_serializing_if`).
///
/// Typical flow:
/// 1. Load whatever is on disk into a `PartialConfig` (missing file → all `None`).
/// 2. Overwrite the target field.
/// 3. Save back.
///
/// `Config::load()` still requires all three fields; `PartialConfig` is only
/// for the write path of `entangle set` and `entangle setup`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartialConfig {
    /// GitHub username, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_username: Option<String>,

    /// Tangled username, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tangled_username: Option<String>,

    /// Origin preference, if set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_preference: Option<OriginPreference>,
}

impl PartialConfig {
    /// Load whatever config fields are present on disk.
    ///
    /// Unlike [`Config::load_from_path`], this never errors on missing fields —
    /// they simply come back as `None`. It *does* error on corrupted JSON, since
    /// a corrupt file can't be safely updated in-place.
    ///
    /// If the file doesn't exist yet, returns an all-`None` default.
    pub fn load_from_path(path: &Path) -> Result<Self, ConfigError> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // No file yet — start from a clean slate.
                return Ok(PartialConfig::default());
            }
            Err(e) => return Err(ConfigError::Unreadable(e)),
        };

        // An empty or whitespace-only file is treated as "no config set" rather
        // than an error, since `set` is adding a field regardless.
        if content.trim().is_empty() {
            return Ok(PartialConfig::default());
        }

        serde_json::from_str(&content).map_err(|e| ConfigError::Corrupted(e.to_string()))
    }

    /// Write the partial config to disk, creating parent directories as needed.
    ///
    /// Only fields that are `Some` are written. Fields that are `None` are
    /// omitted from the JSON, so a partial config file with just one field is
    /// valid and expected.
    pub fn save_to_path(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ConfigError::CannotCreateDir)?;
        }

        let json = serde_json::to_string_pretty(self)
            .expect("PartialConfig serialization should never fail");

        std::fs::write(path, json).map_err(ConfigError::CannotWriteFile)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can arise when loading or saving the config file.
///
/// Each variant corresponds to a distinct user-actionable situation so that
/// error messages can tell the user exactly which `entangle` command to run next.
#[derive(Debug)]
pub enum ConfigError {
    /// `dirs::config_dir()` returned `None` — unusual; platform may not support it.
    NoPlatformConfigDir,

    /// The config file doesn't exist yet. Run `entangle setup` to create one.
    NotFound,

    /// The config file exists but couldn't be opened (e.g. permissions).
    Unreadable(std::io::Error),

    /// The config file is empty or contains only `{}` with no fields.
    Empty,

    /// The JSON parsed but `github_username` is missing. Run `entangle set gh-user`.
    MissingGithubUsername,

    /// The JSON parsed but `tangled_username` is missing. Run `entangle set tngl-user`.
    MissingTangledUsername,

    /// The JSON parsed but `origin_preference` is missing. Run `entangle set origin`.
    MissingOriginPreference,

    /// The JSON is present but couldn't be parsed at all. Re-run `entangle setup`.
    Corrupted(String),

    /// Couldn't create the config directory (e.g. permissions).
    CannotCreateDir(std::io::Error),

    /// Couldn't write the config file (e.g. permissions, disk full).
    CannotWriteFile(std::io::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::NoPlatformConfigDir => write!(
                f,
                "Could not determine a config directory for this platform."
            ),
            ConfigError::NotFound => write!(
                f,
                "No config file found. Run `entangle setup` to create one."
            ),
            ConfigError::Unreadable(e) => write!(f, "Config file exists but couldn't be read: {e}"),
            ConfigError::Empty => write!(
                f,
                "Config file is empty. Run `entangle setup` to fill it in."
            ),
            ConfigError::MissingGithubUsername => write!(
                f,
                "Config is missing `github_username`. Run `entangle set gh-user <username>`."
            ),
            ConfigError::MissingTangledUsername => write!(
                f,
                "Config is missing `tangled_username`. Run `entangle set tngl-user <username>`."
            ),
            ConfigError::MissingOriginPreference => write!(
                f,
                "Config is missing `origin_preference`. Run `entangle set origin <github|tangled>`."
            ),
            ConfigError::Corrupted(msg) => write!(
                f,
                "Config file is unreadable ({msg}). Re-run `entangle setup` to overwrite it."
            ),
            ConfigError::CannotCreateDir(e) => {
                write!(f, "Couldn't create config directory: {e}")
            }
            ConfigError::CannotWriteFile(e) => write!(f, "Couldn't write config file: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

// ---------------------------------------------------------------------------
// Path helper
// ---------------------------------------------------------------------------

/// Returns the path to the config file: `{config_dir}/entangle/config.json`.
///
/// Does not check whether the file or its parent directory exists.
///
/// ## Override via environment variable
///
/// If `ENTANGLE_CONFIG_PATH` is set, its value is used as the config path
/// directly, bypassing the platform config directory. This is useful for:
/// - Integration tests that need an isolated config file.
/// - Power users who want a non-standard config location.
///
/// ```sh
/// ENTANGLE_CONFIG_PATH=/tmp/my-entangle.json entangle setup
/// ```
pub fn config_path() -> Result<PathBuf, ConfigError> {
    // Check for an explicit override first. The env var takes precedence over
    // the platform default, which lets integration tests stay isolated without
    // touching the user's real config directory.
    if let Ok(override_path) = std::env::var("ENTANGLE_CONFIG_PATH") {
        return Ok(PathBuf::from(override_path));
    }
    let base = config_dir().ok_or(ConfigError::NoPlatformConfigDir)?;
    Ok(base.join("entangle").join("config.json"))
}

// ---------------------------------------------------------------------------
// Load
// ---------------------------------------------------------------------------

impl Config {
    /// Load the config from the platform config path.
    ///
    /// Returns a specific [`ConfigError`] variant for each failure mode so that
    /// callers can tell the user exactly which `entangle` command to run next.
    ///
    /// Call order: get path → read file → check emptiness → parse JSON →
    /// check per-field presence → full deserialization.
    pub fn load() -> Result<Config, ConfigError> {
        let path = config_path()?;
        Config::load_from_path(&path)
    }

    /// Load the config from an explicit path.
    ///
    /// This is the testable core of [`Config::load`]; tests pass a `tempfile`
    /// path here to avoid touching the real platform config directory.
    pub fn load_from_path(path: &Path) -> Result<Config, ConfigError> {
        // ── 1. Read the file ────────────────────────────────────────────────
        let content = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ConfigError::NotFound
            } else {
                // Could be a permissions error, a directory where a file was
                // expected, an encoding issue, etc.
                ConfigError::Unreadable(e)
            }
        })?;

        // ── 2. Reject trivially empty files ─────────────────────────────────
        // Covers: zero-byte file, whitespace-only file.
        if content.trim().is_empty() {
            return Err(ConfigError::Empty);
        }

        // ── 3. Parse as generic JSON Value ──────────────────────────────────
        // We parse to `serde_json::Value` first so we can distinguish
        // "field is missing" from "JSON is malformed" — serde's derive macro
        // collapses both into the same error category.
        let value: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| ConfigError::Corrupted(e.to_string()))?;

        // ── 4. Reject empty objects {} ───────────────────────────────────────
        let obj = value.as_object().ok_or(ConfigError::Corrupted(
            "Expected a JSON object at the top level".to_string(),
        ))?;

        if obj.is_empty() {
            return Err(ConfigError::Empty);
        }

        // ── 5. Check each required field individually ────────────────────────
        // Each missing field produces its own error variant so the Display
        // message can name the exact `entangle set` command to run.
        if !obj.contains_key("github_username") {
            return Err(ConfigError::MissingGithubUsername);
        }
        if !obj.contains_key("tangled_username") {
            return Err(ConfigError::MissingTangledUsername);
        }
        if !obj.contains_key("origin_preference") {
            return Err(ConfigError::MissingOriginPreference);
        }

        // ── 6. Full deserialization ──────────────────────────────────────────
        // At this point we know all three keys are present. Deserialization can
        // still fail if a value has the wrong type (e.g., `origin_preference`
        // is `"gitlab"` — not a valid variant). Treat that as Corrupted.
        serde_json::from_value(value).map_err(|e| ConfigError::Corrupted(e.to_string()))
    }

    // ---------------------------------------------------------------------------
    // Save
    // ---------------------------------------------------------------------------

    /// Write the config to the platform config path.
    ///
    /// Creates `{config_dir}/entangle/` if it doesn't already exist.
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = config_path()?;
        self.save_to_path(&path)
    }

    /// Write the config to an explicit path.
    ///
    /// This is the testable core of [`Config::save`]; tests pass a `tempfile`
    /// path here to avoid touching the real platform config directory.
    ///
    /// Creates all parent directories if they don't exist (equivalent to `mkdir -p`).
    pub fn save_to_path(&self, path: &Path) -> Result<(), ConfigError> {
        // ── 1. Create parent directories ─────────────────────────────────────
        // `create_dir_all` is idempotent — no error if the directory already exists.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ConfigError::CannotCreateDir)?;
        }

        // ── 2. Serialize to pretty JSON ──────────────────────────────────────
        // Pretty-printing makes the file human-readable if the user wants to
        // inspect or hand-edit it (not encouraged, but not forbidden).
        let json =
            serde_json::to_string_pretty(self).expect("Config serialization should never fail");

        // ── 3. Write atomically-ish via a newline-terminated string ──────────
        // We write the complete serialized string in one call; partial writes
        // are unlikely on local filesystems but the worst case is a
        // re-run of `entangle setup`, not data loss in the repo.
        std::fs::write(path, json).map_err(ConfigError::CannotWriteFile)?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// A fully-valid config for use as test input.
    fn valid_config() -> Config {
        Config {
            github_username: "cyrusae".to_string(),
            tangled_username: "atdot.fyi".to_string(),
            origin_preference: OriginPreference::Github,
            verbosity_preference: Default::default(),
        }
    }

    /// A fully-valid JSON string matching `valid_config()`.
    fn valid_json() -> &'static str {
        r#"{"github_username":"cyrusae","tangled_username":"atdot.fyi","origin_preference":"github"}"#
    }

    /// Write a string to a NamedTempFile and return it (file stays open).
    fn temp_with(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{content}").unwrap();
        f
    }

    // ── Config::path() ───────────────────────────────────────────────────────

    #[test]
    fn config_path_ends_with_entangle_config_json() {
        // config_path() can theoretically return NoPlatformConfigDir in a
        // stripped CI environment, so we skip instead of panic.
        let Ok(path) = config_path() else { return };

        // Platform-agnostic: check the last two components regardless of separator.
        let mut components: Vec<_> = path.components().collect();
        let file = components.pop().unwrap();
        let dir = components.pop().unwrap();

        assert_eq!(
            file.as_os_str(),
            "config.json",
            "last component should be config.json"
        );
        assert_eq!(
            dir.as_os_str(),
            "entangle",
            "second-to-last component should be entangle"
        );
    }

    // ── Serialization round-trip ─────────────────────────────────────────────

    #[test]
    fn serialize_deserialize_roundtrip() {
        let original = valid_config();
        let json = serde_json::to_string(&original).unwrap();
        let restored: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn origin_preference_tangled_roundtrip() {
        // Ensure the Tangled variant also survives a round-trip.
        let cfg = Config {
            github_username: "cyrusae".to_string(),
            tangled_username: "atdot.fyi".to_string(),
            origin_preference: OriginPreference::Tangled,
            verbosity_preference: Default::default(),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let restored: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, restored);
        assert_eq!(restored.origin_preference, OriginPreference::Tangled);
    }

    // ── Load error cases ─────────────────────────────────────────────────────

    #[test]
    fn load_not_found_for_nonexistent_path() {
        let path = PathBuf::from("/tmp/entangle-test-definitely-does-not-exist-abc123/config.json");
        let err = Config::load_from_path(&path).unwrap_err();
        assert!(
            matches!(err, ConfigError::NotFound),
            "expected NotFound, got: {err}"
        );
    }

    #[test]
    fn load_empty_for_zero_byte_file() {
        let f = temp_with("");
        let err = Config::load_from_path(f.path()).unwrap_err();
        assert!(
            matches!(err, ConfigError::Empty),
            "expected Empty, got: {err}"
        );
    }

    #[test]
    fn load_empty_for_whitespace_only_file() {
        let f = temp_with("   \n\t  ");
        let err = Config::load_from_path(f.path()).unwrap_err();
        assert!(
            matches!(err, ConfigError::Empty),
            "expected Empty, got: {err}"
        );
    }

    #[test]
    fn load_empty_for_empty_json_object() {
        // A valid-JSON but logically-empty config — no fields set at all.
        let f = temp_with("{}");
        let err = Config::load_from_path(f.path()).unwrap_err();
        assert!(
            matches!(err, ConfigError::Empty),
            "expected Empty, got: {err}"
        );
    }

    #[test]
    fn load_corrupted_for_truncated_json() {
        let f = temp_with(r#"{"github_username": "cyrusae""#); // missing closing brace
        let err = Config::load_from_path(f.path()).unwrap_err();
        assert!(
            matches!(err, ConfigError::Corrupted(_)),
            "expected Corrupted, got: {err}"
        );
    }

    #[test]
    fn load_corrupted_for_invalid_utf8_like_content() {
        // Use a non-JSON string that will fail serde_json parsing.
        let f = temp_with("not json at all !!!!");
        let err = Config::load_from_path(f.path()).unwrap_err();
        assert!(
            matches!(err, ConfigError::Corrupted(_)),
            "expected Corrupted, got: {err}"
        );
    }

    #[test]
    fn load_corrupted_for_invalid_origin_preference_value() {
        // All fields are present but origin_preference has an unknown variant.
        let f = temp_with(
            r#"{"github_username":"cyrusae","tangled_username":"atdot.fyi","origin_preference":"gitlab"}"#,
        );
        let err = Config::load_from_path(f.path()).unwrap_err();
        assert!(
            matches!(err, ConfigError::Corrupted(_)),
            "expected Corrupted for unknown origin variant, got: {err}"
        );
    }

    #[test]
    fn load_missing_github_username() {
        let f = temp_with(r#"{"tangled_username":"atdot.fyi","origin_preference":"github"}"#);
        let err = Config::load_from_path(f.path()).unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingGithubUsername),
            "expected MissingGithubUsername, got: {err}"
        );
    }

    #[test]
    fn load_missing_tangled_username() {
        let f = temp_with(r#"{"github_username":"cyrusae","origin_preference":"github"}"#);
        let err = Config::load_from_path(f.path()).unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingTangledUsername),
            "expected MissingTangledUsername, got: {err}"
        );
    }

    #[test]
    fn load_missing_origin_preference() {
        let f = temp_with(r#"{"github_username":"cyrusae","tangled_username":"atdot.fyi"}"#);
        let err = Config::load_from_path(f.path()).unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingOriginPreference),
            "expected MissingOriginPreference, got: {err}"
        );
    }

    #[test]
    fn load_success_for_valid_json() {
        let f = temp_with(valid_json());
        let cfg = Config::load_from_path(f.path()).unwrap();
        assert_eq!(cfg, valid_config());
    }

    /// A config file with additional unknown fields must still load correctly.
    ///
    /// serde's default behaviour is to ignore unknown fields during
    /// deserialization. This test pins that behaviour so future struct changes
    /// (e.g. adding a new field with `#[serde(default)]`) don't accidentally
    /// break users who haven't regenerated their config file yet.
    #[test]
    fn load_ignores_unknown_fields_in_json() {
        // Valid config plus an extra field that doesn't exist in the struct.
        let json = r#"{
            "github_username": "cyrusae",
            "tangled_username": "atdot.fyi",
            "origin_preference": "github",
            "unknown_future_field": 42,
            "another_unknown": "hello"
        }"#;
        let f = temp_with(json);
        let cfg = Config::load_from_path(f.path()).unwrap();
        // Known fields must be loaded; unknown ones silently ignored.
        assert_eq!(cfg, valid_config());
    }

    // ── Save ─────────────────────────────────────────────────────────────────

    #[test]
    fn save_creates_parent_directory_if_missing() {
        // Create a temp dir, then point save() at a nested path that doesn't exist yet.
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("nested").join("dir").join("config.json");

        // The nested directories don't exist — save() must create them.
        valid_config().save_to_path(&path).unwrap();
        assert!(path.exists(), "config.json should have been created");
    }

    #[test]
    fn save_writes_parseable_json() {
        let f = NamedTempFile::new().unwrap();
        valid_config().save_to_path(f.path()).unwrap();

        let content = std::fs::read_to_string(f.path()).unwrap();
        // Must at least be valid JSON.
        let _: serde_json::Value = serde_json::from_str(&content).unwrap();
    }

    // ── Integration: round-trip through the filesystem ───────────────────────

    #[test]
    fn save_then_load_roundtrip() {
        let f = NamedTempFile::new().unwrap();
        let original = valid_config();

        original.save_to_path(f.path()).unwrap();
        let restored = Config::load_from_path(f.path()).unwrap();

        assert_eq!(original, restored);
    }

    #[test]
    fn save_then_load_tangled_origin_roundtrip() {
        let f = NamedTempFile::new().unwrap();
        let original = Config {
            github_username: "cyrusae".to_string(),
            tangled_username: "atdot.fyi".to_string(),
            origin_preference: OriginPreference::Tangled,
            verbosity_preference: Default::default(),
        };

        original.save_to_path(f.path()).unwrap();
        let restored = Config::load_from_path(f.path()).unwrap();

        assert_eq!(original, restored);
        assert_eq!(restored.origin_preference, OriginPreference::Tangled);
    }

    // ── VerbosityLevel ───────────────────────────────────────────────────────

    #[test]
    fn verbosity_default_is_verbose() {
        assert_eq!(VerbosityLevel::default(), VerbosityLevel::Verbose);
    }

    #[test]
    fn verbosity_ordering_quiet_lt_verbose_lt_debug() {
        assert!(VerbosityLevel::Quiet < VerbosityLevel::Verbose);
        assert!(VerbosityLevel::Verbose < VerbosityLevel::Debug);
    }

    #[test]
    fn verbosity_serializes_to_lowercase_string() {
        assert_eq!(
            serde_json::to_string(&VerbosityLevel::Quiet).unwrap(),
            r#""quiet""#
        );
        assert_eq!(
            serde_json::to_string(&VerbosityLevel::Verbose).unwrap(),
            r#""verbose""#
        );
        assert_eq!(
            serde_json::to_string(&VerbosityLevel::Debug).unwrap(),
            r#""debug""#
        );
    }

    #[test]
    fn verbosity_deserializes_from_lowercase_string() {
        let q: VerbosityLevel = serde_json::from_str(r#""quiet""#).unwrap();
        let v: VerbosityLevel = serde_json::from_str(r#""verbose""#).unwrap();
        let d: VerbosityLevel = serde_json::from_str(r#""debug""#).unwrap();
        assert_eq!(q, VerbosityLevel::Quiet);
        assert_eq!(v, VerbosityLevel::Verbose);
        assert_eq!(d, VerbosityLevel::Debug);
    }

    #[test]
    fn verbosity_default_omitted_from_serialized_config() {
        // When verbosity_preference is the default (Verbose), it must not
        // appear in the serialized JSON so existing configs stay clean.
        let cfg = valid_config(); // verbosity_preference = Verbose (default)
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(
            !json.contains("verbosity_preference"),
            "default verbosity must not appear in JSON: {json}"
        );
    }

    #[test]
    fn verbosity_non_default_persisted_in_config() {
        // Non-default levels must be written so the preference is actually saved.
        let cfg = Config {
            github_username: "cyrusae".to_string(),
            tangled_username: "atdot.fyi".to_string(),
            origin_preference: OriginPreference::Github,
            verbosity_preference: VerbosityLevel::Quiet,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(
            json.contains("verbosity_preference"),
            "non-default verbosity must appear in JSON: {json}"
        );
        assert!(json.contains("quiet"), "must serialize to 'quiet': {json}");
    }

    #[test]
    fn verbosity_missing_from_json_loads_as_default() {
        // Existing config files without verbosity_preference must load fine
        // and resolve to Verbose.
        let f = temp_with(
            r#"{"github_username":"cyrusae","tangled_username":"atdot.fyi","origin_preference":"github"}"#,
        );
        let cfg = Config::load_from_path(f.path()).unwrap();
        assert_eq!(
            cfg.verbosity_preference,
            VerbosityLevel::Verbose,
            "absent verbosity_preference must default to Verbose"
        );
    }

    #[test]
    fn effective_verbosity_quiet_flag_overrides_config() {
        let cfg = Config {
            github_username: "cyrusae".to_string(),
            tangled_username: "atdot.fyi".to_string(),
            origin_preference: OriginPreference::Github,
            verbosity_preference: VerbosityLevel::Debug, // config says debug…
        };
        // …but CLI -q overrides it.
        assert_eq!(cfg.effective_verbosity(true, false), VerbosityLevel::Quiet);
    }

    #[test]
    fn effective_verbosity_debug_flag_overrides_config() {
        let cfg = valid_config(); // config says verbose
        assert_eq!(cfg.effective_verbosity(false, true), VerbosityLevel::Debug);
    }

    #[test]
    fn effective_verbosity_no_flags_uses_config_preference() {
        let cfg = Config {
            github_username: "cyrusae".to_string(),
            tangled_username: "atdot.fyi".to_string(),
            origin_preference: OriginPreference::Github,
            verbosity_preference: VerbosityLevel::Quiet,
        };
        assert_eq!(cfg.effective_verbosity(false, false), VerbosityLevel::Quiet);
    }
}
