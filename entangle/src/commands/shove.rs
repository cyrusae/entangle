//! Handler for `entangle shove`.
//!
//! Convenience wrapper for:
//! ```text
//! git push origin --all
//! git push origin --tags
//! ```
//!
//! Because `entangle init` configures `origin` with two `pushurl` entries
//! (GitHub and Tangled), a single push command reaches both forges. `shove`
//! is a "push the whole thing" helper — intended for the first full sync after
//! `init` when you want all branches and tags to land on both forges at once.
//!
//! ## Why `git push` rather than gix
//!
//! gix's push API (v0.70) opens one connection per remote URL. It does not
//! automatically iterate multiple `pushurl` entries the way the `git` binary
//! does. Shelling out to `git push` is therefore both simpler *and* more
//! correct: git reads all `pushurl` lines from `.git/config` and pushes to
//! every one of them in a single command.
//!
//! Pre-push validation (repo detection, origin presence, empty-repo guard)
//! uses `gix` directly — those are local, offline checks that belong in the
//! gix domain.

use std::path::Path;
use std::process::Command;

use crate::output;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Entry point called by `main.rs` for the `shove` subcommand.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let work_dir = std::env::current_dir()?;
    run_with_paths(&work_dir)
}

/// Core implementation, factored out for testability.
///
/// All pre-push validation is done via `gix` (offline). The actual push
/// delegates to the `git` binary so that multiple `pushurl` entries in
/// `.git/config` are all reached by a single command.
pub fn run_with_paths(work_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // ── 1. Verify we're inside a git repository ───────────────────────────────
    //
    // gix::discover searches upward from work_dir so that `entangle shove`
    // works correctly whether run from the repo root or a subdirectory.
    let repo = match gix::discover(work_dir) {
        Ok(r) => r,
        Err(_) => {
            return Err("not a git repository. \
                 Navigate to your project directory and run `entangle init` to set one up."
                .into());
        }
    };

    // ── 2. Verify that an origin remote is configured ─────────────────────────
    //
    // `entangle init` is responsible for setting up origin with both push URLs.
    // If origin is missing we explain what to do rather than letting `git push`
    // fail with a generic "no such remote" error.
    let has_origin = match repo.try_find_remote_without_url_rewrite("origin") {
        None | Some(Err(_)) => false,
        Some(Ok(_)) => true,
    };
    if !has_origin {
        return Err("no 'origin' remote is configured. \
             Run `entangle init` to set up the GitHub and Tangled push remotes."
            .into());
    }

    // ── 3. Guard against empty repositories (no commits yet) ──────────────────
    //
    // An unborn HEAD (zero commits) causes `git push` to fail with an opaque
    // refspec error. We catch it here and give a clear instruction instead.
    if repo.head_id().is_err() {
        return Err("no commits to push. \
             Make your first commit, then run `entangle shove` again."
            .into());
    }

    // ── 4. Push all branches to both forges ───────────────────────────────────
    //
    // Because origin has two pushurl entries, `git push origin --all` sends
    // to both GitHub and Tangled in one invocation. Git inherits the calling
    // terminal's SSH agent, so auth works the same as a normal git push.
    println!(
        "{}",
        output::progress("Pushing all branches to both forges…")
    );
    let branch_status = Command::new("git")
        .args(["push", "origin", "--all"])
        .current_dir(work_dir)
        .status()?;
    if !branch_status.success() {
        let code = branch_status.code().unwrap_or(1);
        return Err(format!(
            "branch push failed (exit {code}). \
             Check the output above for details, fix the issue, and re-run {}.",
            output::cmd("entangle shove")
        )
        .into());
    }

    // ── 5. Push all tags to both forges ──────────────────────────────────────
    println!("{}", output::progress("Pushing tags to both forges…"));
    let tag_status = Command::new("git")
        .args(["push", "origin", "--tags"])
        .current_dir(work_dir)
        .status()?;
    if !tag_status.success() {
        let code = tag_status.code().unwrap_or(1);
        return Err(format!(
            "tag push failed (exit {code}). \
             Check the output above for details, fix the issue, and re-run {}.",
            output::cmd("entangle shove")
        )
        .into());
    }

    println!(
        "{}",
        output::success("All branches and tags pushed to both forges.")
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── Pre-push validation ───────────────────────────────────────────────────

    #[test]
    fn shove_errors_on_non_git_directory() {
        let dir = TempDir::new().unwrap();
        let result = run_with_paths(dir.path());
        assert!(result.is_err(), "shove must error in a non-git directory");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not a git repository"),
            "error must mention 'not a git repository': {msg}"
        );
        assert!(
            msg.contains("entangle init"),
            "error must suggest entangle init: {msg}"
        );
    }

    #[test]
    fn shove_errors_when_no_origin_configured() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        // No remotes configured — origin is absent.
        let result = run_with_paths(dir.path());
        assert!(
            result.is_err(),
            "shove must error when origin is not configured"
        );
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("origin"), "error must mention 'origin': {msg}");
        assert!(
            msg.contains("entangle init"),
            "error must suggest entangle init: {msg}"
        );
    }

    #[test]
    fn shove_errors_when_repo_has_no_commits() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        // Set up origin (simulating post-`entangle init` state) but add no commits.
        crate::git::create_origin_remote(
            dir.path(),
            "git@github.com:user/repo.git",
            &[
                "git@tangled.org:user.example.com/repo",
                "git@github.com:user/repo.git",
            ],
        )
        .unwrap();
        let result = run_with_paths(dir.path());
        assert!(
            result.is_err(),
            "shove must error when there are no commits"
        );
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("commit"), "error must mention 'commit': {msg}");
    }

    /// Verify that origin configured with a non-standard remote name does not
    /// satisfy the origin check.
    #[test]
    fn shove_errors_when_only_upstream_remote_configured() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        // Add a remote named "upstream" — origin must still be absent.
        use std::io::Write as _;
        let config_path = dir.path().join(".git").join("config");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&config_path)
            .unwrap();
        writeln!(file, "\n[remote \"upstream\"]").unwrap();
        writeln!(file, "\turl = git@github.com:user/repo.git").unwrap();
        writeln!(file, "\tfetch = +refs/heads/*:refs/remotes/upstream/*").unwrap();

        let result = run_with_paths(dir.path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("origin"), "error must mention 'origin': {msg}");
    }
}
