``` 
User runs: entangle init my-new-project --create
  │
  ├──► 1. Check for GH_TOKEN -> Hits GitHub API -> Repo created on GitHub!
  ├──► 2. Check for Tangled Session -> Hits PDS API -> Repo created on Tangled!
  └──► 3. Automatically runs git remote set-url and populates both SSH push targets.
```

This would be very cool and sexy of me.

Needs:
- Config to track the tokens -> Use `keyring` and OS keyring instead of tracking tokens myself? I'm not in the worst company (npm...) if I plaintext the auth info, but
- Call com.atproto.repo.createRecord to create a repo on the Tangled namespace: do I need to know their knot to create that record?
- GitHub has an API for repo creation

Major feature with breaking changes (changes config one way or another, radically extends scope of how much git mutation can happen)

TODO:
- [ ] Exact location in the logic of create happening
- [ ] Debate `--create` flag on `init` or separate command (probably flag or flag-and-alias)
- [ ] Tradeoffs of using keychain versus tokens/app passwords--sexy option of doing both by default
- [ ] Auth logic: keychain
- [ ] Auth logic: `.tokens.json` additional config file
- [ ] Selection flow

```
┌────────────────────────┐
               │ 1. Check Environment   │ ──► Found? Use it.
               └────────────────────────┘
                           │
                     Not set / Empty
                           ▼
               ┌────────────────────────┐
               │ 2. Query OS Keyring    │ ──► Found? Use it.
               └────────────────────────┘
                           │
                 Missing Backend/Entry
                           ▼
               ┌────────────────────────┐
               │ 3. Read tokens.json    │ ──► Found? Use it.
               └────────────────────────┘
                           │
                      Not present
                           ▼
               ┌────────────────────────┐
               │ 4. Return Auth Error   │ ──► Prompt `entangle setup`
               └────────────────────────┘
```

Can enforce file permissions for the new file on unixalikes:
```
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;

// Create tokens.json so ONLY the current user can read/write it (chmod 600)
let mut options = OpenOptions::new();
options.write(true).create(true);
#[cfg(unix)]
options.mode(0o600);
```

Conceptual sketch:

> Environment Variables: Always check ENTANGLE_GH_TOKEN or ENTANGLE_TNGL_TOKEN first. This is universally standard for scripts, automation, and ephemeral environments.
> System Keyring: If env vars aren't set, try to fetch the secret securely from Apple Keychain, Windows Credential Manager, or Linux Secret Service via the keyring crate.
> Plain-Text Config File Fallback: If the keyring returns a specific error indicating that no system credential store is available (e.g., a headless Linux box without D-Bus configured), gracefully look for a plain text token file or field in config.json.

(**Note:** Try to set the env vars myself or?)

```
use keyring::{Entry, Error as KeyringError};
use std::env;

struct Credentials {
    github_token: String,
}

impl Credentials {
    pub fn get_github_token(username: &str, config_fallback: &Config) -> Result<String, String> {
        // 1. Check Env Vars First (Best practice for automation)
        if let Ok(token) = env::var("ENTANGLE_GH_TOKEN") {
            return Ok(token);
        }

        // 2. Try the OS Keyring
        let entry = Entry::new("entangle-mirror:github", username)
            .map_err(|e| format!("Failed to initialize keyring entry: {e}"))?;

        match entry.get_password() {
            Ok(token) => Ok(token),
            
            // 3. Handle Keyring Fallbacks
            Err(KeyringError::NoWorkspace) | Err(KeyringError::NoBackend) => {
                // The OS doesn't have a keyring daemon running (e.g., raw Docker/CI/headless Linux)
                if let Some(token) = &config_fallback.gh_token {
                    Ok(token.clone())
                } else {
                    Err("No keyring available, and no fallback token found in config.json".into())
                }
            }
            Err(KeyringError::NoEntry) => {
                // Keyring exists, but this user hasn't authenticated yet
                Err("No token found. Please run `entangle setup` to authenticate.".into())
            }
            Err(other) => Err(format!("Secure storage error: {other}")),
        }
    }
}
```

> During entangle setup, if the keyring fails with a NoBackend error, print a brief, helpful warning to the terminal:
> ⚠️ No system keyring detected. Saving token in plain text to .tokens.json.
> This keeps the user informed and gives them the option to use an environment variable instead if they prefer.

---

**Breaking changes include:** 

- `entangle setup`/`entangle set` gains new criteria
- introduction of second config file (do want it to be separate from the existing config file for privacy and backwards-compatibility) 
- new class of possible network errors and partial failure states