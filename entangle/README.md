# entangle

**entangle** is a small CLI that wires up your git remotes so a single `git push` sends your code to both [GitHub](https://github.com) and [Tangled](https://tangled.sh) simultaneously.

```
entangle setup    # configure your usernames once
entangle init     # wire up remotes in this repository
entangle shove    # first-time push to both forges
```

> **Scope note:** entangle currently supports GitHub ↔ Tangled mirroring only. Issues and PRs expanding it to other forges or mirroring workflows are very welcome.

---

## Installation

entangle is not yet published to crates.io. Install directly from source:

```bash
cargo install --git https://github.com/cyrusae/entangle --locked
```

Or build manually:

```bash
git clone https://github.com/cyrusae/entangle
cd entangle/entangle
cargo build --release
# binary is at target/release/entangle
```

**Requirements:** Rust 1.85+ (edition 2024), an SSH key configured for both GitHub and Tangled.

---

## Quick start

### 1. Configure your usernames

```bash
entangle setup
```

Interactive prompts for your GitHub username, your Tangled handle (e.g. `yourname.bsky.social`), and which forge you want as the primary fetch remote. These are stored once in your platform config directory and reused for every repository.

You can also set individual values non-interactively:

```bash
entangle set gh-user   your-github-username
entangle set tngl-user yourhandle.bsky.social
entangle set origin    github   # or: tangled
```

### 2. Wire up a repository

Run from inside an existing local repository (or an empty directory — `entangle init` will run `git init` for you):

```bash
entangle init
```

entangle will prompt for a repository name and configure `origin` with push URLs for both forges. If your repo has a different name on each forge, pass both:

```bash
entangle init my-repo my-repo-alias
```

Run `entangle init --help` for the full argument reference.

### 3. Push to both forges at once

After your first commit:

```bash
entangle shove
```

This runs `git push origin --all && git push origin --tags`. Because `origin` has two push URLs configured, both forges receive every branch and tag in one command.

For day-to-day work, `git push` continues to work exactly as before — entangle does not change your workflow after `init`.

---

## Commands

| Command | Description |
|---|---|
| `entangle setup` | Interactive configuration of usernames and origin preference |
| `entangle set <key> <value>` | Set a single config field non-interactively |
| `entangle init [repo] [alias]` | Wire up push remotes in the current repository |
| `entangle shove` | Push all branches and tags to both forges |

Run `entangle <command> --help` for full per-command usage.

### `entangle set` keys

| Key | Aliases | Values |
|---|---|---|
| `gh-user` | `github-user` | Your GitHub username |
| `tngl-user` | `tangled-user` | Your Tangled ATProto handle |
| `origin` | — | `github` / `gh` or `tangled` / `tngl` |

---

## Repository name rules

entangle enforces the **most restrictive common ground** of GitHub and Tangled naming rules. A handful of repository names that are technically valid on GitHub will be rejected because they cannot be created on Tangled.

Specifically, repository names must:

- Be 1–100 characters long
- Contain only **lowercase alphanumeric characters and hyphens**
- Not start or end with a hyphen
- Not contain consecutive hyphens (`--`)

Notable differences from GitHub's rules:

- **Periods are not allowed.** GitHub permits `my.project`; entangle does not. Use `my-project` instead.
- **Underscores are not allowed.** GitHub permits `my_project`; entangle does not. Use `my-project` instead.
- **Uppercase is normalized to lowercase** before validation, so `MyRepo` is accepted and stored as `myrepo`.

If your existing repository uses a name that doesn't pass these rules, use the `alias` argument to `entangle init` to give it a different name on the mirror forge:

```bash
entangle init my.existing.repo my-existing-repo
```

---

## Configuration

Config is stored in your platform config directory:

| Platform | Path |
|---|---|
| Linux / BSD | `~/.config/entangle/config.json` |
| macOS | `~/Library/Application Support/entangle/config.json` |
| Windows | `%APPDATA%\entangle\config.json` |

You can override the path for scripting or testing with the `ENTANGLE_CONFIG_PATH` environment variable.

---

## Known limitations

**SSH URL format only.** entangle constructs `git@host:user/repo` SSH URLs and assumes the default SSH port (22) and the standard `git` user. Users with non-standard SSH configurations (custom ports, `Host` aliases in `~/.ssh/config`) will need to edit the generated remote URLs manually after running `entangle init`.

**SSH error messages are matched in English.** When `entangle init` checks whether a remote is reachable, it classifies failures by inspecting the text output of the `ssh` subprocess. If your system's SSH binary has been localized, an authentication failure may be reported as a generic connectivity problem ("couldn't reach remote") rather than the more specific "SSH key not set up" message. The behaviour in that case is still safe — entangle will prompt you to accept the override — but the error description may be less helpful.

---

## License

MIT — see [LICENSE](LICENSE).
