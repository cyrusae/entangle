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
pub fn run(
    repo: Option<String>,
    alias: Option<String>,
    quiet: bool,
    debug: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path()?;
    let work_dir = std::env::current_dir()?;
    run_with_paths(repo, alias, &path, &work_dir, quiet, debug)
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
pub fn run_with_paths(
    repo: Option<String>,
    alias: Option<String>,
    config_path: &Path,
    work_dir: &Path,
    quiet: bool,
    debug: bool,
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

    // ── 7. Inspect existing remotes; handle overwrite prompt ─────────────────
    //
    // We read the current `origin` state AFTER showing the URL preview so the
    // user can see the intended configuration before being asked about conflicts.
    use crate::git::OriginStatus;

    let origin_status = git::get_origin_status(work_dir)?;

    vlog!(verbosity, Debug, "  [debug] origin status: {:?}", origin_status);

    match &origin_status {
        OriginStatus::Absent => {
            // No existing `origin` remote — Step 10 will create one from scratch.
            vlog!(verbosity, Debug, "  [debug] no origin remote found; will create from scratch");
        }

        OriginStatus::Present { fetch_url, push_urls } => {
            // ── 7a. Early exit if already fully configured ────────────────────
            //
            // Both push URLs being present means a previous `entangle init` (or
            // manual setup) has already wired up the dual-push configuration.
            // Nothing left to do — exit cleanly rather than re-adding duplicates.
            let has_origin_push = push_urls.iter().any(|u| u == &origin_url);
            let has_mirror_push = push_urls.iter().any(|u| u == &mirror_url);

            vlog!(verbosity, Debug,
                "  [debug] push_urls={push_urls:?} has_origin_push={has_origin_push} has_mirror_push={has_mirror_push}"
            );

            if has_origin_push && has_mirror_push {
                vlog!(verbosity, Verbose, "");
                vlog!(verbosity, Verbose, "✓ Both push remotes are already configured. Nothing to do.");
                vlog!(verbosity, Verbose, "  Run `entangle shove` to push to both forges.");
                return Ok(());
            }

            // ── 7b. Overwrite prompt if fetch URL doesn't match ───────────────
            //
            // If the existing `origin` fetch URL differs from the one we'd set
            // (e.g., a GitLab URL from a previous project), we must ask before
            // touching it.  Two outcomes:
            //   • Replace  → Step 10 will swap the fetch URL and add push URLs.
            //   • Proceed  → Step 10 will only add push URLs, leaving fetch alone.
            //   • Abort    → exit cleanly, no changes.
            if fetch_url != &origin_url {
                vlog!(verbosity, Debug,
                    "  [debug] fetch URL mismatch: existing={fetch_url} expected={origin_url}"
                );

                let replace = prompt_replace_origin(fetch_url, &origin_url)?;

                if !replace {
                    let proceed = prompt_proceed_anyway(fetch_url)?;
                    if !proceed {
                        // "cancelled" is always printed — it's the user's confirmation
                        // that the abort happened, not just an informational tip.
                        println!("Init cancelled. No changes were made.");
                        return Ok(());
                    }
                    // Proceeding without replacing: warn about the resulting state.
                    // Step 10 will add push URLs but leave the fetch URL as-is.
                    vlog!(verbosity, Verbose, "");
                    vlog!(verbosity, Verbose,
                        "⚠  Note: origin fetch URL ({fetch_url}) will be kept as-is."
                    );
                    vlog!(verbosity, Verbose,
                        "   Push URLs will be added — pushes will reach both forges,"
                    );
                    vlog!(verbosity, Verbose, "   but fetches will come from the existing origin.");
                }
                // If replace == true, Step 10 will replace the origin and add push URLs.
            }
            // If fetch_url == origin_url, Step 10 will add missing push URLs silently.
        }
    }

    // Step 10 will continue from here: add push URLs to origin (replacing the
    // fetch URL first if `replace` was chosen above).

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
        );
        assert!(result.is_err(), "must error when config is missing");
    }

    // ── Git init behaviour ────────────────────────────────────────────────────

    #[test]
    fn run_initializes_git_repo_in_fresh_directory() {
        let (_dir, config_path, work_dir) = fresh_dirs();
        assert!(!git::is_git_repo(&work_dir), "precondition: not yet a git repo");

        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false).unwrap();

        assert!(
            git::is_git_repo(&work_dir),
            "work_dir must be a git repo after run_with_paths"
        );
    }

    #[test]
    fn run_is_idempotent_on_existing_repo() {
        let (_dir, config_path, work_dir) = fresh_dirs();

        // First run — initializes.
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false).unwrap();
        // Second run — must not error.
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false).unwrap();

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
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false)
            .expect("must succeed when no origin remote is configured");
        // The test passes if run_with_paths does not error. Output is checked
        // in integration tests.
    }

    #[test]
    fn run_with_matching_origin_url_proceeds_to_url_preview() {
        // Origin fetch URL already matches what we'd set — no prompt, proceed.
        let (_dir, config_path, work_dir) = fresh_dirs();
        // First run initializes the git repo.
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false).unwrap();
        // Set up an origin with a matching URL (what a github-preference config gives).
        append_origin(
            &work_dir,
            "git@github.com:cyrusae/entangle.git",
            &[],
        );
        // Second run sees matching origin — must not error, no prompt.
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false)
            .expect("must succeed when origin fetch URL matches expected URL");
    }

    #[test]
    fn run_exits_early_when_both_push_urls_already_configured() {
        // Both push URLs present → early exit with success, no changes needed.
        let (_dir, config_path, work_dir) = fresh_dirs();
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false).unwrap();

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
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false)
            .expect("must succeed (early exit) when both push URLs are already configured");
    }

    #[test]
    fn run_proceeds_when_only_one_push_url_present() {
        // Only one push URL present → must proceed (not early-exit) so Step 10
        // can add the missing one.
        let (_dir, config_path, work_dir) = fresh_dirs();
        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false).unwrap();

        // Add origin with only the GitHub push URL (Tangled missing).
        append_origin(
            &work_dir,
            "git@github.com:cyrusae/entangle.git",
            &["git@github.com:cyrusae/entangle.git"],
        );

        run_with_paths(Some("entangle".to_string()), None, &config_path, &work_dir, false, false)
            .expect("must succeed when only one push URL is configured");
    }
}
