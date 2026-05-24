//! Terminal output helpers — colors, icons, and spinner.
//!
//! All user-visible formatting is defined here so the visual style stays
//! consistent across all four commands. Call sites import only what they need.
//!
//! ## Style guide
//!
//! | Category  | Icon | Color        | Use for                                        |
//! |-----------|------|--------------|------------------------------------------------|
//! | Success   | `✓`  | Bold green   | Operation completed successfully               |
//! | Warning   | `!`  | Bold yellow  | Non-fatal issue the user should read           |
//! | Error     | `✗`  | Bold red     | Validation failure (inline, before re-prompt)  |
//! | Progress  | —    | Dimmed       | In-flight operation ("Pushing…")               |
//! | Tip       | —    | `Tip:` bold  | Optional improvement hints                     |
//! | URL       | —    | Cyan         | Git remote URLs                                |
//! | Command   | —    | Bold         | `entangle <cmd>` references in text            |
//! | Debug     | —    | Dimmed       | `[debug]` diagnostic lines                     |
//!
//! ## Colors in tests
//!
//! Integration tests capture stdout via `Command::output()` which is not a TTY.
//! `owo-colors` still emits ANSI codes in this mode (always-on by default), but
//! test assertions use substring matching (`contains("✓")`, `contains("setup")`)
//! so the surrounding escape sequences are transparent. A future `NO_COLOR`
//! environment variable check can be added here centrally if needed.
//!
//! ## Spinner
//!
//! [`remote_check_spinner`] returns an `indicatif::ProgressBar` configured for
//! network-call feedback. In non-TTY environments (CI, piped output) indicatif
//! disables drawing automatically — no special-casing is needed in tests.

use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Inline message formatters
// ---------------------------------------------------------------------------

/// `✓ <msg>` — bold green. Use for completed operations.
pub fn success(msg: &str) -> String {
    format!("{} {msg}", "✓".bold().green())
}

/// `! <msg>` — bold yellow. Use for non-fatal warnings the user should note.
pub fn warn(msg: &str) -> String {
    format!("{} {msg}", "!".bold().yellow())
}

/// `✗ <msg>` — bold red. Use for inline validation errors before a re-prompt.
///
/// Does not exit the process. Fatal top-level errors are formatted by
/// [`error_prefix`] in `main.rs`.
pub fn error_inline(msg: &str) -> String {
    format!("{} {msg}", "✗".bold().red())
}

/// Dimmed text. Use for in-flight progress messages ("Pushing…", "Checking…").
pub fn progress(msg: &str) -> String {
    msg.dimmed().to_string()
}

/// `Tip: <msg>` — `Tip:` is bold; message is normal weight.
///
/// Use for optional improvement hints (.gitignore, README.md suggestions).
pub fn tip(msg: &str) -> String {
    format!("  {} {msg}", "Tip:".bold())
}

/// Cyan text. Use for git remote URLs in output.
pub fn url(s: &str) -> String {
    s.cyan().to_string()
}

/// Bold text. Use for `entangle <cmd>` command references embedded in sentences.
///
/// Wraps the name in backtick-quotes so it reads naturally in plain text too:
/// `` `entangle shove` ``.
pub fn cmd(name: &str) -> String {
    format!("`{}`", name.bold())
}

/// Dimmed `[debug]` tag. Use as a prefix on all `VerbosityLevel::Debug` lines.
pub fn debug_tag() -> String {
    "[debug]".dimmed().to_string()
}

/// Bold red `Error:`. Used by `main.rs` for the top-level error prefix.
pub fn error_prefix() -> String {
    "Error:".bold().red().to_string()
}

// ---------------------------------------------------------------------------
// Spinner
// ---------------------------------------------------------------------------

/// Create a spinner for network operations (remote accessibility checks).
///
/// The spinner draws to stderr so it does not interfere with stdout. It will
/// self-disable in non-TTY environments (CI, piped output). Call
/// `spinner.finish_and_clear()` when the operation completes, then print the
/// result via `println!` / `vlog!` as normal.
///
/// ```rust,ignore
/// let sp = output::remote_check_spinner("Checking remote accessibility…");
/// let result = remote_validator(&origin_url, &mirror_url);
/// sp.finish_and_clear();
/// // … handle result
/// ```
pub fn remote_check_spinner(msg: &str) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", " "]),
    );
    spinner.set_message(msg.to_string());
    spinner.enable_steady_tick(Duration::from_millis(80));
    spinner
}
