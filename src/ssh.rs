use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::config::{Config, Profile, expand_tilde};
use crate::ui;

const BEGIN: &str = "# >>> gg managed hosts (change with `gg profile`, then `gg ssh sync`) >>>";
const END: &str = "# <<< gg managed hosts <<<";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHost {
    pub alias: String,
    pub hostname: Option<String>,
    pub identity_file: Option<String>,
}

pub fn ssh_dir() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("cannot determine home directory")?
        .join(".ssh"))
}

pub fn config_file() -> Result<PathBuf> {
    Ok(ssh_dir()?.join("config"))
}

fn split_kv(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let idx = line.find(|c: char| c.is_whitespace() || c == '=')?;
    let key = line[..idx].to_ascii_lowercase();
    let value = line[idx..]
        .trim_start_matches(|c: char| c.is_whitespace() || c == '=')
        .trim();
    Some((key, value.trim_matches('"').to_string()))
}

fn strip_block(content: &str) -> String {
    let mut out = Vec::new();
    let mut inside = false;
    let mut skip_blank = false;
    for line in content.lines() {
        if line.trim() == BEGIN {
            inside = true;
            continue;
        }
        if inside {
            if line.trim() == END {
                inside = false;
                skip_blank = true;
            }
            continue;
        }
        if skip_blank && line.trim().is_empty() {
            skip_blank = false;
            continue;
        }
        skip_blank = false;
        out.push(line);
    }
    let mut s = out.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

/// Concrete (non-wildcard) Host entries in an ssh config.
pub fn parse_hosts(content: &str) -> Vec<SshHost> {
    let mut hosts: Vec<SshHost> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    for line in content.lines() {
        let Some((key, value)) = split_kv(line) else { continue };
        match key.as_str() {
            "host" => {
                current.clear();
                for pat in value.split_whitespace() {
                    if pat.contains(['*', '?', '!']) {
                        continue;
                    }
                    if let Some(i) = hosts.iter().position(|h| h.alias == pat) {
                        current.push(i);
                    } else {
                        hosts.push(SshHost {
                            alias: pat.to_string(),
                            hostname: None,
                            identity_file: None,
                        });
                        current.push(hosts.len() - 1);
                    }
                }
            }
            "match" => current.clear(),
            "hostname" | "identityfile" => {
                for &i in &current {
                    let slot = if key == "hostname" {
                        &mut hosts[i].hostname
                    } else {
                        &mut hosts[i].identity_file
                    };
                    slot.get_or_insert_with(|| value.clone());
                }
            }
            _ => {}
        }
    }
    hosts
}

fn render_host(p: &Profile) -> String {
    let key = if p.ssh_key.contains(' ') {
        format!("\"{}\"", p.ssh_key)
    } else {
        p.ssh_key.clone()
    };
    format!(
        "Host {}\n    HostName {}\n    User git\n    IdentityFile {}\n    IdentitiesOnly yes\n",
        p.ssh_alias, p.hostname, key
    )
}

pub struct SyncPlan {
    pub content: String,
    pub added: Vec<String>,
    /// Aliases already defined by the user outside the managed block, with a warning if they differ.
    pub existing: Vec<(String, Option<String>)>,
}

/// Computes the new ssh config: user-written hosts are left untouched; missing
/// aliases go into a managed block placed before the first Host/Match section.
pub fn plan_sync(content: &str, cfg: &Config) -> SyncPlan {
    let base = strip_block(content);
    let hosts = parse_hosts(&base);
    let mut added = Vec::new();
    let mut existing = Vec::new();
    let mut block = String::new();
    for p in cfg.profiles.values() {
        if let Some(h) = hosts.iter().find(|h| h.alias == p.ssh_alias) {
            let mut issues = Vec::new();
            if !h
                .hostname
                .as_deref()
                .unwrap_or(&p.ssh_alias)
                .eq_ignore_ascii_case(&p.hostname)
            {
                issues.push(format!(
                    "HostName is {:?}, profile expects {}",
                    h.hostname.as_deref().unwrap_or("unset"),
                    p.hostname
                ));
            }
            if h.identity_file.as_deref().map(expand_tilde) != Some(p.key_path()) {
                issues.push(format!(
                    "IdentityFile is {:?}, profile expects {}",
                    h.identity_file.as_deref().unwrap_or("unset"),
                    p.ssh_key
                ));
            }
            existing.push((p.ssh_alias.clone(), (!issues.is_empty()).then(|| issues.join("; "))));
            continue;
        }
        if !block.is_empty() {
            block.push('\n');
        }
        block.push_str(&render_host(p));
        added.push(p.ssh_alias.clone());
    }

    let content = if block.is_empty() {
        base
    } else {
        let block = format!("{BEGIN}\n{block}{END}\n");
        let lines: Vec<&str> = base.lines().collect();
        let first_section = lines
            .iter()
            .position(|l| split_kv(l).is_some_and(|(k, _)| k == "host" || k == "match"));
        match first_section {
            Some(i) => {
                let mut s = lines[..i].iter().map(|l| format!("{l}\n")).collect::<String>();
                s.push_str(&block);
                s.push('\n');
                s.push_str(&lines[i..].join("\n"));
                s.push('\n');
                s
            }
            None if base.trim().is_empty() => block,
            None => format!("{}\n\n{block}", base.trim_end()),
        }
    };
    SyncPlan {
        content,
        added,
        existing,
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).with_context(|| format!("chmod {}", path.display()))
}
#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) -> Result<()> {
    Ok(())
}

pub fn ensure_ssh_dir() -> Result<PathBuf> {
    let dir = ssh_dir()?;
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        set_mode(&dir, 0o700)?;
    }
    Ok(dir)
}

/// Writes missing Host aliases for all profiles into ~/.ssh/config (backing it up first).
pub fn sync(cfg: &Config) -> Result<()> {
    ensure_ssh_dir()?;
    let path = config_file()?;
    let existed = path.exists();
    let old = if existed {
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
    } else {
        String::new()
    };
    let plan = plan_sync(&old, cfg);
    for (alias, issue) in &plan.existing {
        match issue {
            None => println!(
                "{}",
                ui::ok(format!("{alias}: already configured in {}", path.display()))
            ),
            Some(i) => eprintln!(
                "! {alias}: defined by you in {} but {i}. Edit it manually.",
                path.display()
            ),
        }
    }
    if plan.content == old {
        println!("{}", ui::ok("SSH config is up to date."));
        return Ok(());
    }
    if existed {
        let backup = path.with_file_name("config.gg-backup");
        fs::copy(&path, &backup).with_context(|| format!("backing up to {}", backup.display()))?;
        println!(
            "{}",
            ui::dim(format!("Backed up {} to {}", path.display(), backup.display()))
        );
    }
    fs::write(&path, &plan.content).with_context(|| format!("writing {}", path.display()))?;
    if !existed {
        set_mode(&path, 0o600)?;
    }
    for alias in &plan.added {
        println!(
            "{} {alias}: added to {}",
            console::style("+").green().bold(),
            path.display()
        );
    }
    if plan.added.is_empty() {
        println!("Removed stale gg-managed entries from {}", path.display());
    }
    Ok(())
}

pub fn detect_hosts() -> Vec<SshHost> {
    let Ok(path) = config_file() else { return Vec::new() };
    let content = fs::read_to_string(path).unwrap_or_default();
    parse_hosts(&content)
        .into_iter()
        .filter(|h| h.hostname.is_some() && h.identity_file.is_some())
        .collect()
}

pub fn keygen(profile: &Profile, no_passphrase: bool) -> Result<()> {
    let key = profile.key_path();
    if key.exists() {
        bail!("{} already exists; refusing to overwrite it", key.display());
    }
    if let Some(dir) = key.parent()
        && !dir.exists()
    {
        fs::create_dir_all(dir)?;
        set_mode(dir, 0o700)?;
    }
    let mut cmd = Command::new("ssh-keygen");
    cmd.args(["-t", "ed25519", "-C", &profile.email, "-f"]).arg(&key);
    if no_passphrase {
        cmd.args(["-N", ""]);
    }
    let status = cmd.status().context("failed to run ssh-keygen")?;
    if std::os::unix::process::ExitStatusExt::signal(&status) == Some(libc::SIGINT) {
        return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "ssh-keygen interrupted").into());
    }
    if !status.success() {
        bail!("ssh-keygen failed");
    }
    print_public_key(profile)
}

pub fn print_public_key(profile: &Profile) -> Result<()> {
    let mut pub_path = profile.key_path().into_os_string();
    pub_path.push(".pub");
    let pub_path = PathBuf::from(pub_path);
    let key = fs::read_to_string(&pub_path).with_context(|| format!("reading {}", pub_path.display()))?;
    println!(
        "\n{}\n\n{}",
        ui::heading(&format!("Public key ({})", pub_path.display())),
        ui::bold(key.trim())
    );
    if profile.hostname.eq_ignore_ascii_case("github.com") {
        println!(
            "\nAdd it to the {} account at https://github.com/settings/ssh/new",
            profile.username.as_deref().unwrap_or("matching")
        );
    } else {
        println!("\nAdd it to your account's SSH keys on {}.", profile.hostname);
    }
    Ok(())
}

/// Runs `ssh -T git@<alias>` and reports which account authenticated.
pub fn test(profile: &Profile) -> Result<bool> {
    let out = Command::new("ssh")
        .args(["-T", "-o", "ConnectTimeout=10", &format!("git@{}", profile.ssh_alias)])
        .stdin(Stdio::inherit())
        .output()
        .context("failed to run ssh")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let text = text.trim();
    let ok = out.status.success()
        || [
            "successfully authenticated",
            "Welcome to GitLab",
            "authenticated via",
            "logged in as",
        ]
        .iter()
        .any(|m| text.contains(m));
    let account = text
        .strip_prefix("Hi ")
        .and_then(|r| r.split_once('!'))
        .map(|(u, _)| u.to_string())
        .or_else(|| {
            text.split_once("@")
                .and_then(|(_, r)| r.split_once('!'))
                .map(|(u, _)| u.to_string())
        });
    if !ok {
        eprintln!("{}", ui::fail(format!("{}: {}", profile.ssh_alias, text)));
        return Ok(false);
    }
    match (&profile.username, &account) {
        (Some(want), Some(got)) if !want.eq_ignore_ascii_case(got) => {
            eprintln!(
                "! {}: authenticated as '{got}', but profile expects '{want}'",
                profile.ssh_alias
            );
            Ok(false)
        }
        (_, Some(got)) => {
            println!(
                "{}",
                ui::ok(format!("{}: authenticated as {}", profile.ssh_alias, ui::bold(got)))
            );
            Ok(true)
        }
        _ => {
            println!("{}", ui::ok(format!("{}: {}", profile.ssh_alias, text)));
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        let mut c = Config::default();
        for n in ["personal", "work"] {
            let mut p = Profile::with_defaults(n);
            p.ssh_key = format!("~/.ssh/github_{n}");
            c.profiles.insert(n.into(), p);
        }
        c
    }

    const USER_CONFIG: &str = "Host github-personal\n    HostName github.com\n    User git\n    IdentityFile ~/.ssh/github_personal\n    IdentitiesOnly yes\n";

    #[test]
    fn parses_hosts() {
        let hosts = parse_hosts(
            "Host *\n  ForwardAgent no\nHost a b\n  HostName=github.com\n  IdentityFile \"~/.ssh/k\"\nMatch all\n  HostName x\n",
        );
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[1].alias, "b");
        assert_eq!(hosts[1].hostname.as_deref(), Some("github.com"));
        assert_eq!(hosts[1].identity_file.as_deref(), Some("~/.ssh/k"));
    }

    #[test]
    fn sync_keeps_user_entries_and_is_idempotent() {
        let c = cfg();
        let content = format!("ServerAliveInterval 60\n\n{USER_CONFIG}");
        let plan = plan_sync(&content, &c);
        assert_eq!(plan.added, vec!["github-work"]);
        assert_eq!(plan.existing.len(), 1);
        assert!(plan.existing[0].1.is_none());
        assert!(plan.content.starts_with("ServerAliveInterval 60\n\n# >>> gg managed"));
        assert!(plan.content.contains(USER_CONFIG));
        assert_eq!(plan.content.matches("Host github-personal").count(), 1);
        assert_eq!(plan_sync(&plan.content, &c).content, plan.content);

        let mut fewer = cfg();
        fewer.profiles.remove("work");
        assert_eq!(plan_sync(&plan.content, &fewer).content, content);
    }

    #[test]
    fn sync_empty_and_mismatch() {
        let c = cfg();
        let plan = plan_sync("", &c);
        assert_eq!(plan.added.len(), 2);
        assert!(plan.content.starts_with(BEGIN));
        assert_eq!(parse_hosts(&plan.content).len(), 2);

        let plan = plan_sync(&USER_CONFIG.replace("github_personal", "other"), &c);
        assert!(plan.existing[0].1.as_deref().unwrap().contains("IdentityFile"));
    }
}
