# Changelog

All notable changes to gg are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and gg uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/ChenezDigital/gg/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/ChenezDigital/gg/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/ChenezDigital/gg/releases/tag/v0.1.0
