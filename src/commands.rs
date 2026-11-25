use std::path::Path;

use anyhow::{Result, bail};

use crate::config::{Config, Profile, validate_profile_name};
use crate::{git, interactive, ssh, ui};

#[derive(clap::Args, Debug, Default, Clone)]
pub struct ProfileFields {
    /// Commit author name (user.name)
    #[arg(long)]
    pub author: Option<String>,
    /// Commit author email (user.email)
    #[arg(long)]
    pub email: Option<String>,
    /// Account login used to verify `gg ssh test`
    #[arg(long)]
    pub username: Option<String>,
    /// Git host [default: github.com]
    #[arg(long)]
    pub hostname: Option<String>,
    /// SSH Host alias [default: github-<profile>]
    #[arg(long)]
    pub alias: Option<String>,
    /// Private key path [default: ~/.ssh/github_<profile>]
    #[arg(long)]
    pub key: Option<String>,
}

impl ProfileFields {
    fn is_empty(&self) -> bool {
        [
            &self.author,
            &self.email,
            &self.username,
            &self.hostname,
            &self.alias,
            &self.key,
        ]
        .iter()
        .all(|f| f.is_none())
    }

    fn apply(&self, p: &mut Profile) {
        let set = |dst: &mut String, v: &Option<String>| {
            if let Some(v) = v {
                *dst = v.trim().to_string();
            }
        };
        set(&mut p.name, &self.author);
        set(&mut p.email, &self.email);
        set(&mut p.hostname, &self.hostname);
        set(&mut p.ssh_alias, &self.alias);
        set(&mut p.ssh_key, &self.key);
        if let Some(u) = &self.username {
            p.username = Some(u.trim().to_string()).filter(|u| !u.is_empty());
        }
    }
}

fn key_status(p: &Profile) -> String {
    if p.key_path().exists() {
        ui::ok("key ok")
    } else {
        ui::fail("key missing")
    }
}

pub fn list_profiles(cfg: &Config) {
    if cfg.profiles.is_empty() {
        println!("No profiles. Run `gg setup` or `gg profile add <name>`.");
        return;
    }
    let w = cfg.profiles.keys().map(String::len).max().unwrap_or(0) + 2;
    for (name, p) in &cfg.profiles {
        let mark = if cfg.default.as_deref() == Some(name) {
            ui::bold("★")
        } else {
            " ".into()
        };
        let badge = ui::badge(cfg, name);
        let pad = " ".repeat(w.saturating_sub(name.len() + 2));
        let user = p
            .username
            .as_deref()
            .map(|u| ui::dim(format!(" @{u}")))
            .unwrap_or_default();
        println!(
            "{mark} {badge}{pad}  {} {}{user}",
            p.name,
            ui::dim(format!("<{}>", p.email))
        );
        println!(
            "  {}  git@{} -> {}  {}",
            " ".repeat(w),
            p.ssh_alias,
            p.ssh_key,
            key_status(p)
        );
    }
    println!("{}", ui::dim("  ★ default for new clones"));
}

pub enum RemoteState {
    Profile(String),
    Direct(String),
    Unmanaged,
}

pub fn remote_state(cfg: &Config, url: &str) -> RemoteState {
    if let Some(n) = remote_profile(cfg, url) {
        return RemoteState::Profile(n.clone());
    }
    match git::parse_remote(url) {
        Some(r) if cfg.profiles.values().any(|p| p.hostname.eq_ignore_ascii_case(&r.host)) => {
            RemoteState::Direct(r.host)
        }
        _ => RemoteState::Unmanaged,
    }
}

fn remote_profile<'a>(cfg: &'a Config, url: &str) -> Option<&'a String> {
    git::parse_remote(url).and_then(|r| cfg.profile_by_alias(&r.host).map(|(n, _)| n))
}

fn remote_tag(cfg: &Config, url: &str) -> String {
    match remote_state(cfg, url) {
        RemoteState::Profile(n) => format!("{} {}", ui::dim("[ssh:"), ui::badge(cfg, &n) + &ui::dim("]")),
        RemoteState::Direct(host) => ui::warn(format!("[{host} directly: no account key, run `gg use <profile>`]")),
        RemoteState::Unmanaged => ui::dim("[not managed by gg]"),
    }
}

/// The profile for the current repository: explicit, else gg.profile/identity, else the default.
fn repo_profile<'a>(cfg: &'a Config, explicit: Option<&str>) -> Result<(String, &'a Profile)> {
    if explicit.is_none()
        && let Some(n) = identity_profile(cfg, None)
    {
        return cfg.resolve(Some(&n));
    }
    cfg.resolve(explicit)
}

/// Before network commands, repoint remotes that use the raw host (e.g. `git@github.com:`)
/// to the repository profile's SSH alias; otherwise SSH offers no account key.
fn fix_direct_remotes(cfg: &Config) -> Result<()> {
    let Some(name) = git::get(None, &["config", "--local", "gg.profile"]) else {
        return Ok(());
    };
    let Ok(p) = cfg.get(&name) else { return Ok(()) };
    for msg in git::rewrite_remotes(None, p, cfg, true)? {
        eprintln!("{}", ui::warn(format!("gg: {msg} (profile '{name}')")));
    }
    Ok(())
}

fn require_repo() -> Result<()> {
    if git::repo_root(None).is_none() {
        bail!("not inside a Git repository");
    }
    Ok(())
}

/// Resolves owner/repo shorthand or rewrites a URL for the profile; unknown hosts are kept.
fn remote_url_for(cfg: &Config, explicit: Option<&str>, url: &str) -> Result<(String, Option<String>)> {
    if cfg.profiles.is_empty() {
        if git::is_shorthand(url) {
            bail!("'{url}' needs a profile to expand; run `gg setup` or pass a full URL");
        }
        return Ok((url.to_string(), None));
    }
    let (name, p) = repo_profile(cfg, explicit)?;
    if git::is_shorthand(url) && !Path::new(url).exists() {
        let path = git::normalize_path(url).unwrap_or_else(|| url.to_string());
        return Ok((git::ssh_url(p, &path), Some(name)));
    }
    match git::rewrite_url(url, p, cfg) {
        Some(u) => Ok((u, Some(name))),
        None => Ok((url.to_string(), None)),
    }
}

pub fn remote_list(cfg: &Config) -> Result<()> {
    require_repo()?;
    let remotes = git::remotes(None);
    if remotes.is_empty() {
        println!("No remotes. Add one with `gg remote add origin owner/repo`.");
    }
    let w = remotes.iter().map(String::len).max().unwrap_or(0);
    for r in remotes {
        let url = git::get(None, &["remote", "get-url", &r]).unwrap_or_default();
        println!("{}  {url} {}", ui::bold(format!("{r:<w$}")), remote_tag(cfg, &url));
        let push = git::get(None, &["remote", "get-url", "--push", &r]).unwrap_or_default();
        if push != url {
            println!("{:<w$}  {push} (push) {}", "", remote_tag(cfg, &push));
        }
    }
    Ok(())
}

pub fn remote_add(cfg: &Config, profile: Option<&str>, name: &str, url: &str, fetch: bool) -> Result<i32> {
    require_repo()?;
    let (url, via) = remote_url_for(cfg, profile, url)?;
    let mut args = vec!["remote", "add"];
    if fetch {
        args.push("-f");
    }
    args.extend([name, url.as_str()]);
    let code = git::passthrough(&args)?;
    if code == 0 {
        match via {
            Some(p) => println!(
                "{}",
                ui::ok(format!("Added remote {name} -> {url} via {}", ui::badge(cfg, &p)))
            ),
            None => println!("{}", ui::ok(format!("Added remote {name} -> {url}"))),
        }
    }
    Ok(code)
}

pub fn remote_edit(cfg: &Config, profile: Option<&str>, name: &str, url: Option<&str>, push: bool) -> Result<i32> {
    require_repo()?;
    let mut get = vec!["remote", "get-url"];
    if push {
        get.push("--push");
    }
    get.push(name);
    let Some(current) = git::get(None, &get) else {
        bail!("no such remote '{name}'")
    };
    let url = match url {
        Some(u) => u.to_string(),
        None => interactive::input_text(&format!("New URL for {name}"), &current)?,
    };
    let (url, via) = remote_url_for(cfg, profile, &url)?;
    let mut set = vec!["remote", "set-url"];
    if push {
        set.push("--push");
    }
    set.extend([name, url.as_str()]);
    let code = git::passthrough(&set)?;
    if code == 0 {
        let via = via.map(|p| format!(" (profile '{p}')")).unwrap_or_default();
        println!(
            "{}",
            ui::ok(format!(
                "{name}{}: {current} -> {url}{via}",
                if push { " (push)" } else { "" }
            ))
        );
    }
    Ok(code)
}

pub fn remote_delete(name: &str) -> Result<i32> {
    require_repo()?;
    let code = git::passthrough(&["remote", "remove", name])?;
    if code == 0 {
        println!("{}", ui::ok(format!("Removed remote {name}")));
    }
    Ok(code)
}

/// The profile the current repo commits as (gg.profile, else matched by email).
pub fn identity_profile(cfg: &Config, dir: Option<&Path>) -> Option<String> {
    git::get(dir, &["config", "--local", "gg.profile"])
        .filter(|n| cfg.profiles.contains_key(n))
        .or_else(|| {
            let email = git::get(dir, &["config", "user.email"])?;
            cfg.profile_by_email(&email).map(|(n, _)| n.clone())
        })
}

fn row(label: &str, value: impl std::fmt::Display) {
    println!("  {}  {value}", ui::dim(format!("{label:<9}")));
}

pub fn whoami(cfg: &Config) -> Result<()> {
    if let Some(root) = git::repo_root(None) {
        let repo = root.rsplit('/').next().unwrap_or(&root).to_string();
        println!("{}", ui::heading(&format!("Repository  {repo}")));
        row("Path", ui::dim(&root));
        if let Some(b) = git::get(None, &["branch", "--show-current"]) {
            row("Branch", &b);
        }
        let local = git::get(None, &["config", "--local", "user.email"]).is_some();
        let name = git::get(None, &["config", "user.name"]).unwrap_or_else(|| "(unset)".into());
        let email = git::get(None, &["config", "user.email"]).unwrap_or_else(|| "(unset)".into());
        let ident = identity_profile(cfg, None);
        match &ident {
            Some(i) => row("Profile", ui::badge(cfg, i)),
            None => row("Profile", ui::warn("none, run `gg use <profile>`")),
        }
        let scope = if local { "repo config" } else { "global config" };
        row("Author", format!("{name} {}", ui::dim(format!("<{email}> ({scope})"))));
        let remotes = git::remotes(None);
        if remotes.is_empty() {
            row("Remotes", ui::dim("none"));
        }
        for (idx, r) in remotes.iter().enumerate() {
            let url = git::get(None, &["remote", "get-url", r]).unwrap_or_default();
            let label = if idx == 0 { "Remotes" } else { "" };
            row(label, format!("{} {url} {}", ui::bold(r), remote_tag(cfg, &url)));
            if let (Some(v), Some(i)) = (remote_profile(cfg, &url), &ident)
                && v != i
            {
                row(
                    "",
                    ui::warn(format!(
                        "commits as '{i}' but '{r}' authenticates as '{v}'. Run `gg use <profile>`."
                    )),
                );
            }
        }
    } else {
        println!("{}", ui::heading("Not inside a Git repository"));
        let default = cfg
            .default
            .as_deref()
            .map(|d| ui::badge(cfg, d))
            .unwrap_or_else(|| ui::dim("none"));
        row("Clones", format!("new clones use {default}"));
        let name = git::get(None, &["config", "--global", "user.name"]).unwrap_or_else(|| "(unset)".into());
        let email = git::get(None, &["config", "--global", "user.email"]).unwrap_or_else(|| "(unset)".into());
        row(
            "Global",
            format!("{name} {}", ui::dim(format!("<{email}> (gg never changes it)"))),
        );
    }
    println!("\n{}", ui::heading("Profiles"));
    list_profiles(cfg);
    Ok(())
}

pub fn set_default(cfg: &mut Config, name: &str) -> Result<()> {
    cfg.get(name)?;
    cfg.default = Some(name.to_string());
    cfg.save()?;
    println!(
        "{}",
        ui::ok(format!("Default profile for new clones: {}", ui::badge(cfg, name)))
    );
    Ok(())
}

pub fn use_profile(cfg: &mut Config, name: &str, also_default: bool) -> Result<()> {
    let p = cfg.get(name)?.clone();
    if git::repo_root(None).is_some() {
        git::apply_profile(None, name, &p, cfg)?;
        println!(
            "{}",
            ui::ok(format!(
                "This repository now uses {} {} via {}",
                ui::badge(cfg, name),
                p.name,
                p.ssh_alias
            ))
        );
        if also_default {
            set_default(cfg, name)?;
        }
    } else {
        set_default(cfg, name)?;
    }
    Ok(())
}

const CLONE_VALUE_OPTS: &[&str] = &[
    "-b",
    "--branch",
    "-o",
    "--origin",
    "-c",
    "--config",
    "--depth",
    "--reference",
    "--reference-if-able",
    "-u",
    "--upload-pack",
    "--template",
    "--separate-git-dir",
    "-j",
    "--jobs",
    "--filter",
    "--shallow-since",
    "--shallow-exclude",
    "--server-option",
    "--bundle-uri",
    "--ref-format",
];

/// `gg clone` arguments: the extracted profile, the remaining git arguments and the indexes of
/// the positional ones (repository, optional directory).
pub struct CloneArgs {
    pub profile: Option<String>,
    pub args: Vec<String>,
    pub positionals: Vec<usize>,
    pub bare: bool,
}

pub fn parse_clone_args(input: &[String], mut profile: Option<String>) -> Result<CloneArgs> {
    let mut args = Vec::new();
    let mut positionals = Vec::new();
    let mut bare = false;
    let mut it = input.iter();
    let mut only_positionals = false;
    while let Some(a) = it.next() {
        if only_positionals || !a.starts_with('-') || a == "-" {
            positionals.push(args.len());
            args.push(a.clone());
        } else if a == "--" {
            only_positionals = true;
            args.push(a.clone());
        } else if a == "-p" || a == "--profile" {
            profile = Some(it.next().cloned().ok_or_else(|| anyhow::anyhow!("{a} needs a value"))?);
        } else if let Some(v) = a.strip_prefix("--profile=") {
            profile = Some(v.to_string());
        } else {
            bare |= a == "--bare" || a == "--mirror";
            args.push(a.clone());
            if CLONE_VALUE_OPTS.contains(&a.as_str())
                && let Some(v) = it.next()
            {
                args.push(v.clone());
            }
        }
    }
    if positionals.is_empty() {
        bail!("usage: gg clone [-p <profile>] <owner/repo | url> [dir] [git clone options]");
    }
    if positionals.len() > 2 {
        bail!("too many arguments: expected a repository and an optional directory");
    }
    Ok(CloneArgs {
        profile,
        args,
        positionals,
        bare,
    })
}

fn clone_dir_name(url: &str, bare: bool) -> String {
    let last = url.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or(url);
    let base = last.strip_suffix(".git").unwrap_or(last);
    if bare { format!("{base}.git") } else { base.to_string() }
}

pub fn clone(cfg: &Config, profile: Option<&str>, input: &[String]) -> Result<i32> {
    let mut ca = parse_clone_args(input, profile.map(str::to_string))?;
    let (name, p) = cfg.resolve(ca.profile.as_deref())?;
    let repo_idx = ca.positionals[0];
    let repo = ca.args[repo_idx].clone();
    let url = if Path::new(&repo).exists() {
        repo.clone()
    } else if git::is_shorthand(&repo) {
        git::ssh_url(p, &git::normalize_path(&repo).unwrap_or(repo.clone()))
    } else if let Some(u) = git::rewrite_url(&repo, p, cfg) {
        u
    } else {
        eprintln!(
            "{}",
            ui::warn(format!("{repo} is not on {}; cloning it unchanged", p.hostname))
        );
        repo.clone()
    };
    ca.args[repo_idx] = url.clone();
    let dir = ca
        .positionals
        .get(1)
        .map(|&i| ca.args[i].clone())
        .unwrap_or_else(|| clone_dir_name(&url, ca.bare));
    println!(
        "Cloning {url} as '{name}' {}",
        ui::dim(format!("({} <{}>)", p.name, p.email))
    );
    let mut full = vec!["clone".to_string()];
    full.extend(ca.args);
    let code = git::passthrough(&full)?;
    if code != 0 {
        return Ok(code);
    }
    git::apply_profile(Some(Path::new(&dir)), &name, p, cfg)?;
    println!(
        "{}",
        ui::ok(format!("Repository '{dir}' configured for {}", ui::badge(cfg, &name)))
    );
    Ok(0)
}

pub fn profile_add(cfg: &mut Config, name: &str, fields: &ProfileFields) -> Result<()> {
    validate_profile_name(name)?;
    if cfg.profiles.contains_key(name) {
        bail!("profile '{name}' already exists; use `gg profile edit {name}`");
    }
    let mut p = Profile::with_defaults(name);
    fields.apply(&mut p);
    if fields.author.is_none() || fields.email.is_none() {
        p = interactive::prompt_profile(name, p)?;
    }
    p.validate()?;
    if let Some((other, _)) = cfg.profile_by_alias(&p.ssh_alias) {
        bail!("SSH alias '{}' is already used by profile '{other}'", p.ssh_alias);
    }
    cfg.profiles.insert(name.to_string(), p.clone());
    if cfg.default.is_none() {
        cfg.default = Some(name.to_string());
    }
    cfg.save()?;
    println!("{}", ui::ok(format!("Added profile {}", ui::badge(cfg, name))));
    next_steps(name, &p);
    Ok(())
}

fn next_steps(name: &str, p: &Profile) {
    println!("Next steps:");
    if !p.key_path().exists() {
        println!("  gg ssh keygen {name}   # create {}", p.ssh_key);
    }
    println!(
        "  gg ssh sync            # add '{}' to ~/.ssh/config if missing",
        p.ssh_alias
    );
    println!("  gg ssh test {name}");
}

pub fn profile_edit(cfg: &mut Config, name: &str, fields: &ProfileFields) -> Result<()> {
    let mut p = cfg.get(name)?.clone();
    if fields.is_empty() {
        p = interactive::prompt_profile(name, p)?;
    } else {
        fields.apply(&mut p);
    }
    p.validate()?;
    if let Some((other, _)) = cfg.profile_by_alias(&p.ssh_alias).filter(|(n, _)| n.as_str() != name) {
        bail!("SSH alias '{}' is already used by profile '{other}'", p.ssh_alias);
    }
    let old = cfg.profiles.insert(name.to_string(), p.clone());
    cfg.save()?;
    println!("{}", ui::ok(format!("Updated profile {}", ui::badge(cfg, name))));
    if old.is_some_and(|o| o.ssh_alias != p.ssh_alias || o.ssh_key != p.ssh_key || o.hostname != p.hostname) {
        println!("SSH settings changed: run `gg ssh sync`, then `gg use {name}` in affected repositories.");
    }
    Ok(())
}

pub fn profile_remove(cfg: &mut Config, name: &str) -> Result<()> {
    cfg.get(name)?;
    cfg.profiles.remove(name);
    if cfg.default.as_deref() == Some(name) {
        cfg.default = None;
    }
    cfg.save()?;
    println!(
        "{}",
        ui::ok(format!(
            "Removed profile '{name}' (SSH keys were not deleted). Run `gg ssh sync` to drop its managed SSH entry."
        ))
    );
    Ok(())
}

pub fn ssh_test(cfg: &Config, name: Option<&str>) -> Result<i32> {
    cfg.ensure_profiles()?;
    let targets: Vec<&Profile> = match name {
        Some(n) => vec![cfg.get(n)?],
        None => cfg.profiles.values().collect(),
    };
    let mut ok = true;
    for p in targets {
        ok &= ssh::test(p)?;
    }
    Ok(if ok { 0 } else { 1 })
}

/// Warns before commit/push when the repo's identity and SSH account disagree.
fn repo_warning(cfg: &Config) {
    if cfg.profiles.is_empty() || git::repo_root(None).is_none() {
        return;
    }
    let ident = identity_profile(cfg, None);
    let via = git::get(None, &["remote", "get-url", "origin"]).and_then(|u| remote_profile(cfg, &u).cloned());
    match (ident, via) {
        (None, _) => {
            eprintln!(
                "{}",
                ui::warn("gg: this repository has no profile; run `gg use <profile>` to set identity and SSH account.")
            )
        }
        (Some(i), Some(v)) if i != v => {
            eprintln!(
                "{}",
                ui::warn(format!(
                    "gg: commits as '{i}' but origin authenticates as '{v}'. Run `gg use <profile>`."
                ))
            )
        }
        _ => {}
    }
}

pub fn git_passthrough(cfg: &Config, args: &[String]) -> Result<i32> {
    let sub = args.first().map(String::as_str);
    if matches!(sub, Some("push" | "pull" | "fetch")) && git::repo_root(None).is_some() {
        fix_direct_remotes(cfg)?;
    }
    if matches!(sub, Some("commit" | "push")) {
        repo_warning(cfg);
    }
    git::passthrough(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn clone_args() {
        let ca = parse_clone_args(&s(&["--depth", "1", "a/b", "-p", "work", "dest", "-b", "dev"]), None).unwrap();
        assert_eq!(ca.profile.as_deref(), Some("work"));
        assert_eq!(ca.args, s(&["--depth", "1", "a/b", "dest", "-b", "dev"]));
        assert_eq!(ca.positionals, vec![2, 3]);

        let ca = parse_clone_args(&s(&["--bare", "--profile=x", "git@h:a/b.git"]), None).unwrap();
        assert!(ca.bare);
        assert_eq!(ca.profile.as_deref(), Some("x"));
        assert!(parse_clone_args(&s(&["--depth", "1"]), None).is_err());
    }

    #[test]
    fn dir_names() {
        assert_eq!(
            clone_dir_name("git@github-personal:octocat/hello-world.git", false),
            "hello-world"
        );
        assert_eq!(clone_dir_name("https://github.com/a/b/", false), "b");
        assert_eq!(clone_dir_name("git@h:a/b.git", true), "b.git");
    }
}
