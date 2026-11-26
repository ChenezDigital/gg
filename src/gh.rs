//! Optional GitHub CLI (`gh`) integration: offer `gh`'s logged-in accounts as new profiles and
//! add SSH keys to an account with that account's own token. Without `gh`, or when a call fails,
//! everything here quietly does nothing and gg falls back to manual steps.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};

use crate::config::{Config, Profile, home_dir};

/// A `gh` login: one account on one host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub host: String,
    pub user: String,
    /// The account `gh` currently uses for this host.
    pub active: bool,
}

/// What the account's API reports, for pre-filling a profile.
#[derive(Debug, Default)]
pub struct Details {
    pub name: Option<String>,
    pub email: Option<String>,
}

/// The permission `gh ssh-key add` needs; `gh auth login` does not grant it by default.
pub const KEY_SCOPE: &str = "admin:public_key";

pub fn available() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        Command::new("gh")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// Same lookup order as `gh`: `$GH_CONFIG_DIR`, `$XDG_CONFIG_HOME/gh`, `%AppData%\GitHub CLI`
/// on Windows, then `~/.config/gh`.
fn config_dir() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(d) = var("GH_CONFIG_DIR") {
        return Some(d);
    }
    if let Some(d) = var("XDG_CONFIG_HOME") {
        return Some(d.join("gh"));
    }
    if cfg!(windows)
        && let Some(d) = var("AppData")
    {
        return Some(d.join("GitHub CLI"));
    }
    home_dir().map(|h| h.join(".config").join("gh"))
}

/// Accounts `gh` is logged in to, or none when `gh` is not installed.
pub fn accounts() -> Vec<Account> {
    if !available() {
        return Vec::new();
    }
    config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("hosts.yml")).ok())
        .map(|s| parse_hosts(&s))
        .unwrap_or_default()
}

/// `gh` accounts that no profile covers yet (same host and username).
pub fn unimported(cfg: &Config) -> Vec<Account> {
    accounts()
        .into_iter()
        .filter(|a| {
            !cfg.profiles.values().any(|p| {
                p.hostname.eq_ignore_ascii_case(&a.host)
                    && p.username.as_deref().is_some_and(|u| u.eq_ignore_ascii_case(&a.user))
            })
        })
        .collect()
}

/// The `gh` account matching a profile's host and username.
pub fn account_for(p: &Profile) -> Option<Account> {
    let user = p.username.as_deref()?;
    accounts()
        .into_iter()
        .find(|a| a.host.eq_ignore_ascii_case(&p.hostname) && a.user.eq_ignore_ascii_case(user))
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(s)
}

/// Reads `hosts.yml`. Since gh 2.40 each host lists its accounts under `users:` and names the
/// active one in `user:`; older files only have `user:`. Only that small, gh-written subset of
/// YAML is understood.
pub fn parse_hosts(content: &str) -> Vec<Account> {
    struct Host {
        name: String,
        field_indent: Option<usize>,
        users_indent: Option<usize>,
        user_indent: Option<usize>,
        users: Vec<String>,
        active: Option<String>,
    }
    fn flush(h: Option<Host>, out: &mut Vec<Account>) {
        let Some(h) = h else { return };
        let mut users = h.users;
        if users.is_empty() {
            users.extend(h.active.clone());
        }
        for user in users {
            let active = h.active.as_deref() == Some(user.as_str());
            out.push(Account {
                host: h.name.clone(),
                user,
                active,
            });
        }
    }

    let mut out = Vec::new();
    let mut host: Option<Host> = None;
    for line in content.lines() {
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let indent = line.len() - t.len();
        let Some((key, val)) = t.split_once(':') else { continue };
        let (key, val) = (unquote(key), unquote(val));
        if indent == 0 {
            flush(host.take(), &mut out);
            host = Some(Host {
                name: key.to_string(),
                field_indent: None,
                users_indent: None,
                user_indent: None,
                users: Vec::new(),
                active: None,
            });
            continue;
        }
        let Some(h) = host.as_mut() else { continue };
        if let Some(ui) = h.users_indent {
            if indent > ui {
                if *h.user_indent.get_or_insert(indent) == indent && !key.is_empty() {
                    h.users.push(key.to_string());
                }
                continue;
            }
            h.users_indent = None;
        }
        if *h.field_indent.get_or_insert(indent) != indent {
            continue;
        }
        match key {
            "users" if val.is_empty() => {
                h.users_indent = Some(indent);
                h.user_indent = None;
            }
            "user" if !val.is_empty() => h.active = Some(val.to_string()),
            _ => {}
        }
    }
    flush(host, &mut out);
    out
}

fn gh_cmd() -> Command {
    let mut c = Command::new("gh");
    // No pager, no prompts, no update notices in captured output.
    c.env("GH_PAGER", "")
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1");
    c
}

fn token(acc: &Account) -> Result<String> {
    let out = gh_cmd()
        .args(["auth", "token", "--hostname", &acc.host, "--user", &acc.user])
        .stdin(Stdio::null())
        .output()
        .context("failed to run gh")?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || token.is_empty() {
        bail!(
            "gh has no token for {} on {}: {}",
            acc.user,
            acc.host,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(token)
}

/// `gh` acting as `acc` (rather than the host's active account).
fn gh_as(acc: &Account) -> Result<Command> {
    let t = token(acc)?;
    let mut c = gh_cmd();
    c.env("GH_TOKEN", &t)
        .env("GH_ENTERPRISE_TOKEN", &t)
        .env("GH_HOST", &acc.host);
    Ok(c)
}

/// Name and email from the account's profile. The email is the public one, else GitHub's
/// private no-reply address (the account's other addresses need an extra token scope).
pub fn details(acc: &Account) -> Result<Details> {
    let out = gh_as(acc)?
        .args([
            "api",
            "user",
            "--hostname",
            &acc.host,
            "--jq",
            r#"[.login, (.id|tostring), (.name // ""), (.email // "")] | join("\t")"#,
        ])
        .stdin(Stdio::null())
        .output()
        .context("failed to run gh")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let f: Vec<&str> = text.trim_end_matches(['\r', '\n']).split('\t').collect();
    let field = |i: usize| f.get(i).map(|s| s.trim()).filter(|s| !s.is_empty());
    let email = field(3).map(str::to_string).or_else(|| {
        let (login, id) = (field(0)?, field(1)?);
        acc.host
            .eq_ignore_ascii_case("github.com")
            .then(|| noreply_email(id, login))
    });
    Ok(Details {
        name: field(2).map(str::to_string),
        email,
    })
}

pub fn noreply_email(id: &str, login: &str) -> String {
    format!("{id}+{login}@users.noreply.github.com")
}

pub enum Upload {
    Added,
    /// The account's token lacks [`KEY_SCOPE`].
    NeedsScope,
}

/// Adds a public key to the account as an authentication key. A key that is already there
/// counts as added.
pub fn add_key(acc: &Account, pub_key: &Path, title: &str) -> Result<Upload> {
    let out = gh_as(acc)?
        .args(["ssh-key", "add"])
        .arg(pub_key)
        .args(["--title", title, "--type", "authentication"])
        .stdin(Stdio::null())
        .output()
        .context("failed to run gh")?;
    let err = String::from_utf8_lossy(&out.stderr);
    if out.status.success() || err.contains("already exists") {
        return Ok(Upload::Added);
    }
    if err.contains(KEY_SCOPE) {
        return Ok(Upload::NeedsScope);
    }
    bail!("gh ssh-key add failed: {}", err.trim())
}

fn switch(host: &str, user: &str) -> Result<()> {
    let ok = gh_cmd()
        .args(["auth", "switch", "--hostname", host, "--user", user])
        .stdout(Stdio::null())
        .status()
        .context("failed to run gh")?
        .success();
    if !ok {
        bail!("gh auth switch to {user} failed");
    }
    Ok(())
}

/// Runs `gh auth refresh` (a browser or device-code sign-in) to add [`KEY_SCOPE`] to the
/// account. `gh` only refreshes the active account, so another one is switched to for the
/// duration and the previous active account is restored afterwards.
pub fn grant_key_scope(acc: &Account) -> Result<()> {
    let previous = accounts().into_iter().find(|a| a.host == acc.host && a.active);
    let must_switch = previous.as_ref().is_some_and(|p| p.user != acc.user);
    if must_switch {
        switch(&acc.host, &acc.user)?;
    }
    let status = gh_cmd()
        .env_remove("GH_PROMPT_DISABLED")
        .args(["auth", "refresh", "--hostname", &acc.host, "--scopes", KEY_SCOPE])
        .status()
        .context("failed to run gh");
    if must_switch && let Some(p) = &previous {
        switch(&p.host, &p.user)?;
    }
    if !status?.success() {
        bail!("gh auth refresh did not finish");
    }
    Ok(())
}

/// A valid profile name derived from a login (`Octo.Cat` → `octo-cat`).
pub fn profile_name_for(login: &str) -> String {
    let s: String = login
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "github".into() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc(host: &str, user: &str, active: bool) -> Account {
        Account {
            host: host.into(),
            user: user.into(),
            active,
        }
    }

    #[test]
    fn multi_account_hosts() {
        let yml = "github.com:\n    git_protocol: ssh\n    users:\n        alice:\n            oauth_token: x\n        alice-work: {}\n    user: alice-work\nghe.example.com:\n    users:\n        \"bob\":\n    user: bob\n";
        assert_eq!(
            parse_hosts(yml),
            vec![
                acc("github.com", "alice", false),
                acc("github.com", "alice-work", true),
                acc("ghe.example.com", "bob", true),
            ]
        );
    }

    #[test]
    fn legacy_single_account_and_junk() {
        let yml = "# comment\ngithub.com:\n  oauth_token: secret\n  user: carol\n  git_protocol: https\n";
        assert_eq!(parse_hosts(yml), vec![acc("github.com", "carol", true)]);
        assert!(parse_hosts("").is_empty());
        assert!(parse_hosts("  stray: value\nnot yaml at all\n").is_empty());
    }

    #[test]
    fn names_from_logins() {
        assert_eq!(profile_name_for("Octo.Cat"), "octo-cat");
        assert_eq!(profile_name_for("alice_w"), "alice_w");
        assert_eq!(profile_name_for("..."), "github");
        assert_eq!(noreply_email("42", "octo"), "42+octo@users.noreply.github.com");
    }
}
