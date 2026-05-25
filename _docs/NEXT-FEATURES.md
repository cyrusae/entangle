# Next Features Design

Three features targeting the next development cycle, in increasing order of complexity:

1. **Non-interactive flags** (`--yes`, `--overwrite`) — surgical additions to `entangle init`
2. **`entangle status`** — new subcommand for repo state inspection
3. **`entangle init --create`** — programmatic repo creation on both forges (v1.0 anchor feature)

---

## Feature 1: Non-interactive flags (`--yes` and `--overwrite`)

### Motivation

`entangle init entangle --yes && entangle shove` should be a valid (if bold) one-liner. The current init flow has several interactive prompts; these flags let users skip them in scripts or when they know exactly what they want.

### `--yes`

Accepts all interactive prompts using their **default answer**, enabling fully non-interactive execution when repo name is passed as an argument.

```bash
entangle init myrepo --yes              # non-interactive, uses defaults
entangle init myrepo --yes --overwrite  # fully automated, aggressive
```

**Decision point mapping:**

| Prompt | Default | `--yes` behavior |
|---|---|---|
| Repo name (no arg provided) | — | still prompts — arg is required for `--yes` to be fully non-interactive |
| Optional alias (no second arg) | none | skips prompt, uses none |
| "Replace origin?" [Y/n] | Y | yes, replace |
| "Proceed with existing origin?" [Y/n] | Y | yes, proceed |
| "Network error — accept anyway?" [y/N] | **N** | **no** — fails safe |

The network override prompt is the exception: its default is N, and silently overriding a network failure in an automated context would be a footgun. `--yes` respects the N default there.

### `--overwrite`

Forces replacement of an existing non-matching origin remote without prompting. Also overrides the network error prompt (if you're scripting with `--overwrite`, you presumably want to push through transient network issues too).

**What `--overwrite` skips:**
- "Replace origin?" prompt — acts as if user said Y
- "Proceed with existing origin?" fallback prompt
- "Network error — accept anyway?" prompt — acts as if user said Y

**What `--overwrite` does NOT skip:**
- Hard auth failures (SSH key not configured)
- Hard "repo not found" errors — these are real problems, not warnings

### How they compose

| Invocation | Behavior |
|---|---|
| `entangle init myrepo` | interactive (current behavior) |
| `entangle init myrepo --yes` | non-interactive, safe defaults, fails on network error |
| `entangle init myrepo --overwrite` | prompts present, but remote/origin conflicts auto-resolved aggressively |
| `entangle init myrepo --yes --overwrite` | fully automated — suitable for scripting |

### Implementation notes

- Add `yes: bool` and `overwrite: bool` to the `Init` clap struct in `cli.rs`
- Thread both flags through the init command handler in `commands/init.rs`
- No new crates required
- `--overwrite` subsumes the existing post-MVP `--overwrite` note in DESIGN.md — this is that feature

---

## Feature 2: `entangle status`

### Purpose

Let users (and scripts) quickly check whether a repository is set up for entangled mirroring, without running a full init. Also useful as a diagnostic when something seems wrong.

### CLI interface

```bash
entangle status           # human-readable output, current directory
entangle status --quiet   # exit-code only, no output (for scripting)
```

### States and output

**Not a git repo** (exit code 2):
```
✗ Not a git repository.
  Run `git init` or `entangle init <repo>` to get started.
```

**Git repo, no remotes configured** (exit code 1):
```
○ Git repository found, but no remotes configured.
  Run `entangle init <repo>` to set up mirroring.
```

**Git repo, remotes present, not entangled** (exit code 1):
```
○ Git repository found. Current remotes:
    origin  git@github.com:cyrusae/myrepo.git (fetch)
    origin  git@github.com:cyrusae/myrepo.git (push)
  Not entangled — no Tangled push URL configured.
  Run `entangle init myrepo` to add mirroring.
```

**Partially configured (one forge missing)** (exit code 1):
```
⚠ Partially entangled. origin has a push URL configured for one forge only:
    push → git@github.com:cyrusae/myrepo.git
  Missing Tangled push URL. Re-run `entangle init myrepo` to complete setup.
```

**Fully entangled, matches config** (exit code 0):
```
✓ Entangled.
    fetch   git@github.com:cyrusae/myrepo.git
    push →  git@tangled.org:atdot.fyi/myrepo
    push →  git@github.com:cyrusae/myrepo.git
```

**Fully entangled, config mismatch** (exit code 0 — it IS entangled, just not as expected):
```
✓ Entangled (with caveats).
    fetch   git@github.com:cyrusae/myrepo.git
    push →  git@tangled.org:someone-else/myrepo
    push →  git@github.com:cyrusae/myrepo.git
  ⚠ Tangled push URL uses 'someone-else', but your config says 'atdot.fyi'.
    Was this set up manually? Run `entangle init` to reconfigure if needed.
```

### Exit codes (for scripting)

| Code | Meaning |
|---|---|
| 0 | Fully entangled (both push URLs present) |
| 1 | Git repo exists but not entangled (or only partially) |
| 2 | Not a git repository |

### Implementation notes

- New `Status` subcommand in `cli.rs`
- Reuses gix remote inspection logic from `init.rs` — extract shared logic to `git.rs` if not already done during init implementation
- Config load is opportunistic: status works without a config, it just can't perform the username mismatch check
- `--quiet` suppresses all output, exits with the appropriate code only
- No network calls — purely local inspection

### How cute to get about divergences

The state machine above covers the main cases. Additional edge cases worth handling in the middle tier:

- **Remotes present but none named `origin`**: list what's there, note that entangle expects an `origin` remote
- **origin exists with a non-GitHub/Tangled URL** (e.g., GitLab): note it, suggest whether to replace or proceed
- **Push URLs present but in wrong order**: flag it — order matters for default push behavior

Anything more exotic than this is probably not worth trying to diagnose automatically. A catch-all "remotes configured but not in an entangle-recognized pattern" covers the rest.

---

## Feature 3: `entangle init --create`

### Overview

`entangle init myrepo --create` creates the repository on both GitHub and Tangled before running the normal init flow. After both repos are created, execution proceeds exactly as a standard `entangle init` would, including remote validation (softened — we just created them) and push URL configuration.

```bash
entangle init myrepo --create
entangle init myrepo my-tangled-alias --create  # different name on each forge
entangle init myrepo --create --private          # private GitHub repo
```

This is the v1.0 anchor feature. It's large enough to be its own milestone; ship non-interactive flags and `status` first.

### What we know about the APIs

**GitHub**: standard REST API.
- Endpoint: `POST https://api.github.com/user/repos`
- Auth: Personal Access Token (PAT) as Bearer token
- Body: `{ "name": "myrepo", "private": false }`

**Tangled**: XRPC procedure `sh.tangled.repo.create`, called at the knot server.
- Endpoint: `POST {tangled_knot_url}/xrpc/sh.tangled.repo.create`
- Auth: ATProto app password → `com.atproto.server.createSession` → Bearer JWT
  - JWTs are minted fresh per invocation and never persisted to disk
- Body: `{ "rkey": "myrepo", "name": "myrepo" }`
  - `rkey` is the repo slug — confirmed to match the SSH URL path component exactly
    (e.g., rkey `entangle` → `git@tangled.org:atdot.fyi/entangle`)
  - `name` is the display name — in practice identical to `rkey` for most users
- The knot URL determines which knot the repo is created under; the knot field in stored
  records is set server-side, not in the request

**From inspecting real AT proto records**: renames preserve `repoDid`; the rkey changes but
the repo's underlying identity is stable. This means `entangle status` could theoretically
verify a repo by DID rather than URL-matching — useful future capability, overkill for now.

### New config: `tangled_knot_url`

`config.json` gains one new optional field:

```json
{
  "github_username": "cyrusae",
  "tangled_username": "atdot.fyi",
  "origin_preference": "github",
  "tangled_knot_url": "https://knot1.tangled.sh"
}
```

- Defaults to `https://knot1.tangled.sh` if absent (covers the vast majority of users)
- Self-hosted knot users set this to their own endpoint URL
- Settable via `entangle set knot-url <url>`

### New config: `tokens.json`

Separate file at `{config_dir}/entangle/tokens.json`. Kept separate from `config.json`
for privacy and backward compatibility — the main config file never contains secrets.

```json
{
  "github_token": "ghp_...",
  "tangled_password": "app-password-here"
}
```

On Unix: created with `chmod 600` (owner read/write only).
On Windows: created in the user's AppData; ACL hardening is a follow-up item.

Note: Tangled stores an **app password**, not a session JWT. JWTs are minted fresh from
the app password on each invocation and never persisted.

### Token resolution

Same three-tier logic for both tokens:

```
1. Check environment variable
      ENTANGLE_GH_TOKEN  /  ENTANGLE_TNGL_PASSWORD
   → found: use it

2. Query OS keyring
      service: "entangle-mirror:github" or "entangle-mirror:tangled"
      user: the configured username
   → found: use it
   → NoBackend / NoWorkspace: fall through (headless/CI environment)
   → NoEntry: stop → "No token found. Run `entangle setup --tokens`."

3. Read tokens.json
   → found: use it, print:
       ⚠ Using token from plaintext file. Consider running `entangle setup`
         to store it in your system keyring instead.
   → not found: stop → "No token found. Run `entangle setup --tokens`."
```

### `entangle setup` additions

`entangle setup` gains a token setup phase, also runnable standalone as
`entangle setup --tokens` to configure just the credential layer.

Token setup flow:
1. Prompt for GitHub PAT (with note pointing to github.com/settings/tokens)
2. Prompt for Tangled app password
3. Attempt to store each in OS keyring; if `NoBackend`, fall back to `tokens.json` with warning
4. Validate both tokens before saving:
   - GitHub: `GET https://api.github.com/user` (confirms token is valid and username matches config)
   - Tangled: `com.atproto.server.createSession` (confirms app password works)
5. Write only after both validate successfully

`--no-validate` flag skips step 4 for offline/air-gapped setup.

### Creation flow

```
entangle init myrepo --create
  │
  ├── 1. Resolve tokens
  │      → GitHub PAT + Tangled app password via three-tier lookup
  │      → Fail fast if either is missing, with targeted error per token
  │
  ├── 2. Create GitHub repo
  │      POST https://api.github.com/user/repos
  │      { "name": "myrepo", "private": false }   (true with --private)
  │      → 422 already exists: stop →
  │           "Repo already exists on GitHub. Run `entangle init myrepo`
  │            without --create to wire up the existing repo."
  │      → 401 auth failure: stop → "GitHub auth failed. Check your PAT."
  │      → success: ✓ Created github.com/cyrusae/myrepo
  │
  ├── 3. Create Tangled repo
  │      POST {tangled_knot_url}/xrpc/sh.tangled.repo.create
  │      { "rkey": "myrepo", "name": "myrepo" }
  │      → already exists: PARTIAL FAILURE — see below
  │      → auth failure: PARTIAL FAILURE — see below
  │      → success: ✓ Created tangled.org/atdot.fyi/myrepo
  │
  └── 4. Proceed with normal entangle init flow (PLAN.md steps 1–11)
         Remote validation (step 6) soft-skips "not found" errors only —
         we just created both repos; brief propagation delay is expected.
         Auth failures and malformed URLs still hard-fail.
```

### Partial failure handling

If GitHub creation succeeds but Tangled fails (or vice versa), the user is left with one
repo and no mirror. entangle should:

1. Print a clear partial failure message naming exactly what was and wasn't created
2. Exit non-zero
3. Suggest explicit recovery options:
   ```
   ✓ Created github.com/cyrusae/myrepo
   ✗ Tangled repo creation failed: [reason]

   To recover:
     (a) Create the Tangled repo manually at tangled.org, then run:
             entangle init myrepo
     (b) Delete the GitHub repo and try again:
             entangle init myrepo --create
   ```

**No automatic rollback.** Silently deleting a newly-created repo is too dangerous.
Recovery is always manual and always explicit.

### `--private` flag

If set, GitHub repo is created with `"private": true`. Tangled repos don't have a
privacy model in the same sense (they live in the AT proto network), so this flag only
affects GitHub. The existing init flow already handles private GitHub repos naturally
via SSH key auth — no special-casing needed downstream.

### Open questions / decisions needed before implementation

- **`entangle create` as top-level alias**: Should `--create` also be available as
  `entangle create myrepo` for discoverability? Probably yes, as a thin alias over the
  same logic. Decide before wiring up clap.

- **GitHub repo options**: The API also supports `description`, `auto_init`, 
  `gitignore_template`, and `license_template`. At minimum `description` is worth
  exposing since the Tangled lexicon supports it too and it's low-friction to add.
  Decide which fields to surface vs. defer.

- **Tangled `defaultBranch`**: The create lexicon supports it. Should entangle detect
  the local repo's default branch (if a local repo already exists) and pass it? Or just
  let Tangled default to `main`? Detecting and passing the local branch is the better
  behavior; it's a small addition.

- **Knot setup UX**: How does a self-hosted knot user know to set `tangled_knot_url`?
  Error messages on creation failure should mention it. Should `entangle setup` ask
  about knot URL proactively, or only if the user explicitly changes it?

### New crates required

- **`ureq`** — lightweight synchronous HTTP client for GitHub API and Tangled XRPC.
  Preferred over `reqwest` since all calls here are synchronous; avoids pulling in an
  async runtime. Revisit if async is needed later.
- **`keyring`** — OS keyring integration (Apple Keychain, Windows Credential Manager,
  Linux Secret Service via D-Bus).

Existing crates (`serde`, `serde_json`, `indicatif`, `dialoguer`, `owo-colors`) cover
the rest of the new surface.

---

## Implementation order

| Order | Feature | Rationale |
|---|---|---|
| 1 | `--yes` + `--overwrite` | No new crates; ships alongside current MVP work |
| 2 | `entangle status` | New subcommand, reuses existing git inspection logic, no new crates; natural companion to a first public release |
| 3 | `entangle init --create` | New crates, new auth layer, new config surface; ships as the v1.0 milestone anchor |
