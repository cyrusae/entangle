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

/// Persisted user configuration.
///
/// Stored as JSON at `{config_dir}/entangle/config.json`.
/// All three fields are required; missing fields produce actionable errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// GitHub username (≤39 chars, alphanumeric + hyphens).
    pub github_username: String,

    /// Tangled username — a valid ATProto handle (e.g. `atdot.fyi`).
    pub tangled_username: String,

    /// Which forge is the primary fetch remote. Defaults to `github`.
    pub origin_preference: OriginPreference,
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
pub fn config_path() -> Result<PathBuf, ConfigError> {
    let base = config_dir().ok_or(ConfigError::NoPlatformConfigDir)?;
    Ok(base.join("entangle").join("config.json"))
}

// ---------------------------------------------------------------------------
// Load / Save  (stubs — implemented in Step 2)
// ---------------------------------------------------------------------------

impl Config {
    /// Load the config from disk.
    ///
    /// Returns a specific [`ConfigError`] variant for each failure mode so that
    /// callers can tell the user exactly what to do next.
    pub fn load() -> Result<Config, ConfigError> {
        // Stub — implemented in Step 2.
        unimplemented!("Config::load — implemented in Step 2")
    }

    /// Write the config to disk, creating `{config_dir}/entangle/` if needed.
    pub fn save(&self) -> Result<(), ConfigError> {
        // Stub — implemented in Step 2.
        unimplemented!("Config::save — implemented in Step 2")
    }
}
