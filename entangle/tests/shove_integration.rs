//! Integration tests for `entangle shove`.
//!
//! These tests spawn the compiled `entangle` binary and verify its exit code
//! and output for the pre-push validation paths. The end-to-end push path is
//! tested with two local bare repositories as fake remotes (no network required).

use std::path::Path;
use std::process::{Command, Output};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Run `entangle shove` in `work_dir` and return the raw [`Output`].
fn run_shove(work_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_entangle"))
        .arg("shove")
        .current_dir(work_dir)
        .output()
        .expect("failed to spawn entangle shove")
}

/// Initialize a bare git repo in `dir` using the system `git` binary.
///
/// Using the git binary here (rather than gix) is intentional: these are
/// integration tests verifying binary behaviour, and setting up a minimal
/// git repo is faster and more predictable with the installed git toolchain.
fn git_init(dir: &Path) {
    let status = Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .expect("git init must succeed");
    assert!(status.status.success(), "git init failed");
}

/// Add an `origin` remote to the git repo in `dir`.
fn git_remote_add_origin(dir: &Path, url: &str) {
    let status = Command::new("git")
        .args(["remote", "add", "origin", url])
        .current_dir(dir)
        .output()
        .expect("git remote add must succeed");
    assert!(status.status.success(), "git remote add failed");
}

// ---------------------------------------------------------------------------
// Error-path integration tests
// ---------------------------------------------------------------------------

/// `entangle shove` in a plain directory (no `.git/`) must exit non-zero and
/// explain that the directory is not a git repository.
#[test]
fn shove_in_non_git_directory_exits_nonzero() {
    let dir = tempfile::TempDir::new().unwrap();
    let out = run_shove(dir.path());

    assert!(
        !out.status.success(),
        "shove must exit non-zero in a non-git directory; exit: {:?}",
        out.status.code()
    );

    // Error message is written to stderr by main.rs ("Error: …").
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("git repository"),
        "stderr must mention 'git repository': {stderr}"
    );
}

/// `entangle shove` in a git repo where no `origin` remote is configured must
/// exit non-zero and suggest `entangle init`.
#[test]
fn shove_without_origin_remote_exits_nonzero() {
    let dir = tempfile::TempDir::new().unwrap();
    git_init(dir.path());
    // Intentionally no `git remote add origin …`.

    let out = run_shove(dir.path());

    assert!(
        !out.status.success(),
        "shove must exit non-zero when origin is not configured; exit: {:?}",
        out.status.code()
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("origin"),
        "stderr must mention 'origin': {stderr}"
    );
    assert!(
        stderr.contains("entangle init"),
        "stderr must suggest 'entangle init': {stderr}"
    );
}

/// `entangle shove` in an empty repository (origin configured, but zero
/// commits) must exit non-zero and tell the user to make a commit first.
#[test]
fn shove_in_empty_repo_exits_nonzero() {
    let dir = tempfile::TempDir::new().unwrap();
    git_init(dir.path());
    git_remote_add_origin(dir.path(), "git@github.com:user/repo.git");
    // Intentionally no commits.

    let out = run_shove(dir.path());

    assert!(
        !out.status.success(),
        "shove must exit non-zero when there are no commits; exit: {:?}",
        out.status.code()
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("commit"),
        "stderr must mention 'commit': {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Local push tests (no SSH — use bare repos as fake remotes)
// ---------------------------------------------------------------------------

/// Full end-to-end push to two local bare repositories acting as fake remotes.
///
/// Uses `file://` URLs — no SSH credentials or network required. Exercises the
/// entire `entangle shove` code path: repo detection, origin check, commit guard,
/// branch push (`--all`), tag push (`--tags`), and the dual-pushurl forwarding.
#[test]
fn shove_pushes_to_two_local_remotes() {
    // Create two bare repos that will act as the two "forges".
    let forge_a = tempfile::TempDir::new().unwrap();
    let forge_b = tempfile::TempDir::new().unwrap();
    for forge_dir in [forge_a.path(), forge_b.path()] {
        let s = Command::new("git")
            .args(["init", "--bare"])
            .current_dir(forge_dir)
            .output()
            .unwrap();
        assert!(s.status.success(), "git init --bare failed");
    }

    // Create the working repo.
    let work = tempfile::TempDir::new().unwrap();
    git_init(work.path());

    // Configure user identity so `git commit` works without global config.
    for (k, v) in [("user.email", "test@example.com"), ("user.name", "Test")] {
        Command::new("git")
            .args(["config", k, v])
            .current_dir(work.path())
            .output()
            .unwrap();
    }

    // Make a commit and a tag.
    std::fs::write(work.path().join("README.md"), b"# test\n").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(work.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(work.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["tag", "v0.1.0"])
        .current_dir(work.path())
        .output()
        .unwrap();

    // Configure origin with the fetch URL pointing to forge_a, and both
    // pushurl entries pointing at forge_a and forge_b.
    let url_a = format!("file://{}", forge_a.path().display());
    let url_b = format!("file://{}", forge_b.path().display());
    git_remote_add_origin(work.path(), &url_a);
    // Replace the implicit push URL with two explicit pushurl entries.
    Command::new("git")
        .args(["remote", "set-url", "--add", "--push", "origin", &url_a])
        .current_dir(work.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["remote", "set-url", "--add", "--push", "origin", &url_b])
        .current_dir(work.path())
        .output()
        .unwrap();

    // Run `entangle shove` — it must succeed and push to both forges.
    let out = run_shove(work.path());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "shove must succeed with local bare remotes; stdout: {stdout}; stderr: {stderr}"
    );
    assert!(
        stdout.contains("✓"),
        "stdout must show success indicator: {stdout}"
    );

    // Verify both bare repos received the branch and the tag.
    for forge_dir in [forge_a.path(), forge_b.path()] {
        let log = Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(forge_dir)
            .output()
            .unwrap();
        assert!(
            log.status.success(),
            "git log must succeed in bare repo at {}",
            forge_dir.display()
        );
        let log_out = String::from_utf8_lossy(&log.stdout);
        assert!(
            log_out.contains("init"),
            "bare repo must contain the commit; log: {log_out}"
        );

        let tags = Command::new("git")
            .args(["tag"])
            .current_dir(forge_dir)
            .output()
            .unwrap();
        let tags_out = String::from_utf8_lossy(&tags.stdout);
        assert!(
            tags_out.contains("v0.1.0"),
            "bare repo must contain the tag; tags: {tags_out}"
        );
    }
}

// ---------------------------------------------------------------------------
// Argument-parsing tests
// ---------------------------------------------------------------------------

/// `entangle shove` takes no arguments; passing one must exit non-zero with a
/// usage/error message. clap handles this automatically.
#[test]
fn shove_with_unexpected_argument_exits_nonzero() {
    let dir = tempfile::TempDir::new().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_entangle"))
        .args(["shove", "unexpected-arg"])
        .current_dir(dir.path())
        .output()
        .expect("failed to spawn entangle shove");

    assert!(
        !out.status.success(),
        "shove must exit non-zero when passed an unexpected argument"
    );
    // clap writes usage errors to stderr.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.is_empty(),
        "stderr must contain an error message for unexpected argument"
    );
}
