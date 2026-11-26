# Usage

- [Concepts](#concepts)
- [Dashboard](#dashboard)
- [Command reference](#command-reference)
- [Everyday workflows](#everyday-workflows)
- [Troubleshooting](#troubleshooting)

## Concepts

A **profile** is a named account, such as `personal` or `work`. It has:

| Field | Example | Used for |
|---|---|---|
| author name | `Jane Doe` | `user.name` of repositories using the profile |
| email | `you@example.com` | `user.email` |
| hostname | `github.com` | The real Git host behind the alias |
| SSH alias | `github-personal` | `Host` entry in `~/.ssh/config`; remotes become `git@github-personal:owner/repo.git` |
| SSH key | `~/.ssh/github_personal` | `IdentityFile` of the alias |
| username (optional) | `octocat` | Checked by `gg ssh test` against the account GitHub reports |

Commit identity and authentication are independent in Git. `user.email` decides who appears in commits, and the SSH key decides which account may push. `gg` keeps them in sync per repository:

- **Inside a repository**, `gg use <profile>` sets the repository's local `user.name`/`user.email`, stores `gg.profile`, and rewrites the remotes on that profile's host to its SSH alias.
- **Outside a repository**, `gg use <profile>` (or `gg default`) sets the **default profile**, which `gg clone` uses for new clones.

The global Git config (`~/.gitconfig`) is never changed.

## Dashboard

Run `gg` with no arguments in a terminal. If no profiles exist, it offers to run `gg setup` first.

```
 gg  git account switcher  ·  repository
╭ Repository ─────────────────────────────────╮╭ Profiles ───────────────────────╮
│ gg-proj  ⎇ main  ↑1  ✎ 3 changed            ││ ★ ● personal @octocat  ✓ key    │
│ as ● personal  Jane Doe <jane@…>            ││   ● work @octocat-work  ✓ key   │
│ origin git@github-personal:Chenez…/gg.git ✓ ││                                 │
╰─────────────────────────────────────────────╯╰─────────────────────────────────╯
╭ Git ────────────────────────╮╭ Accounts & SSH ─────────────╮
│   CHANGES                   ││   OVERVIEW                  │
│ ▶  s Status                 ││    w Who am I               │
│    c Commit                 ││    o Clone                  │
│   …                         ││   …                         │
╰─────────────────────────────╯╰─────────────────────────────╯
```

**Repository pane** (inside a repository):

- Line 1: name, branch, ahead/behind counts against the upstream, and changed files.
- Line 2: the profile the repository commits as, and the author.
- One line per remote:
  - `✓ profile`: authenticates through that profile's alias.
  - `⚠ … ≠ identity`: the remote authenticates as a different profile than the commit identity.
  - `⚠ github.com directly`: no account key is used, so push fails with `Permission denied (publickey)`. Fix it with `gg use`, or let `gg push` repair it.
  - `· unmanaged`: another host.

**Profiles pane:** `★` (`*` in the classic Windows console) marks the default profile; it also shows `@username` and whether the key file exists.

**Keys**

| Key | Action |
|---|---|
| `←` `→`, `Tab`, `Shift-Tab` | Switch between the Git and Accounts & SSH columns |
| `↑` `↓` (or `K` `J`), `Home`, `End` | Move; section headers are skipped |
| `Enter` | Run the selected item |
| Letter next to an item | Select and run it directly |
| `Esc`, `q`, `Ctrl-C` | Quit |

**Items**

| Column | Key | Item | Runs |
|---|---|---|---|
| Git | `s` | Status | `git status` |
| | `c` | Commit | Stages everything (`git add -A`) and commits with a prompted message |
| | `u` | Pull | `gg pull` |
| | `p` | Push | `gg push`, setting the upstream on first push |
| | `b` | Branch | Switch to or create a branch |
| | `g` | Log | Last 25 commits as a graph |
| | `a` / `e` / `x` | Add / edit / delete remote | `gg remote add/edit/delete` |
| | `r` | Use profile here | `gg use` |
| Accounts & SSH | `w` | Who am I | `gg whoami` |
| | `o` | Clone | `gg clone` with prompts |
| | `f` | Default profile | `gg default` |
| | `n` / `m` / `d` | Add / edit / remove profile | `gg profile …` |
| | `t` | Test | `gg ssh test` |
| | `y` | Sync config | `gg ssh sync` |
| | `k` | Generate key | `gg ssh keygen` |
| | `v` | Public key | `gg ssh pubkey` |
| | `i` | Setup wizard | `gg setup` |
| | `q` | Quit | |

The Git column is disabled outside a repository.

Actions run in the normal terminal, so their output and prompts look exactly like the CLI. When an action prints output, `gg` waits for a key, then returns to the dashboard with refreshed information.

`Esc` or `Ctrl-C` inside a prompt cancels that action only and returns to the dashboard. Your terminal settings (echo, line mode, cursor) are restored after every action and on exit, even if a child program such as `ssh-keygen` is interrupted.

## Command reference

`gg --help` and `gg <command> --help` show the same information.

### Identity

| Command | Description |
|---|---|
| `gg whoami` | In a repository: path, branch, profile, author (and whether it comes from repository or global config), and each remote with the account it authenticates as, including mismatch warnings. Elsewhere: the default profile and the global identity. Lists all profiles at the end. |
| `gg use [profile] [--default]` | Applies a profile to the current repository (identity, `gg.profile`, remotes). `--default` also makes it the default. Outside a repository it only sets the default. Prompts if the profile is omitted. |
| `gg default [profile]` | Sets the default profile for new clones. |

### Cloning

```
gg clone [-p|--profile <profile>] <repo> [dir] [git clone options…]
```

`<repo>` can be:

- `owner/repo` → `git@<alias>:owner/repo.git`
- `https://github.com/owner/repo(.git)` or `git@github.com:owner/repo.git` → rewritten to the profile alias
- another profile's alias on the same host (`git@github-work:…`) → rewritten to the selected profile
- a URL on another host, or a local path → cloned unchanged (with a warning for other hosts)

`-p` can appear anywhere, and all other options go to `git clone` (`--depth 1`, `-b main`, `--recurse-submodules`, `--bare`, …). After a successful clone, the profile is applied to the new repository.

### Remotes

| Command | Description |
|---|---|
| `gg remote` / `gg remote list` / `gg remote -v` | Lists remotes (fetch and differing push URLs) with `[ssh: profile]`, `[github.com directly …]` or `[not managed by gg]` |
| `gg remote add <name> <url> [-p profile] [-f]` | Adds a remote through the repository's profile (or `-p`); `owner/repo` shorthand is expanded; `-f` fetches afterwards |
| `gg remote edit <name> [url] [-p profile] [--push]` | Changes a remote URL (prompts with the current one if omitted); `--push` changes only the push URL. Alias: `set-url` |
| `gg remote delete <name>` | Removes a remote. Aliases: `remove`, `rm` |
| `gg remote rename …`, `prune`, … | Any other subcommand is passed to `git remote` |

### Profiles

| Command | Description |
|---|---|
| `gg profile list` | All profiles; `★` marks the default |
| `gg profile add <name> [fields]` | Creates a profile, prompting for anything not given. Prints the next steps (keygen, sync, test) |
| `gg profile edit <name> [fields]` | Updates the given fields, or prompts for all fields if none are given |
| `gg profile remove <name>` | Deletes the profile (keys and SSH config are kept) |

Fields: `--author <name>`, `--email <email>`, `--username <login>`, `--hostname <host>` (default `github.com`), `--alias <ssh-alias>` (default `github-<name>`), `--key <path>` (default `~/.ssh/github_<name>`).

Profile names may contain letters, digits, `-` and `_`. SSH aliases must be unique.

### SSH

| Command | Description |
|---|---|
| `gg ssh sync` | Adds a `Host` entry for each profile whose alias is missing from `~/.ssh/config` (see [configuration](configuration.md#ssh-config)) |
| `gg ssh keygen <profile> [--no-passphrase]` | Runs `ssh-keygen -t ed25519 -C <email>` for the profile's key path. Refuses to overwrite, creates `~/.ssh` with mode 700, then prints the public key and where to add it |
| `gg ssh pubkey <profile>` | Prints the public key |
| `gg ssh test [profile]` | Runs `ssh -T git@<alias>` for one or all profiles and reports the account. Fails if it differs from `username`. Exits 1 if any test fails |

### Setup

`gg setup` is a wizard that:

1. Imports existing `Host` entries from `~/.ssh/config` that point to a key, offering them as profiles.
2. Asks for the author name and email of each profile, and lets you add more.
3. Sets the default profile.
4. Offers to generate missing keys.
5. Offers to add missing SSH aliases (with a backup).
6. Offers to test the connections.

### Git passthrough

Any command that is not a `gg` command is passed to `git` with the same arguments, stdin, stdout and exit code: `gg commit -m msg`, `gg log --oneline`, `gg rebase -i HEAD~3`, `gg stash pop`, …

Before passing the command on, `gg` adds two checks:

- **`push`, `pull`, `fetch`:** in a repository with a profile set by `gg use` or `gg clone` (`gg.profile`), remotes that use the raw host (`git@github.com:` or `https://github.com/`) are rewritten to the profile's alias, with a note on stderr. Remotes using another profile's alias are left alone.
- **`commit`, `push`:** warns if the repository has no profile, or if it commits as one profile while `origin` authenticates as another.

Use `gg git <args>` to run Git commands whose names `gg` uses itself, such as `gg git clone …` or `gg git remote add …`, without any rewriting.

### Other

| Command | Description |
|---|---|
| `gg completions <bash\|zsh\|fish\|elvish\|powershell>` | Prints a completion script |
| `gg self-update [--check]` | Installs the latest release, after verifying its checksum; `--check` only reports. See [installation](installation.md#updating) |
| `gg --version` | Version |

Exit codes: `0` success; `1` a `gg` error (printed as `gg: …`); otherwise the exit code of the Git or SSH command that was run (`128+n` if it was killed by signal `n`). Cancelled prompts in the dashboard are not errors.

## Everyday workflows

**Clone a work repository while the default is personal**

```bash
gg clone -p work company/service
```

**A repository cloned with plain `git clone` fails to push**

```bash
gg use work          # identity + remote alias in one go
gg push
```

**Check everything at once**

```bash
gg whoami && gg ssh test
```

**Add a new GitHub account**

```bash
gg profile add client --author "Your Name" --email you@client.com --username you-client
gg ssh keygen client           # paste the printed key into GitHub → Settings → SSH keys
gg ssh sync && gg ssh test client
```

**Start a new project and publish it**

```bash
git init project && cd project
gg use personal
gg remote add origin octocat/project
gg commit -m "Initial commit" --allow-empty
gg push -u origin HEAD
```

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `Permission denied (publickey)` | The remote uses `github.com` directly, or the key isn't added to the account. Run `gg remote` to check, `gg use <profile>` to fix the remote, and `gg ssh test` to check the key. |
| `gg ssh test` says the account differs | The key is registered on another GitHub account. Remove it there and add it to the right one, or fix `--username`. |
| Commits show the wrong author | Run `gg whoami`. If the author comes from global config, run `gg use <profile>`. |
| `… needs an interactive terminal` | Prompts are disabled when stdin or stdout is not a TTY. Pass the values as arguments instead. |
| Terminal stops echoing after an interrupted program | Fixed in current versions; if it still happens, type `reset` and press Enter. |
