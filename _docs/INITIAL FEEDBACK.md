# Initial Feedback: Adversarial Review & Test Recommendations

This document provides an adversarial review of the `entangle` codebase and recommends additional test cases to ensure robustness.

## Adversarial Review

### 1. Non-Atomic `.git/config` Editing
The `git.rs` module (functions like `set_origin_fetch_url` and `add_push_urls_to_origin`) uses a read-modify-write pattern: it reads the entire `.git/config` into a `String`, performs text manipulation, and writes it back.
*   **Risk**: This is not atomic. If another process (e.g., a background `git fetch`, an IDE, or another `entangle` instance) modifies `.git/config` between the read and the write, those changes will be clobbered.
*   **Recommendation**: Use a proper config-editing library (like `gix-config`'s mutation API) if possible, or implement file locking to ensure exclusive access during modification.

### 2. Brittle Error Classification
`remote.rs` classifies SSH/Git errors by searching for specific English substrings like `"permission denied"` or `"repository not found"`.
*   **Risk**: If the user's system is configured with a non-English locale, the `git` or `ssh` output may be localized, causing `entangle` to fail to classify the error correctly. It will default to a `NetworkError` and prompt the user to "Accept anyway," which might be confusing if the real problem is a clear `AuthFailure` or `NotFound`.
*   **Recommendation**: Investigate if `gix` provides structured error variants for these cases that bypass the need for string parsing.

### 3. Aggressive Sanitization
`validate.rs` strips all single and double quotes from input.
*   **Risk**: While this prevents some forms of injection, it's a "silent" modification of user intent. If a forge ever allowed quotes in a username (unlikely but theoretically possible), `entangle` would mangle it. More importantly, it's inconsistent with the "reject dangerous characters loudly" philosophy used for other characters.
*   **Recommendation**: Treat quotes the same as other special characters—reject them loudly if they aren't allowed, rather than stripping them silently.
> **OVERRIDEN:** This is desired behavior.

### 4. Hardcoded SSH URL Format
`urls.rs` constructs URLs using the `git@host:user/repo` format.
*   **Risk**: This assumes the default SSH port (22) and the standard `git` user. Users with custom SSH configurations (e.g., in `~/.ssh/config` using a different `Host` alias or port) might find these URLs don't work for them, even if a standard `git clone` would.
*   **Recommendation**: Allow users to override the base SSH host/user string in the config, or use a more flexible URL construction that can respect SSH aliases.

### 5. Lack of Atomicity in `entangle init`
The `init` command performs several side-effecting operations in sequence: `git init`, adding remotes, etc.
*   **Risk**: If the process is interrupted or fails halfway (e.g., during the remote accessibility check), the repository might be left in a "half-baked" state (e.g., a `.git` folder exists but the `origin` remote is missing or incomplete).
*   **Recommendation**: Ensure operations are as idempotent as possible (the current code does a good job of this) and consider a "cleanup" or "rollback" mechanism for failed initializations.

### 6. Case Sensitivity in Config Parsing
`git.rs`'s `read_push_urls` expects `[remote "origin"]` exactly.
*   **Risk**: While standard, Git config is technically case-insensitive for the section name (`remote`). A config containing `[Remote "origin"]` would be missed by `entangle` but respected by `git`.
*   **Recommendation**: Use case-insensitive matching for the `remote` part of the section header.

---

## Recommended Test Cases

### 1. Edge Case Usernames/Handles
*   **ATProto Handles**: Test handles with the maximum number of labels, handles with maximum length labels (63 chars), and handles with numeric-only labels (which are valid except for the TLD).
*   **Repo Names**: Test repo names that are exactly 100 characters long.

### 2. File System Stress Tests
*   **Read-only Config**: Set the config directory to read-only and verify that `entangle setup` and `entangle set` produce clear, helpful error messages.
*   **Full Disk**: Simulate a full disk during `entangle init` to see how it handles a partial write to `.git/config`.
*   **Symlinks**: Run `entangle init` in a directory that is a symbolic link. Ensure it detects the repo correctly and writes to the correct `.git/config`.

### 3. Concurrent Modification
*   **Race Condition**: Write a test that spawns two threads: one running `entangle set` and another rapidly modifying the config file with `git config`. Check for lost updates.

### 4. Malformed Configs
*   **Corrupt `.git/config`**: Manually corrupt `.git/config` (e.g., remove a closing bracket) and run `entangle init`. Ensure it doesn't panic and provides a sane error.
*   **Corrupt `config.json`**: Provide a `config.json` that is valid JSON but has incorrect types (e.g., `github_username` is a number).

### 5. Network & SSH Scenarios
*   **SSH Hang**: Mock the SSH connection to hang indefinitely and verify that the 30-second timeout in `remote.rs` correctly triggers and detaches the thread.
*   **Non-English Locale**: Mock `gix` output with localized error messages (e.g., in French or German) to see if the "Accept anyway" fallback works as intended.
*   **Locked SSH Key**: Test behavior when the SSH key requires a passphrase but no SSH agent is running.

### 6. Interactive Interrupts
*   **Early Exit**: Use `rexpect` to test a `setup` flow where the user hits Ctrl+C at the *very first* prompt. Ensure no `config.json` is created at all.
*   **Late Exit**: Hit Ctrl+C at the *very last* confirmation prompt in `init` and ensure no changes were committed to `.git/config`.
