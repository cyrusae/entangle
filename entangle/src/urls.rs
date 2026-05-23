//! URL construction for GitHub and Tangled SSH remotes.
//!
//! All functions are pure — no network access, no I/O, no side effects.
//! They accept already-validated strings; callers are responsible for
//! running inputs through `validate::validate_github_username`,
//! `validate::validate_tangled_username`, and `validate::validate_repo_name`
//! before passing them here.
//!
//! ## URL format notes
//!
//! **GitHub**: `git@github.com:{username}/{repo}.git`
//! The `.git` suffix is conventional for GitHub SSH remotes. Omitting it
//! still works, but including it matches what `gh repo clone` produces and
//! avoids surprising diffs in `git remote -v` output.
//!
//! **Tangled**: `git@tangled.org:{username}/{repo}`
//! Tangled does *not* use a `.git` suffix. Adding one would result in a
//! "repository not found" error from the Tangled SSH server.
//!
//! ## `resolve_urls` semantics
//!
//! `resolve_urls` returns `(origin_url, mirror_url)`:
//! - `origin_url`  — the fetch remote (the forge set as `origin_preference`)
//! - `mirror_url`  — the push-only remote on the other forge
//!
//! If `alias` is `Some`, it is used as the repo name on the **mirror** forge
//! only. The origin forge always uses `repo`. This lets users maintain
//! different repo names on the two forges without affecting the primary
//! fetch remote configuration.

use crate::config::{Config, OriginPreference};

// ---------------------------------------------------------------------------
// Public URL builders
// ---------------------------------------------------------------------------

/// Build the GitHub SSH remote URL for a given username and repo name.
///
/// # Example
/// ```
/// # use entangle::urls::build_github_url;
/// assert_eq!(
///     build_github_url("cyrusae", "entangle"),
///     "git@github.com:cyrusae/entangle.git"
/// );
/// ```
pub fn build_github_url(username: &str, repo: &str) -> String {
    format!("git@github.com:{username}/{repo}.git")
}

/// Build the Tangled.org SSH remote URL for a given username and repo name.
///
/// Note: Tangled does **not** use a `.git` suffix.
///
/// # Example
/// ```
/// # use entangle::urls::build_tangled_url;
/// assert_eq!(
///     build_tangled_url("atdot.fyi", "entangle"),
///     "git@tangled.org:atdot.fyi/entangle"
/// );
/// ```
pub fn build_tangled_url(username: &str, repo: &str) -> String {
    format!("git@tangled.org:{username}/{repo}")
}

// ---------------------------------------------------------------------------
// URL resolution
// ---------------------------------------------------------------------------

/// Resolve the origin and mirror URLs from a full config and repo name.
///
/// Returns `(origin_url, mirror_url)` where:
/// - `origin_url` is the URL for the forge named in `config.origin_preference`
///   (this becomes the fetch remote)
/// - `mirror_url` is the URL for the other forge (configured as a push-only URL)
///
/// If `alias` is `Some`, it is used as the repo name on the **mirror** forge.
/// The origin forge always uses `repo`. If `alias` is `None`, both forges
/// use the same `repo` name.
///
/// # Example — GitHub as origin, no alias
/// ```
/// # use entangle::urls::resolve_urls;
/// # use entangle::config::{Config, OriginPreference};
/// let config = Config {
///     github_username: "cyrusae".to_string(),
///     tangled_username: "atdot.fyi".to_string(),
///     origin_preference: OriginPreference::Github,
/// };
/// let (origin, mirror) = resolve_urls(&config, "entangle", None);
/// assert_eq!(origin, "git@github.com:cyrusae/entangle.git");
/// assert_eq!(mirror, "git@tangled.org:atdot.fyi/entangle");
/// ```
pub fn resolve_urls(config: &Config, repo: &str, alias: Option<&str>) -> (String, String) {
    // The mirror repo name is the alias if provided, otherwise the same as origin.
    let mirror_repo = alias.unwrap_or(repo);

    match config.origin_preference {
        OriginPreference::Github => {
            let origin = build_github_url(&config.github_username, repo);
            let mirror = build_tangled_url(&config.tangled_username, mirror_repo);
            (origin, mirror)
        }
        OriginPreference::Tangled => {
            let origin = build_tangled_url(&config.tangled_username, repo);
            let mirror = build_github_url(&config.github_username, mirror_repo);
            (origin, mirror)
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

    fn github_origin_config() -> Config {
        Config {
            github_username: "cyrusae".to_string(),
            tangled_username: "atdot.fyi".to_string(),
            origin_preference: OriginPreference::Github,
        }
    }

    fn tangled_origin_config() -> Config {
        Config {
            github_username: "cyrusae".to_string(),
            tangled_username: "atdot.fyi".to_string(),
            origin_preference: OriginPreference::Tangled,
        }
    }

    // ── build_github_url ─────────────────────────────────────────────────────

    #[test]
    fn github_url_standard() {
        assert_eq!(
            build_github_url("cyrusae", "entangle"),
            "git@github.com:cyrusae/entangle.git"
        );
    }

    #[test]
    fn github_url_includes_dot_git_suffix() {
        let url = build_github_url("cyrusae", "my-repo");
        assert!(url.ends_with(".git"), "GitHub URL must end with .git, got: {url}");
    }

    #[test]
    fn github_url_format_is_ssh_not_https() {
        let url = build_github_url("cyrusae", "entangle");
        assert!(url.starts_with("git@"), "URL must use SSH format (git@), got: {url}");
        assert!(!url.starts_with("https://"), "URL must not use HTTPS format");
    }

    #[test]
    fn github_url_contains_correct_host() {
        let url = build_github_url("cyrusae", "entangle");
        assert!(url.contains("github.com"), "URL must reference github.com, got: {url}");
    }

    #[test]
    fn github_url_preserves_username_and_repo() {
        let url = build_github_url("my-org", "special-repo");
        assert_eq!(url, "git@github.com:my-org/special-repo.git");
    }

    // ── build_tangled_url ────────────────────────────────────────────────────

    #[test]
    fn tangled_url_standard() {
        assert_eq!(
            build_tangled_url("atdot.fyi", "entangle"),
            "git@tangled.org:atdot.fyi/entangle"
        );
    }

    #[test]
    fn tangled_url_has_no_dot_git_suffix() {
        let url = build_tangled_url("atdot.fyi", "entangle");
        assert!(
            !url.ends_with(".git"),
            "Tangled URL must NOT end with .git, got: {url}"
        );
    }

    #[test]
    fn tangled_url_format_is_ssh_not_https() {
        let url = build_tangled_url("atdot.fyi", "entangle");
        assert!(url.starts_with("git@"), "URL must use SSH format (git@), got: {url}");
        assert!(!url.starts_with("https://"), "URL must not use HTTPS format");
    }

    #[test]
    fn tangled_url_contains_correct_host() {
        let url = build_tangled_url("atdot.fyi", "entangle");
        assert!(url.contains("tangled.org"), "URL must reference tangled.org, got: {url}");
    }

    #[test]
    fn tangled_url_preserves_dotted_username_and_repo() {
        // ATProto handles contain dots — verify they're preserved as-is.
        let url = build_tangled_url("user.tngl.sh", "my-repo");
        assert_eq!(url, "git@tangled.org:user.tngl.sh/my-repo");
    }

    // ── resolve_urls: origin selection ───────────────────────────────────────

    #[test]
    fn resolve_github_origin_returns_github_first() {
        let (origin, _mirror) = resolve_urls(&github_origin_config(), "entangle", None);
        assert!(
            origin.contains("github.com"),
            "origin must be GitHub when preference is Github, got: {origin}"
        );
    }

    #[test]
    fn resolve_github_origin_returns_tangled_second() {
        let (_origin, mirror) = resolve_urls(&github_origin_config(), "entangle", None);
        assert!(
            mirror.contains("tangled.org"),
            "mirror must be Tangled when preference is Github, got: {mirror}"
        );
    }

    #[test]
    fn resolve_tangled_origin_returns_tangled_first() {
        let (origin, _mirror) = resolve_urls(&tangled_origin_config(), "entangle", None);
        assert!(
            origin.contains("tangled.org"),
            "origin must be Tangled when preference is Tangled, got: {origin}"
        );
    }

    #[test]
    fn resolve_tangled_origin_returns_github_second() {
        let (_origin, mirror) = resolve_urls(&tangled_origin_config(), "entangle", None);
        assert!(
            mirror.contains("github.com"),
            "mirror must be GitHub when preference is Tangled, got: {mirror}"
        );
    }

    // ── resolve_urls: exact URL values ───────────────────────────────────────

    #[test]
    fn resolve_github_origin_no_alias_exact_urls() {
        let (origin, mirror) = resolve_urls(&github_origin_config(), "entangle", None);
        assert_eq!(origin, "git@github.com:cyrusae/entangle.git");
        assert_eq!(mirror, "git@tangled.org:atdot.fyi/entangle");
    }

    #[test]
    fn resolve_tangled_origin_no_alias_exact_urls() {
        let (origin, mirror) = resolve_urls(&tangled_origin_config(), "entangle", None);
        assert_eq!(origin, "git@tangled.org:atdot.fyi/entangle");
        assert_eq!(mirror, "git@github.com:cyrusae/entangle.git");
    }

    // ── resolve_urls: alias handling ─────────────────────────────────────────

    #[test]
    fn resolve_with_alias_uses_alias_on_mirror_not_origin() {
        // GitHub is origin: origin uses "entangle", mirror (Tangled) uses alias "my-fork".
        let (origin, mirror) =
            resolve_urls(&github_origin_config(), "entangle", Some("my-fork"));
        assert_eq!(origin, "git@github.com:cyrusae/entangle.git",
            "origin must use the original repo name, not the alias");
        assert_eq!(mirror, "git@tangled.org:atdot.fyi/my-fork",
            "mirror must use the alias");
    }

    #[test]
    fn resolve_tangled_origin_with_alias_uses_alias_on_github_mirror() {
        // Tangled is origin: origin uses "entangle", mirror (GitHub) uses alias "mirror-name".
        let (origin, mirror) =
            resolve_urls(&tangled_origin_config(), "entangle", Some("mirror-name"));
        assert_eq!(origin, "git@tangled.org:atdot.fyi/entangle",
            "origin must use the original repo name, not the alias");
        assert_eq!(mirror, "git@github.com:cyrusae/mirror-name.git",
            "mirror must use the alias");
    }

    #[test]
    fn resolve_no_alias_uses_same_name_on_both_forges() {
        let (origin, mirror) = resolve_urls(&github_origin_config(), "entangle", None);
        // Both should contain "entangle" as the repo name.
        assert!(origin.contains("/entangle"), "origin must contain repo name");
        assert!(mirror.contains("/entangle"), "mirror must contain repo name (no alias)");
    }

    #[test]
    fn resolve_alias_none_and_alias_same_as_repo_are_equivalent() {
        // Passing None and passing Some("entangle") should give the same result.
        let (o_none, m_none) = resolve_urls(&github_origin_config(), "entangle", None);
        let (o_some, m_some) = resolve_urls(&github_origin_config(), "entangle", Some("entangle"));
        assert_eq!(o_none, o_some, "origin must be identical");
        assert_eq!(m_none, m_some, "mirror must be identical");
    }
}
