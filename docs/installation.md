# Installation

`gg` is a single static binary. Releases are built for:

| Platform | Target triple | Archive |
|---|---|---|
| Linux x86_64 (amd64) | `x86_64-unknown-linux-musl` | `gg-x86_64-unknown-linux-musl.tar.gz` |
| Linux aarch64 (arm64) | `aarch64-unknown-linux-musl` | `gg-aarch64-unknown-linux-musl.tar.gz` |
| macOS Intel | `x86_64-apple-darwin` | `gg-x86_64-apple-darwin.tar.gz` |
| macOS Apple silicon | `aarch64-apple-darwin` | `gg-aarch64-apple-darwin.tar.gz` |

Linux binaries are statically linked against musl, so they run on any distribution (glibc or musl, including immutable systems such as Fedora Atomic, Bazzite and NixOS) without extra libraries.

Windows is not supported yet.

## Runtime requirements

- `git`
- OpenSSH (`ssh`, `ssh-keygen`), only for the `gg ssh …` commands and key generation
- A terminal with UTF-8 support for the dashboard

On macOS, Git and OpenSSH come with the Xcode Command Line Tools (`xcode-select --install`) or Homebrew.

## Install script

```bash
curl -fsSL https://raw.githubusercontent.com/ChenezDigital/gg/main/install.sh | sh
```

The script:

1. Detects the OS and CPU (`uname`). On a Rosetta shell on Apple silicon, it picks the native `aarch64` build.
2. Downloads `gg-<target>.tar.gz` and `gg-<target>.tar.gz.sha256` from GitHub Releases over HTTPS (TLS 1.2+).
3. Verifies the checksum with `sha256sum` or `shasum -a 256`, and aborts on mismatch.
4. Installs the binary with mode 755 and tells you if the directory is not on your `PATH`.

It needs `curl` or `wget`, `tar`, and `sha256sum` or `shasum`, and runs with any POSIX `sh` (dash, bash, zsh, busybox).

| Variable | Default | Purpose |
|---|---|---|
| `GG_VERSION` | `latest` | Install a specific tag, e.g. `v0.1.0` |
| `GG_INSTALL_DIR` | `~/.local/bin` | Target directory |
| `GG_REPO` | `ChenezDigital/gg` | Download from a fork |
| `GG_BASE_URL` | unset | Download from a mirror; overrides `GG_REPO`/`GG_VERSION`. Only HTTPS, or `http://localhost` / `127.0.0.1` |

Examples:

```bash
curl -fsSL https://raw.githubusercontent.com/ChenezDigital/gg/main/install.sh | GG_VERSION=v0.1.0 sh
curl -fsSL https://raw.githubusercontent.com/ChenezDigital/gg/main/install.sh | GG_INSTALL_DIR=/usr/local/bin sudo -E sh
```

If you'd rather read the script before running it:

```bash
curl -fsSLO https://raw.githubusercontent.com/ChenezDigital/gg/main/install.sh
less install.sh
sh install.sh
```

## Manual install

```bash
target=aarch64-apple-darwin   # pick one from the table above
curl -fsSLO https://github.com/ChenezDigital/gg/releases/latest/download/gg-$target.tar.gz
curl -fsSLO https://github.com/ChenezDigital/gg/releases/latest/download/gg-$target.tar.gz.sha256
shasum -a 256 -c gg-$target.tar.gz.sha256      # or: sha256sum -c …
tar -xzf gg-$target.tar.gz
install -m 755 gg-$target/gg ~/.local/bin/gg
```

On macOS, if you downloaded the archive with a browser, Gatekeeper blocks the unsigned binary. Remove the quarantine flag:

```bash
xattr -d com.apple.quarantine ~/.local/bin/gg
```

Each archive contains:

```
gg-<target>/
├── gg                 # the binary
├── LICENSE
├── README.md
└── completions/
    ├── gg.bash
    ├── _gg            # zsh
    └── gg.fish
```

## Verifying a download

Every release has:

- **Per-archive checksums** (`*.tar.gz.sha256`) and a combined `SHA256SUMS`.
- **Build-provenance attestations**, signed through Sigstore by GitHub Actions. They prove the archive was built by this repository's `release.yml` workflow from the tagged commit. Each one is also published as `gg-<target>.tar.gz.sigstore.json`.

Verify provenance with the [GitHub CLI](https://cli.github.com/):

```bash
gh attestation verify gg-aarch64-apple-darwin.tar.gz --repo ChenezDigital/gg
# or offline, with the published bundle:
gh attestation verify gg-aarch64-apple-darwin.tar.gz --repo ChenezDigital/gg \
  --bundle gg-aarch64-apple-darwin.tar.gz.sigstore.json
```

## From source

Requires Rust 1.88 or newer ([rustup](https://rustup.rs/)).

```bash
cargo install --locked --git https://github.com/ChenezDigital/gg            # latest main
cargo install --locked --git https://github.com/ChenezDigital/gg --tag v0.1.0
```

Or from a checkout:

```bash
git clone https://github.com/ChenezDigital/gg && cd gg
cargo build --release --locked
install -m 755 target/release/gg ~/.local/bin/gg
```

## Shell completions

Release archives include completion files. You can also generate them with `gg completions <shell>`:

```bash
# bash
mkdir -p ~/.local/share/bash-completion/completions
gg completions bash > ~/.local/share/bash-completion/completions/gg

# zsh (make sure ~/.zfunc is in fpath before compinit)
mkdir -p ~/.zfunc && gg completions zsh > ~/.zfunc/_gg

# fish
gg completions fish > ~/.config/fish/completions/gg.fish
```

`elvish` and `powershell` are also accepted.

## Updating

```bash
gg self-update            # download and install the latest release
gg self-update --check    # only report whether one exists
```

`self-update` asks the GitHub API for the latest release, downloads `gg-<target>.tar.gz` and its `.sha256` over HTTPS, and verifies the checksum. It then unpacks the archive and checks that the new binary reports the expected version. Only then does it replace the running binary, with an atomic rename in the same directory. If any step fails, the installed binary is left unchanged. Like `install.sh`, it needs `curl` or `wget`, `sha256sum` or `shasum`, and `tar`. If `gg` is in a directory you can't write to (for example `/usr/local/bin`), run it with `sudo`.

Binaries that another tool owns are not replaced; `gg` tells you the command to use instead:

| Installed with | Update with |
|---|---|
| `install.sh` or a manual download | `gg self-update`, or run the install script again |
| `cargo install` (`~/.cargo/bin`) | `cargo install --locked --force --git https://github.com/ChenezDigital/gg` |
| Homebrew or Nix | Your package manager |

### Update notice

Once a day, `gg` checks for a newer release in a background process, so commands never wait for the network. When one exists, it prints a line like this after the command:

```
! gg 0.2.0 is available (you have 0.1.0). Update with: gg self-update
```

The check only runs when the output is a terminal, never in CI (`CI` is set), and never for `gg self-update` or `gg completions`. It sends one unauthenticated request to `api.github.com` and nothing else. The result is cached in `~/.cache/gg/latest-version` (`~/Library/Caches/gg/` on macOS). To turn it off, set `GG_NO_UPDATE_CHECK=1` or add `update_check = false` to `config.toml`.

You can also get release emails by choosing **Watch → Custom → Releases** on the GitHub repository.

The configuration in `~/.config/gg` is kept across updates.

## Uninstalling

```bash
rm ~/.local/bin/gg
rm -r ~/.config/gg                     # profiles (optional)
rm -r ~/.cache/gg                      # update-check cache (~/Library/Caches/gg on macOS)
```

`gg` may also have written:

- a managed block in `~/.ssh/config` between `# >>> gg managed hosts` and `# <<< gg managed hosts <<<`, with a backup in `~/.ssh/config.gg-backup`;
- keys you generated with `gg ssh keygen`;
- `user.name`, `user.email` and `gg.profile` in individual repositories' `.git/config`.

None of these need removing for Git to keep working. Delete the managed block if you no longer want the aliases, but repositories cloned through an alias (`git@github-work:…`) rely on it.
