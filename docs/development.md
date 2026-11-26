# Development

- [Setup](#setup)
- [Tests](#tests)
- [Fuzzing](#fuzzing)
- [Continuous integration](#continuous-integration)
- [Security scanning](#security-scanning)
- [Releasing](#releasing)
- [Repository settings](#repository-settings)

## Setup

Requirements: Rust **1.88** or newer (the MSRV, set by `rust-version` in `Cargo.toml`), plus `git` and OpenSSH for the tests.

```bash
git clone git@github-personal:ChenezDigital/gg.git && cd gg
cargo build
cargo run -- whoami              # run without installing
cargo install --path . --locked  # install to ~/.cargo/bin
```

Before pushing, run what CI runs:

```bash
cargo fmt --all
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo deny check                 # cargo install --locked cargo-deny
```

Optional checks for workflow and installer changes:

```bash
actionlint                       # workflow syntax and expressions
zizmor .github/workflows         # workflow security (pipx install zizmor)
shellcheck --shell=sh install.sh
cargo clippy --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings   # Windows code paths, from Linux/macOS (rustup target add x86_64-pc-windows-msvc)
```

`install.ps1` must stay compatible with Windows PowerShell 5.1: no `??`, `?:`, `&&`/`||` pipeline chains or `-Parallel`; always pass `-UseBasicParsing`; keep everything inside the `& { }` block and use `throw`, not `exit`, because `irm | iex` runs in the caller's session. Keep the file ASCII-only (5.1 reads BOM-less scripts in the ANSI code page). CI runs it under both 5.1 and 7 (`install.ps1 (PowerShell …)` jobs). To check syntax locally with PowerShell 7 and PSScriptAnalyzer:

```powershell
Invoke-ScriptAnalyzer install.ps1 -Settings @{ Rules = @{ PSUseCompatibleSyntax = @{ Enable = $true; TargetVersions = @('5.1', '7.0') } } }
```

Code style: `rustfmt` defaults with `max_width = 120` (`rustfmt.toml`), clippy clean, and comments only where intent isn't obvious.

## Tests

- **Unit tests** sit next to the code (`#[cfg(test)]`): URL parsing and rewriting, clone argument parsing, ssh config parsing and the managed block (idempotency, preserving user entries, mismatch detection), and config validation.
- **Integration tests** (`tests/cli.rs`) run the compiled binary against real temporary Git repositories. Each test has its own environment:

  | Variable | Value |
  |---|---|
  | `HOME`, `XDG_CONFIG_HOME` | Temporary directory |
  | `GIT_CONFIG_GLOBAL` | A file in that directory |
  | `GIT_CONFIG_NOSYSTEM` | `1` |
  | `GIT_SSH_COMMAND` | `false` (so nothing can reach the network) |

  This means the tests never read or change your own `~/.gitconfig`, `~/.ssh` or gg config. They cover profiles and the default, `ssh sync` preserving user entries, `use`, passthrough and commit, `clone` from a local repository, `remote add/edit/delete`, and the push repair.

The dashboard and prompts need a terminal and are tested by hand. To check terminal restoration, start `gg`, choose **Generate key**, press `Ctrl-C` at the passphrase prompt, quit, and confirm that typing is still echoed.

## Fuzzing

`fuzz/` is a [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) (libFuzzer) crate. It calls the library in `src/lib.rs` and needs a nightly toolchain:

| Target | Checks |
|---|---|
| `remote_url` | `parse_remote`/`rewrite_url` never panic; a rewritten URL parses to the profile alias with the same path, and rewriting it again gives the same URL |
| `ssh_config` | `parse_hosts`/`plan_sync` never panic; after a sync every alias exists, a second sync changes nothing, and none of your lines are lost |
| `clone_args` | `parse_clone_args` never panics and returns 1–2 positional arguments with valid indexes |

```bash
cargo install --locked cargo-fuzz
cd fuzz
cargo +nightly fuzz run -O ssh_config -- -max_total_time=120
cargo +nightly fuzz run -O ssh_config artifacts/ssh_config/crash-…   # replay a crash
```

Turn each crash into a unit test next to the fixed code.

## Continuous integration

All workflows are in `.github/workflows/`.

| Workflow | Trigger | Jobs |
|---|---|---|
| `ci.yml` | Push to `main`, pull requests | **Lint** (rustfmt, clippy `-D warnings`) · **Test** on Linux x86_64/arm64 and macOS arm64/x86_64 (`cargo test --locked`, `--version` and completion smoke tests) · **MSRV** (`cargo check` on 1.88) · **Installer** (shellcheck) |
| `security.yml` | Push, pull requests, Mondays, manual | cargo-deny, OSV-Scanner, gitleaks, zizmor, dependency review |
| `fuzz.yml` | Push to `main`, pull requests, Mondays, manual | Each fuzz target for 60 s (10 min on the weekly run); crash inputs are uploaded as artifacts |
| `codeql.yml` | Push, pull requests, Wednesdays | CodeQL for Rust and GitHub Actions |
| `scorecard.yml` | Push to `main`, Tuesdays, branch-protection changes | OpenSSF Scorecard |
| `release.yml` | `v*` tags | Builds and publishes releases (see [Releasing](#releasing)) |

Conventions in all workflows:

- **Least privilege.** Each workflow starts with `permissions: {}`, and each job asks only for what it needs (`contents: read`, `security-events: write` for SARIF, `id-token`/`attestations: write` for provenance, `contents: write` only in the release publish job).
- **Pinned actions.** Every `uses:` names a full commit SHA with the version in a comment. Dependabot updates them weekly.
- **No credential persistence.** `actions/checkout` uses `persist-credentials: false`.
- **No template injection.** Values such as `github.ref_name` are passed through `env:` and never put straight into `run:` scripts (checked by zizmor).
- **No caches in the release workflow**, to rule out cache poisoning of release builds.
- `cargo … --locked` everywhere, so CI uses exactly the committed `Cargo.lock`.

## Security scanning

These are the tools widely used by open-source Rust projects. Results that support SARIF appear under **Security → Code scanning** on GitHub. Others fail the check, with details in the job log.

| Tool | Detects | Where it runs | Config |
|---|---|---|---|
| **cargo-deny** (advisories) | Crates with [RustSec](https://rustsec.org) advisories (vulnerable, unsound), yanked versions, unmaintained direct dependencies | `security.yml` | `deny.toml` `[advisories]` |
| **cargo-deny** (bans, licenses, sources) | Licenses not on the allow list, wildcard version requirements, crates from outside crates.io (for example git dependencies or other registries), duplicate versions (warning) | `security.yml` | `deny.toml` |
| **OSV-Scanner** (Google) | Known vulnerabilities in `Cargo.lock` from [OSV.dev](https://osv.dev), which combines RustSec, GHSA and others | `security.yml` → Code scanning | |
| **gitleaks** | Committed secrets (tokens, private keys, passwords) in the **full Git history** | `security.yml` (the CLI, pinned by version and SHA-256; the gitleaks Action needs a paid license for organizations) | Add `.gitleaks.toml` to allow false positives |
| **zizmor** | Workflow weaknesses: template injection, excessive permissions, unpinned actions, credential persistence, cache poisoning, dangerous triggers | `security.yml` → Code scanning | |
| **dependency-review** | Pull requests that add a dependency with a known vulnerability (moderate or worse) or a disallowed license | `security.yml` (pull requests only) | Inline in the workflow |
| **CodeQL** | Static analysis (`security-extended` queries) of Rust code and of workflow files | `codeql.yml` → Code scanning | |
| **OpenSSF Scorecard** | Supply-chain practices: pinning, branch protection, token permissions, signed releases, maintenance, … Publishes a public score (badge in README) | `scorecard.yml` → Code scanning | |
| **Dependabot** | Outdated crates and actions; opens grouped weekly PRs after a 7-day cooldown, so brand-new (possibly malicious) releases aren't adopted right away | GitHub | `.github/dependabot.yml` |

The weekly scheduled runs catch advisories published after the code last changed.

Dependabot does not update the gitleaks CLI or cargo-fuzz (`CARGO_FUZZ_VERSION` in `fuzz.yml`). To update it, change `GITLEAKS_VERSION` and `GITLEAKS_SHA256` in `security.yml`, using the value from the release's `gitleaks_<version>_checksums.txt`.

### Handling findings

- **RustSec/OSV advisory**: run `cargo update -p <crate>`. If no fixed version exists and the issue doesn't affect `gg`, add an ignore with a reason to `deny.toml` (`[advisories] ignore = [{ id = "RUSTSEC-…", reason = "…" }]`).
- **New license**: check that it is permissive and compatible with MIT, then add it to `deny.toml` and to the `allow-licenses` list in `security.yml`.
- **gitleaks hit**: rotate the secret first, then remove it from history. Deleting the file in a new commit is not enough.
- **zizmor/CodeQL**: fix the issue, or dismiss it in the Security tab with a reason.

## Releasing

Releases are built entirely by GitHub Actions from a tag. Versions follow [Semantic Versioning](https://semver.org/):

| Change | Bump | Example |
|---|---|---|
| Bug fixes only, no behavior anyone relies on changes | patch | 0.1.0 → 0.1.1 |
| New commands, options or platforms; backward compatible | minor | 0.1.1 → 0.2.0 |
| Breaking change (removed or renamed commands/options, incompatible `config.toml`) | major (minor while 0.x) | 0.9.0 → 0.10.0 / 1.4.0 → 2.0.0 |

Published tags are never moved or reused; if a release is wrong, ship the next patch version.

1. On a branch, update `version` in `Cargo.toml` and run `cargo check` so `Cargo.lock` follows. In `CHANGELOG.md`, move the `Unreleased` entries under a new `## [x.y.z] - YYYY-MM-DD` heading and update the compare links at the bottom.
2. Open a pull request ("Release vx.y.z") and merge it once CI passes.
3. Tag the merge commit on `main` and push the tag:

   ```bash
   git switch main && git pull --ff-only
   git tag -a v0.2.0 -m "gg v0.2.0"
   git push origin v0.2.0
   ```

4. `release.yml` then:
   1. **Check version**: fails if the tag is not `v` + the `Cargo.toml` version.
   2. **Build**: builds each target natively on its own runner, with no cross-compiling:

      | Target | Runner | Notes |
      |---|---|---|
      | `x86_64-unknown-linux-musl` | `ubuntu-24.04` | Static, any distribution |
      | `aarch64-unknown-linux-musl` | `ubuntu-24.04-arm` | Static, any distribution (Raspberry Pi 4/5, Graviton, …) |
      | `x86_64-apple-darwin` | `macos-15-intel` | Intel Macs |
      | `aarch64-apple-darwin` | `macos-15` | Apple silicon |
      | `x86_64-pc-windows-msvc` | `windows-2025` | Static C runtime (`.cargo/config.toml`) |
      | `aarch64-pc-windows-msvc` | `windows-11-arm` | Static C runtime |

      Each job runs the test suite for its target, builds with `--release --locked` (LTO, stripped), and packages `gg-<target>.tar.gz` (`.zip` for Windows, made with Windows' own `tar.exe`) containing the binary, LICENSE, README and completions, with a `.sha256`. It also creates a signed **build provenance attestation** (Sigstore, SLSA) for the archive and publishes its bundle as `<archive>.sigstore.json`.
   3. **Publish**: collects the archives, writes `SHA256SUMS`, and creates the GitHub release with generated notes. Tags with a hyphen (`v0.2.0-rc.1`) are marked as pre-releases.

Asset names contain no version, so `releases/latest/download/gg-<target>.tar.gz` (or `.zip`) always points at the newest release; `install.sh` and `install.ps1` rely on this.

If a release job fails before anything is published (no GitHub release exists yet), fix the problem, delete the tag (`git push --delete origin v0.2.0 && git tag -d v0.2.0`) and tag again. Once a release is published, leave its tag alone and release the next patch version instead.

### macOS signing

The macOS binaries are not notarized. Files downloaded with a browser get the quarantine attribute; `install.sh` uses curl, so they don't. See [installation](installation.md#manual-install). Notarization needs an Apple Developer account and can be added to the build job later.

### Windows signing

The Windows binaries are not Authenticode-signed, so SmartScreen may warn when `gg.exe` is downloaded with a browser ("Unblock" in the file's Properties, or use `install.ps1`, whose downloads are not marked). Signing needs a code-signing certificate and can be added to the build job later.

## Repository settings

Recommended once the repository is public:

- **Settings → Code security**: enable Dependabot alerts and security updates, secret scanning with push protection, and **private vulnerability reporting** (used by `SECURITY.md`).
- **Settings → Rules → Rulesets**: protect `main` with a ruleset rather than classic branch protection, because Scorecard can read rulesets without an admin token. Block force pushes and deletion, require a pull request with one approval, and require the `CI`, `Security`, `CodeQL` and `Fuzz` checks (up to date with `main`).
- **Settings → Actions → General**: set workflow permissions to *Read repository contents*, and require approval for workflows from first-time contributors.
- **Settings → Tags** (rulesets): restrict who can create `v*` tags, since a tag triggers a release.
