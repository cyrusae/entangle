//! Input sanitization and validation.
//!
//! All user-facing input passes through this module before being used anywhere.
//! Functions here are pure (no I/O, no side effects) so they are fully unit-testable offline.
//!
//! ## Order of operations (always): sanitize first, then validate.
//!
//! This matters. `"CyrusAE"` becomes `"cyrusae"` before the GitHub username
//! regex runs — the user gets a clean success, not a confusing "uppercase not
//! allowed" error. Every public function in this module sanitizes before it validates.
//!
//! ## Error message style
//!
//! All validation errors follow the pattern:
//!   `"'<input>' is not a valid <field>: <reason>."`
//!
//! The offending input is always included so the user knows exactly what was rejected.
//! The reason is always included so they know what to fix.

// ---------------------------------------------------------------------------
// ValidationError
// ---------------------------------------------------------------------------

/// Errors produced by sanitization or validation.
///
/// The `input` field is the raw string the user provided (before sanitization),
/// included in all error messages so users can spot typos immediately.
#[derive(Debug, PartialEq, Eq)]
pub enum ValidationError {
    /// Input contains a shell metacharacter or space — rejected loudly.
    ///
    /// This is intentionally a hard error (not a silent strip) so the user
    /// knows something unexpected is in their input. The offending character
    /// is named in the message.
    DangerousCharacter {
        /// The character that triggered the rejection.
        ch: char,
    },

    /// Input failed the GitHub username rules.
    InvalidGithubUsername {
        /// The sanitized value that was tested.
        input: String,
        /// Human-readable reason (e.g. "must not start with a hyphen").
        reason: &'static str,
    },

    /// Input failed the ATProto handle rules.
    InvalidTangledUsername {
        /// The sanitized value that was tested.
        input: String,
        /// Human-readable reason.
        reason: &'static str,
    },

    /// Input failed the repository name rules.
    InvalidRepoName {
        /// The sanitized value that was tested.
        input: String,
        /// Human-readable reason.
        reason: &'static str,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::DangerousCharacter { ch } => write!(
                f,
                "Input contains a disallowed character: '{}'. \
                 Shell metacharacters, spaces, and control characters are not allowed.",
                ch.escape_debug()
            ),
            ValidationError::InvalidGithubUsername { input, reason } => {
                write!(f, "'{input}' is not a valid GitHub username: {reason}.")
            }
            ValidationError::InvalidTangledUsername { input, reason } => {
                write!(f, "'{input}' is not a valid Tangled username: {reason}.")
            }
            ValidationError::InvalidRepoName { input, reason } => {
                write!(f, "'{input}' is not a valid repository name: {reason}.")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

// ---------------------------------------------------------------------------
// Sanitization
// ---------------------------------------------------------------------------

/// Characters that are rejected loudly rather than stripped silently.
///
/// Includes spaces, all common shell metacharacters, and ASCII control
/// characters that could be used for git config injection (e.g. newlines).
/// We strip quotes silently (a user wrapping a value in quotes is being
/// overly careful but harmless), but anything in this list in a username
/// or repo name is almost certainly a mistake worth flagging.
///
/// Note: the specific validators (`validate_github_username` etc.) also
/// enforce an allowlist (`[a-z0-9-]`) that catches anything not in this
/// list. This list is the first line of defence and is intentionally
/// comprehensive so `sanitize` alone is a meaningful guard if reused.
const DANGEROUS_CHARS: &[char] = &[
    // Whitespace and separators
    ' ', '\t', '\n', '\r', // Shell expansion / substitution
    '$', '`', // Shell control flow
    ';', '|', '&', // Redirection
    '>', '<', // Globbing and pattern matching
    '*', '?', '[', ']', // Grouping
    '(', ')', '{', '}', // Path / escape characters
    '\\', '~',
];

/// Sanitize a raw user input string.
///
/// Steps applied in order:
/// 1. Strip leading and trailing whitespace.
/// 2. Remove single and double quotes silently.
/// 3. Reject any remaining character from [`DANGEROUS_CHARS`] with a loud error.
/// 4. Lowercase everything.
///
/// Returns the sanitized string on success, or [`ValidationError::DangerousCharacter`]
/// if a disallowed character is found.
///
/// # Why this order?
///
/// Stripping quotes before the metacharacter check means `"hello"` → `hello`
/// without triggering a false positive. Lowercasing last means the dangerous-char
/// check runs on the post-strip value, keeping the logic straightforward.
pub fn sanitize(input: &str) -> Result<String, ValidationError> {
    // Step 1: strip surrounding whitespace.
    let trimmed = input.trim();

    // Step 2: remove single and double quotes silently.
    // A user who typed `entangle set gh-user "cyrusae"` meant well.
    let dequoted: String = trimmed
        .chars()
        .filter(|c| *c != '\'' && *c != '"')
        .collect();

    // Step 3: reject dangerous characters loudly.
    // We check after quote removal so bare quotes don't accidentally mask
    // something suspicious — but in practice quotes are the only thing we strip.
    for ch in dequoted.chars() {
        if DANGEROUS_CHARS.contains(&ch) {
            return Err(ValidationError::DangerousCharacter { ch });
        }
    }

    // Step 4: lowercase.
    Ok(dequoted.to_lowercase())
}

// ---------------------------------------------------------------------------
// GitHub username validation
// ---------------------------------------------------------------------------

/// Validate a GitHub username.
///
/// Sanitizes first, then enforces:
/// - 1–39 characters
/// - Only alphanumeric characters or hyphens (`[a-z0-9-]` after lowercasing)
/// - Must not start with a hyphen
/// - Must not end with a hyphen
/// - Must not contain consecutive hyphens (`--`)
///
/// Returns the sanitized, validated username on success.
pub fn validate_github_username(input: &str) -> Result<String, ValidationError> {
    let s = sanitize(input)?;

    let err = |reason| {
        Err(ValidationError::InvalidGithubUsername {
            input: s.clone(),
            reason,
        })
    };

    if s.is_empty() {
        return err("must not be empty");
    }
    if s.len() > 39 {
        return err("must be 39 characters or fewer");
    }
    if s.starts_with('-') {
        return err("must not start with a hyphen");
    }
    if s.ends_with('-') {
        return err("must not end with a hyphen");
    }
    if s.contains("--") {
        return err("must not contain consecutive hyphens");
    }
    if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return err("may only contain alphanumeric characters and hyphens");
    }

    Ok(s)
}

// ---------------------------------------------------------------------------
// Tangled (ATProto) username validation
// ---------------------------------------------------------------------------

/// Validate a Tangled username (an ATProto handle).
///
/// Sanitizes first, then validates against the ATProto handle regex:
/// `/^([a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]([a-zA-Z]{0,61}[a-zA-Z])?$/`
///
/// In plain terms:
/// - Must contain at least one dot (i.e. have at least two labels)
/// - Each label: starts and ends with alphanumeric, hyphens allowed in the middle
/// - The final label (TLD) must be letters only
/// - No underscores anywhere
///
/// Returns the sanitized, validated username on success.
///
/// # Examples
/// - `atdot.fyi` — valid
/// - `user.tngl.sh` — valid (subdomain handle)
/// - `a.b` — valid (minimal)
/// - `nodot` — invalid (no TLD)
/// - `has_under.score` — invalid (underscore)
pub fn validate_tangled_username(input: &str) -> Result<String, ValidationError> {
    let s = sanitize(input)?;

    let err = |reason| {
        Err(ValidationError::InvalidTangledUsername {
            input: s.clone(),
            reason,
        })
    };

    if s.is_empty() {
        return err("must not be empty");
    }

    // ATProto handle regex (case-insensitive — we've already lowercased, but
    // the regex allows upper too so it works either way).
    //
    // We validate manually rather than pulling in the `regex` crate to keep
    // compile times down and keep this module dependency-free.
    //
    // Rules encoded below:
    //   - Must have at least two dot-separated labels
    //   - Non-final labels: [a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?
    //     (1–63 chars, alphanumeric + interior hyphens, no leading/trailing hyphen)
    //   - Final label (TLD): [a-z]([a-z]{0,61}[a-z])? — letters only, 1–63 chars
    //   - No underscores (already caught by sanitize? No — underscores are not in
    //     DANGEROUS_CHARS because they're legal in some contexts. Catch them here.)

    if s.contains('_') {
        return err("underscores are not allowed in ATProto handles");
    }

    let labels: Vec<&str> = s.split('.').collect();

    if labels.len() < 2 {
        return err("must contain at least one dot (e.g. 'user.bsky.social')");
    }

    // Validate non-final labels.
    let (tld, non_tld_labels) = labels.split_last().unwrap();

    for label in non_tld_labels {
        validate_atproto_non_tld_label(label).map_err(|reason| {
            ValidationError::InvalidTangledUsername {
                input: s.clone(),
                reason,
            }
        })?;
    }

    // Validate the TLD — letters only.
    validate_atproto_tld_label(tld).map_err(|reason| ValidationError::InvalidTangledUsername {
        input: s.clone(),
        reason,
    })?;

    Ok(s)
}

/// Validate a single non-TLD ATProto label (e.g. `atdot` in `atdot.fyi`).
///
/// Rules: 1–63 chars, `[a-z0-9]` with interior hyphens allowed,
/// no leading or trailing hyphen.
fn validate_atproto_non_tld_label(label: &str) -> Result<(), &'static str> {
    if label.is_empty() {
        return Err("each part of the handle must not be empty (check for double dots)");
    }
    if label.len() > 63 {
        return Err("each part of the handle must be 63 characters or fewer");
    }
    if label.starts_with('-') {
        return Err("each part of the handle must not start with a hyphen");
    }
    if label.ends_with('-') {
        return Err("each part of the handle must not end with a hyphen");
    }
    if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("each part of the handle may only contain alphanumeric characters and hyphens");
    }
    Ok(())
}

/// Validate the TLD label of an ATProto handle (e.g. `fyi` in `atdot.fyi`).
///
/// Rules: 1–63 chars, letters only (`[a-z]`), no hyphens, no digits.
fn validate_atproto_tld_label(tld: &str) -> Result<(), &'static str> {
    if tld.is_empty() {
        return Err("must end with a valid TLD (e.g. '.fyi', '.social', '.sh')");
    }
    if tld.len() > 63 {
        return Err("TLD must be 63 characters or fewer");
    }
    if !tld.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err("TLD must contain only letters (e.g. '.fyi', '.social')");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Repository name validation
// ---------------------------------------------------------------------------

/// Validate a repository name.
///
/// Sanitizes first, then enforces the most restrictive superset of GitHub and
/// Tangled naming rules (GitHub allows periods; we don't, to avoid needing
/// special-case handling for names that are legal on one forge but not the other):
///
/// - 1–100 characters
/// - Lowercase alphanumeric characters and hyphens only (no periods, underscores, spaces)
/// - Must not start with a hyphen
/// - Must not end with a hyphen
/// - Must not contain consecutive hyphens (`--`)
///
/// Returns the sanitized, validated name on success.
pub fn validate_repo_name(input: &str) -> Result<String, ValidationError> {
    let s = sanitize(input)?;

    let err = |reason| {
        Err(ValidationError::InvalidRepoName {
            input: s.clone(),
            reason,
        })
    };

    if s.is_empty() {
        return err("must not be empty");
    }
    if s.len() > 100 {
        return err("must be 100 characters or fewer");
    }
    if s.starts_with('-') {
        return err("must not start with a hyphen");
    }
    if s.ends_with('-') {
        return err("must not end with a hyphen");
    }
    if s.contains("--") {
        return err("must not contain consecutive hyphens");
    }
    if s.contains('.') {
        return err("periods are not allowed (use hyphens instead)");
    }
    if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return err("may only contain alphanumeric characters and hyphens");
    }

    Ok(s)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ────────────────────────────────────────────────────────────────────────
    // Sanitization
    // ────────────────────────────────────────────────────────────────────────

    #[test]
    fn sanitize_lowercases() {
        assert_eq!(sanitize("CyrusAE").unwrap(), "cyrusae");
    }

    #[test]
    fn sanitize_strips_surrounding_whitespace() {
        assert_eq!(sanitize("  hello  ").unwrap(), "hello");
    }

    #[test]
    fn sanitize_removes_double_quotes() {
        assert_eq!(sanitize(r#""cyrusae""#).unwrap(), "cyrusae");
    }

    #[test]
    fn sanitize_removes_single_quotes() {
        assert_eq!(sanitize("'cyrusae'").unwrap(), "cyrusae");
    }

    #[test]
    fn sanitize_removes_mixed_quotes() {
        assert_eq!(sanitize(r#"'cy"rus'ae"#).unwrap(), "cyrusae");
    }

    #[test]
    fn sanitize_rejects_space() {
        let err = sanitize("hello world").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: ' ' }
        ));
    }

    #[test]
    fn sanitize_rejects_backtick() {
        let err = sanitize("hello`world").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: '`' }
        ));
    }

    #[test]
    fn sanitize_rejects_dollar_sign() {
        let err = sanitize("$VAR").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: '$' }
        ));
    }

    #[test]
    fn sanitize_rejects_semicolon() {
        let err = sanitize("foo;bar").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: ';' }
        ));
    }

    #[test]
    fn sanitize_rejects_pipe() {
        let err = sanitize("foo|bar").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: '|' }
        ));
    }

    #[test]
    fn sanitize_rejects_ampersand() {
        let err = sanitize("foo&bar").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: '&' }
        ));
    }

    #[test]
    fn sanitize_rejects_gt() {
        let err = sanitize("foo>bar").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: '>' }
        ));
    }

    #[test]
    fn sanitize_rejects_lt() {
        let err = sanitize("foo<bar").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: '<' }
        ));
    }

    #[test]
    fn sanitize_rejects_newline() {
        let err = sanitize("foo\nbar").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: '\n' }
        ));
    }

    #[test]
    fn sanitize_rejects_carriage_return() {
        let err = sanitize("foo\rbar").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: '\r' }
        ));
    }

    #[test]
    fn sanitize_rejects_asterisk() {
        let err = sanitize("foo*bar").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: '*' }
        ));
    }

    #[test]
    fn sanitize_rejects_backslash() {
        let err = sanitize("foo\\bar").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: '\\' }
        ));
    }

    #[test]
    fn sanitize_rejects_tilde() {
        let err = sanitize("~foo").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: '~' }
        ));
    }

    #[test]
    fn dangerous_char_display_uses_escape_debug_for_newline() {
        let err = ValidationError::DangerousCharacter { ch: '\n' };
        let msg = err.to_string();
        // escape_debug renders '\n' as the two-character sequence \n, not a raw newline.
        assert!(
            msg.contains("\\n"),
            "newline must be rendered as \\n in error message, got: {msg}"
        );
        assert!(
            !msg.contains('\n'),
            "raw newline must not appear in error message: {msg}"
        );
    }

    // ────────────────────────────────────────────────────────────────────────
    // GitHub username validation
    // ────────────────────────────────────────────────────────────────────────

    #[test]
    fn github_valid_simple() {
        assert_eq!(validate_github_username("cyrusae").unwrap(), "cyrusae");
    }

    #[test]
    fn github_valid_with_hyphen() {
        assert_eq!(validate_github_username("cyrus-ae").unwrap(), "cyrus-ae");
    }

    #[test]
    fn github_valid_at_exactly_39_chars() {
        // 39 'a's — should pass.
        let name = "a".repeat(39);
        assert_eq!(validate_github_username(&name).unwrap(), name);
    }

    #[test]
    fn github_invalid_at_40_chars() {
        // 40 chars — must fail.
        let name = "a".repeat(40);
        let err = validate_github_username(&name).unwrap_err();
        assert!(matches!(err, ValidationError::InvalidGithubUsername { .. }));
    }

    #[test]
    fn github_uppercase_is_lowercased_before_validation() {
        // "CyrusAE" sanitizes to "cyrusae" — must succeed, not fail on uppercase.
        assert_eq!(validate_github_username("CyrusAE").unwrap(), "cyrusae");
    }

    #[test]
    fn github_quoted_input_is_dequoted() {
        assert_eq!(validate_github_username(r#""cyrusae""#).unwrap(), "cyrusae");
    }

    #[test]
    fn github_leading_hyphen_invalid() {
        let err = validate_github_username("-cyrusae").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidGithubUsername { .. }));
    }

    #[test]
    fn github_trailing_hyphen_invalid() {
        let err = validate_github_username("cyrusae-").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidGithubUsername { .. }));
    }

    #[test]
    fn github_consecutive_hyphens_invalid() {
        let err = validate_github_username("cy--rusae").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidGithubUsername { .. }));
    }

    #[test]
    fn github_underscore_invalid() {
        let err = validate_github_username("cyrus_ae").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidGithubUsername { .. }));
    }

    #[test]
    fn github_empty_invalid() {
        let err = validate_github_username("").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidGithubUsername { .. }));
    }

    #[test]
    fn github_single_char_valid() {
        assert_eq!(validate_github_username("a").unwrap(), "a");
    }

    // ────────────────────────────────────────────────────────────────────────
    // Tangled (ATProto) username validation
    // ────────────────────────────────────────────────────────────────────────

    #[test]
    fn tangled_valid_simple() {
        assert_eq!(validate_tangled_username("atdot.fyi").unwrap(), "atdot.fyi");
    }

    #[test]
    fn tangled_valid_subdomain() {
        assert_eq!(
            validate_tangled_username("user.tngl.sh").unwrap(),
            "user.tngl.sh"
        );
    }

    #[test]
    fn tangled_valid_minimal() {
        // Minimal valid ATProto handle: single-char label + single-char TLD.
        assert_eq!(validate_tangled_username("a.b").unwrap(), "a.b");
    }

    #[test]
    fn tangled_label_valid_at_exactly_63_chars() {
        let label = format!("{}.fyi", "a".repeat(63));
        assert_eq!(validate_tangled_username(&label).unwrap(), label);
    }

    #[test]
    fn tangled_label_invalid_at_64_chars() {
        let label = format!("{}.fyi", "a".repeat(64));
        let err = validate_tangled_username(&label).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidTangledUsername { .. }
        ));
    }

    #[test]
    fn tangled_tld_valid_at_exactly_63_chars() {
        let tld = format!("user.{}", "a".repeat(63));
        assert_eq!(validate_tangled_username(&tld).unwrap(), tld);
    }

    #[test]
    fn tangled_tld_invalid_at_64_chars() {
        let tld = format!("user.{}", "a".repeat(64));
        let err = validate_tangled_username(&tld).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidTangledUsername { .. }
        ));
    }

    #[test]
    fn tangled_uppercase_lowercased_before_validation() {
        assert_eq!(validate_tangled_username("AtDot.FYI").unwrap(), "atdot.fyi");
    }

    #[test]
    fn tangled_no_dot_invalid() {
        let err = validate_tangled_username("nodot").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidTangledUsername { .. }
        ));
    }

    #[test]
    fn tangled_underscore_invalid() {
        let err = validate_tangled_username("has_under.score").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidTangledUsername { .. }
        ));
    }

    #[test]
    fn tangled_tld_with_digit_invalid() {
        // TLD must be letters only — digits not allowed.
        let err = validate_tangled_username("user.fyi2").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidTangledUsername { .. }
        ));
    }

    #[test]
    fn tangled_label_leading_hyphen_invalid() {
        let err = validate_tangled_username("-user.fyi").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidTangledUsername { .. }
        ));
    }

    #[test]
    fn tangled_label_trailing_hyphen_invalid() {
        let err = validate_tangled_username("user-.fyi").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidTangledUsername { .. }
        ));
    }

    #[test]
    fn tangled_empty_label_from_double_dot_invalid() {
        let err = validate_tangled_username("user..fyi").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidTangledUsername { .. }
        ));
    }

    #[test]
    fn tangled_empty_tld_invalid() {
        // Trailing dot → empty TLD.
        let err = validate_tangled_username("user.fyi.").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidTangledUsername { .. }
        ));
    }

    #[test]
    fn tangled_empty_invalid() {
        let err = validate_tangled_username("").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::InvalidTangledUsername { .. }
        ));
    }

    #[test]
    fn tangled_hyphen_in_middle_of_label_valid() {
        // Interior hyphens in non-TLD labels are fine.
        assert_eq!(
            validate_tangled_username("my-handle.bsky.social").unwrap(),
            "my-handle.bsky.social"
        );
    }

    #[test]
    fn tangled_numeric_subdomain_valid() {
        // Digits are allowed in non-TLD labels.
        assert_eq!(
            validate_tangled_username("user123.fyi").unwrap(),
            "user123.fyi"
        );
    }

    // ────────────────────────────────────────────────────────────────────────
    // Repository name validation
    // ────────────────────────────────────────────────────────────────────────

    #[test]
    fn repo_valid_simple() {
        assert_eq!(validate_repo_name("entangle").unwrap(), "entangle");
    }

    #[test]
    fn repo_valid_with_hyphen() {
        assert_eq!(validate_repo_name("my-project").unwrap(), "my-project");
    }

    #[test]
    fn repo_valid_single_char() {
        assert_eq!(validate_repo_name("a").unwrap(), "a");
    }

    #[test]
    fn repo_valid_at_exactly_100_chars() {
        let name = "a".repeat(100);
        assert_eq!(validate_repo_name(&name).unwrap(), name);
    }

    #[test]
    fn repo_invalid_at_101_chars() {
        let name = "a".repeat(101);
        let err = validate_repo_name(&name).unwrap_err();
        assert!(matches!(err, ValidationError::InvalidRepoName { .. }));
    }

    #[test]
    fn repo_uppercase_lowercased_before_validation() {
        assert_eq!(validate_repo_name("ENTANGLE").unwrap(), "entangle");
    }

    #[test]
    fn repo_leading_hyphen_invalid() {
        let err = validate_repo_name("-entangle").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidRepoName { .. }));
    }

    #[test]
    fn repo_trailing_hyphen_invalid() {
        let err = validate_repo_name("entangle-").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidRepoName { .. }));
    }

    #[test]
    fn repo_consecutive_hyphens_invalid() {
        let err = validate_repo_name("en--tangle").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidRepoName { .. }));
    }

    #[test]
    fn repo_period_invalid() {
        // Periods are legal on GitHub but not on Tangled — we reject them.
        let err = validate_repo_name("my.project").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidRepoName { .. }));
    }

    #[test]
    fn repo_underscore_invalid() {
        let err = validate_repo_name("my_project").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidRepoName { .. }));
    }

    #[test]
    fn repo_empty_invalid() {
        let err = validate_repo_name("").unwrap_err();
        assert!(matches!(err, ValidationError::InvalidRepoName { .. }));
    }

    // ────────────────────────────────────────────────────────────────────────
    // Cross-cutting: sanitize-before-validate ordering
    // ────────────────────────────────────────────────────────────────────────

    #[test]
    fn github_sanitize_before_validate_mixed_case_passes() {
        // "CyrusAE" must pass (sanitized to "cyrusae" first), not fail with
        // "uppercase not allowed". This pins the sanitize-first contract.
        let result = validate_github_username("CyrusAE");
        assert_eq!(result.unwrap(), "cyrusae");
    }

    #[test]
    fn repo_sanitize_before_validate_mixed_case_passes() {
        let result = validate_repo_name("MyRepo");
        assert_eq!(result.unwrap(), "myrepo");
    }

    #[test]
    fn tangled_sanitize_before_validate_mixed_case_passes() {
        let result = validate_tangled_username("MyHandle.FYI");
        assert_eq!(result.unwrap(), "myhandle.fyi");
    }

    #[test]
    fn dangerous_char_propagates_through_github_validator() {
        // Shell metacharacter → DangerousCharacter, not InvalidGithubUsername.
        let err = validate_github_username("cyrus$ae").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: '$' }
        ));
    }

    #[test]
    fn dangerous_char_propagates_through_repo_validator() {
        let err = validate_repo_name("my|repo").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: '|' }
        ));
    }

    #[test]
    fn dangerous_char_propagates_through_tangled_validator() {
        let err = validate_tangled_username("user;name.fyi").unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DangerousCharacter { ch: ';' }
        ));
    }
}
