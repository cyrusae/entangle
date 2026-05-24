//! Integration tests for `entangle setup`.
//!
//! ## Two test categories
//!
//! **PTY tests** (`#[cfg(unix)]`): spawn the binary in a pseudo-terminal via
//! `rexpect`, wait for each prompt, then send the answer. These tests exercise
//! the full interactive flow because dialoguer requires a real TTY.
//!
//! **Pipe tests** (all platforms): spawn the binary with piped stdin (no TTY).
//! dialoguer immediately errors "not a terminal" so these tests only exercise
//! the *no-partial-write guarantee* — the config must not be written on error.
//!
//! ## Isolation
//!
//! `ENTANGLE_CONFIG_PATH` is set to a per-test temp file so the user's real
//! config directory is never touched.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Shared helpers (all platforms)
// ---------------------------------------------------------------------------

/// Spawn `entangle setup` with piped stdin (no PTY).
///
/// dialoguer will error immediately on the first prompt — these invocations
/// are only useful for testing the *no-write-on-error* guarantee.
fn run_setup_piped(dir: &TempDir, stdin_bytes: &[u8]) -> (std::process::Output, PathBuf) {
    let config_path = dir.path().join("config.json");
    let output = spawn_entangle_piped(&["setup"], &config_path, stdin_bytes);
    (output, config_path)
}

fn spawn_entangle_piped(
    args: &[&str],
    config_path: &Path,
    stdin_bytes: &[u8],
) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_entangle"))
        .args(args)
        .env("ENTANGLE_CONFIG_PATH", config_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn entangle");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin_bytes)
        .expect("failed to write stdin");

    child.wait_with_output().expect("failed to wait for child")
}

/// Read the config file as `serde_json::Value`.
fn read_config_json(path: &Path) -> serde_json::Value {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("could not read config at {}: {e}", path.display()));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("config not valid JSON: {e}\ncontent: {content}"))
}

/// Write a config file as JSON (for tests that need a pre-existing config).
fn write_config_json(path: &Path, github: &str, tangled: &str, origin: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let json = serde_json::json!({
        "github_username": github,
        "tangled_username": tangled,
        "origin_preference": origin,
    });
    std::fs::write(path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
}

fn stderr_str(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}
fn stdout_str(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

// ---------------------------------------------------------------------------
// No-partial-write guarantee (all platforms, piped stdin)
//
// When dialoguer errors "not a terminal", the config must still not be written.
// The two-test suite below covers: (a) no pre-existing file, (b) pre-existing
// file that must not be mutated.
// ---------------------------------------------------------------------------

/// Piped stdin → dialoguer errors → config file must not be created.
#[test]
fn no_pty_does_not_create_config() {
    let dir = TempDir::new().unwrap();
    let (output, config_path) = run_setup_piped(&dir, b"cyrusae\natdot.fyi\ngithub\n");

    assert!(
        !config_path.exists(),
        "config must not be written when dialoguer cannot get a terminal\
         \nstdout: {}\nstderr: {}",
        stdout_str(&output),
        stderr_str(&output)
    );
}

/// Piped stdin → dialoguer errors → pre-existing config must not be mutated.
#[test]
fn no_pty_leaves_existing_config_unchanged() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.json");
    write_config_json(&config_path, "cyrusae", "atdot.fyi", "github");

    let original = std::fs::read_to_string(&config_path).unwrap();

    // Even if we send "new-name", dialoguer errors before reading it.
    spawn_entangle_piped(&["setup"], &config_path, b"new-name\n");

    let after = std::fs::read_to_string(&config_path).unwrap();
    assert_eq!(original, after, "pre-existing config must not be changed");
}

// ---------------------------------------------------------------------------
// Interactive tests via PTY (Unix only)
//
// rexpect spawns the binary in a pseudo-terminal so dialoguer gets a real TTY.
// Each test waits for the prompt text to appear, then sends the answer.
//
// Notes:
//   - Prompt text is matched as substrings; ANSI escape codes from dialoguer's
//     ColorfulTheme appear around but not inside the prompt text strings.
//   - Timeout is 10 seconds per prompt — generous for a local test.
//   - Tests are gated on #[cfg(unix)] because rexpect uses Unix PTY APIs.
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod pty_tests {
    use super::*;
    use rexpect::session::spawn_command;

    const TIMEOUT_MS: Option<u64> = Some(10_000);

    /// Open a PTY session for `entangle setup` with `ENTANGLE_CONFIG_PATH` set.
    fn pty_setup(config_path: &Path) -> rexpect::session::PtySession {
        // Build Command by value — builder methods return &mut Command so we
        // can't chain .into(); we need to pass the owned Command to spawn_command.
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_entangle"));
        cmd.arg("setup");
        cmd.env("ENTANGLE_CONFIG_PATH", config_path);
        spawn_command(cmd, TIMEOUT_MS).expect("failed to spawn PTY session")
    }

    // ── Happy path ────────────────────────────────────────────────────────────

    #[test]
    fn fresh_setup_writes_full_config() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        let mut p = pty_setup(&config_path);

        p.exp_string("GitHub username").unwrap();
        p.send_line("cyrusae").unwrap();

        p.exp_string("Tangled username").unwrap();
        p.send_line("atdot.fyi").unwrap();

        p.exp_string("Origin preference").unwrap();
        p.send_line("github").unwrap();

        p.exp_string("saved").unwrap();
        p.exp_eof().unwrap();

        assert!(config_path.exists(), "config file not created");
        let cfg = read_config_json(&config_path);
        assert_eq!(cfg["github_username"], "cyrusae");
        assert_eq!(cfg["tangled_username"], "atdot.fyi");
        assert_eq!(cfg["origin_preference"], "github");
    }

    #[test]
    fn fresh_setup_with_tangled_origin() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        let mut p = pty_setup(&config_path);

        p.exp_string("GitHub username").unwrap();
        p.send_line("cyrusae").unwrap();
        p.exp_string("Tangled username").unwrap();
        p.send_line("atdot.fyi").unwrap();
        p.exp_string("Origin preference").unwrap();
        p.send_line("tangled").unwrap();
        p.exp_string("saved").unwrap();
        p.exp_eof().unwrap();

        assert_eq!(
            read_config_json(&config_path)["origin_preference"],
            "tangled"
        );
    }

    #[test]
    fn fresh_setup_gh_alias_accepted() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        let mut p = pty_setup(&config_path);

        p.exp_string("GitHub username").unwrap();
        p.send_line("cyrusae").unwrap();
        p.exp_string("Tangled username").unwrap();
        p.send_line("atdot.fyi").unwrap();
        p.exp_string("Origin preference").unwrap();
        p.send_line("gh").unwrap();
        p.exp_string("saved").unwrap();
        p.exp_eof().unwrap();

        assert_eq!(
            read_config_json(&config_path)["origin_preference"],
            "github"
        );
    }

    #[test]
    fn fresh_setup_tngl_alias_accepted() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        let mut p = pty_setup(&config_path);

        p.exp_string("GitHub username").unwrap();
        p.send_line("cyrusae").unwrap();
        p.exp_string("Tangled username").unwrap();
        p.send_line("atdot.fyi").unwrap();
        p.exp_string("Origin preference").unwrap();
        p.send_line("tngl").unwrap();
        p.exp_string("saved").unwrap();
        p.exp_eof().unwrap();

        assert_eq!(
            read_config_json(&config_path)["origin_preference"],
            "tangled"
        );
    }

    /// Quoted + mixed-case username is sanitized before saving.
    #[test]
    fn fresh_setup_sanitises_username() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        let mut p = pty_setup(&config_path);

        p.exp_string("GitHub username").unwrap();
        p.send_line("\"CyrusAE\"").unwrap(); // quoted, mixed case
        p.exp_string("Tangled username").unwrap();
        p.send_line("atdot.fyi").unwrap();
        p.exp_string("Origin preference").unwrap();
        p.send_line("github").unwrap();
        p.exp_string("saved").unwrap();
        p.exp_eof().unwrap();

        assert_eq!(read_config_json(&config_path)["github_username"], "cyrusae");
    }

    // ── Re-prompt on invalid input ────────────────────────────────────────────

    #[test]
    fn invalid_github_username_triggers_reprompt() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        let mut p = pty_setup(&config_path);

        p.exp_string("GitHub username").unwrap();
        p.send_line("-invalid").unwrap(); // leading hyphen — must fail

        // Setup should re-prompt (error message + prompt again).
        p.exp_string("GitHub username").unwrap();
        p.send_line("cyrusae").unwrap();

        p.exp_string("Tangled username").unwrap();
        p.send_line("atdot.fyi").unwrap();
        p.exp_string("Origin preference").unwrap();
        p.send_line("github").unwrap();
        p.exp_string("saved").unwrap();
        p.exp_eof().unwrap();

        assert_eq!(read_config_json(&config_path)["github_username"], "cyrusae");
    }

    #[test]
    fn invalid_tangled_username_triggers_reprompt() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        let mut p = pty_setup(&config_path);

        p.exp_string("GitHub username").unwrap();
        p.send_line("cyrusae").unwrap();
        p.exp_string("Tangled username").unwrap();
        p.send_line("nodot").unwrap(); // no TLD separator — must fail
        p.exp_string("Tangled username").unwrap(); // re-prompt
        p.send_line("atdot.fyi").unwrap();
        p.exp_string("Origin preference").unwrap();
        p.send_line("github").unwrap();
        p.exp_string("saved").unwrap();
        p.exp_eof().unwrap();

        assert_eq!(
            read_config_json(&config_path)["tangled_username"],
            "atdot.fyi"
        );
    }

    #[test]
    fn invalid_origin_triggers_reprompt() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        let mut p = pty_setup(&config_path);

        p.exp_string("GitHub username").unwrap();
        p.send_line("cyrusae").unwrap();
        p.exp_string("Tangled username").unwrap();
        p.send_line("atdot.fyi").unwrap();
        p.exp_string("Origin preference").unwrap();
        p.send_line("gitlab").unwrap(); // unrecognised — must fail
        p.exp_string("Origin preference").unwrap(); // re-prompt
        p.send_line("github").unwrap();
        p.exp_string("saved").unwrap();
        p.exp_eof().unwrap();

        assert_eq!(
            read_config_json(&config_path)["origin_preference"],
            "github"
        );
    }

    // ── Pre-existing config — "already set" prompts ───────────────────────────

    /// All fields set; user keeps them all (presses Enter / 'y').
    #[test]
    fn setup_with_existing_config_keep_all() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        write_config_json(&config_path, "cyrusae", "atdot.fyi", "github");

        let mut p = pty_setup(&config_path);

        // Three "Keep it?" confirms — press Enter each time (default = yes).
        p.exp_string("Keep it?").unwrap();
        p.send_line("").unwrap();
        p.exp_string("Keep it?").unwrap();
        p.send_line("").unwrap();
        p.exp_string("Keep it?").unwrap();
        p.send_line("").unwrap();
        p.exp_string("saved").unwrap();
        p.exp_eof().unwrap();

        let cfg = read_config_json(&config_path);
        assert_eq!(cfg["github_username"], "cyrusae");
        assert_eq!(cfg["tangled_username"], "atdot.fyi");
        assert_eq!(cfg["origin_preference"], "github");
    }

    /// User answers 'n' to the first "Keep it?" and supplies a new username.
    #[test]
    fn setup_with_existing_config_change_one_field() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        write_config_json(&config_path, "old-name", "atdot.fyi", "github");

        let mut p = pty_setup(&config_path);

        // First field: answer 'n' to change it.
        p.exp_string("Keep it?").unwrap();
        p.send_line("n").unwrap();
        p.exp_string("GitHub username").unwrap();
        p.send_line("cyrusae").unwrap();

        // Second and third fields: keep them.
        p.exp_string("Keep it?").unwrap();
        p.send_line("").unwrap();
        p.exp_string("Keep it?").unwrap();
        p.send_line("").unwrap();

        p.exp_string("saved").unwrap();
        p.exp_eof().unwrap();

        let cfg = read_config_json(&config_path);
        assert_eq!(cfg["github_username"], "cyrusae", "should have changed");
        assert_eq!(cfg["tangled_username"], "atdot.fyi", "should be unchanged");
        assert_eq!(cfg["origin_preference"], "github", "should be unchanged");
    }

    // ── No-partial-write guarantee (PTY version) ──────────────────────────────

    /// Ctrl+C on the first prompt must not write the config.
    #[test]
    fn ctrl_c_on_first_prompt_does_not_write_config() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        let mut p = pty_setup(&config_path);

        p.exp_string("GitHub username").unwrap();
        // Send Ctrl+C (ASCII 0x03).
        p.send_control('c').unwrap();
        // Process exits; wait for EOF.
        let _ = p.exp_eof();

        assert!(
            !config_path.exists(),
            "config must not be written after Ctrl+C on first prompt"
        );
    }

    /// Ctrl+C mid-setup (after the first field is answered) must not mutate
    /// a pre-existing config.
    #[test]
    fn ctrl_c_mid_setup_leaves_existing_config_unchanged() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("config.json");
        write_config_json(&config_path, "cyrusae", "atdot.fyi", "github");

        let original = std::fs::read_to_string(&config_path).unwrap();

        let mut p = pty_setup(&config_path);

        // Answer the first prompt, then Ctrl+C.
        p.exp_string("GitHub username").unwrap();
        p.send_line("cyrusae").unwrap();
        p.exp_string("Tangled username").unwrap();
        p.send_control('c').unwrap();
        let _ = p.exp_eof();

        let after = std::fs::read_to_string(&config_path).unwrap();
        assert_eq!(
            original, after,
            "pre-existing config must be unchanged after Ctrl+C"
        );
    }
}
