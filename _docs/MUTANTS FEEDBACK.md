# Mutation Testing Feedback & Action Items

This document tracks findings from running `cargo-mutants` on the `entangle` crate. It outlines completed configuration work, survived mutants, and remaining test gaps to implement.

---

## Status Dashboard

*   [x] **Create `.cargo/mutants.toml` Configuration File** (Completed)
*   [x] **Add `mutants` Dev-Dependency** (Completed)
*   [x] **Add Code-Level Skipping to Ignored Blocks** (Completed)
*   [x] **Add Interactive `init` Integration Tests** (Completed)
*   [x] **Add Partially-Configured Remote Tests** (Completed)
*   [x] **Add Length Boundary Validation Tests** (Completed)
*   [x] **Set up pull-request diff mutation CI pipeline** (Completed)

---

## Completed Configuration Work

### 1. Style & Styling Exclusions
*   **Action Taken:** Excluded [output.rs](file:///Users/watcher/GitHere/entangle/entangle/src/output.rs) and [main.rs](file:///Users/watcher/GitHere/entangle/entangle/src/main.rs) in [.cargo/mutants.toml](file:///Users/watcher/GitHere/entangle/entangle/.cargo/mutants.toml).
*   **Rationale:** Asserting on raw ANSI coloring or literal character wrappers makes test suites brittle. Excluding them prevents `cargo-mutants` from generating redundant mutants in them.

### 2. Code-Level Skips (`#[cfg_attr(test, mutants::skip)]`)
*   **Action Taken:** Added attributes to skip testing unreachable operating system borders, signal triggers, or network components:
    *   `validate_remotes` & `full_error_chain` in [remote.rs](file:///Users/watcher/GitHere/entangle/entangle/src/remote.rs)
    *   `is_cancelled` in [init.rs](file:///Users/watcher/GitHere/entangle/entangle/src/commands/init.rs)
    *   `is_cancelled` & `handle_cancel` in [setup.rs](file:///Users/watcher/GitHere/entangle/entangle/src/commands/setup.rs)
*   **Rationale:** Avoids useless mutants for signals (like Ctrl+C via `SIGINT`) which terminate the subprocess before the code can run, or network checks that are mock-bypassed in offline test environments.

### 3. Dependencies
*   **Action Taken:** Added `mutants` version `0.0.3` to the `[dev-dependencies]` block in [Cargo.toml](file:///Users/watcher/GitHere/entangle/entangle/Cargo.toml).
*   **Rationale:** Gating it in dev-dependencies ensures the release compilation of `entangle` remains lightweight and dependency-free, while providing the attribute definitions during test compiles.

### 4. Interactive `init` Integration Tests
*   **Action Taken:** Added `interactive_init_prompts_for_repo_and_alias` PTY test in [init_integration.rs](file:///Users/watcher/GitHere/entangle/entangle/tests/init_integration.rs).
*   **Rationale:** Simulates user typing inputs into interactive prompts when `entangle init` is run without repository arguments. Asserts repository creation, preview URLs, and `.git/config` updates.

### 5. Partially-Configured Remote Tests
*   **Action Taken:** Added `init_with_only_mirror_push_url_configured_adds_origin_push_url` and `init_with_only_origin_push_url_configured_adds_mirror_push_url` tests in [init_integration.rs](file:///Users/watcher/GitHere/entangle/entangle/tests/init_integration.rs).
*   **Rationale:** Covers cases where a repository already has one of the push URLs set. Verifies that running `init` successfully appends the missing URL without duplicating the existing one or causing incorrect early exits.

### 6. Length Boundary Validation Tests
*   **Action Taken:** Added `tangled_label_valid_at_exactly_63_chars`, `tangled_label_invalid_at_64_chars`, `tangled_tld_valid_at_exactly_63_chars`, and `tangled_tld_invalid_at_64_chars` unit tests to `tests` module in [validate.rs](file:///Users/watcher/GitHere/entangle/entangle/src/validate.rs).
*   **Rationale:** Verifies behavior exactly at the 63-character boundary limit (valid) and 64-character limit (invalid) for ATProto labels and TLDs.

### 7. Pull-Request Diff Mutation CI Pipeline
*   **Action Taken:** Created [.github/workflows/mutants.yml](file:///Users/watcher/GitHere/entangle/.github/workflows/mutants.yml) workflow file.
*   **Rationale:** Automatically triggers mutation testing on pull requests targeting `main`. Uses the `--in-diff` flag to generate and test mutants *only* on lines modified in the PR, keeping execution time fast (~1–2 minutes) and preventing CI bloat.
