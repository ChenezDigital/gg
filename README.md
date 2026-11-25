# gg — Git account switcher

[![CI](https://github.com/ChenezDigital/gg/actions/workflows/ci.yml/badge.svg)](https://github.com/ChenezDigital/gg/actions/workflows/ci.yml)
[![Security](https://github.com/ChenezDigital/gg/actions/workflows/security.yml/badge.svg)](https://github.com/ChenezDigital/gg/actions/workflows/security.yml)
[![CodeQL](https://github.com/ChenezDigital/gg/actions/workflows/codeql.yml/badge.svg)](https://github.com/ChenezDigital/gg/actions/workflows/codeql.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/ChenezDigital/gg/badge)](https://scorecard.dev/viewer/?uri=github.com/ChenezDigital/gg)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

`gg` manages several Git accounts on one machine, such as a personal and a work GitHub account. Each **profile** pairs a commit identity (`user.name`/`user.email`) with an SSH host alias and key, so every repository commits *and* authenticates as the right account.

- `gg` opens a full-screen dashboard (arrow keys, two columns, one-key shortcuts).
- `gg clone owner/repo` clones through your default account's SSH key and configures the repository.
- `gg use work` switches the current repository to another account.
- `gg ssh …` generates keys, maintains `~/.ssh/config` aliases and tests which account a key logs in as.
- Every other command goes to Git: `gg commit`, `gg push`, `gg log`, …

## Install

**Linux and macOS** (x86_64 and Apple silicon / aarch64):

```bash
curl -fsSL https://raw.githubusercontent.com/ChenezDigital/gg/main/install.sh | sh
```

The script downloads the latest release for your platform, verifies its SHA-256 checksum and installs `gg` to `~/.local/bin`.

**Manual download:** grab `gg-<target>.tar.gz` from [Releases](https://github.com/ChenezDigital/gg/releases) (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`) and put `gg` on your `PATH`.

**From source** (Rust 1.88+):

```bash
cargo install --locked --git https://github.com/ChenezDigital/gg
```

Windows is not supported yet. See [docs/installation.md](docs/installation.md) for pinned versions, checksum and provenance verification, shell completions and uninstalling.

## Quick start

```bash
gg setup                    # imports ~/.ssh/config hosts, creates profiles, keys and SSH aliases
gg ssh test                 # checks which account each key authenticates as
gg clone octocat/project      # clones as the default profile
cd project && gg use work   # or switches this repository to another account
gg whoami                   # shows identity, remotes and accounts
gg                          # opens the dashboard
```

`gg` never changes your global Git config.

## Documentation

| Document | Contents |
|---|---|
| [Installation](docs/installation.md) | Install script options, manual install, verification, completions, uninstall |
| [Usage](docs/usage.md) | Every command, the dashboard and its keys, everyday workflows |
| [Configuration](docs/configuration.md) | Config file format, environment variables, Git and SSH files `gg` touches |
| [Architecture](docs/architecture.md) | How `gg` works internally: modules, URL rewriting, remote repair, terminal handling |
| [Development](docs/development.md) | Building, testing, CI, security scanning and the release process |

## License

[MIT](LICENSE)
