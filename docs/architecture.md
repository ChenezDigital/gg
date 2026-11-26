# Architecture

`gg` is a single Rust binary (about 3,000 lines) that runs the system `git`, `ssh` and `ssh-keygen` instead of reimplementing them. Everything it changes is ordinary Git or OpenSSH configuration, so removing `gg` leaves a working setup behind.

## Modules

```
src/
├── main.rs         CLI definition (clap derive), dispatch, exit codes
├── lib.rs          Exposes the modules below to main.rs and the fuzz targets (not a stable API)
├── config.rs       Profile/Config types, config.toml location, load/save, validation
├── git.rs          Remote URL parsing and rewriting, git process helpers
├── ssh.rs          ~/.ssh/config parsing, managed block, keygen, connection test
├── commands.rs     Command implementations (whoami, use, clone, remote, profile, passthrough)
├── interactive.rs  Prompts (dialoguer), setup wizard, dashboard actions, terminal state
├── tui.rs          Dashboard (ratatui + crossterm)
├── ui.rs           Shared colors and message formatting (console)
└── update.rs       self-update and the daily new-release notice
tests/cli.rs        End-to-end tests of the binary in an isolated HOME
fuzz/               cargo-fuzz targets: remote_url, ssh_config, clone_args
```

Dependencies, and why each is used:

| Crate | Purpose |
|---|---|
| `clap`, `clap_complete` | Argument parsing, help, shell completions |
| `serde`, `toml` | config.toml |
| `anyhow` | Error context |
| `dirs` | Home directory |
| `dialoguer`, `console` | Prompts and colored output |
| `ratatui`, `crossterm` | Dashboard |
| `ctrlc`, `libc` (Unix), `crossterm_winapi` (Windows) | Ctrl-C handling and terminal mode save/restore |
| `sha2` | Verifies `self-update` downloads |

`cargo deny` checks every dependency for advisories, license and source on each push (see [development](development.md#security-scanning)).

## Command dispatch

`main.rs` parses the arguments with clap:

- **No subcommand**: `interactive::menu()` runs the setup wizard if no profiles exist, then `tui::run()`. Without a TTY it prints an error that suggests `gg --help`.
- **Known subcommand**: the matching `commands::*` function, which returns `Result<i32>` or `Result<()>`.
- **`gg git …`**: passed straight to `git`.
- **Anything else**: `#[command(external_subcommand)]` captures it, and `commands::git_passthrough` runs the checks described below before calling `git`.
- `gg remote <unknown>` likewise passes to `git remote`.

Errors print as `gg: <message with context>` and exit 1. Child processes inherit stdio; their exit code is returned, and a signal maps to `128+n`.

## Remote URLs

`git::parse_remote` turns any of these into `{ host, path }`:

| Form | Example |
|---|---|
| scp-like | `git@github.com:owner/repo.git`, `github-work:owner/repo` |
| ssh | `ssh://git@github.com[:22]/owner/repo.git`, `git+ssh://…` |
| https/http | `https://user@github.com/owner/repo` |
| git | `git://github.com/owner/repo.git` |

Input is treated as a URL only if the text before `://` is a valid scheme (letters, digits, `+`, `.`, `-`); anything else, such as `host:a b://c`, is parsed as scp-like. Local paths (`/x`, `./x`, `file://…`) and other schemes return `None` and are never modified. The path is normalized: surrounding `/` are trimmed and `.git` is appended if missing.

`git::rewrite_url(url, profile, cfg)` returns `git@<profile.ssh_alias>:<path>` only when the host is:

1. the profile's `hostname` (direct access), or
2. the profile's own alias, or
3. the alias of **another profile with the same hostname** (switching accounts).

Any other host returns `None`. So a GitLab remote is never repointed at a GitHub alias, and an alias you defined yourself for an unrelated server is left alone.

`is_shorthand` recognizes `owner/repo`: exactly two non-empty segments, neither `.` nor `..`, with no `:`, `@` or `\`.

## Profile resolution

| Question | Answer |
|---|---|
| Which profile does this repository use? (`identity_profile`) | Local `gg.profile` if it names an existing profile, else the profile whose `email` equals `user.email` |
| Which profile does this remote authenticate as? (`remote_state`) | The profile whose `ssh_alias` is the URL host; a URL whose host is a profile's `hostname` is "direct" |
| Which profile does a new clone or remote use? (`Config::resolve`) | `-p` if given, else the repository's profile (for `remote add`), else `default`, else the only profile; otherwise an error |

## Main flows

**`gg use <p>`**: validate `p`, run `git::apply_profile`, which sets local `user.name`, `user.email` and `gg.profile`, then `rewrite_remotes(only_direct = false)` reports each remote as rewritten or left unchanged. Outside a repository, only `default` is saved.

**`gg clone`**: `parse_clone_args` extracts `-p/--profile[=]` from any position and finds the first positional argument (the repository) and an optional second one (the directory). It also notes `--bare`. The repository is expanded (shorthand) or rewritten, `git clone` runs with the remaining arguments unchanged, and on success the target directory is derived the way Git does (`clone_dir_name`) before `apply_profile` runs on it.

**Push/pull/fetch repair**: `fix_direct_remotes` runs `rewrite_remotes(only_direct = true)` when `gg.profile` is set. Only URLs whose host is the profile's `hostname` change; remotes that already use some alias are respected.

**Commit/push warning** (`repo_warning`): no profile → hint to run `gg use`. The commit profile differs from the profile `origin` authenticates as → mismatch warning. These are warnings only; the Git command still runs.

## SSH config handling

`ssh::parse_hosts` is a small parser for `Host` blocks. It reads `HostName` and `IdentityFile` with case-insensitive keywords, handles several patterns per `Host`, and skips wildcard patterns (`*`, `?`, `!`). `Match` blocks end a `Host` block and are not interpreted.

`plan_sync` is a pure function `(old content, config) → (new content, added, existing)`, so it can be unit-tested:

1. Remove any existing managed block (`strip_block`).
2. Parse your remaining hosts. Each profile alias that is already defined is reported (with a warning if `HostName` or `IdentityFile` differ); the rest are rendered with `render_host`.
3. If anything was rendered, insert the block between the markers just before the first `Host`/`Match` line, so global options at the top keep applying.

Lines are kept byte for byte, including `\r` in CRLF files. `strip_block` removes a block only when a begin marker has an end marker after it; a stray marker line is dropped, but the lines after it are kept.

`sync` writes a backup and then the new content. A newly created file gets mode 600, and `~/.ssh` is created with mode 700. It is idempotent: running it again produces identical bytes.

`ssh::test` runs `ssh -T -o ConnectTimeout=10 git@<alias>` and searches the output for GitHub's `Hi <login>!` greeting.

## Updates

`update.rs` uses the system `curl`/`wget` and `tar`, as `install.sh` does, so it adds no HTTP or TLS crates; Windows 10+ ships `curl.exe` and a bsdtar `tar.exe` that also reads `.zip` (`update::tar` calls it by full path so an MSYS GNU tar on `PATH` is not used). SHA-256 is computed in-process with `sha2`.

- **`latest`** reads `tag_name` from `api.github.com/repos/ChenezDigital/gg/releases/latest`. That endpoint never returns pre-releases.
- **`self_update`** works in this order:
  1. Refuses binaries owned by cargo, Homebrew or Nix, and development builds in `target/` (`managed_by`), and prints their update command.
  2. Picks the asset from the compile-time OS and architecture (`.zip` on Windows, `.tar.gz` elsewhere).
  3. Downloads the archive and its `.sha256` into a temporary directory.
  4. `install_archive` verifies the checksum, unpacks, and requires `<new> --version` to print `gg <expected>`.
  5. Copies the new binary to a staging file next to the old one, sets mode `755`, and `rename`s it over the old one. That keeps the swap atomic, and it works while the old binary is running. Windows cannot replace a running `.exe` but can rename it, so there the old one is first renamed to `gg.exe.old` (put back if the second rename fails) and deleted by `remove_leftover` on a later run.
- **`notify`** runs in `main` after a command succeeds:
  1. If the cached latest version (`latest-version` in the user cache directory) is newer, it prints a hint to stderr.
  2. If the cache is older than 24 hours, it records the attempt and spawns `gg __update-check` (hidden) with null stdio in its own process group (on Windows: `CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW`). That process refreshes the cache, so the command never waits for the network, and Ctrl-C doesn't reach it.
  3. It does nothing unless stderr is a terminal, or when `CI` or `GG_NO_UPDATE_CHECK` is set, or when `update_check = false`.

## Dashboard and the terminal

The dashboard is an immediate-mode ratatui app: `App` holds the state (config, repository snapshot, column, selection, status message), and `draw` renders it each frame.

- **Layout**: a title line; Repository (55%) and Profiles (45%) panes; the Git and Accounts & SSH lists side by side; a details box for the selected item; a key hint footer. The pane heights adapt to the content and the terminal size.
- **Entries** are static tables of `Entry::Header | Entry::Item { key, label, desc, action }`. Navigation skips headers. Hotkeys are unique across both columns, so one keypress selects and runs an item.
- **Repository snapshot** (`App::refresh`): a few fast local `git` calls (branch, `rev-list --left-right --count @{u}...HEAD`, `status --porcelain`, author, remote URLs), refreshed after every action. No network access.

### Running actions

Actions reuse the CLI code, so prompts and output are identical. `run_suspended`:

1. leaves raw mode and the alternate screen, restores the original termios, and shows the cursor;
2. prints a banner and runs `interactive::run_action`;
3. **restores the original termios again**, because a child such as `ssh-keygen` may have been killed while echo was off;
4. if the action printed output, waits for a key;
5. re-enters the alternate screen and raw mode, clears, refreshes, and shows `✓ finished`, `· cancelled` or `✗ error` in the status line.

### Ctrl-C

Prompts read in raw mode, but the kernel still sends SIGINT to the whole foreground process group, including `gg` and any child.

- A `ctrlc` handler is installed once. In plain CLI mode it restores the termios captured at start-up, shows the cursor, and exits 130.
- While the dashboard runs (`DASHBOARD` flag), the handler does nothing. The prompt returns an `Interrupted` error, or the child exits from the signal; `ssh::keygen` maps that to `Interrupted`. `run_action` turns it into "cancelled" and the dashboard continues.
- In plain CLI mode the failed read can return before the handler exits, so `main` also maps an `Interrupted` error to exit status 130.
- `tui::run` saves the termios before entering and restores it on every exit path, including errors.
- On Windows, `interactive::TtyState` is the console input mode (`GetConsoleMode`/`SetConsoleMode` via `crossterm_winapi`) instead of termios, Ctrl-C arrives as a console control event (`ctrlc` uses `SetConsoleCtrlHandler`), and a child killed by Ctrl-C exits with `STATUS_CONTROL_C_EXIT`, which `ssh::keygen` maps to `Interrupted`. A console read is not interrupted by Ctrl-C, so every dialoguer call goes through `interactive::prompt`, which sets `PROMPTING`; when Ctrl-C arrives during a dashboard prompt the handler sets `CANCELLED` and injects an Enter key with `WriteConsoleInputW`, and `prompt` turns the answer into an `Interrupted` error. For that reason text inputs accept empty answers at the dialoguer level and enforce required values in a validator that lets the cancelled read through. crossterm reports key releases on Windows, so the dashboard only acts on `KeyEventKind::Press`. The classic console window (conhost, used by Windows PowerShell 5.1 and `cmd`) only has WGL4 fonts, so symbols such as `★ ✓ ▶ ⏎` render as boxes and shift the rest of the row; `ui::glyphs()` swaps in WGL4 stand-ins unless `WT_SESSION`, `TERM_PROGRAM` or `ConEmuANSI` is set (override with `GG_GLYPHS`).

## Design decisions

- **Never touch global Git config.** Identity is set per repository, so a forgotten switch cannot commit to the wrong account everywhere. The default profile lives in gg's own config.
- **SSH aliases over `core.sshCommand` or `includeIf`.** The alias is stored in the remote URL itself, so plain `git`, IDEs and other tools authenticate correctly without `gg`.
- **Don't edit what the user wrote.** `~/.ssh/config` changes are limited to a marked block, with a backup.
- **Wrap git, don't shadow it.** Only commands that need account awareness (`clone`, `remote`) are reimplemented, with `gg git …` as an escape hatch. Command names avoid Git's (`use`, `setup`, `whoami`, `profile`, `ssh`).
- **Static binaries.** Linux releases target musl, so one file runs on any distribution (including immutable ones such as Bazzite or Silverblue).
