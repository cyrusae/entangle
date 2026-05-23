# entangle

**Language:** Rust

**Output:** CLI tool

**Target:** Linux, MacOS, Windows

**Goal:** Easy setup for mirroring GitHub repos to Tangled.org in one command

## Manual process this is replacing

From the Tangled.org docs, the thing I have to do by hand at the moment:

> You can configure your local repository to push to both Tangled and, say, GitHub. You may already have the following setup:
> 
```bash
$ git remote -v
origin  git@github.com:username/my-project.git (fetch)
origin  git@github.com:username/my-project.git (push)
```
>
 Now add Tangled as an additional push URL to the same remote:
>
```bash
git remote set-url --add --push origin git@tangled.org:user.tngl.sh/my-project
```
>
 You also need to re-add the original URL as a push destination (Git will now use the original URL to fetch only):
>
```bash
git remote set-url --add --push origin git@github.com:username/my-project.git
```
>
 Verify your configuration:
>
```bash
$ git remote -v
origin  git@github.com:username/my-project.git (fetch)
origin  git@tangled.org:user.tngl.sh/my-project (push)
origin  git@github.com:username/my-project.git (push)
```
>
 Notice that there’s one fetch URL (the primary remote) and two push URLs. 

The goal is to **never do that manually again**.

## Commands to implement

- `entangle init`: interactive if empty, or `entangle init repo-name-as-positional-argument optional-alias-as-positional-argument`
- `entangle setup`: interactive setup of GitHub and Tangled usernames and default origin preference (defaults to GitHub)
- `entangle set [gh-user | github-user || tngl-user | tangled-user || origin {gh | github || tngl | tangled}]` to manually establish config preferences
- `entangle shove` finishing-up helper: convenience alias for `git push origin --all && git push origin --tags`. One-time "push the whole thing" helper for the first sync after `entangle init`; since `origin` already has two push URLs configured, one command hits both forges automatically.

### Using `entangle setup`

- Setup creates a JSON config file at `.config/entangle/config.json` or appropriate equivalent (use `dirs` crate to handle directories)
- Interactive setup: prompt for GitHub username, Tangled username, and origin preference (GitHub or Tangled, default GitHub), in that order
- Individual features set with `entangle set`: `gh-user | github-user username`,  `tngl-user | tangled-user username`, `origin` can be `gh | github` or `tngl | tangled`.
- `entangle set` modifies the global `config`

#### Config file format:

```json
{
     "github_username": "...",
     "tangled_username": "...",
     "origin_preference": "github" // or "tangled"
}
```

**`origin_preference`**: which remote is the fetch remote.

##### Example of `entangle set`

```bash
# Pretend you're me
entangle set gh-user cyrusae # this checks GitHub username validity
entangle set tngl-user atdot.fyi # this checks ATProto username validity
entangle set origin github # this is already the default option
```

### Using `entangle init`

When a user runs `entangle init`, what should happen:

1. Check if a valid config exists; if not, stop and refer the user to `entangle setup`
2. Without arguments: prompt for repo name and then for optional mirror name
3. With arguments: `entangle init arg1 arg2` where `arg1` is the repo name and `arg2` is the optional alias for its mirror (i.e., by default, `arg1` is the GitHub repo name and `arg2` if present becomes the Tangled repo name)
4. Check whether name(s) given are valid repo names
5. Build the prospective GitHub and Tangled URLs
   - GitHub: `git@github.com:{github_username}/{repo}.git`
   - Tangled: `git@tangled.org:{tangled_username}/{repo}` (note: no `.git`; Tangled username is a full ATProto handle, e.g. `atdot.fyi`)
6. Check if intended remotes are valid — **two-stage:**
	1. **Local regex first**: validate the constructed URL strings before touching the network (fast fail on obviously malformed input)
	2. **`gix` SSH `ls-refs`**: attempt a real connection to each remote (equivalent to `git ls-remote`). Fail the default-origin remote first. Distinguish three error cases:
		- **Not found**: repo doesn't exist at that URL → stop, warn user to check repo name / whether the repo has been initialized on that forge
		- **Auth failure**: SSH handshake failed → stop, warn user their SSH key may not be configured for that forge (different problem from a typo)
		- **Network error** (timeout, no route to host): → warn and offer an override prompt ("Couldn't reach remote. Accept anyway? [y/N]") so the tool remains usable offline
	- Note: private GitHub repos are handled naturally by this approach — if the user's SSH key has access, `ls-refs` succeeds regardless of repo visibility. No special-casing needed.
7. Check if the folder is already a git repository
	1. If not, `git init` and print informative message ("Folder is not a git repository, initializing...")
	2. Check for `.gitignore` and `README.md`; suggest user to add them on their next commit if either or both are missing
8. Check if remotes are already configured
	1. If both GitHub and Tangled remotes exist according to `git remote -v`, stop early
	2. If `origin` doesn't match the generated `origin` URL based on `repo-name`, prompt:
		```
		An origin remote already exists: git@gitlab.com:someone/something.git
		Replace it with git@github.com:{user}/{repo}.git? [Y/n]
		```
		- **Yes**: replace and continue normally
		- **No**: prompt `Add push URLs to existing origin anyway? [Y/n]`
			- **Yes**: continue (origin fetch URL stays as-is; push URLs will be added to whatever is there — this is unusual but not blocked)
			- **No**: abort
9. Add the non-default remote as push origin 
10. Re-add the default remote as push origin (order matters, default last)
11. Finish and return to user verification of the set remotes

Ending prompt should include a suggestion to `entangle shove` for a first-time mirror to sync all repo contents (including all branches).

## Coding and preferences

- **Use the `gix` crate for interacting with git.** Avoid shelling out to `git` except for tests.
- **Modular code**: if I have to scroll twice, the file might be doing too many things at once.
- Add documentation comments.
- Comment prolifically in general.
- Unit tests and integration tests: use `gix` for unit tests, directly use `git` for integration tests. *See the testing doc for details.*

### Test cases and sanitization

*See the testing doc for details.*

## Decisions to make during development

- Consistent style guide for progress messages
- Consistent style guide for error messages 
- Formatting of terminal output
- **Crates for terminal output (decided):**
	- `clap` — argument parsing
	- `dialoguer` — interactive prompts (`setup`, Y/n confirmations, "already set to X, change?" pattern); handles Ctrl+C gracefully
	- `indicatif` — spinners/progress for network calls (`ls-refs` can take a moment)
	- `owo-colors` — terminal colors
	- `serde` + `serde_json` — config file serialization
	- `dirs` — platform-aware config directory (already noted above)
	- `gix` — all git operations (already noted above)
- *Comprehensive* test suite — how to maximize coverage?

## Post-MVP considerations/additions

- Add `-o | --overwrite` flag to skip the overwriting warnings
	- *Decision point:* How aggressively does `--overwrite` let `entangle` act? Decide this based on failure cases established during testing.
- Add `-q | --quiet` flag to do things silently instead of verbosely (I prefer verbose as a default), and/or a `config`-level preference on verbosity (default `"verbosity_preference": "verbose"`)
	- *Decision point:* What output still happens if `quiet` is on? Decide this based on the final draft of the verbose messages.
- `entangle help` or `-h`/`--help` shows help; `entangle command --help` shows per-command help; `entangle` lists valid commands
- `entangle version` or `-v`/`--version` shows version
- `entangle one-or-more illegal-arguments from-a-user` fails helpfully