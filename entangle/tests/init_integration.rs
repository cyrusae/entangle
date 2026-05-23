//! Integration tests for `entangle init` — Steps 8 and 9.
//!
//! ## What these tests cover
//!
//! **Step 8 — piped-stdin tests (all platforms)**
//! - Fresh directory: `entangle init` creates a `.git` repo and prints the
//!   initialization message.
//! - Idempotency: running `entangle init` twice in the same directory does
//!   not re-initialize; the init message is absent on the second run.
//! - Missing config: the command errors with an actionable message pointing
//!   the user to `entangle setup`.
//! - `.gitignore` / `README.md` suggestions appear when those files are absent
//!   and do not appear when they are present.
//! - Valid alias: `entangle init myrepo myalias` is accepted without error.
//!
//! **Step 9 — PTY tests (`#[cfg(unix)]` via rexpect)**
//! - Replace=yes: user accepts the replace prompt → continues to URL preview.
//! - Replace=no, proceed=yes: user keeps existing origin → warning message shown.
//! - Replace=no, proceed=no: user aborts → "Init cancelled", no URL preview.
//! - Early exit: both push URLs already present → success message, no prompts.
//!
//! ## Why piped stdin for Step 8 tests
//!
//! All Step 8 tests pass the repo name as a CLI positional argument, so
//! `dialoguer` is never invoked. The binary runs non-interactively and can be
//! driven with `Command::output()`.
//!
//! ## Why rexpect for Step 9 prompt tests
//!
//! `dialoguer::Confirm` requires a real TTY. `rexpect` spawns the binary in a
//! pseudo-terminal so dialoguer sees a TTY and renders its prompts. PTY tests
//! are gated on `#[cfg(unix)]` because rexpect uses Unix PTY APIs.
//!
//! ## Isolation
//!
//! `ENTANGLE_CONFIG_PATH` is set to a per-test temp file. The binary is run
//! with `.current_dir(work_dir)` so it sees the temp directory as cwd.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Write a minimal valid config JSON to `path`, creating parent dirs as needed.
fn write_valid_config(path: &Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let json = serde_json::json!({
        "github_username": "cyrusae",
        "tangled_username": "atdot.fyi",
        "origin_preference": "github",
    });
    std::fs::write(path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
}

/// Spawn `entangle init` with the given extra args, config path, and work dir.
fn run_init(args: &[&str], config_path: &Path, work_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_entangle"))
        .arg("init")
        .args(args)
        .env("ENTANGLE_CONFIG_PATH", config_path)
        .current_dir(work_dir)
        .output()
        .expect("failed to spawn entangle init")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Create a TempDir, config path, and a fresh (non-repo) work subdirectory.
fn setup_dirs() -> (TempDir, PathBuf, PathBuf) {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("config.json");
    let work_dir = dir.path().join("work");
    std::fs::create_dir(&work_dir).unwrap();
    write_valid_config(&config_path);
    (dir, config_path, work_dir)
}

// ---------------------------------------------------------------------------
// Git repo initialization
// ---------------------------------------------------------------------------

#[test]
fn fresh_init_creates_dot_git_directory() {
    let (_dir, config_path, work_dir) = setup_dirs();

    let output = run_init(&["entangle"], &config_path, &work_dir);
    assert!(
        output.status.success(),
        "init must succeed\nstdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    assert!(
        work_dir.join(".git").exists(),
        ".git must exist after fresh init"
    );
}

#[test]
fn fresh_init_prints_initialization_message() {
    let (_dir, config_path, work_dir) = setup_dirs();

    let output = run_init(&["entangle"], &config_path, &work_dir);
    let out = stdout(&output);
    assert!(
        out.contains("nitializ"),
        "fresh init must print initialization message\nstdout: {out}"
    );
}

#[test]
fn second_init_does_not_print_initialization_message() {
    let (_dir, config_path, work_dir) = setup_dirs();

    // First run — initializes.
    run_init(&["entangle"], &config_path, &work_dir);

    // Second run — must not re-initialize.
    let output = run_init(&["entangle"], &config_path, &work_dir);
    assert!(
        output.status.success(),
        "second init must succeed\nstdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    let out = stdout(&output);
    assert!(
        !out.contains("nitializ"),
        "second run must not print initialization message\nstdout: {out}"
    );
}

#[test]
fn second_init_exits_successfully() {
    let (_dir, config_path, work_dir) = setup_dirs();
    run_init(&["entangle"], &config_path, &work_dir);
    let output = run_init(&["entangle"], &config_path, &work_dir);
    assert!(output.status.success(), "second init must exit 0");
}

// ---------------------------------------------------------------------------
// Config validation
// ---------------------------------------------------------------------------

#[test]
fn missing_config_exits_nonzero() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("nonexistent.json");
    let work_dir = dir.path().join("work");
    std::fs::create_dir(&work_dir).unwrap();

    let output = run_init(&["entangle"], &config_path, &work_dir);
    assert!(
        !output.status.success(),
        "must exit non-zero when config is missing"
    );
}

#[test]
fn missing_config_mentions_setup_in_output() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("nonexistent.json");
    let work_dir = dir.path().join("work");
    std::fs::create_dir(&work_dir).unwrap();

    let output = run_init(&["entangle"], &config_path, &work_dir);
    let err = stderr(&output);
    assert!(
        err.contains("setup"),
        "error output must mention 'setup'\nstderr: {err}"
    );
}

#[test]
fn missing_config_does_not_create_git_repo() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("nonexistent.json");
    let work_dir = dir.path().join("work");
    std::fs::create_dir(&work_dir).unwrap();

    run_init(&["entangle"], &config_path, &work_dir);
    assert!(
        !work_dir.join(".git").exists(),
        ".git must not be created when config is missing"
    );
}

// ---------------------------------------------------------------------------
// .gitignore and README.md suggestions
// ---------------------------------------------------------------------------

#[test]
fn suggests_gitignore_when_absent() {
    let (_dir, config_path, work_dir) = setup_dirs();
    // No .gitignore in work_dir.

    let output = run_init(&["entangle"], &config_path, &work_dir);
    let out = stdout(&output);
    assert!(
        out.contains(".gitignore"),
        "must suggest adding .gitignore when absent\nstdout: {out}"
    );
}

#[test]
fn no_gitignore_suggestion_when_present() {
    let (_dir, config_path, work_dir) = setup_dirs();
    std::fs::write(work_dir.join(".gitignore"), b"target/\n*.tmp\n").unwrap();

    let output = run_init(&["entangle"], &config_path, &work_dir);
    let out = stdout(&output);
    assert!(
        !out.contains("Add a .gitignore"),
        ".gitignore suggestion must not appear when file already exists\nstdout: {out}"
    );
}

#[test]
fn suggests_readme_when_absent() {
    let (_dir, config_path, work_dir) = setup_dirs();
    // No README.md in work_dir.

    let output = run_init(&["entangle"], &config_path, &work_dir);
    let out = stdout(&output);
    assert!(
        out.contains("README.md"),
        "must suggest adding README.md when absent\nstdout: {out}"
    );
}

#[test]
fn no_readme_suggestion_when_present() {
    let (_dir, config_path, work_dir) = setup_dirs();
    std::fs::write(work_dir.join("README.md"), b"# My Project\n").unwrap();

    let output = run_init(&["entangle"], &config_path, &work_dir);
    let out = stdout(&output);
    assert!(
        !out.contains("Add a README.md"),
        "README.md suggestion must not appear when file already exists\nstdout: {out}"
    );
}

// ---------------------------------------------------------------------------
// URL preview
// ---------------------------------------------------------------------------

#[test]
fn output_contains_github_url() {
    let (_dir, config_path, work_dir) = setup_dirs();

    let output = run_init(&["entangle"], &config_path, &work_dir);
    let out = stdout(&output);
    assert!(
        out.contains("github.com"),
        "output must show the GitHub URL\nstdout: {out}"
    );
}

#[test]
fn output_contains_tangled_url() {
    let (_dir, config_path, work_dir) = setup_dirs();

    let output = run_init(&["entangle"], &config_path, &work_dir);
    let out = stdout(&output);
    assert!(
        out.contains("tangled.org"),
        "output must show the Tangled URL\nstdout: {out}"
    );
}

#[test]
fn output_contains_repo_name() {
    let (_dir, config_path, work_dir) = setup_dirs();

    let output = run_init(&["my-project"], &config_path, &work_dir);
    let out = stdout(&output);
    assert!(
        out.contains("my-project"),
        "output must include the repo name\nstdout: {out}"
    );
}

// ---------------------------------------------------------------------------
// Alias
// ---------------------------------------------------------------------------

#[test]
fn alias_accepted_and_appears_in_mirror_url() {
    let (_dir, config_path, work_dir) = setup_dirs();

    let output = run_init(&["entangle", "my-alias"], &config_path, &work_dir);
    assert!(
        output.status.success(),
        "must succeed with valid alias\nstdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    let out = stdout(&output);
    // Mirror URL should use the alias, not the repo name.
    assert!(
        out.contains("my-alias"),
        "alias must appear in the mirror URL\nstdout: {out}"
    );
}

#[test]
fn invalid_repo_name_exits_nonzero() {
    let (_dir, config_path, work_dir) = setup_dirs();

    let output = run_init(&["-bad-name"], &config_path, &work_dir);
    assert!(
        !output.status.success(),
        "invalid repo name must exit non-zero"
    );
}

#[test]
fn invalid_repo_name_does_not_create_git_repo() {
    let (_dir, config_path, work_dir) = setup_dirs();

    run_init(&["-bad-name"], &config_path, &work_dir);
    assert!(
        !work_dir.join(".git").exists(),
        ".git must not be created when repo name is invalid"
    );
}

// ---------------------------------------------------------------------------
// Step 9 — early exit (all platforms, piped stdin)
//
// These tests exercise the "both push URLs already present" path which does
// NOT trigger any dialoguer prompts — the binary exits before reaching them.
// ---------------------------------------------------------------------------

/// Append an `[remote "origin"]` section to `.git/config`, optionally with
/// push URLs. Appends so that gix's core config entries are preserved.
fn append_origin_remote(work_dir: &Path, fetch_url: &str, push_urls: &[&str]) {
    use std::io::Write as _;
    let cfg = work_dir.join(".git").join("config");
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&cfg)
        .expect("must open .git/config");
    writeln!(f, "\n[remote \"origin\"]").unwrap();
    writeln!(f, "\turl = {fetch_url}").unwrap();
    writeln!(f, "\tfetch = +refs/heads/*:refs/remotes/origin/*").unwrap();
    for u in push_urls {
        writeln!(f, "\tpushurl = {u}").unwrap();
    }
}

#[test]
fn early_exit_when_both_push_urls_already_configured() {
    let (_dir, config_path, work_dir) = setup_dirs();

    // First init to create the git repo (no origin yet).
    let first = run_init(&["entangle"], &config_path, &work_dir);
    assert!(first.status.success(), "first init must succeed");

    // Manually add origin with both push URLs already set.
    append_origin_remote(
        &work_dir,
        "git@github.com:cyrusae/entangle.git",
        &[
            "git@github.com:cyrusae/entangle.git",
            "git@tangled.org:atdot.fyi/entangle",
        ],
    );

    let output = run_init(&["entangle"], &config_path, &work_dir);
    assert!(
        output.status.success(),
        "must exit 0 when already configured\nstdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    let out = stdout(&output);
    assert!(
        out.contains("already configured"),
        "output must mention 'already configured'\nstdout: {out}"
    );
}

// ---------------------------------------------------------------------------
// Step 9 — overwrite prompt tests (Unix only, PTY via rexpect)
//
// These tests require a real TTY because `dialoguer::Confirm` won't render
// its prompt over piped stdin. rexpect spawns the binary in a
// pseudo-terminal so dialoguer behaves as it would for a real user.
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod pty_overwrite_tests {
    use super::*;
    use rexpect::session::spawn_command;

    /// Timeout for rexpect operations (ms). Long enough for a debug build.
    const TIMEOUT_MS: Option<u64> = Some(10_000);

    /// Shared PTY test setup: valid config + git repo with a GitLab origin
    /// (a URL that doesn't match what `entangle init entangle` would generate
    /// with the standard test config).
    fn setup_with_gitlab_origin() -> (TempDir, PathBuf, PathBuf) {
        let (dir, config_path, work_dir) = setup_dirs();
        // Create the git repo first (init_if_needed runs before remote inspection).
        gix::init(&work_dir).expect("gix::init must succeed");
        // Add a GitLab remote that won't match the github-preference config.
        append_origin_remote(
            &work_dir,
            "git@gitlab.com:someone/something.git",
            &[],
        );
        (dir, config_path, work_dir)
    }

    fn make_cmd(args: &[&str], config_path: &Path, work_dir: &Path) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_entangle"));
        cmd.arg("init");
        for a in args {
            cmd.arg(a);
        }
        cmd.env("ENTANGLE_CONFIG_PATH", config_path);
        cmd.current_dir(work_dir);
        cmd
    }

    #[test]
    fn replace_yes_continues_to_url_preview() {
        let (_dir, config_path, work_dir) = setup_with_gitlab_origin();

        let cmd = make_cmd(&["entangle"], &config_path, &work_dir);
        let mut p = spawn_command(cmd, TIMEOUT_MS).expect("failed to spawn PTY session");

        // Wait for the replace prompt.
        p.exp_string("Replace it with").unwrap();
        // Accept default (Y) by pressing Enter.
        p.send_line("").unwrap();

        // URL preview must appear after the prompt is answered.
        p.exp_string("github.com").unwrap();
        p.exp_eof().unwrap();
    }

    #[test]
    fn replace_no_proceed_yes_shows_warning_and_continues() {
        let (_dir, config_path, work_dir) = setup_with_gitlab_origin();

        let cmd = make_cmd(&["entangle"], &config_path, &work_dir);
        let mut p = spawn_command(cmd, TIMEOUT_MS).expect("failed to spawn PTY session");

        // Decline the replace prompt.
        // rexpect's send() writes to a LineWriter that only auto-flushes on
        // '\n'. After send("n") we must call flush() explicitly so the byte
        // actually reaches the process's stdin before we wait for the next
        // prompt — otherwise 'n' sits in the write buffer indefinitely.
        p.exp_string("Replace it with").unwrap();
        p.send("n").unwrap();
        p.flush().unwrap();

        // Wait for the second prompt, then accept its default (Yes) with
        // send_line("") — the '\n' triggers LineWriter's auto-flush.
        p.exp_string("anyway").unwrap();
        p.send_line("").unwrap();

        // Warning about keeping the existing origin must appear.
        p.exp_string("Note").unwrap();

        p.exp_eof().unwrap();
    }

    #[test]
    fn replace_no_proceed_no_prints_cancelled_and_exits() {
        let (_dir, config_path, work_dir) = setup_with_gitlab_origin();

        let cmd = make_cmd(&["entangle"], &config_path, &work_dir);
        let mut p = spawn_command(cmd, TIMEOUT_MS).expect("failed to spawn PTY session");

        // Decline both prompts. Each send("n") must be followed by flush()
        // so the byte leaves the LineWriter buffer and reaches the process.
        p.exp_string("Replace it with").unwrap();
        p.send("n").unwrap();
        p.flush().unwrap();

        p.exp_string("anyway").unwrap();
        p.send("n").unwrap();
        p.flush().unwrap();

        // Abort message must appear.
        p.exp_string("cancelled").unwrap();

        p.exp_eof().unwrap();
    }
}
