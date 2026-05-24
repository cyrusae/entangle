# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Added
- Add setup integration tests and ENTANGLE_CONFIG_PATH env var override (#6)
- Implement entangle shove command (Step 11) (#14)
- Wire remote validation (Step 7) into entangle init (#12)
- Step 9: entangle init — remote inspection and overwrite prompt (#10)
- Step 8: entangle init — git detection and local setup (#9)
- Step 7: Remote validation (remote.rs) (#8)
- Step 6: URL construction (urls.rs) (#7)
- Add setup integration tests and ENTANGLE_CONFIG_PATH env var override (#6)
- Implement entangle setup with interactive prompts, pre-existing config detection, and Ctrl+C safety (#5)
- Implement entangle set command with validation, partial config updates, and confirmation output (#4)
- Add input sanitization and validation module with exhaustive tests (#3)
- Implement Config load/save with full error variants and tests (#2)
- Add project skeleton with all four subcommands stubbed and Cargo dependencies (#1)

### Fixed
- Fix push URL order: mirror (non-default) first, origin (default) last (#11)

### Changed
- Draft README with install, usage, caveats (#20)
- Add Step 14 to PLAN.md: release packaging, crates.io, CI/CD refinement (#18)
- Add GitHub Actions CI workflow (cargo test + clippy + fmt on ubuntu/macos/windows) (#17)
- Step 13: hardening and edge cases (TESTING.md coverage) (#16)
- Clarify entangle shove hint text: all branches/tags, first-setup nudge (#13)
