# Configuration

`gg` reads and writes three things:

| File | Written by | Contents |
|---|---|---|
| `~/.config/gg/config.toml` | `gg setup`, `gg profile`, `gg default` | Profiles and the default profile |
| `<repo>/.git/config` | `gg use`, `gg clone`, `gg remote`, push/pull/fetch repair | `user.name`, `user.email`, `gg.profile`, remote URLs |
| `~/.ssh/config` | `gg ssh sync`, `gg setup` | A managed block of `Host` entries (after asking) |

It never writes `~/.gitconfig` or any other global Git setting.

## config.toml

### Location

1. `$GG_CONFIG`, if set (any path)
2. `$XDG_CONFIG_HOME/gg/config.toml`, if `XDG_CONFIG_HOME` is an absolute path
3. `~/.config/gg/config.toml` (Linux **and** macOS; on Windows `%USERPROFILE%\.config\gg\config.toml`)

On Windows, `~` in paths such as `ssh_key` means `%USERPROFILE%`, the same directory OpenSSH and Git for Windows use for `~/.ssh`.

The file is created on first save, with its parent directory.

### Format

```toml
default = "personal"
update_check = true                   # optional; false turns off the daily new-release check

[profiles.personal]
name = "Jane Doe"                 # user.name
email = "jane@example.com"             # user.email
hostname = "github.com"               # real host (default: github.com)
ssh_alias = "github-personal"         # Host alias in ~/.ssh/config
ssh_key = "~/.ssh/github_personal"    # private key; ~ is expanded
username = "octocat"                    # optional, checked by `gg ssh test`

[profiles.work]
name = "Your Name"
email = "you@company.com"
ssh_alias = "github-work"
ssh_key = "~/.ssh/github_work"
username = "octocat-work"
```

Rules (checked when saving through `gg`, and when a profile is used):

- Profile names contain only `A-Z a-z 0-9 - _`.
- `name` is not empty; `email` contains `@`.
- `hostname` and `ssh_alias` contain no whitespace, `/`, `:` or `@`.
- Each `ssh_alias` belongs to one profile.
- `default` names an existing profile, or is omitted.

You can edit the file by hand; the next `gg` command uses it. After changing `hostname`, `ssh_alias` or `ssh_key`, run `gg ssh sync`.

### Other hosts

Profiles are not limited to GitHub. For a self-hosted GitLab:

```toml
[profiles.gitlab]
name = "Your Name"
email = "you@company.com"
hostname = "gitlab.company.com"
ssh_alias = "gitlab-company"
ssh_key = "~/.ssh/gitlab_company"
```

`owner/repo` shorthand and URL rewriting then use `gitlab.company.com`. `gg ssh test` expects GitHub's greeting (`Hi <user>!`); other hosts' greetings are shown but not verified.

## Repository settings

`gg use <profile>` (and `gg clone`) run the equivalent of:

```bash
git config user.name  "<name>"
git config user.email "<email>"
git config gg.profile "<profile>"
git remote set-url <remote> git@<ssh_alias>:<owner>/<repo>.git   # for each remote on <hostname>
```

`gg.profile` records which profile the repository belongs to. The push/pull/fetch repair runs only when it is set. For `whoami`, the commit/push warnings, `gg remote add` and the dashboard, `gg` falls back to the profile whose `email` matches the repository's `user.email`. `gg remote add` falls back further to the default profile.

Remotes are rewritten only when they point at the profile's `hostname`, its alias, or the alias of another profile on the **same** host. Remotes on other hosts and local paths are never touched.

## SSH config

`gg ssh sync` compares the profiles with `~/.ssh/config`:

- **Your entries are never edited.** If a `Host` line already defines a profile's alias, that entry is used as is. If its `HostName` or `IdentityFile` differ from the profile, `gg` prints a warning so you can fix it yourself.
- **Missing aliases** are written into a managed block:

```sshconfig
ServerAliveInterval 60          # your global options stay first

# >>> gg managed hosts (change with `gg profile`, then `gg ssh sync`) >>>
Host github-work
    HostName github.com
    User git
    IdentityFile ~/.ssh/github_work
    IdentitiesOnly yes
# <<< gg managed hosts <<<

Host github-personal            # your own entries, unchanged
    ...
```

- The block goes before the first `Host`/`Match` line, because OpenSSH uses the first value it finds for each option.
- The block is regenerated on every sync. Removed profiles disappear from it, and running it again changes nothing.
- Before each change, the previous file is copied to `~/.ssh/config.gg-backup`.
- `IdentitiesOnly yes` makes SSH offer only that key. Without it, an agent holding the other account's key could log you in as the wrong user.

To take over a managed entry, copy it outside the markers and edit it; `gg` then treats it as yours.

### Keys and permissions

`gg ssh keygen <profile>` runs:

```bash
ssh-keygen -t ed25519 -C "<email>" -f <ssh_key>      # -N "" with --no-passphrase
```

- `~/.ssh` is created with mode `700` if needed; `ssh-keygen` writes the private key as `600`.
- Existing keys are never overwritten.
- Add the printed `.pub` content at <https://github.com/settings/ssh/new> **while logged into the matching account**. A key can be registered on only one GitHub account.

## Environment variables

| Variable | Effect |
|---|---|
| `GG_CONFIG` | Path of the config file |
| `XDG_CONFIG_HOME` | Base config directory (absolute paths only) |
| `HOME` | Used for `~`, `~/.ssh` and the default config directory |
| `GG_NO_UPDATE_CHECK` | Set to `1` to turn off the daily new-release check |
| `CI` | When set, the new-release check is skipped |
| `XDG_CACHE_HOME` | Base cache directory for `gg/latest-version` (Linux) |
| `NO_COLOR` | Disables colors (also disabled when the output is not a terminal) |
| `GG_VERSION`, `GG_INSTALL_DIR`, `GG_REPO`, `GG_BASE_URL` | Installer only; see [installation](installation.md#install-script) |

Git's own variables (`GIT_SSH_COMMAND`, `GIT_DIR`, …) work as usual because `gg` runs the real `git`.
