//! Handler for `entangle init`.
//!
//! Full flow across Steps 8–10:
//!
//!  1. Check config is valid; refer to `entangle setup` if not.        ← Step 8
//!  2. Collect repo name and optional alias (CLI args or prompts).      ← Step 8
//!  3. Validate repo name(s).                                           ← Step 8
//!  4. Build prospective GitHub and Tangled SSH URLs.                   ← Step 8
//!  5. Detect git repo status; `git init` if needed.                   ← Step 8
//!  6. Suggest `.gitignore` / `README.md` if absent.                   ← Step 8
//!  7. Inspect existing remotes; handle overwrite/proceed prompts.      ← Step 9
//!  8. Add non-origin push URL, then origin push URL (order matters).  ← Step 10
//!  9. Print final remote state and suggest `entangle shove`.           ← Step 10
//!
//! ## Testability
//!
//! [`run`] resolves the platform config path and current working directory, then
//! delegates to [`run_with_paths`], which accepts explicit paths so tests can
//! pass a `tempfile` config path and a temp work directory without touching the
//! user's real config or cwd.
//!
//! ## Interactive vs. CLI-arg mode
//!
//! - `entangle init`                 → prompts for repo name and optional alias
//! - `entangle init myrepo`          → uses `myrepo`, no alias (no prompt)
//! - `entangle init myrepo myalias`  → uses `myrepo` + `myalias` (no prompt)
//!
//! Interactive prompts use `dialoguer` and require a TTY. When a repo name is
//! supplied as a CLI arg, no dialoguer is invoked, making that path fully
//! testable with piped stdin.

use std::path::Path;

use dialoguer::{theme::ColorfulTheme, Input};

use crate::config::{config_path, Config, ConfigError, VerbosityLevel};
use crate::git;
use crate::remote;
use crate::urls::resolve_urls;
use crate::validate::validate_repo_name;

// ---------------------------------------------------------------------------
// Verbosity helper
// ---------------------------------------------------------------------------

/// Print a formatted message if `$verbosity` is at or above `$level`.
///
/// Usage mirrors `println!` — format string and arguments are forwarded as-is.
/// The level is named without the `VerbosityLevel::` prefix for brevity:
///
/// ```ignore
/// vlog!(verbosity, Verbose, "✓ {}", message);
/// vlog!(verbosity, Debug,   "  push_urls: {:?}", urls);
/// ```
macro_rules! vlog {
    ($verbosity:expr, $level:ident, $($arg:tt)*) => {
        if $verbosity >= VerbosityLevel::$level {
            println!($($arg)*);
        }
    };
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Entry point called by `main.rs` for the `init` subcommand.
///
/// `quiet` and `debug` map directly to the `-q` / `--debug` CLI flags and
/// override the `verbosity_preference` stored in the config file.
///
/// Remote validation can be bypassed for integration tests by setting the
/// `ENTANGLE_SKIP_REMOTE_CHECK` environment variable to any value. This avoids
/// real SSH connections in tests that spawn the binary as a subprocess.
pub fn run(
    repo: Option<String>,
    alias: Option<String>,
    quiet: bool,
    debug: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path()?;
    let work_dir = std::env::current_dir()?;
    let skip_check = std::env::var("ENTANGLE_SKIP_REMOTE_CHECK").is_ok();
    run_with_paths(repo, alias, &path, &work_dir, quiet, debug, |origin, mirror| {
        if skip_check {
            return Ok(());
        }
        remote::validate_remotes(origin, mirror)
            .map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })
    })
}

// ---------------------------------------------------------------------------
// Testable core
// ---------------------------------------------------------------------------

/// Run init against an explicit config path and working directory.
///
/// Separated from [`run`] so tests can supply a [`tempfile`] config path and
/// temp work directory instead of touching the user's real config or cwd.
///
/// `quiet` and `debug` correspond to the CLI flags; they override the
/// `verbosity_preference` field in the loaded config. Pass both as `false`
/// to use the config-file preference (which defaults to [`VerbosityLevel::Verbose`]).
///
/// `remote_validator` is called with `(origin_url, mirror_url)` to verify
/// that both remotes are reachable before touching `.git/config`. In
/// production this is [`remote::validate_remotes`] (which does real SSH
/// ls-refs). In unit tests pass `|_, _| Ok(())` to skip network I/O.
pub fn run_with_paths(
    repo: Option<String>,
    alias: Option<String>,
    config_path: &Path,
    work_dir: &Path,
    quiet: bool,
    debug: bool,
    remote_validator: impl Fn(&str, &str) -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    // ── 1. Load config ───────────────────────────────────────────────────────
    let config = match Config::load_from_path(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", config_error_message(&e));
            return Err(e.into());
        }
    };

    let verbosity = config.effective_verbosity(quiet, debug);

    // ── 2 & 3. Collect and validate repo name ───────────────────────────────
    //
    // Interactive mode (no CLI arg): prompt loops until the user provides a
    // valid name — prompt_repo_name() validates internally and only returns Ok
    // on success. We also offer an alias prompt in this mode.
    //
    // CLI mode (arg provided): validate immediately and error without prompting.
    // The alias, if not given as a second CLI arg, is simply absent — no prompt.
    let (repo_name, interactive) = match repo {
        Some(r) => {
            let v = validate_repo_name(&r).map_err(|e| {
                eprintln!("Error: {e}");
                e
            })?;
            (v, false)
        }
        None => (prompt_repo_name()?, true),
    };

    let alias_name: Option<String> = match (alias, interactive) {
        // Alias supplied via CLI — validate it.
        (Some(a), _) => {
            let v = validate_repo_name(&a).map_err(|e| {
                eprintln!("Error: {e}");
                e
            })?;
            Some(v)
        }
        // Interactive mode, no CLI alias → prompt (blank = no alias).
        (None, true) => prompt_alias_optional()?,
        // CLI mode, no alias arg → no alias, no prompt.
        (None, false) => None,
    };

    // ── 4. Build prospective URLs ────────────────────────────────────────────
    let (origin_url, mirror_url) =
        resolve_urls(&config, &repo_name, alias_name.as_deref());

    // ── 5. Detect / initialize git repo ─────────────────────────────────────
    let was_initialized = git::init_if_needed(work_dir)?;
    if was_initialized {
        vlog!(verbosity, Verbose, "Folder is not a git repository — initializing...");
        vlog!(verbosity, Verbose, "✓ Git repository initialized.");
    } else {
        vlog!(verbosity, Verbose, "✓ Git repository detected.");
    }

    // ── 6. Suggest .gitignore / README.md if absent ──────────────────────────
    if !git::has_gitignore(work_dir) {
        vlog!(verbosity, Verbose, "  Tip: Add a .gitignore to avoid committing build artifacts.");
    }
    if !git::has_readme(work_dir) {
        vlog!(verbosity, Verbose, "  Tip: Add a README.md to describe your project.");
    }

    // ── 6b. Preview resolved URLs ─────────────────────────────────────────────
    vlog!(verbosity, Verbose, "");
    vlog!(verbosity, Verbose, "Configuring remotes for '{repo_name}':");
    vlog!(verbosity, Verbose, "  Origin (fetch + push): {origin_url}");
    vlog!(verbosity, Verbose, "  Mirror (push only):    {mirror_url}");

    // ── 7. Inspect existing remotes ──────────────────────────────────────────
    use crate::git::OriginStatus;

    let origin_status = git::get_origin_status(work_dir)?;

    vlog!(verbosity, Debug, "  [debug] origin status: {:?}", origin_status);

    // ── 7a. Early exit if already fully configured ────────────────────────────
    //
    // Both push URLs present → a previous `entangle init` (or manual setup)
    // already wired the dual-push configuration. Exit cleanly before hitting
    // the network — no point validating if nothing needs to change.
    if let OriginStatus::Present { push_urls, .. } = &origin_status {
        let has_origin_push = push_urls.iter().any(|u| u == &origin_url);
        let has_mirror_push = push_urls.iter().any(|u| u == &mirror_url);

        vlog!(verbosity, Debug,
            "  [debug] push_urls={push_urls:?} has_origin_push={has_origin_push} has_mirror_push={has_mirror_push}"
        );

        if has_origin_push && has_mirror_push {
            vlog!(verbosity, Verbose, "");
            vlog!(verbosity, Verbose, "✓ Both push remotes are already configured. Nothing to do.");
            vlog!(verbosity, Verbose, "  Run `entangle shove` to push all branches and tags to both forges.");
            return Ok(());
        }
    }

    // ── 7b. Validate remote accessibility ─────────────────────────────────────
    //
    // Verify both SSH endpoints are reachable (or the user accepts the offline
    // override) before touching `.git/config`. Placed after URL preview so the
    // user sees the intended targets before we go to the network, and after the
    // early-exit check so we don't hit the network for already-configured repos.
    //
    // Three outcomes from the validator:
    //   • Ok(())           → both reachable (or user accepted offline override).
    //   • NotFound/Auth    → hard stop; user must fix the URL or SSH key first.
    //   • NetworkError declined → OfflineAborted; user cancelled at the prompt.
    vlog!(verbosity, Verbose, "");
    vlog!(verbosity, Verbose, "Checking remote accessibility…");
    if let Err(e) = remote_validator(&origin_url, &mirror_url) {
        eprintln!("Error: {e}");
        return Err(e);
    }
    vlog!(verbosity, Verbose, "✓ Both remotes are accessible.");

    // ── 7c. Handle overwrite prompt if fetch URL doesn't match ───────────────
    //
    // Two variables capture the decisions made here so Step 10 and the
    // post-action note can use them:
    //
    //   `replace_fetch_url`   — true if the user chose to replace the existing
    //                           origin fetch URL. Step 10 swaps it before
    //                           adding push URLs.
    //
    //   `kept_existing_fetch` — Some(url) if the user chose to proceed without
    //                           replacing. Step 10 skips touching the fetch URL;
    //                           a ⚠ note is shown after Step 10 completes.
    let mut replace_fetch_url = false;
    let mut kept_existing_fetch: Option<String> = None;

    match &origin_status {
        OriginStatus::Absent => {
            // No existing `origin` remote — Step 10 will create one from scratch.
            vlog!(verbosity, Debug, "  [debug] no origin remote found; will create from scratch");
        }

        OriginStatus::Present { fetch_url, push_urls: _ } => {
            if fetch_url != &origin_url {
                // Fetch URL mismatch (e.g., GitLab remote, a fork, different user).
                // Three outcomes: replace, proceed-as-is, or abort.
                vlog!(verbosity, Debug,
                    "  [debug] fetch URL mismatch: existing={fetch_url} expected={origin_url}"
                );

                let replace = prompt_replace_origin(fetch_url, &origin_url)?;

                if replace {
                    replace_fetch_url = true;
                } else {
                    let proceed = prompt_proceed_anyway(fetch_url)?;
                    if !proceed {
                        // Always printed — the user's confirmation that the abort
                        // happened, not a suppressible informational tip.
                        println!("Init cancelled. No changes were made.");
                        return Ok(());
                    }
                    kept_existing_fetch = Some(fetch_url.clone());
                }
            }
            // If fetch_url == origin_url, Step 10 adds missing push URLs silently.
        }
    }

    vlog!(verbosity, Debug,
        "  [debug] replace_fetch_url={replace_fetch_url} kept_existing_fetch={kept_existing_fetch:?}"
    );

    // ── Step 10: Configure remotes ───────────────────────────────────────────
    //
    // Three paths depending on what Step 9 found and decided:
    //
    //   Absent              → create origin from scratch with both push URLs.
    //   Present, replaced   → replace fetch URL, then add both push URLs.
    //   Present, kept/match → leave fetch URL alone, add whichever push URLs
    //                         are missing (the caller already checked that at
    //                         least one is absent — the "both present" path
    //                         returned early in Step 9).
    match &origin_status {
        OriginStatus::Absent => {
            // Non-default (mirror) forge first, default (origin) forge last.
            // This matches the convention in the Tangled docs and in DESIGN.md
            // steps 9–10: the origin URL is "re-added" as a push URL after the
            // mirror, so it appears last in the config.
            git::create_origin_remote(
                work_dir,
                &origin_url,
                &[mirror_url.as_str(), origin_url.as_str()],
            )?;
            vlog!(verbosity, Debug, "  [debug] created origin remote with fetch + 2 push URLs");
        }

        OriginStatus::Present { fetch_url: _, push_urls } => {
            if replace_fetch_url {
                git::set_origin_fetch_url(work_dir, &origin_url)?;
                vlog!(verbosity, Debug, "  [debug] replaced origin fetch URL → {origin_url}");
            }

            // Add whichever push URLs are not yet present, preserving the
            // non-default-first, default-last ordering convention.
            let mut to_add: Vec<&str> = Vec::new();
            if !push_urls.iter().any(|u| u == &mirror_url) {
                to_add.push(mirror_url.as_str());
            }
            if !push_urls.iter().any(|u| u == &origin_url) {
                to_add.push(origin_url.as_str());
            }
            vlog!(verbosity, Debug, "  [debug] push URLs to add: {:?}", to_add);
            if !to_add.is_empty() {
                git::add_push_urls_to_origin(work_dir, &to_add)?;
            }
        }
    }

    // ── Print final remote state ─────────────────────────────────────────────
    //
    // Show a `git remote -v`-style summary of what origin looks like after the
    // changes. The fetch URL is `origin_url` unless the user chose to keep an
    // existing URL (`kept_existing_fetch`).
    let final_fetch_url = match &kept_existing_fetch {
        Some(url) => url.as_str(),
        None => origin_url.as_str(),
    };

    vlog!(verbosity, Verbose, "");
    // Display mirrors the actual config order: mirror (non-default) first,
    // origin (default) last — matching `git remote -v` output conventions.
    vlog!(verbosity, Verbose, "✓ Remotes configured for '{repo_name}':");
    vlog!(verbosity, Verbose, "");
    vlog!(verbosity, Verbose, "  origin  {final_fetch_url}  (fetch)");
    vlog!(verbosity, Verbose, "  origin  {mirror_url}  (push)");
    vlog!(verbosity, Verbose, "  origin  {origin_url}  (push)");
    vlog!(verbosity, Verbose, "");
    vlog!(verbosity, Verbose, "Run `entangle shove` to push all branches and tags to both forges.");

    // ── Post-action note for the "kept existing fetch URL" path ──────────────
    //
    // Placed here — after Step 10 — so "was kept" and "have been added" are
    // factually accurate at the point the user reads them.
    if let Some(ref existing_url) = kept_existing_fetch {
        vlog!(verbosity, Verbose, "");
        vlog!(verbosity, Verbose,
            "⚠  Note: origin fetch URL ({existing_url}) was kept as-is."
        );
        vlog!(verbosity, Verbose,
            "   Push URLs have been added — pushes will reach both forges,"
        );
        vlog!(verbosity, Verbose, "   but fetches will come from this origin.");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Prompt helpers (interactive mode)
// ---------------------------------------------------------------------------

/// Ask the user whether to replace an existing origin with a different URL.
///
/// Prints the existing URL on its own line before the `Confirm` so the long
/// URLs don't crowd the prompt itself. Default is `true` (replace).
fn prompt_replace_origin(
    existing_url: &str,
    new_url: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    println!();
    println!("An origin remote already exists: {existing_url}");
    let theme = ColorfulTheme::default();
    match dialoguer::Confirm::with_theme(&theme)
        .with_prompt(format!("Replace it with {new_url}?"))
        .default(true)
        .interact()
    {
        Ok(v) => Ok(v),
        Err(e) if is_cancelled(&e) => {
            eprintln!("\nInit cancelled. No changes were made.");
            Err(e.into())
        }
        Err(e) => Err(e.into()),
    }
}

/// Ask the user whether to add push URLs to an origin whose fetch URL we are
/// NOT replacing. Default is `true` (proceed).
fn prompt_proceed_anyway(existing_url: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let theme = ColorfulTheme::default();
    match dialoguer::Confirm::with_theme(&theme)
        .with_prompt(format!(
            "Add push URLs to existing origin ({existing_url}) anyway?"
        ))
        .default(true)
        .interact()
    {
        Ok(v) => Ok(v),
        Err(e) if is_cancelled(&e) => {
            eprintln!("\nInit cancelled. No changes were made.");
            Err(e.into())
        }
        Err(e) => Err(e.into()),
    }
}

/// Prompt for the repository name, re-prompting on validation failure.
fn prompt_repo_name() -> Result<String, Box<dyn std::error::Error>> {
    let theme = ColorfulTheme::default();
    loop {
        let raw = match Input::<String>::with_theme(&theme)
            .with_prompt("Repository name (on GitHub)")
            .interact_text()
        {
            Ok(v) => v,
            Err(e) if is_cancelled(&e) => {
                eprintln!("\nInit cancelled. No changes were made.");
                return Err(e.into());
            }
            Err(e) => return Err(e.into()),
        };

        match validate_repo_name(&raw) {
            Ok(validated) => return Ok(validated),
            Err(e) => eprintln!("  ✗ {e}"),
        }
    }
}

/// Prompt for an optional Tangled alias. Empty input → `None`.
fn prompt_alias_optional() -> Result<Option<String>, Box<dyn std::error::Error>> {
    let theme = ColorfulTheme::default();
    loop {
        let raw = match Input::<String>::with_theme(&theme)
            .with_prompt("Alias on Tangled (leave blank to use the same name)")
            .allow_empty(true)
            .interact_text()
        {
            Ok(v) => v,
            Err(e) if is_cancelled(&e) => {
                eprintln!("\nInit cancelled. No changes were made.");
                return Err(e.into());
            }
            Err(e) => return Err(e.into()),
        };

        if raw.trim().is_empty() {
            return Ok(None);
        }

        match validate_repo_name(&raw) {
            Ok(validated) => return Ok(Some(validated)),
            Err(e) => eprintln!("  ✗ {e}"),
        }
    }
}

/// Returns `true` if a dialoguer error looks like a user cancellation (Ctrl+C or
/// broken pipe) rather than an unexpected infrastructure failure.
fn is_cancelled(e: &dialoguer::Error) -> bool {
    match e {
        dialoguer::Error::IO(io_err) => matches!(
            io_err.kind(),
            std::io::ErrorKind::Interrupted | std::io::ErrorKind::BrokenPipe
        ),
    }
}

// ---------------------------------------------------------------------------
// Config error messaging
// ---------------------------------------------------------------------------

/// Produce a human-readable, actionable message for each `ConfigError` variant.
///
/// Each message tells the user not just what went wrong but what to run next.
fn config_error_message(e: &ConfigError) -> String {
    match e {
        ConfigError::NoPlatformConfigDir => {
            "Could not determine the platform config directory. \
             Set ENTANGLE_CONFIG_PATH to an explicit path."
                .to_string()
        }
        ConfigError::NotFound => {
            "No configuration found. Run `entangle setup` to get started.".to_string()
        }
        ConfigError::Unreadable(_) => {
            "Could not read the configuration file (permission error?). \
             Check file permissions or re-run `entangle setup`."
                .to_string()
        }
        ConfigError::Empty => {
            "Configuration file is empty. Run `entangle setup`.".to_string()
        }
        ConfigError::MissingGithubUsername => {
            "GitHub username not set. Run `entangle set gh-user <username>`.".to_string()
        }
        ConfigError::MissingTangledUsername => {
            "Tangled username not set. Run `entangle set tngl-user <handle>`.".to_string()
        }
        ConfigError::MissingOriginPreference => {
            "Origin preference not set. Run `entangle setup` or \
             `entangle set origin <github|tangled>`."
                .to_string()
        }
        ConfigError::Corrupted(_) => {
            "Configuration file is corrupted. Re-run `entangle setup` to recreate it.".to_string()
        }
        ConfigError::CannotCreateDir(_) | ConfigError::CannotWriteFile(_) => {
            format!("Configuration I/O error: {e}. Check file permissions.")
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
    use tempfile::TempDir;

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// No-op remote validator for unit tests — skips all network I/O.
    ///
    /// Pass this wherever `run_with_paths` requires a `remote_validator`.
    /// Integration tests that spawn the binary use `ENTANGLE_SKIP_REMOTE_CHECK`
    /// instead; this function is only for in-process unit tests.
    fn skip_validate(_: &str, _: &str) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn write_valid_config(path: &Path) {
        let cfg = Config {
            github_username: "cyrusae".to_string(),
            tangled_username: "atdot.fyi".to_string(),
            origin_preference: OriginPreference::Github,
            verbosity_preference: Default::default(),
        };
        cfg.save_to_path(path).unwrap();
    }

    fn fresh_dirs() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        let work_dir = dir.path().join("work");
        std::fs::create_dir(&work_dir).unwrap();
        write_valid_config(&config_path);
        (dir, config_path, work_dir)
    }

    // ── Config error paths ────────────────────────────────────────────────────

    #[test]
    fn missing_config_returns_error() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("nonexistent.json");
        let work_dir = dir.path().join("work");
        std::fs::create_dir(&work_dir).unwrap();

        let result = run_with_paths(
            Some("myrepo".to_string()),
            None,
            &config_path,
            &work_dir,
            false,
            false,
            skip_validate,
        );
        assert!(result.is_err(), "must error when config is missing");
    }

    // ── Git init behaviour ────────────────────────────────────────────────────

    #[test]
    fn run_initializes_git_repo_in_fresh_directory() {
        let (_dir, config_path, work_dir) = fresh_dirs();
        assert!(!git::is_git_repo(&work_dir), "precondition: not yet a git repo");

        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate).unwrap();

        assert!(
            git::is_git_repo(&work_dir),
            "work_dir must be a git repo after run_with_paths"
        );
    }

    #[test]
    fn run_is_idempotent_on_existing_repo() {
        let (_dir, config_path, work_dir) = fresh_dirs();

        // First run — initializes.
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate).unwrap();
        // Second run — must not error.
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate).unwrap();

        assert!(git::is_git_repo(&work_dir));
    }

    // ── Validation ────────────────────────────────────────────────────────────

    #[test]
    fn invalid_repo_name_returns_error() {
        let (_dir, config_path, work_dir) = fresh_dirs();

        let result = run_with_paths(
            Some("-invalid-leading-hyphen".to_string()),
            None,
            &config_path,
            &work_dir,
            false,
            false,
            skip_validate,
        );
        assert!(result.is_err(), "invalid repo name must cause an error");
        // No git repo should have been created.
        assert!(
            !git::is_git_repo(&work_dir),
            "git repo must not be created when repo name is invalid"
        );
    }

    #[test]
    fn invalid_alias_returns_error() {
        let (_dir, config_path, work_dir) = fresh_dirs();

        let result = run_with_paths(
            Some("my-repo".to_string()),
            Some("-bad-alias".to_string()),
            &config_path,
            &work_dir,
            false,
            false,
            skip_validate,
        );
        assert!(result.is_err(), "invalid alias must cause an error");
    }

    #[test]
    fn valid_alias_is_accepted() {
        let (_dir, config_path, work_dir) = fresh_dirs();

        run_with_paths(
            Some("my-repo".to_string()),
            Some("mirror-name".to_string()),
            &config_path,
            &work_dir,
            false,
            false,
            skip_validate,
        )
        .unwrap();
    }

    // ── config_error_message ──────────────────────────────────────────────────

    #[test]
    fn error_message_for_not_found_mentions_setup() {
        let msg = config_error_message(&ConfigError::NotFound);
        assert!(msg.contains("setup"), "NotFound message must mention setup: {msg}");
    }

    #[test]
    fn error_message_for_missing_github_username_mentions_set() {
        let msg = config_error_message(&ConfigError::MissingGithubUsername);
        assert!(
            msg.contains("gh-user"),
            "MissingGithubUsername must mention 'gh-user': {msg}"
        );
    }

    #[test]
    fn error_message_for_missing_tangled_username_mentions_set() {
        let msg = config_error_message(&ConfigError::MissingTangledUsername);
        assert!(
            msg.contains("tngl-user"),
            "MissingTangledUsername must mention 'tngl-user': {msg}"
        );
    }

    #[test]
    fn error_message_for_corrupted_mentions_setup() {
        let msg = config_error_message(&ConfigError::Corrupted("bad json".to_string()));
        assert!(msg.contains("setup"), "Corrupted message must mention setup: {msg}");
    }

    // ── Remote inspection paths (non-interactive) ─────────────────────────────
    //
    // Tests that exercise Step 9 logic without triggering dialoguer (which
    // needs a TTY). All cases here are ones where no prompt is shown:
    //
    //   (a) No `origin` remote       → proceeds silently to Step 10 placeholder
    //   (b) Both push URLs present   → early-exit success message
    //   (c) Origin URL matches       → proceeds silently to Step 10 placeholder
    //
    // Cases that trigger a prompt (origin URL mismatch) are tested via
    // PTY integration tests in `tests/init_integration.rs`.

    /// Write a [remote "origin"] section to .git/config; appends to the
    /// existing gix-generated config so its core settings are preserved.
    fn append_origin(work_dir: &Path, fetch_url: &str, push_urls: &[&str]) {
        use std::io::Write as _;
        let cfg = work_dir.join(".git").join("config");
        let mut f = std::fs::OpenOptions::new().append(true).open(cfg).unwrap();
        writeln!(f, "\n[remote \"origin\"]").unwrap();
        writeln!(f, "\turl = {fetch_url}").unwrap();
        writeln!(f, "\tfetch = +refs/heads/*:refs/remotes/origin/*").unwrap();
        for u in push_urls {
            writeln!(f, "\tpushurl = {u}").unwrap();
        }
    }

    #[test]
    fn run_with_no_origin_proceeds_to_url_preview() {
        // Fresh repo, no remotes — must print the URL preview and return Ok.
        let (_dir, config_path, work_dir) = fresh_dirs();
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate)
            .expect("must succeed when no origin remote is configured");
        // The test passes if run_with_paths does not error. Output is checked
        // in integration tests.
    }

    #[test]
    fn run_with_matching_origin_url_proceeds_to_url_preview() {
        // Origin fetch URL already matches what we'd set — no prompt, proceed.
        let (_dir, config_path, work_dir) = fresh_dirs();
        // First run initializes the git repo.
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate).unwrap();
        // Set up an origin with a matching URL (what a github-preference config gives).
        append_origin(
            &work_dir,
            "git@github.com:cyrusae/entangle.git",
            &[],
        );
        // Second run sees matching origin — must not error, no prompt.
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate)
            .expect("must succeed when origin fetch URL matches expected URL");
    }

    #[test]
    fn run_exits_early_when_both_push_urls_already_configured() {
        // Both push URLs present → early exit with success, no changes needed.
        let (_dir, config_path, work_dir) = fresh_dirs();
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate).unwrap();

        // Add origin with BOTH push URLs already set.
        append_origin(
            &work_dir,
            "git@github.com:cyrusae/entangle.git",
            &[
                "git@github.com:cyrusae/entangle.git",
                "git@tangled.org:atdot.fyi/entangle",
            ],
        );

        // Should return Ok (early exit, not an error).
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate)
            .expect("must succeed (early exit) when both push URLs are already configured");
    }

    #[test]
    fn run_proceeds_when_only_one_push_url_present() {
        // Only one push URL present → must proceed (not early-exit) so Step 10
        // can add the missing one.
        let (_dir, config_path, work_dir) = fresh_dirs();
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate).unwrap();

        // Add origin with only the GitHub push URL (Tangled missing).
        append_origin(
            &work_dir,
            "git@github.com:cyrusae/entangle.git",
            &["git@github.com:cyrusae/entangle.git"],
        );

        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate)
            .expect("must succeed when only one push URL is configured");
    }

    // ── Step 10: Verify configured remote state ───────────────────────────────
    //
    // These tests use `git::get_origin_status` to read back the actual `.git/config`
    // state after `run_with_paths` completes, confirming that Step 10 wrote the
    // correct entries.

    #[test]
    fn run_configures_origin_remote_in_fresh_repo() {
        // After the very first run on a fresh directory, origin must have the
        // expected fetch URL and both push URLs in the correct order:
        // mirror (Tangled) first, origin (GitHub) last — matching the Tangled
        // docs convention and DESIGN.md steps 9–10.
        let (_dir, config_path, work_dir) = fresh_dirs();
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate)
            .unwrap();

        let status = git::get_origin_status(&work_dir).unwrap();
        match status {
            git::OriginStatus::Present { fetch_url, push_urls } => {
                assert_eq!(fetch_url, "git@github.com:cyrusae/entangle.git");
                assert_eq!(push_urls.len(), 2, "must configure both push URLs: {push_urls:?}");
                // Order: mirror (Tangled, non-default) first, origin (GitHub, default) last.
                assert_eq!(push_urls[0], "git@tangled.org:atdot.fyi/entangle",
                    "Tangled (mirror) must be the first push URL");
                assert_eq!(push_urls[1], "git@github.com:cyrusae/entangle.git",
                    "GitHub (origin) must be the second (last) push URL");
            }
            git::OriginStatus::Absent => panic!("expected Present after init, got Absent"),
        }
    }

    #[test]
    fn run_adds_both_push_urls_when_origin_has_matching_url_but_none() {
        // Origin fetch URL already matches what entangle would set, but no push
        // URLs are configured. run_with_paths must add both without a prompt,
        // in the correct order (mirror first, origin last).
        let (_dir, config_path, work_dir) = fresh_dirs();
        gix::init(&work_dir).unwrap();

        // Write origin with correct fetch URL but no push URLs.
        {
            use std::io::Write as _;
            let cfg = work_dir.join(".git").join("config");
            let mut f = std::fs::OpenOptions::new().append(true).open(cfg).unwrap();
            writeln!(f, "\n[remote \"origin\"]").unwrap();
            writeln!(f, "\turl = git@github.com:cyrusae/entangle.git").unwrap();
            writeln!(f, "\tfetch = +refs/heads/*:refs/remotes/origin/*").unwrap();
        }

        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate)
            .unwrap();

        let status = git::get_origin_status(&work_dir).unwrap();
        match status {
            git::OriginStatus::Present { fetch_url, push_urls } => {
                assert_eq!(fetch_url, "git@github.com:cyrusae/entangle.git");
                assert_eq!(push_urls.len(), 2, "both push URLs must be added: {push_urls:?}");
                // Order: mirror (Tangled, non-default) first, origin (GitHub, default) last.
                assert_eq!(push_urls[0], "git@tangled.org:atdot.fyi/entangle",
                    "Tangled (mirror) must be the first push URL");
                assert_eq!(push_urls[1], "git@github.com:cyrusae/entangle.git",
                    "GitHub (origin) must be the second (last) push URL");
            }
            git::OriginStatus::Absent => panic!("expected Present"),
        }
    }

    #[test]
    fn run_is_fully_idempotent_after_step_10() {
        // Running twice on the same repo must succeed both times.
        // Second run sees both push URLs → early-exits cleanly.
        let (_dir, config_path, work_dir) = fresh_dirs();
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate)
            .unwrap();
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false, skip_validate)
            .unwrap();

        let status = git::get_origin_status(&work_dir).unwrap();
        match status {
            git::OriginStatus::Present { push_urls, .. } => {
                // Must still have exactly the two push URLs — second run must
                // not have duplicated them.
                let origin_count = push_urls
                    .iter()
                    .filter(|u| u.as_str() == "git@github.com:cyrusae/entangle.git")
                    .count();
                let mirror_count = push_urls
                    .iter()
                    .filter(|u| u.as_str() == "git@tangled.org:atdot.fyi/entangle")
                    .count();
                assert_eq!(origin_count, 1, "origin push URL must not be duplicated");
                assert_eq!(mirror_count, 1, "mirror push URL must not be duplicated");
            }
            git::OriginStatus::Absent => panic!("expected Present"),
        }
    }

    #[test]
    fn run_with_alias_uses_alias_for_tangled_push_url() {
        // When an alias is supplied, the Tangled push URL must use the alias
        // instead of the primary repo name.
        let (_dir, config_path, work_dir) = fresh_dirs();
        run_with_paths(
            Some("my-repo".to_string()),
            Some("mirror-alias".to_string()),
            &config_path,
            &work_dir,
            false,
            false,
            skip_validate,
        )
        .unwrap();

        let status = git::get_origin_status(&work_dir).unwrap();
        match status {
            git::OriginStatus::Present { push_urls, .. } => {
                assert!(
                    push_urls.iter().any(|u| u.contains("mirror-alias")),
                    "Tangled push URL must use the alias: {push_urls:?}"
                );
                assert!(
                    push_urls.iter().any(|u| u.contains("my-repo")),
                    "GitHub push URL must use the primary repo name: {push_urls:?}"
                );
            }
            git::OriginStatus::Absent => panic!("expected Present"),
        }
    }
}
