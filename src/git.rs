use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::config::{Config, Profile};

#[derive(Debug, PartialEq, Eq)]
pub struct RemoteUrl {
    pub host: String,
    /// Repository path such as `owner/repo.git`.
    pub path: String,
}

pub fn normalize_path(p: &str) -> Option<String> {
    let p = p.trim_matches('/');
    if p.is_empty() {
        return None;
    }
    Some(if p.ends_with(".git") {
        p.to_string()
    } else {
        format!("{p}.git")
    })
}

fn strip_user(s: &str) -> &str {
    s.rsplit_once('@').map_or(s, |(_, h)| h)
}

/// Like Git, only `[A-Za-z0-9+.-]` before `://` makes a URL; anything else is scp-like (`host:path`).
fn is_scheme(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Parses https://, ssh:// and scp-like (`git@host:owner/repo`) remote URLs.
pub fn parse_remote(url: &str) -> Option<RemoteUrl> {
    let url = url.trim();
    if let Some((scheme, rest)) = url.split_once("://")
        && is_scheme(scheme)
    {
        if !matches!(scheme, "https" | "http" | "ssh" | "git" | "git+ssh") {
            return None;
        }
        let (authority, path) = rest.split_once('/')?;
        let host = strip_user(authority);
        let host = host.split_once(':').map_or(host, |(h, _)| h);
        return Some(RemoteUrl {
            host: host.to_string(),
            path: normalize_path(path)?,
        });
    }
    let (left, path) = url.split_once(':')?;
    // As in Git, a separator before the colon means a local path, and so does a drive letter on Windows.
    let is_sep = |c: char| c == '/' || (cfg!(windows) && c == '\\');
    let drive = cfg!(windows) && left.len() == 1 && left.as_bytes()[0].is_ascii_alphabetic();
    if left.is_empty() || left.contains(is_sep) || drive {
        return None;
    }
    Some(RemoteUrl {
        host: strip_user(left).to_string(),
        path: normalize_path(path)?,
    })
}

/// `owner/repo` shorthand (no scheme, host or extra path segments).
pub fn is_shorthand(s: &str) -> bool {
    let mut parts = s.split('/');
    let ok = |x: Option<&str>| x.is_some_and(|x| !x.is_empty() && x != "." && x != "..");
    !s.contains([':', '@', '\\']) && ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

pub fn ssh_url(profile: &Profile, path: &str) -> String {
    format!("git@{}:{}", profile.ssh_alias, path)
}

/// Rewrites a URL so it goes through `profile`'s SSH alias. Returns None when the
/// URL points to a host this profile does not handle.
pub fn rewrite_url(url: &str, profile: &Profile, cfg: &Config) -> Option<String> {
    let r = parse_remote(url)?;
    let same_host = r.host.eq_ignore_ascii_case(&profile.hostname)
        || r.host == profile.ssh_alias
        || cfg
            .profile_by_alias(&r.host)
            .is_some_and(|(_, p)| p.hostname.eq_ignore_ascii_case(&profile.hostname));
    same_host.then(|| ssh_url(profile, &r.path))
}

pub fn run(dir: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("git");
    if let Some(d) = dir {
        cmd.arg("-C").arg(d);
    }
    let out = cmd
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("failed to run git (is it installed?)")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Like `run`, but returns None on failure or empty output (e.g. unset config keys).
pub fn get(dir: Option<&Path>, args: &[&str]) -> Option<String> {
    run(dir, args).ok().filter(|s| !s.is_empty())
}

pub fn repo_root(dir: Option<&Path>) -> Option<String> {
    get(dir, &["rev-parse", "--show-toplevel"])
}

pub fn remotes(dir: Option<&Path>) -> Vec<String> {
    get(dir, &["remote"])
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Runs git with inherited stdio and returns its exit code.
pub fn passthrough<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> Result<i32> {
    let status = Command::new("git")
        .args(args)
        .status()
        .context("failed to run git (is it installed?)")?;
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return Ok(128 + sig);
        }
    }
    Ok(status.code().unwrap_or(1))
}

/// Applies a profile to a repository: local identity, `gg.profile`, and remote URLs.
pub fn apply_profile(dir: Option<&Path>, name: &str, profile: &Profile, cfg: &Config) -> Result<()> {
    run(dir, &["config", "--local", "user.name", &profile.name])?;
    run(dir, &["config", "--local", "user.email", &profile.email])?;
    run(dir, &["config", "--local", "gg.profile", name])?;
    for msg in rewrite_remotes(dir, profile, cfg, false)? {
        println!("  {msg}");
    }
    Ok(())
}

/// Points remotes at the profile's SSH alias and returns a message per change.
/// With `only_direct`, only URLs using the raw hostname (e.g. `git@github.com:`, which
/// authenticates with no account key) are touched; other aliases are left alone.
pub fn rewrite_remotes(dir: Option<&Path>, profile: &Profile, cfg: &Config, only_direct: bool) -> Result<Vec<String>> {
    let mut msgs = Vec::new();
    for remote in remotes(dir) {
        for push in [false, true] {
            let mut args = vec!["remote", "get-url"];
            if push {
                args.push("--push");
            }
            args.push(&remote);
            let Some(url) = get(dir, &args) else { continue };
            if push && get(dir, &["config", &format!("remote.{remote}.pushurl")]).is_none() {
                continue;
            }
            let label = format!("{remote}{}", if push { " (push)" } else { "" });
            if only_direct && !parse_remote(&url).is_some_and(|r| r.host.eq_ignore_ascii_case(&profile.hostname)) {
                continue;
            }
            match rewrite_url(&url, profile, cfg) {
                Some(new) if new != url => {
                    let mut set = vec!["remote", "set-url"];
                    if push {
                        set.push("--push");
                    }
                    set.extend([remote.as_str(), new.as_str()]);
                    run(dir, &set)?;
                    msgs.push(format!("{label}: {url} -> {new}"));
                }
                Some(_) => {}
                None if !only_direct => msgs.push(format!(
                    "{label}: left unchanged ({url} is not on {})",
                    profile.hostname
                )),
                None => {}
            }
        }
    }
    Ok(msgs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> (Config, Profile) {
        let mut c = Config::default();
        let mut personal = Profile::with_defaults("personal");
        personal.ssh_alias = "github-personal".into();
        let mut work = Profile::with_defaults("work");
        work.ssh_alias = "github-work".into();
        c.profiles.insert("personal".into(), personal.clone());
        c.profiles.insert("work".into(), work);
        (c, personal)
    }

    #[test]
    fn parses_common_forms() {
        let want = Some(RemoteUrl {
            host: "github.com".into(),
            path: "octocat/hello-world.git".into(),
        });
        for u in [
            "git@github.com:octocat/hello-world.git",
            "git@github.com:octocat/hello-world",
            "github.com:octocat/hello-world.git",
            "https://github.com/octocat/hello-world",
            "https://github.com/octocat/hello-world.git/",
            "https://user:tok@github.com/octocat/hello-world.git",
            "ssh://git@github.com/octocat/hello-world.git",
            "ssh://git@github.com:22/octocat/hello-world.git",
        ] {
            assert_eq!(parse_remote(u), want, "{u}");
        }
        assert_eq!(parse_remote("/tmp/local/repo"), None);
        assert_eq!(parse_remote(r"C:\src\repo").is_none(), cfg!(windows));
        assert_eq!(parse_remote(r"..\x:y").is_none(), cfg!(windows));
        assert_eq!(parse_remote("./a:b/c"), None);
        assert_eq!(parse_remote("file:///tmp/x"), None);
        // Only a real scheme before "://" makes a URL (found by fuzzing).
        let odd = parse_remote("git@github-work:a b://c").unwrap();
        assert_eq!((odd.host.as_str(), odd.path.as_str()), ("github-work", "a b://c.git"));
    }

    #[test]
    fn rewrites_only_matching_hosts() {
        let (c, p) = cfg();
        let want = Some("git@github-personal:octocat/hello-world.git".to_string());
        assert_eq!(rewrite_url("git@github.com:octocat/hello-world.git", &p, &c), want);
        assert_eq!(rewrite_url("https://github.com/octocat/hello-world", &p, &c), want);
        assert_eq!(rewrite_url("git@github-work:octocat/hello-world.git", &p, &c), want);
        assert_eq!(rewrite_url("git@github-personal:octocat/hello-world.git", &p, &c), want);
        assert_eq!(rewrite_url("git@gitlab.com:octocat/hello-world.git", &p, &c), None);
        assert_eq!(rewrite_url("/srv/repo.git", &p, &c), None);
    }

    #[test]
    fn shorthand() {
        assert!(is_shorthand("octocat/hello-world"));
        assert!(!is_shorthand("octocat"));
        assert!(!is_shorthand("a/b/c"));
        assert!(!is_shorthand("../x"));
        assert!(!is_shorthand("git@github.com:a/b"));
    }
}
