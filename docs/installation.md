# Installation

`gg` is a single static binary. Releases are built for:

| Platform | Target triple | Archive |
|---|---|---|
| Linux x86_64 (amd64) | `x86_64-unknown-linux-musl` | `gg-x86_64-unknown-linux-musl.tar.gz` |
| Linux aarch64 (arm64) | `aarch64-unknown-linux-musl` | `gg-aarch64-unknown-linux-musl.tar.gz` |
| macOS Intel | `x86_64-apple-darwin` | `gg-x86_64-apple-darwin.tar.gz` |
| macOS Apple silicon | `aarch64-apple-darwin` | `gg-aarch64-apple-darwin.tar.gz` |
| Windows x64 | `x86_64-pc-windows-msvc` | `gg-x86_64-pc-windows-msvc.zip` |
| Windows ARM64 | `aarch64-pc-windows-msvc` | `gg-aarch64-pc-windows-msvc.zip` |

Linux binaries are statically linked against musl, so they run on any distribution (glibc or musl, including immutable systems such as Fedora Atomic, Bazzite and NixOS) without extra libraries.

Windows binaries run on Windows 10 1809 / Windows Server 2019 and later. The C runtime is linked statically, so they need no Visual C++ Redistributable.

## Runtime requirements

- `git`
- OpenSSH (`ssh`, `ssh-keygen`), only for the `gg ssh …` commands and key generation
- A terminal with UTF-8 support for the dashboard

On macOS, Git and OpenSSH come with the Xcode Command Line Tools (`xcode-select --install`) or Homebrew.

On Windows, install [Git for Windows](https://git-scm.com/download/win) (or `winget install Git.Git`). OpenSSH Client is built into Windows 10 1809 and later (Settings → System → Optional features). `gg` works in Windows PowerShell 5.1, PowerShell 7, Command Prompt, Windows Terminal and Git Bash; the dashboard looks best in Windows Terminal.

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

## Windows install script

Works in Windows PowerShell 5.1 (built into Windows) and PowerShell 7+:

```powershell
irm https://raw.githubusercontent.com/ChenezDigital/gg/main/install.ps1 | iex
```

The script:

1. Picks the build for the machine's native CPU (x64 or ARM64), even from an emulated or 32-bit PowerShell.
2. Downloads `gg-<target>.zip` and its `.sha256` from GitHub Releases over HTTPS (it turns on TLS 1.2 for Windows PowerShell 5.1).
3. Verifies the checksum with `Get-FileHash`, and aborts on mismatch.
4. Installs `gg.exe` and `completions\` to `%LOCALAPPDATA%\Programs\gg` and adds that folder to your **user** `PATH` (no administrator rights needed). Open a new terminal afterwards.

It runs in its own scope, so it changes no settings in your session apart from `PATH`. It accepts the same `GG_VERSION`, `GG_INSTALL_DIR`, `GG_REPO` and `GG_BASE_URL` variables as `install.sh`, plus `GG_NO_MODIFY_PATH=1`:

```powershell
$env:GG_VERSION = 'v0.1.0'; irm https://raw.githubusercontent.com/ChenezDigital/gg/main/install.ps1 | iex
```

To read it first, download it and run it as a file (`-ExecutionPolicy Bypass` applies to this run only):

```powershell
irm https://raw.githubusercontent.com/ChenezDigital/gg/main/install.ps1 -OutFile install.ps1
notepad install.ps1
powershell -ExecutionPolicy Bypass -File install.ps1
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

On Windows:

```powershell
$target = 'x86_64-pc-windows-msvc'   # or aarch64-pc-windows-msvc
$base = "https://github.com/ChenezDigital/gg/releases/latest/download"
irm "$base/gg-$target.zip" -OutFile "gg-$target.zip"
irm "$base/gg-$target.zip.sha256" -OutFile "gg-$target.zip.sha256"
(Get-FileHash "gg-$target.zip" -Algorithm SHA256).Hash   # compare with the .sha256 file
Expand-Archive "gg-$target.zip" -DestinationPath .
# then copy gg-$target\gg.exe to a folder on your PATH
```

Each archive contains:

```
gg-<target>/
├── gg                 # the binary (gg.exe on Windows)
├── LICENSE
├── README.md
└── completions/
    ├── gg.bash
    ├── _gg            # zsh
    ├── gg.fish
    └── gg.ps1         # PowerShell
```

## Verifying a download

Every release has:

- **Per-archive checksums** (`*.tar.gz.sha256`, `*.zip.sha256`) and a combined `SHA256SUMS`.
- **Build-provenance attestations**, signed through Sigstore by GitHub Actions. They prove the archive was built by this repository's `release.yml` workflow from the tagged commit. Each one is also published next to the archive as `<archive>.sigstore.json`.

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

PowerShell (5.1 and 7+): add this line to your profile (`notepad $PROFILE`; create the file if it does not exist):

```powershell
gg completions powershell | Out-String | Invoke-Expression
```

`elvish` is also accepted.

## Updating

```bash
gg self-update            # download and install the latest release
gg self-update --check    # only report whether one exists
```

`self-update` asks the GitHub API for the latest release, downloads the archive for your platform and its `.sha256` over HTTPS, and verifies the checksum. It then unpacks the archive and checks that the new binary reports the expected version. Only then does it replace the running binary, with an atomic rename in the same directory. If any step fails, the installed binary is left unchanged. It needs `curl` or `wget` and `tar`; Windows 10 and later include both (`curl.exe`, `tar.exe`). If `gg` is in a directory you can't write to (for example `/usr/local/bin`), run it with `sudo` (on Windows, from an elevated terminal).

Windows does not allow overwriting a running program, so on Windows the old `gg.exe` is renamed to `gg.exe.old` first and deleted the next time `gg` runs.

Binaries that another tool owns are not replaced; `gg` tells you the command to use instead:

| Installed with | Update with |
|---|---|
| `install.sh`, `install.ps1` or a manual download | `gg self-update`, or run the install script again |
| `cargo install` (`~/.cargo/bin`) | `cargo install --locked --force --git https://github.com/ChenezDigital/gg` |
| Homebrew or Nix | Your package manager |

### Update notice

Once a day, `gg` checks for a newer release in a background process, so commands never wait for the network. When one exists, it prints a line like this after the command:

```
! gg 0.2.0 is available (you have 0.1.0). Update with: gg self-update
```

The check only runs when the output is a terminal, never in CI (`CI` is set), and never for `gg self-update` or `gg completions`. It sends one unauthenticated request to `api.github.com` and nothing else. The result is cached in `~/.cache/gg/latest-version` (`~/Library/Caches/gg/` on macOS, `%LOCALAPPDATA%\gg\` on Windows). To turn it off, set `GG_NO_UPDATE_CHECK=1` or add `update_check = false` to `config.toml`.

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

On Windows, delete `%LOCALAPPDATA%\Programs\gg`, `%USERPROFILE%\.config\gg` and `%LOCALAPPDATA%\gg`, and remove the first folder from your user `PATH` (Settings → System → About → Advanced system settings → Environment Variables).

None of these need removing for Git to keep working. Delete the managed block if you no longer want the aliases, but repositories cloned through an alias (`git@github-work:…`) rely on it.
