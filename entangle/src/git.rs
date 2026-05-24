//! `gix` wrappers for local git operations.
//!
//! All functions in this module are purely local — no network access, no SSH.
//! Network operations (SSH ls-refs, push) live in `remote.rs`.
//!
//! ## Repo detection
//!
//! We use `gix::open(path)` for detection, which checks the exact directory
//! without searching upward. This means:
//! - A directory with `.git/` inside → detected as a repo.
//! - A subdirectory of a repo → NOT detected here (we check the cwd, not parents).
//!
//! `gix::discover(path)` (which does search upward) is used elsewhere (e.g. in
//! `remote.rs`) for operations that need the nearest enclosing repo.
//!
//! ## Initialization
//!
//! `gix::init(path)` fails with `DirectoryExists` if `.git/` is already present,
//! so we check [`is_git_repo`] first and only call init when needed. This is
//! also what gives us the idempotency guarantee: running `entangle init` twice
//! in the same directory correctly reports "already a git repo" on the second run.
//!
//! ## Design note on the git binary
//!
//! We use `gix` rather than shelling out to `git` in all production code.
//! Integration tests may call the `git` binary directly to verify observable
//! repository state (e.g., `git remote -v`), but that is the only sanctioned use.

use std::path::Path;

// ---------------------------------------------------------------------------
// Repo detection and initialization
// ---------------------------------------------------------------------------

/// Returns `true` if `path` is the root of a git repository.
///
/// Uses `gix::open`, which checks the exact directory without searching upward.
/// A directory that is *inside* a git repo (but is not its root) returns `false`.
///
/// # Examples
/// ```no_run
/// # use std::path::Path;
/// // Returns true if .git/ exists in /my/project
/// // Returns false if /my/project is a subdirectory of a repo
/// ```
pub fn is_git_repo(path: &Path) -> bool {
    gix::open(path).is_ok()
}

/// Initialize a git repository at `path` if one does not already exist there.
///
/// Returns `true` if a new repository was created, `false` if `path` was
/// already a git repository (and nothing was changed).
///
/// Errors if `gix::init` fails for a reason other than the directory already
/// being a git repo (e.g., permission denied, IO error).
pub fn init_if_needed(path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    if is_git_repo(path) {
        return Ok(false);
    }
    // gix::init would fail with DirectoryExists if .git/ is already present,
    // but we've already checked above, so this branch only runs for fresh dirs.
    gix::init(path)?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// Filesystem presence checks
// ---------------------------------------------------------------------------

/// Returns `true` if a `.gitignore` file exists directly in `path`.
///
/// Does not search subdirectories or parent directories.
pub fn has_gitignore(path: &Path) -> bool {
    path.join(".gitignore").exists()
}

/// Returns `true` if a `README.md` file exists directly in `path`.
///
/// Looks for `README.md` specifically (the conventional casing on GitHub and
/// Tangled). On case-insensitive filesystems (macOS APFS, Windows NTFS) this
/// will also match `readme.md` or `Readme.md` — that is acceptable; the point
/// is to detect that *some* README is present, not to enforce exact casing.
/// `README.txt` and other extensions do not match regardless of case handling.
pub fn has_readme(path: &Path) -> bool {
    path.join("README.md").exists()
}

// ---------------------------------------------------------------------------
// Remote inspection
// ---------------------------------------------------------------------------

/// The state of the `origin` remote in the repository.
///
/// Used by `entangle init` (Step 9) to decide whether to configure remotes
/// from scratch, add missing push URLs, or prompt the user to handle a conflict.
#[derive(Debug, Clone)]
pub enum OriginStatus {
    /// No remote named `origin` is configured in this repository.
    Absent,

    /// A remote named `origin` exists.
    ///
    /// `push_urls` lists all explicitly configured push URLs
    /// (`remote.origin.pushurl` entries in `.git/config`). An empty `Vec`
    /// means no push URLs are explicitly set; git then falls back to
    /// `fetch_url` for pushes, but that *implicit* fallback is **not**
    /// included in `push_urls`.
    Present {
        /// The remote's primary URL (`remote.origin.url`).
        fetch_url: String,

        /// All explicitly configured push URLs (`remote.origin.pushurl`).
        push_urls: Vec<String>,
    },
}

/// Returns the current status of the `origin` remote in `work_dir`.
///
/// Requires `work_dir` to already be a git repository; returns an error if
/// `gix::open` fails (e.g., not a git repo or permission denied).
///
/// ## Why `.git/config` is read directly for push URLs
///
/// gix's `Remote` typed API only surfaces a single push URL (the last one
/// written). Git itself allows multiple `remote.origin.pushurl` entries, and
/// entangle adds exactly two. Reading `.git/config` directly gives us all of
/// them without fighting the gix API.
pub fn get_origin_status(work_dir: &Path) -> Result<OriginStatus, Box<dyn std::error::Error>> {
    let repo = gix::open(work_dir)?;

    let fetch_url = match repo.try_find_remote_without_url_rewrite("origin") {
        // No `origin` remote at all.
        None => return Ok(OriginStatus::Absent),
        // Malformed or incomplete remote section — treat as absent so
        // `entangle init` can safely re-configure without panicking.
        Some(Err(_)) => return Ok(OriginStatus::Absent),
        Some(Ok(remote)) => match remote.url(gix::remote::Direction::Fetch) {
            Some(url) => url.to_string(),
            None => String::new(),
        },
    };

    let push_urls = read_push_urls(work_dir, "origin");
    Ok(OriginStatus::Present {
        fetch_url,
        push_urls,
    })
}

// ---------------------------------------------------------------------------
// Remote configuration (write operations)
// ---------------------------------------------------------------------------

/// Append a new `[remote "origin"]` section to `.git/config`.
///
/// Writes the fetch URL, the standard `+refs/heads/*` fetch refspec, and any
/// supplied push URLs in order.
///
/// # When to call
///
/// Only call this when [`get_origin_status`] returns [`OriginStatus::Absent`].
/// Do not call when an `origin` remote already exists — use
/// [`set_origin_fetch_url`] and [`add_push_urls_to_origin`] instead.
pub fn create_origin_remote(
    work_dir: &Path,
    fetch_url: &str,
    push_urls: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    let config_path = work_dir.join(".git").join("config");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&config_path)?;
    writeln!(file, "\n[remote \"origin\"]")?;
    writeln!(file, "\turl = {fetch_url}")?;
    writeln!(file, "\tfetch = +refs/heads/*:refs/remotes/origin/*")?;
    for url in push_urls {
        writeln!(file, "\tpushurl = {url}")?;
    }
    Ok(())
}

/// Replace the `url =` line in the existing `[remote "origin"]` section.
///
/// Reads `.git/config`, rewrites the first `url` key it finds inside the
/// `[remote "origin"]` section, and writes the result back atomically.
///
/// # When to call
///
/// Call this when [`get_origin_status`] returns `Present` and the user has
/// confirmed that they want the fetch URL replaced (`replace_fetch_url == true`).
pub fn set_origin_fetch_url(
    work_dir: &Path,
    new_url: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = work_dir.join(".git").join("config");
    let content = std::fs::read_to_string(&config_path)?;
    let modified = replace_url_in_origin_section(&content, new_url);
    atomic_write(&config_path, &modified)?;
    Ok(())
}

/// Append one or more `pushurl =` entries to the existing `[remote "origin"]` section.
///
/// Reads `.git/config`, inserts the new `pushurl` lines just before the next
/// section header (or at the end of the file if `[remote "origin"]` is the last
/// section), and writes the result back.
///
/// Pass only the push URLs that are not already present — this function does not
/// deduplicate. The caller is responsible for checking [`OriginStatus`]'s
/// `push_urls` vector first.
pub fn add_push_urls_to_origin(
    work_dir: &Path,
    push_urls: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    if push_urls.is_empty() {
        return Ok(());
    }
    let config_path = work_dir.join(".git").join("config");
    let content = std::fs::read_to_string(&config_path)?;
    let modified = insert_push_urls_in_config(&content, push_urls);
    atomic_write(&config_path, &modified)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Pure text-transform helpers
// ---------------------------------------------------------------------------

/// Returns `true` if `trimmed` (a single line from `.git/config`, already
/// trimmed of whitespace) is the header for `[<section> "<subsection>"]`.
///
/// Section names are compared **case-insensitively** (per the git config spec:
/// section names like `remote`, `branch`, `core` are case-insensitive).
/// Subsection names (the value inside the quotes) are **case-sensitive** (also
/// per the spec: `[remote "origin"]` ≠ `[remote "Origin"]`).
///
/// So `[Remote "origin"]`, `[REMOTE "origin"]`, and `[remote "origin"]` all
/// match when `section = "remote"` and `subsection = "origin"`, but
/// `[remote "Origin"]` does not.
fn section_header_matches(trimmed: &str, section: &str, subsection: &str) -> bool {
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return false;
    }
    let inner = trimmed[1..trimmed.len() - 1].trim();
    // Expected shape: `remote "origin"` (after stripping the outer brackets).
    if let Some(quote_pos) = inner.find('"') {
        let name = inner[..quote_pos].trim();
        let rest = inner[quote_pos..].trim(); // e.g. `"origin"`
        let expected_rest = format!("\"{subsection}\"");
        name.eq_ignore_ascii_case(section) && rest == expected_rest
    } else {
        false
    }
}

/// Write `content` to `path` atomically: create a unique temp file in the
/// same directory, write to it, then rename it over the target.
///
/// Using a unique temp file (via [`tempfile::Builder`]) rather than a fixed
/// `.lock` sibling means concurrent callers cannot collide on the lock name.
/// The temp file is created with `O_CREAT | O_EXCL` semantics so no two
/// processes ever write to the same temp path. The rename is atomic on the
/// same filesystem (POSIX guarantee), so readers always see either the old
/// file or the new one, never a partially-written intermediate state.
///
/// Creating the temp file in the same directory as the target (via
/// `tempfile_in`) ensures both paths are on the same filesystem, which is
/// required for `rename` to be atomic. If `persist` fails (e.g., the target
/// is on a different filesystem), the temp file is automatically cleaned up
/// by `tempfile`'s `Drop` implementation.
///
/// The `.lock` suffix is retained for recognizability — other tools (e.g.,
/// text editors) use the same convention to detect in-progress writes.
fn atomic_write(path: &std::path::Path, content: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    let dir = path.parent().ok_or("path has no parent directory")?;
    let mut tmp = tempfile::Builder::new()
        .suffix(".lock")
        .tempfile_in(dir)?;
    tmp.write_all(content.as_bytes())?;
    tmp.persist(path)?;
    Ok(())
}

/// Replace the `url = ...` line inside the `[remote "origin"]` section of a
/// git config string.
///
/// Only the first `url` key encountered after the `[remote "origin"]` header is
/// replaced; subsequent sections and keys are untouched. If the section or key
/// is not found the original text is returned unchanged.
///
/// Indentation is preserved: the replacement line uses the same leading
/// whitespace as the original.
///
/// Section-name matching is case-insensitive (`[Remote "origin"]` matches);
/// the subsection name `"origin"` is case-sensitive.
fn replace_url_in_origin_section(config_text: &str, new_url: &str) -> String {
    let mut in_section = false;
    let mut replaced = false;
    let mut result = String::with_capacity(config_text.len() + new_url.len());

    for line in config_text.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') {
            in_section = section_header_matches(trimmed, "remote", "origin");
            result.push_str(line);
            result.push('\n');
            continue;
        }

        if in_section
            && !replaced
            && let Some(eq_pos) = trimmed.find('=')
        {
            let key = trimmed[..eq_pos].trim();
            if key.eq_ignore_ascii_case("url") {
                // Preserve the original indentation.
                let indent_len = line.len() - line.trim_start().len();
                let indent = &line[..indent_len];
                result.push_str(indent);
                result.push_str("url = ");
                result.push_str(new_url);
                result.push('\n');
                replaced = true;
                continue;
            }
        }

        result.push_str(line);
        result.push('\n');
    }

    result
}

/// Insert `pushurl = ...` entries into the `[remote "origin"]` section.
///
/// Lines are inserted immediately before the next section header, or at the end
/// of the string if `[remote "origin"]` is the last section. Push URLs are
/// written with a single leading tab (`\t`) to match git's own config style.
///
/// If no `[remote "origin"]` section exists the string is returned unchanged.
/// An empty `push_urls` slice also returns the string unchanged.
///
/// Section-name matching is case-insensitive; subsection name is case-sensitive.
fn insert_push_urls_in_config(config_text: &str, push_urls: &[&str]) -> String {
    if push_urls.is_empty() {
        return config_text.to_string();
    }

    let mut in_section = false;
    let mut inserted = false;
    let mut result = String::with_capacity(config_text.len() + push_urls.len() * 60);

    for line in config_text.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') {
            if in_section && !inserted {
                // Leaving the origin section — insert before the next header.
                for url in push_urls {
                    result.push_str("\tpushurl = ");
                    result.push_str(url);
                    result.push('\n');
                }
                inserted = true;
            }
            in_section = section_header_matches(trimmed, "remote", "origin");
        }

        result.push_str(line);
        result.push('\n');
    }

    // Origin section ran to end of file and we haven't inserted yet.
    if in_section && !inserted {
        for url in push_urls {
            result.push_str("\tpushurl = ");
            result.push_str(url);
            result.push('\n');
        }
    }

    result
}

/// Read all `pushurl` entries for `remote_name` from `.git/config`.
///
/// Multiple `[remote "<name>"]` sections are all visited — git config merges
/// values across repeated sections with the same header, and so does this
/// parser.
///
/// Returns an empty `Vec` if the config file is unreadable or the remote has
/// no explicitly configured push URLs.
///
/// ## Parsing notes
///
/// - Subsection names (the remote name in quotes) are **case-sensitive** per
///   the git config spec, so `[remote "origin"]` ≠ `[remote "Origin"]`.
/// - Config keys (`pushurl`) are **case-insensitive** per the git config spec.
/// - Comment lines (`#` or `;`) are ignored.
/// - Inline comments after values are **not** stripped — this matches the
///   common case where push URLs never carry inline comments.
fn read_push_urls(work_dir: &Path, remote_name: &str) -> Vec<String> {
    let config_path = work_dir.join(".git").join("config");
    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut in_section = false;
    let mut push_urls = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip blank lines and comments.
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        if trimmed.starts_with('[') {
            // Section names are case-insensitive per the git config spec;
            // subsection names (the remote name in quotes) are case-sensitive.
            // section_header_matches handles both correctly.
            in_section = section_header_matches(trimmed, "remote", remote_name);
            continue;
        }

        if in_section && let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim();
            // Config keys are case-insensitive.
            if key.eq_ignore_ascii_case("pushurl") {
                let value = trimmed[eq_pos + 1..].trim();
                push_urls.push(value.to_string());
            }
        }
    }
    push_urls
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── is_git_repo ───────────────────────────────────────────────────────────

    #[test]
    fn is_git_repo_returns_false_for_empty_directory() {
        let dir = TempDir::new().unwrap();
        assert!(
            !is_git_repo(dir.path()),
            "empty directory must not be a git repo"
        );
    }

    #[test]
    fn is_git_repo_returns_false_for_directory_with_files_but_no_git() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("main.rs"),
            b"fn main() { println!(\"hi\"); }",
        )
        .unwrap();
        assert!(
            !is_git_repo(dir.path()),
            "directory with files but no .git must not be a git repo"
        );
    }

    #[test]
    fn is_git_repo_returns_true_after_gix_init() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).expect("gix::init must succeed on a fresh temp dir");
        assert!(
            is_git_repo(dir.path()),
            "directory with .git must be a git repo"
        );
    }

    #[test]
    fn is_git_repo_returns_false_for_subdirectory_of_repo() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        let sub = dir.path().join("src");
        std::fs::create_dir(&sub).unwrap();
        assert!(
            !is_git_repo(&sub),
            "subdirectory of a git repo must not itself be detected as a repo root"
        );
    }

    // ── init_if_needed ────────────────────────────────────────────────────────

    #[test]
    fn init_if_needed_creates_git_repo_and_returns_true() {
        let dir = TempDir::new().unwrap();
        let was_new = init_if_needed(dir.path()).expect("init_if_needed must succeed");
        assert!(
            was_new,
            "must return true when initializing a fresh directory"
        );
        assert!(
            is_git_repo(dir.path()),
            "directory must be a git repo after init"
        );
    }

    #[test]
    fn init_if_needed_is_idempotent_and_returns_false() {
        let dir = TempDir::new().unwrap();
        // Initialize once.
        init_if_needed(dir.path()).unwrap();
        // Call again — must succeed without error and return false.
        let was_new = init_if_needed(dir.path()).expect("second call must not error");
        assert!(
            !was_new,
            "must return false for an already-initialized repo"
        );
    }

    #[test]
    fn init_if_needed_does_not_overwrite_existing_repo_files() {
        let dir = TempDir::new().unwrap();
        // Write a file into the prospective repo root before init.
        std::fs::write(dir.path().join("my_file.txt"), b"hello").unwrap();
        init_if_needed(dir.path()).unwrap();
        // File must still be there.
        assert!(
            dir.path().join("my_file.txt").exists(),
            "init must not destroy pre-existing files in the directory"
        );
    }

    // ── has_gitignore ─────────────────────────────────────────────────────────

    #[test]
    fn has_gitignore_returns_false_when_absent() {
        let dir = TempDir::new().unwrap();
        assert!(!has_gitignore(dir.path()));
    }

    #[test]
    fn has_gitignore_returns_true_when_present() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".gitignore"), b"target/\n").unwrap();
        assert!(has_gitignore(dir.path()));
    }

    #[test]
    fn has_gitignore_does_not_match_differently_named_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("gitignore"), b"target/\n").unwrap(); // no dot
        assert!(
            !has_gitignore(dir.path()),
            "file named 'gitignore' (no leading dot) must not match"
        );
    }

    // ── has_readme ────────────────────────────────────────────────────────────

    #[test]
    fn has_readme_returns_false_when_absent() {
        let dir = TempDir::new().unwrap();
        assert!(!has_readme(dir.path()));
    }

    #[test]
    fn has_readme_returns_true_when_readme_md_present() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("README.md"), b"# My Project\n").unwrap();
        assert!(has_readme(dir.path()));
    }

    // Note: we do NOT test that `readme.md` (lowercase) fails to match.
    // On macOS (APFS) and Windows (NTFS), the filesystem is case-insensitive,
    // so `path.join("README.md").exists()` returns true for any casing.
    // This is acceptable behaviour — the goal is to detect any README.md,
    // and case-insensitive matches are not false positives on those platforms.

    #[test]
    fn has_readme_does_not_match_readme_txt() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("README.txt"), b"My Project\n").unwrap();
        assert!(
            !has_readme(dir.path()),
            "README.txt must not match — we only check for README.md"
        );
    }

    // ── get_origin_status ─────────────────────────────────────────────────────

    /// Write a [remote "origin"] section to .git/config with the given fetch URL
    /// and optional push URLs. Appends to the existing config so gix's own
    /// initialization data is preserved.
    fn append_origin_remote(work_dir: &Path, fetch_url: &str, push_urls: &[&str]) {
        use std::io::Write as _;
        let config_path = work_dir.join(".git").join("config");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&config_path)
            .expect("must open .git/config");
        writeln!(file, "\n[remote \"origin\"]").unwrap();
        writeln!(file, "\turl = {fetch_url}").unwrap();
        writeln!(file, "\tfetch = +refs/heads/*:refs/remotes/origin/*").unwrap();
        for url in push_urls {
            writeln!(file, "\tpushurl = {url}").unwrap();
        }
    }

    #[test]
    fn get_origin_status_absent_for_fresh_repo() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        let status = get_origin_status(dir.path()).expect("must not error");
        assert!(
            matches!(status, OriginStatus::Absent),
            "fresh repo with no remotes must return Absent"
        );
    }

    #[test]
    fn get_origin_status_present_with_no_push_urls() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        append_origin_remote(dir.path(), "git@github.com:cyrusae/entangle.git", &[]);
        let status = get_origin_status(dir.path()).expect("must not error");
        match status {
            OriginStatus::Present {
                fetch_url,
                push_urls,
            } => {
                assert_eq!(fetch_url, "git@github.com:cyrusae/entangle.git");
                assert!(
                    push_urls.is_empty(),
                    "no pushurl entries means empty push_urls vec"
                );
            }
            OriginStatus::Absent => panic!("expected Present, got Absent"),
        }
    }

    #[test]
    fn get_origin_status_present_with_one_push_url() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        append_origin_remote(
            dir.path(),
            "git@github.com:cyrusae/entangle.git",
            &["git@github.com:cyrusae/entangle.git"],
        );
        let status = get_origin_status(dir.path()).expect("must not error");
        match status {
            OriginStatus::Present { push_urls, .. } => {
                assert_eq!(push_urls.len(), 1);
                assert_eq!(push_urls[0], "git@github.com:cyrusae/entangle.git");
            }
            OriginStatus::Absent => panic!("expected Present, got Absent"),
        }
    }

    #[test]
    fn get_origin_status_present_with_two_push_urls() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        append_origin_remote(
            dir.path(),
            "git@github.com:cyrusae/entangle.git",
            &[
                "git@github.com:cyrusae/entangle.git",
                "git@tangled.org:atdot.fyi/entangle",
            ],
        );
        let status = get_origin_status(dir.path()).expect("must not error");
        match status {
            OriginStatus::Present { push_urls, .. } => {
                assert_eq!(push_urls.len(), 2, "must return both push URLs");
                assert!(push_urls.contains(&"git@github.com:cyrusae/entangle.git".to_string()));
                assert!(push_urls.contains(&"git@tangled.org:atdot.fyi/entangle".to_string()));
            }
            OriginStatus::Absent => panic!("expected Present, got Absent"),
        }
    }

    #[test]
    fn get_origin_status_absent_when_different_remote_name_present() {
        // A remote named "upstream" must not be reported as "origin".
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        use std::io::Write as _;
        let config_path = dir.path().join(".git").join("config");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&config_path)
            .unwrap();
        writeln!(file, "\n[remote \"upstream\"]").unwrap();
        writeln!(file, "\turl = git@github.com:someone/repo.git").unwrap();
        writeln!(file, "\tfetch = +refs/heads/*:refs/remotes/upstream/*").unwrap();

        let status = get_origin_status(dir.path()).expect("must not error");
        assert!(
            matches!(status, OriginStatus::Absent),
            "a remote named 'upstream' must not be detected as 'origin'"
        );
    }

    #[test]
    fn read_push_urls_returns_empty_for_repo_with_no_pushurl() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        append_origin_remote(dir.path(), "git@github.com:cyrusae/entangle.git", &[]);
        let urls = read_push_urls(dir.path(), "origin");
        assert!(urls.is_empty(), "no pushurl entries should yield empty vec");
    }

    // ── section_header_matches ────────────────────────────────────────────────

    #[test]
    fn section_header_matches_standard_lowercase() {
        assert!(section_header_matches(
            "[remote \"origin\"]",
            "remote",
            "origin"
        ));
    }

    #[test]
    fn section_header_matches_uppercase_section_name() {
        assert!(section_header_matches(
            "[Remote \"origin\"]",
            "remote",
            "origin"
        ));
    }

    #[test]
    fn section_header_matches_allcaps_section_name() {
        assert!(section_header_matches(
            "[REMOTE \"origin\"]",
            "remote",
            "origin"
        ));
    }

    #[test]
    fn section_header_matches_subsection_is_case_sensitive() {
        // Subsection names are case-sensitive per the git config spec.
        assert!(!section_header_matches(
            "[remote \"Origin\"]",
            "remote",
            "origin"
        ));
        assert!(!section_header_matches(
            "[remote \"ORIGIN\"]",
            "remote",
            "origin"
        ));
    }

    #[test]
    fn section_header_matches_different_remote_name() {
        assert!(!section_header_matches(
            "[remote \"upstream\"]",
            "remote",
            "origin"
        ));
    }

    #[test]
    fn section_header_matches_different_section() {
        assert!(!section_header_matches(
            "[branch \"main\"]",
            "remote",
            "origin"
        ));
    }

    #[test]
    fn section_header_matches_no_subsection() {
        assert!(!section_header_matches("[core]", "remote", "origin"));
    }

    #[test]
    fn read_push_urls_handles_case_insensitive_section_name() {
        // [Remote "origin"] must be treated the same as [remote "origin"].
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        use std::io::Write as _;
        let config_path = dir.path().join(".git").join("config");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&config_path)
            .unwrap();
        writeln!(file, "\n[Remote \"origin\"]").unwrap();
        writeln!(file, "\turl = git@github.com:cyrusae/entangle.git").unwrap();
        writeln!(file, "\tpushurl = git@tangled.org:atdot.fyi/entangle").unwrap();

        let urls = read_push_urls(dir.path(), "origin");
        assert_eq!(
            urls,
            vec!["git@tangled.org:atdot.fyi/entangle"],
            "case-insensitive section name must be recognized"
        );
    }

    #[test]
    fn read_push_urls_handles_case_insensitive_key() {
        // Git config keys are case-insensitive; "PushUrl" and "PUSHURL" must
        // be treated the same as "pushurl".
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        use std::io::Write as _;
        let config_path = dir.path().join(".git").join("config");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&config_path)
            .unwrap();
        writeln!(file, "\n[remote \"origin\"]").unwrap();
        writeln!(file, "\turl = git@github.com:cyrusae/entangle.git").unwrap();
        writeln!(file, "\tPushUrl = git@tangled.org:atdot.fyi/entangle").unwrap();

        let urls = read_push_urls(dir.path(), "origin");
        assert_eq!(urls, vec!["git@tangled.org:atdot.fyi/entangle"]);
    }

    // ── replace_url_in_origin_section ─────────────────────────────────────────

    #[test]
    fn replace_url_in_origin_section_replaces_url() {
        let config = "[core]\n\trepositoryformatversion = 0\n\n[remote \"origin\"]\n\turl = git@github.com:old/repo.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n";
        let result = replace_url_in_origin_section(config, "git@github.com:new/repo.git");
        assert!(
            result.contains("\turl = git@github.com:new/repo.git"),
            "must contain the new url: {result}"
        );
        assert!(
            !result.contains("url = git@github.com:old/repo.git"),
            "must not contain old url: {result}"
        );
        assert!(result.contains("[core]"), "must preserve [core] section");
    }

    #[test]
    fn replace_url_in_origin_section_preserves_indentation() {
        let config = "[remote \"origin\"]\n\t\turl = old\n";
        let result = replace_url_in_origin_section(config, "new");
        // The double-tab indentation from the original must be preserved.
        assert!(
            result.contains("\t\turl = new"),
            "must preserve original indentation: {result}"
        );
    }

    #[test]
    fn replace_url_in_origin_section_does_not_touch_other_remotes() {
        let config =
            "[remote \"upstream\"]\n\turl = upstream_url\n[remote \"origin\"]\n\turl = old_url\n";
        let result = replace_url_in_origin_section(config, "new_url");
        assert!(
            result.contains("upstream_url"),
            "must not modify upstream remote"
        );
        assert!(result.contains("url = new_url"), "must update origin url");
        assert!(!result.contains("url = old_url"), "old url must be gone");
    }

    #[test]
    fn replace_url_in_origin_section_returns_unchanged_when_section_absent() {
        let config = "[core]\n\trepositoryformatversion = 0\n";
        let result = replace_url_in_origin_section(config, "some_url");
        // No [remote "origin"] section — the url must not appear in the result.
        assert!(
            !result.contains("some_url"),
            "must not insert url when section is absent: {result}"
        );
        assert!(
            result.contains("[core]"),
            "core section must still be present"
        );
    }

    // ── insert_push_urls_in_config ────────────────────────────────────────────

    #[test]
    fn insert_push_urls_in_config_inserts_before_next_section() {
        let config =
            "[remote \"origin\"]\n\turl = origin_url\n[branch \"main\"]\n\tremote = origin\n";
        let result = insert_push_urls_in_config(config, &["push_url_1", "push_url_2"]);
        let push1_pos = result
            .find("pushurl = push_url_1")
            .expect("push_url_1 must be in result");
        let branch_pos = result
            .find("[branch")
            .expect("[branch] must still be in result");
        assert!(
            push1_pos < branch_pos,
            "pushurls must appear before [branch] header"
        );
        assert!(
            result.contains("pushurl = push_url_2"),
            "push_url_2 must be present"
        );
    }

    #[test]
    fn insert_push_urls_in_config_appends_when_last_section() {
        let config = "[remote \"origin\"]\n\turl = origin_url\n\tfetch = +refs/heads/*\n";
        let result = insert_push_urls_in_config(config, &["push_url_1"]);
        assert!(
            result.contains("\tpushurl = push_url_1"),
            "must add pushurl at end"
        );
    }

    #[test]
    fn insert_push_urls_in_config_uses_tab_indent() {
        let config = "[remote \"origin\"]\n\turl = url\n";
        let result = insert_push_urls_in_config(config, &["my_url"]);
        assert!(
            result.contains("\tpushurl = my_url"),
            "pushurl must be tab-indented: {result}"
        );
    }

    #[test]
    fn insert_push_urls_in_config_returns_unchanged_for_empty_slice() {
        let config = "[remote \"origin\"]\n\turl = url\n";
        let result = insert_push_urls_in_config(config, &[]);
        assert_eq!(
            result, config,
            "empty push_urls must return unchanged string"
        );
    }

    #[test]
    fn insert_push_urls_in_config_returns_unchanged_when_section_absent() {
        let config = "[core]\n\trepositoryformatversion = 0\n";
        let result = insert_push_urls_in_config(config, &["some_url"]);
        // No origin section — string unchanged (modulo the line-by-line reconstruction).
        assert!(
            !result.contains("pushurl"),
            "must not insert pushurl when section is absent"
        );
    }

    // ── create_origin_remote ──────────────────────────────────────────────────

    #[test]
    fn create_origin_remote_creates_section_with_fetch_and_push_urls() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        create_origin_remote(
            dir.path(),
            "git@github.com:user/repo.git",
            &["git@github.com:user/repo.git", "git@tangled.org:user/repo"],
        )
        .unwrap();

        let status = get_origin_status(dir.path()).unwrap();
        match status {
            OriginStatus::Present {
                fetch_url,
                push_urls,
            } => {
                assert_eq!(fetch_url, "git@github.com:user/repo.git");
                assert_eq!(push_urls.len(), 2, "must have both push URLs");
                assert!(push_urls.contains(&"git@github.com:user/repo.git".to_string()));
                assert!(push_urls.contains(&"git@tangled.org:user/repo".to_string()));
            }
            OriginStatus::Absent => panic!("expected Present after create_origin_remote"),
        }
    }

    #[test]
    fn create_origin_remote_with_no_push_urls_creates_section() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        create_origin_remote(dir.path(), "git@github.com:user/repo.git", &[]).unwrap();

        let status = get_origin_status(dir.path()).unwrap();
        match status {
            OriginStatus::Present {
                fetch_url,
                push_urls,
            } => {
                assert_eq!(fetch_url, "git@github.com:user/repo.git");
                assert!(push_urls.is_empty(), "no push URLs should be configured");
            }
            OriginStatus::Absent => panic!("expected Present"),
        }
    }

    // ── set_origin_fetch_url ──────────────────────────────────────────────────

    #[test]
    fn set_origin_fetch_url_replaces_existing_url() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        append_origin_remote(dir.path(), "git@github.com:old/repo.git", &[]);

        set_origin_fetch_url(dir.path(), "git@github.com:new/repo.git").unwrap();

        let status = get_origin_status(dir.path()).unwrap();
        match status {
            OriginStatus::Present { fetch_url, .. } => {
                assert_eq!(
                    fetch_url, "git@github.com:new/repo.git",
                    "fetch URL must be updated"
                );
            }
            OriginStatus::Absent => panic!("expected Present"),
        }
    }

    #[test]
    fn set_origin_fetch_url_preserves_push_urls() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        append_origin_remote(
            dir.path(),
            "git@github.com:old/repo.git",
            &["git@github.com:old/repo.git"],
        );

        set_origin_fetch_url(dir.path(), "git@github.com:new/repo.git").unwrap();

        let status = get_origin_status(dir.path()).unwrap();
        match status {
            OriginStatus::Present {
                fetch_url,
                push_urls,
            } => {
                assert_eq!(fetch_url, "git@github.com:new/repo.git");
                assert_eq!(push_urls, vec!["git@github.com:old/repo.git"]);
            }
            OriginStatus::Absent => panic!("expected Present"),
        }
    }

    // ── add_push_urls_to_origin ───────────────────────────────────────────────

    #[test]
    fn add_push_urls_to_origin_appends_to_existing_remote() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        append_origin_remote(dir.path(), "git@github.com:user/repo.git", &[]);

        add_push_urls_to_origin(
            dir.path(),
            &["git@github.com:user/repo.git", "git@tangled.org:user/repo"],
        )
        .unwrap();

        let urls = read_push_urls(dir.path(), "origin");
        assert_eq!(urls.len(), 2, "must have both push URLs");
        assert!(urls.contains(&"git@github.com:user/repo.git".to_string()));
        assert!(urls.contains(&"git@tangled.org:user/repo".to_string()));
    }

    #[test]
    fn add_push_urls_to_origin_is_noop_for_empty_slice() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        append_origin_remote(dir.path(), "git@github.com:user/repo.git", &[]);

        add_push_urls_to_origin(dir.path(), &[]).unwrap();

        let urls = read_push_urls(dir.path(), "origin");
        assert!(
            urls.is_empty(),
            "no push URLs should be added from an empty slice"
        );
    }

    #[test]
    fn add_push_urls_to_origin_preserves_fetch_url() {
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        append_origin_remote(dir.path(), "git@github.com:user/repo.git", &[]);

        add_push_urls_to_origin(dir.path(), &["git@tangled.org:user/repo"]).unwrap();

        let status = get_origin_status(dir.path()).unwrap();
        match status {
            OriginStatus::Present { fetch_url, .. } => {
                assert_eq!(
                    fetch_url, "git@github.com:user/repo.git",
                    "fetch URL must be preserved"
                );
            }
            OriginStatus::Absent => panic!("expected Present"),
        }
    }

    #[test]
    fn read_push_urls_collects_across_multiple_sections() {
        // Git allows the same section header to appear multiple times;
        // all occurrences contribute to the same logical remote.
        let dir = TempDir::new().unwrap();
        gix::init(dir.path()).unwrap();
        use std::io::Write as _;
        let config_path = dir.path().join(".git").join("config");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&config_path)
            .unwrap();
        writeln!(file, "\n[remote \"origin\"]").unwrap();
        writeln!(file, "\turl = git@github.com:cyrusae/entangle.git").unwrap();
        writeln!(file, "\tpushurl = git@github.com:cyrusae/entangle.git").unwrap();
        // Second section with the same header (git appends push URLs this way).
        writeln!(file, "\n[remote \"origin\"]").unwrap();
        writeln!(file, "\tpushurl = git@tangled.org:atdot.fyi/entangle").unwrap();

        let urls = read_push_urls(dir.path(), "origin");
        assert_eq!(
            urls.len(),
            2,
            "both pushurl entries from repeated sections must be collected"
        );
        assert!(urls.contains(&"git@github.com:cyrusae/entangle.git".to_string()));
        assert!(urls.contains(&"git@tangled.org:atdot.fyi/entangle".to_string()));
    }
}
