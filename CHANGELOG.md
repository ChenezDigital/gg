# Changelog

All notable changes to gg are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and gg uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-11-26

### Added

- GitHub CLI (`gh`) integration. `gg setup`, the dashboard's *Add profile* and `gg profile add` offer `gh`'s logged-in accounts as new profiles, pre-filled with the account's username, name and public (or no-reply) email. Setup also asks which `gh` account an imported `~/.ssh/config` entry signs in as.
- New keys (setup, `gg ssh keygen`, dashboard *Generate key* and *Public key*) can be added to the profile's GitHub account with `gh`, using that account's own login. If `gh` lacks the `admin:public_key` permission, gg offers the sign-in that grants it and switches `gh`'s active account back afterwards.
- After a clone, gg asks whether to switch to the new repository. In the dashboard, the Git column becomes available right away; `gg clone` (or leaving the dashboard after switching) opens a shell in the repository.

### Changed

- `~/.ssh/config` is kept in sync automatically: every command and every profile change adds missing aliases and drops stale ones (with a backup), so the dashboard's *Sync config* item is gone and setup no longer asks. `gg ssh sync` remains for the detailed report. Its messages now go to stderr.
- Prompts use the same ✓ / ✗ symbols as the rest of gg, and the dashboard shows changed files as `±` instead of `✎`.
- The "Press any key to return to gg…" line is ended, so the shell prompt after quitting starts on its own line.

## [0.1.1] - 2026-11-26

### Fixed

- Windows: the dashboard no longer shows boxes (`⍰`) or misaligned profile rows in the classic console window used by Windows PowerShell 5.1 and `cmd`. gg now uses symbols every Windows console font has (`*`, `√`, `×`, `►`) there; Windows Terminal, VS Code and ConEmu keep the full set. `GG_GLYPHS=plain|fancy` overrides the choice.
- Windows: Ctrl-C now cancels prompts opened from the dashboard (setup wizard, profile questions such as "Add another profile?") instead of leaving you stuck until you answer.
- Ctrl-C at a prompt now always exits with status 130; it sometimes printed `gg: IO error: read interrupted` and exited with 1.

## [0.1.0] - 2026-11-25

First release.

### Added

- Per-repository Git accounts: commit identity plus SSH host alias and key per profile.
- Interactive dashboard, setup wizard and Git shortcuts (`gg clone`, `gg commit`, `gg push`, ...).
- `~/.ssh/config` management, key generation and `gg ssh test`.
- `gg self-update` and a daily new-release notice.
- Linux (musl), macOS and Windows (x64 and ARM64) builds with checksums and build provenance attestations.
- `install.sh` and `install.ps1` (Windows PowerShell 5.1 and PowerShell 7).

[Unreleased]: https://github.com/ChenezDigital/gg/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/ChenezDigital/gg/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/ChenezDigital/gg/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/ChenezDigital/gg/releases/tag/v0.1.0
