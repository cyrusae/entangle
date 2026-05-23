# Implementation plan

Step-by-step scaffold. Each step ends with runnable, testable code — no step requires the next one to be useful. Work through them in order; later steps build on earlier ones.

Cross-cutting reminders (apply throughout):

- Most behavior should be testable offline; flag anything that genuinely needs a network call
- Documentation comments and inline comments throughout — don't defer these to a cleanup pass
- Sample data for tests: `cyrusae` (GitHub), `atdot.fyi` (Tangled), `entangle` (repo)
- Windows is a stated target but an untested risk; note any platform-specific assumptions as they appear

---

## Step 1: Project skeleton

**Goal**: `cargo run -- --help` works and lists all four subcommands.

- `cargo new entangle --bin`
- Add all dependencies to `Cargo.toml`:
  - `clap` (with `derive` feature)
  - `dialoguer`
  - `indicatif`
  - `owo-colors`
  - `serde` + `serde_json`
  - `dirs`
  - `gix`
- Define `clap` command/subcommand structure: `setup`, `set`, `init`, `shove`
- Stub each subcommand handler to print `"not yet implemented"` and return `Ok(())`
- `entangle` with no subcommand prints available commands (not an error)
- Sketch and document `src/` module layout — decide now so later steps slot in cleanly. Suggested structure:

  ```text
  src/
    main.rs         # clap wiring, dispatch
    cli.rs          # clap type definitions
    config.rs       # Config struct, load/save
    validate.rs     # sanitization and validation
    urls.rs         # URL construction
    git.rs          # gix wrappers (repo detection, remotes, push)
    remote.rs       # SSH ls-refs, RemoteCheckResult
    commands/
      setup.rs
      set.rs
      init.rs
      shove.rs
  ```

**Runnable check**: `cargo build` succeeds. `cargo run -- --help`, `cargo run -- init`, etc. all produce output without panicking.

**Tests**: `cargo test` passes (compilation only at this point).

---

## Step 2: Config — data model and file I/O

**Goal**: Can read and write a config file programmatically. No interactive prompts yet.

- Define `Config` struct with `serde` derives:

  ```rust
  pub struct Config {
      pub github_username: String,
      pub tangled_username: String,
      pub origin_preference: OriginPreference, // enum: GitHub | Tangled
  }
  ```
  
- Implement `Config::path() -> PathBuf` using `dirs::config_dir()`
- Implement `Config::load() -> Result<Config, ConfigError>` — reads from `{config_dir}/entangle/config.json`. Return distinct error variants for:
  - File not found (no config yet)
  - File exists but isn't readable (permissions)
  - Valid UTF-8/JSON but empty object `{}`
  - Valid JSON but missing one or more required fields (one error variant per field, so callers can say "run `entangle set gh-user`")
  - Corrupted/unparseable JSON (single catch-all: "config is unreadable, re-run `entangle setup`")
- Implement `Config::save(&self) -> Result<(), ConfigError>` — writes to same path, creating `{config_dir}/entangle/` if needed. Errors for: can't create directory, can't write file.

**Tests**:
- Unit: serialize a well-formed `Config`, deserialize it back, assert equality
- Unit: each malformed-config error case (empty file, `{}`, missing `github_username`, missing `tangled_username`, missing `origin_preference`, truncated JSON)
- Unit: `save()` creates the directory if it doesn't exist
- Unit: `Config::path()` returns a path ending in `entangle/config.json` (platform-agnostic assertion)
- Integration: round-trip — `save()` then `load()` returns the same struct

**📋 Decision baked in**: unique errors per missing field (not a single "config invalid"), because missing-field errors are actionable and should tell the user which `entangle set` command to run. Parse failures get a single catch-all since the file is unreadable regardless of why.

**⚠️ Windows note**: `dirs::config_dir()` returns `AppData\Roaming` on Windows vs. `~/.config` on Unix. Document this in `config.rs`. No action needed yet, but flag it.

---

## Step 3: Input validation module

**Goal**: Centralized, fully-tested sanitization and validation for all user input. Pure functions, no I/O, no UI.

- Implement sanitization (applied first, before any validation):
  - Lowercase all input
  - Strip leading/trailing whitespace
  - Remove single and double quotes silently
  - Reject any input containing spaces, backticks, or shell metacharacters (`$`, `;`, `|`, `&`, `>`, `<`, `` ` ``) — loud error, not silent strip
- Implement `validate_github_username(input: &str) -> Result<String, ValidationError>`
  - Sanitize first, then validate: ≤39 chars, alphanumeric + hyphens only, no leading/trailing/consecutive hyphens
- Implement `validate_tangled_username(input: &str) -> Result<String, ValidationError>`
  - Sanitize first, then validate against ATProto regex: `/^([a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]([a-zA-Z]{0,61}[a-zA-Z])?$/`
- Implement `validate_repo_name(input: &str) -> Result<String, ValidationError>`
  - Sanitize first, then validate: 1–100 chars, lowercase alphanumeric + hyphens, no consecutive/leading/trailing hyphens, no periods

**Tests** (exhaustive boundary cases from TESTING.md):
- GitHub: valid at exactly 39 chars; invalid at 40; consecutive hyphens; leading hyphen; trailing hyphen; mixed case input lowercased before validation
- Tangled: `a.b` (minimal valid); `atdot.fyi` (valid); `user.tngl.sh` (valid with subdomain); no TLD (invalid); underscore in domain (invalid)
- Repo: 1 char (valid); 100 chars (valid); 101 chars (invalid); period (invalid); consecutive hyphens (invalid); leading hyphen (invalid)
- Sanitization: `"CyrusAE"` → `cyrusae` (quotes stripped, then lowercased); backtick → loud error; `$VAR` → loud error
- **Order**: confirm sanitization happens before validation (test with `"CyrusAE"` — should pass, not fail on uppercase)

**📋 Decision point**: What is the error message style for validation failures? Decide here so it's consistent across all commands. Suggested form: `"'{input}' is not a valid GitHub username: {reason}."` — specific, includes the offending input, states the reason.

---

## Step 4: `entangle set`

**Goal**: Can set individual config values from the command line. Config is read, updated, and written correctly.

- Wire `entangle set gh-user <username>` → sanitize/validate → load config (or create empty if missing) → update field → save
- Wire `entangle set tngl-user <username>` → same pattern
- Wire `entangle set origin <github|tangled>` → validate option → update → save
- `clap` handles missing-argument errors (e.g., `entangle set gh-user` with no value); confirm the error messages are informative
- Re-setting an already-set value silently overwrites with the same data — no special behavior

**Tests**:
- Unit: each `set` variant updates the correct field and leaves others unchanged
- Unit: invalid GitHub username → validation error, config not written
- Unit: invalid Tangled username → validation error, config not written
- Unit: `set origin bogus` → error, config not written
- Unit: `set` on a config missing other fields → updates just the one field, doesn't clobber the rest
- Integration: `entangle set gh-user cyrusae` → config file on disk has `"github_username": "cyrusae"`

**📋 Decision point**: Should `entangle set` print a confirmation of what it just set? (e.g., `"GitHub username set to: cyrusae"`) — decide on the progress message convention here, since `set` is the simplest case. Recommendation: yes, always confirm, because silent success is hard to debug.

---

## Step 5: `entangle setup`

**Goal**: Interactive first-time (and repeat) setup works end-to-end.

- Use `dialoguer` to prompt for GitHub username, Tangled username, and origin preference in order
- Validate each input before accepting; re-prompt with error message on invalid input (don't just reject and exit)
- If a field is already set in the config, use a "currently set to X. Change? [Y/n]" prompt — skip if user says no, prompt for new value if yes
- Write config only after all prompts complete (no partial saves)
- Handle Ctrl+C gracefully: `dialoguer` propagates this as an error; catch it and exit cleanly without writing a partial config

**Tests**:
- Unit: prompt logic correctly detects pre-existing config fields
- Integration: `entangle setup` with piped input (dialoguer supports non-TTY input for testing) — valid inputs → config written
- Integration: invalid input in first prompt → re-prompt, valid on retry → config written
- Integration: `entangle setup` with already-valid config → correct "already set to X" prompts appear
- Integration: Ctrl+C mid-setup → no config written (or previously valid config unchanged if one existed)

**📋 Decision baked in**: On Ctrl+C, do not save partial config. A partial config is worse than no config — the user can always re-run `entangle setup`.

---

## Step 6: URL construction

**Goal**: Given a config and repo name(s), build correct SSH URLs. Pure functions, no network.

- Implement `build_github_url(username: &str, repo: &str) -> String`
  - Output: `git@github.com:{username}/{repo}.git`
- Implement `build_tangled_url(username: &str, repo: &str) -> String`
  - Output: `git@tangled.org:{username}/{repo}` (no `.git`)
- Implement `resolve_urls(config: &Config, repo: &str, alias: Option<&str>) -> (String, String)`
  - Returns `(origin_url, mirror_url)` based on `origin_preference`
  - If `alias` is provided, it becomes the repo name on the non-origin forge
  - If no alias, same repo name is used for both

**Tests**:
- Unit: `build_github_url("cyrusae", "entangle")` → `"git@github.com:cyrusae/entangle.git"`
- Unit: `build_tangled_url("atdot.fyi", "entangle")` → `"git@tangled.org:atdot.fyi/entangle"`
- Unit: `resolve_urls` with GitHub as origin → GitHub URL is first (origin), Tangled is second (mirror)
- Unit: `resolve_urls` with Tangled as origin → Tangled URL is first, GitHub is second
- Unit: `resolve_urls` with alias present → alias used for mirror forge, original name used for origin forge
- Unit: `resolve_urls` with no alias → same name on both forges

---

## Step 7: Remote validation

**Goal**: Can check whether an SSH remote actually exists, with three distinct error paths.

- Implement `RemoteCheckResult` enum: `Ok`, `NotFound`, `AuthFailure`, `NetworkError(String)`
- Implement `check_remote(url: &str) -> RemoteCheckResult` using `gix` SSH ls-refs
  - Map `gix` error types to the three named cases; document the mapping in comments
  - Include timeout handling — network errors should not hang indefinitely
- Implement `validate_remotes(origin_url: &str, mirror_url: &str) -> Result<(), RemoteError>`
  - Checks origin first, then mirror
  - On `NetworkError`: surfaces the offline override prompt ("Couldn't reach remote — accept anyway? [y/N]")
  - On `NotFound` or `AuthFailure`: hard stop, distinct error messages

**Tests**:
- Unit (offline): test each `RemoteCheckResult` variant by injecting mock transport or pointing at localhost
- Unit: `validate_remotes` fails on origin before trying mirror
- Unit: network error triggers override prompt logic (test the logic path, not the actual prompt)
- Integration (online, skippable with a feature flag or `#[ignore]`): real ls-refs against `github.com/cyrusae/entangle`
- Integration: non-routable address → `NetworkError` path (e.g., `git@192.0.2.1:user/repo`)

**📋 Decision point**: How do `gix` errors surface in user-facing messages? Options: (a) expose `gix` error text directly, (b) always map to one of the three named cases with `entangle`-authored messages. Recommendation: (b) — map everything to named cases, include `gix` error detail only in a debug/verbose mode. Decide verbosity levels here if not already settled.

---

## Step 8: `entangle init` — git detection and local setup

**Goal**: `entangle init` can validate config, collect repo name(s), detect git status of the current directory, and initialize if needed. No remote logic yet.

- Check config exists and is valid → refer to `entangle setup` if not (step 1 of init flow)
- Parse CLI args or prompt interactively for repo name and optional alias (steps 2–3)
- Validate repo name(s) using the validation module (step 4)
- Check if the current directory is a git repo using `gix`
  - If not: initialize it with `gix`, print `"Folder is not a git repository, initializing..."`
  - Check for `.gitignore` and `README.md`; print suggestions for any that are missing
- Build URLs (step 5) — needed later but construct them here so they're available for the remote steps

**Tests**:
- Unit: git-repo detection on a non-repo temp directory
- Unit: git-repo detection on an existing repo
- Unit: `.gitignore` detection (present / absent)
- Unit: `README.md` detection (present / absent)
- Integration: `entangle init` in a fresh temp directory → git repo initialized, appropriate messages printed
- Integration (idempotency): run `entangle init` twice in the same fresh directory → second run sees the initialized repo, does not re-initialize

---

## Step 9: `entangle init` — remote inspection and overwrite prompt

**Goal**: `entangle init` reads existing remotes and handles the overwrite prompt flow correctly.

- Read existing remotes from the repo using `gix`
- Early exit with a success message if both GitHub and Tangled push URLs are already present (step 8.1)
- Detect if `origin` exists but doesn't match the generated origin URL (step 8.2)
- Implement the two-level overwrite prompt using `dialoguer`:
  ```
  An origin remote already exists: git@gitlab.com:someone/something.git
  Replace it with git@github.com:cyrusae/entangle.git? [Y/n]
    → No → Add push URLs to existing origin anyway? [Y/n]
              → No → abort (clean exit, no changes made)
  ```
- Output for the "proceed anyway" path should explicitly state what state the user is left in

**Tests**:
- Unit: remote inspection with no remotes configured
- Unit: remote inspection with origin matching expected URL
- Unit: remote inspection with origin not matching (GitLab URL, etc.)
- Unit: remote inspection with both GitHub + Tangled already set → early exit
- Unit: remote inspection with remotes but none named `origin`
- Integration: overwrite prompt, replace=yes → continues normally
- Integration: overwrite prompt, replace=no, proceed=yes → continues with warning message
- Integration: overwrite prompt, replace=no, proceed=no → clean abort, no changes

**📋 Decision point**: What exactly does the "proceed anyway" success output say? Draft it here. Suggested: `"Heads up: origin fetch URL is git@gitlab.com:…, not a GitHub or Tangled remote. Push URLs have been added — your pushes will go to both forges, but pulls will come from the existing origin."` Finalize wording before moving to step 10.

---

## Step 10: `entangle init` — remote configuration and confirmation

**Goal**: `entangle init` wires up push remotes in the correct order and confirms the final state.

- Add non-default-forge push URL via `gix` (e.g., Tangled if GitHub is origin)
- Add default-forge push URL via `gix` (order matters — default is last)
- Print the final `git remote -v`-equivalent output so the user can verify
- Print the `entangle shove` suggestion

**Tests**:
- Unit: push URL order is correct (non-default first, default last)
- Unit: `gix` remote state after the operation matches expected (origin fetch URL + two push URLs)
- Integration: full end-to-end `entangle init` in a fresh repo with no existing remotes → `git remote -v` output matches expected format
- Integration (idempotency): `entangle init` on a repo that already has both push URLs configured → early exit at step 9, no writes
- Integration: `entangle init` on a repo with a matching origin but no push URLs → adds push URLs correctly

---

## Step 11: `entangle shove`

**Goal**: `entangle shove` pushes all branches and tags to both remotes.

- Verify the current directory is a git repo; error if not
- Run `git push origin --all` then `git push origin --tags` via `gix`
  - Since `origin` has two push URLs configured, one command reaches both forges
- Informative error messages if either push fails (surface the underlying error)
- `entangle shove` with unexpected arguments → error with explanation (clap handles this)

**Tests**:
- Integration: `entangle shove` in a directory that isn't a git repo → informative error
- Integration: `entangle shove` in a repo with no commits → informative error (nothing to push)
- Integration: `entangle shove` with unexpected arguments → error with explanation
- Integration (online, `#[ignore]`): `entangle shove` in a configured repo → both remotes receive the push

---

## Step 12: Output polish

**Goal**: Consistent, readable terminal output across all commands. No new functionality — audit and harmonize what exists.

- Audit all output strings across all commands; write out the style guide that emerges:
  - Progress messages (what's happening now)
  - Success messages (what just happened)
  - Warning messages (something's off but not fatal)
  - Error messages (fatal; always suggest what to do next)
- Apply `owo-colors` consistently across all four categories
- Add `indicatif` spinners to: remote validation (`check_remote` calls), push operations (`entangle shove`)
- Ensure no command exits silently on success — always confirm what happened

**Tests**:
- Visual review of all command outputs in a real terminal
- Integration: no command produces output that references internal types or `gix` error strings (unless in a future debug/verbose mode)

**📋 Decision point**: This is the right moment to decide `--quiet` / verbosity behavior (noted as post-MVP but the shape of it becomes clear here). At minimum, decide what output is non-suppressible (errors and the final state confirmation) vs. what would be silenced by `--quiet`.

---

## Step 13: Hardening and edge cases

**Goal**: Systematic coverage of the remaining TESTING.md cases not addressed in earlier steps.

Work through each section of TESTING.md and confirm coverage:

- **Config and file I/O**: permission-denied on config directory; unreadable config file; all partial-config variants
- **Input boundary cases**: all max/min-length inputs; `a.b` Tangled address; shell metacharacters
- **Argument parsing**: `entangle init` with 3+ args; `entangle set` subcommands missing their values; `entangle` alone
- **Config state weirdness**: config with one field missing; Ctrl+C mid-`setup` on existing config
- **Git state mismatches**: GitLab origin; remotes present but none named `origin`; multiple unrecognized remotes
- **Case handling**: confirm sanitization-before-validation end-to-end (not just unit tested)
- **Windows**: test config path construction; test that no hardcoded `/` separators sneak in

For each case, either confirm it's already covered by an earlier step's tests or add a new test here.

**📋 Final documentation pass**: Once hardening is done, update DESIGN.md and TESTING.md to reflect anything discovered during implementation that changed the approach. Note any cases deferred to post-MVP.
