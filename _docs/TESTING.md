# Testing doc

## Reminders

- Add documentation comments.
- Comment prolifically in general.
- Unit tests and integration tests: use `gix` for unit tests, directly use `git` for integration tests.
- Sample data when real URLs/usernames are needed should be mine: `cyrusae` GitHub username, `atdot.fyi` Tangled username, `entangle` repo.
- Only use real data as opposed to mocks when absolutely necessary. It should be possible to test most of the crate's behavior with my computer off wifi.

Many of the test scenarios below require further interactive discussion and/or will become clearer through development.

## Input sanitization

Sanitize all user input to lowercase. No inputs should include spaces, underscores, or accented or special characters. 

Remove single and double quotes silently (if a user being overly thoughtful thought they needed to `entangle set gh-user "username"`, that's unnecessary of them but not worth erroring).

### Repo names

- Most restrictive of both GitHub and Tangled:
	- 100 characters max
	- Lowercase alphanumeric and hyphens only; no underscores, periods, or spaces
	- No consecutive hyphens 
	- No leading/trailing hyphens

Currently opting not to support GitHub-legal names that wouldn't be valid on Tangled (GitHub allows periods). I don't think the fuss is worth it to potentially special-case "legal on GitHub but requires a Tangled alias".

### Usernames

- GitHub:
	- Up to 39 characters
	- Only alphanumeric characters or hyphens
	- Cannot start or end with a hyphen
	- Cannot have consecutive hyphens
- Tangled:
	- Must be ATProto-legal: valid URLs without underscores
	- Arbitrary subdomains are allowed
	- Must end in a legal TLD
	- Regex: `/^([a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]([a-zA-Z]{0,61}[a-zA-Z])?$/` 

## Test cases

**Non-exhaustive** lists.

- Does `entangle init` stop and refer to `setup` if no config is present or config is malformed?
- Does `entangle init` stop early in a folder that's already been configured with GitHub and Tangled remotes?
- How does `entangle init` fail if `git init` fails?
- Does `entangle init` correctly handle repos where an origin is already set and the mirror is being added?
- Does `entangle init` run end-to-end successfully when it should?
- Does `entangle set` and the `entangle setup` interactive form fail usefully when given unsanitary inputs for usernames?
- Does `entangle set` and the `entangle setup` interactive form fail usefully when given an illegal option for `origin`?
- Do `entangle setup` and `entangle set` create and edit well-formed config files? Are config files successfully edited when updated? 
- Does `entangle set` care if you're re-inputting what's already in the config file, or does it just silently overwrite the data with itself? Why?
- What happens when you run `entangle setup` with an already-valid config file?
- Does `entangle shove` have error messages if the git operations fail?
- Failure cases: malformed URLs, unreachable repos, malformed repo names, malformed usernames, errors from `gix`, errors from `git` 

*How do errors from a dependency (crate or git) pass through to `entangle`'s output?*

### Config and file I/O:

- What if `.config/entangle/` can't be created (permission denied, read-only filesystem)? *(Informative error.)*
- What if the config file exists but isn't readable?
- What if the config file is empty, or contains valid JSON but empty object `{}`?
- What if the config file is partially corrupted JSON (e.g., missing closing brace)?
- **Idempotency question:** If `entangle init` fails partway through (e.g., after `git init` but before remotes are added), can you run it again safely or does it get stuck?
	- *It should be safe to run again: `entangle init` on an initialized git repo sees that `git init` isn't necessary and moves on to remotes.*

*Probably single error for any "config file exists but is unusable" situation, recommending re-running `entangle setup` to produce a new one to overwrite it. Or `cat` the file for the user's perusal? How best to handle this error family?*

### SSH/network vs. repo existence:

Remote validation uses `gix` SSH `ls-refs` (equivalent to `git ls-remote`), with local regex as a fast-fail pre-check. Three distinct error paths — these are separate because they represent different user problems:

- **Not found**: repo doesn't exist at the constructed URL → user has a typo or hasn't initialized the repo yet
- **Auth failure**: SSH handshake failed → user's SSH key isn't configured for that forge (not a typo problem)
- **Network error** (timeout, no route to host) → offer override prompt; tool should be usable offline

Private GitHub repos are supported for free: if the user's SSH key has access, `ls-refs` succeeds regardless of repo visibility. No special-casing needed, but worth a note in `--help` that mirroring a private GitHub repo to public Tangled makes the code public.

Test cases:
- Does `entangle init` produce the correct error for a repo that doesn't exist vs. an SSH key that isn't set up?
- Does the network-error override prompt appear on timeout, and does accepting it allow `init` to proceed?
- Does a private GitHub repo get validated correctly when SSH keys are configured?
- Does a private GitHub repo fail with an auth error (not a not-found error) when SSH keys are *not* configured for GitHub?

### Input boundary cases:

- Username exactly at max length (39 for GitHub, edge of Tangled's regex)? *(Still valid)*
- Repo name exactly 100 chars? *(Still valid)*
- Repo name exactly 1 char (is that valid)? *(Still valid)*
- Tangled address `a.b` (minimal valid domain)? *(Still valid)*
- What about backticks in input, or other shell metacharacters beyond quotes? *(Fail sanitization loudly.)*

### Argument parsing:

- `entangle init` with 3+ arguments? *(Should fail informatively.)*
- `entangle set origin` without a value? *(Should fail informatively.)*
- `entangle set gh-user` without a username? *(Should fail informatively.)*
- `entangle shove` with unexpected arguments (does it error or ignore)? *(Error with explanation.)*

### Config state weirdness:

- What if config exists but is missing required fields (e.g., has `github_username` but not `tangled_username`)?
- Running `entangle setup` on an already-valid config—does it prompt again, or skip to editing? What if they hit Ctrl+C halfway through?

*Invalid config should be checked at multiple points. Re-running `entangle setup` should change the prompt: `x is already set to y. Change? Enter new:` (rephrase this, though). Unique errors for each missing field (apply to above config I/O questions also).*

### Git state mismatches:

- Repo has `origin` pointing to something that's neither GitHub nor Tangled (e.g., a GitLab URL)?
- Repo has remotes but none named `origin`?
- Repo has multiple remotes but not the canonical GitHub + Tangled combination?
- User manually added a remote between init attempts—does init see it? *(`init` should see it because `init` is looking from scratch every time.)*

*In situations where the answer is "potentially correct but not a forge being supported", fail informatively with a message soliciting contributions to extend support to more forges.*

### Case handling:

- Input `CyrusAE` for username, `ENTANGLE` for repo—does lowercase sanitization happen before or after validation? (Should be before, but worth testing the assumption.)
