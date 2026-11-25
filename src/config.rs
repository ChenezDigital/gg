use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const DEFAULT_HOSTNAME: &str = "github.com";

fn default_hostname() -> String {
    DEFAULT_HOSTNAME.to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Commit author name (user.name).
    pub name: String,
    /// Commit author email (user.email).
    pub email: String,
    /// Real Git host, e.g. github.com.
    #[serde(default = "default_hostname")]
    pub hostname: String,
    /// SSH Host alias from ~/.ssh/config, e.g. github-personal.
    pub ssh_alias: String,
    /// Private key path, e.g. ~/.ssh/github_personal.
    pub ssh_key: String,
    /// Account login, used to verify `gg ssh test`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

impl Profile {
    pub fn with_defaults(profile_name: &str) -> Self {
        Profile {
            name: String::new(),
            email: String::new(),
            hostname: default_hostname(),
            ssh_alias: format!("github-{profile_name}"),
            ssh_key: format!("~/.ssh/github_{profile_name}"),
            username: None,
        }
    }

    pub fn key_path(&self) -> PathBuf {
        expand_tilde(&self.ssh_key)
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            bail!("author name must not be empty");
        }
        if !self.email.contains('@') {
            bail!("'{}' is not a valid email", self.email);
        }
        for (what, v) in [("hostname", &self.hostname), ("SSH alias", &self.ssh_alias)] {
            if v.is_empty() || v.contains(char::is_whitespace) || v.contains(['/', ':', '@']) {
                bail!("invalid {what} '{v}'");
            }
        }
        if self.ssh_key.trim().is_empty() {
            bail!("SSH key path must not be empty");
        }
        Ok(())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// `false` turns off the daily check for new releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_check: Option<bool>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

/// `$GG_CONFIG`, else `$XDG_CONFIG_HOME/gg/config.toml`, else `~/.config/gg/config.toml`
/// (also on macOS, matching most command-line tools there).
pub fn config_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("GG_CONFIG") {
        return Ok(p.into());
    }
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home_dir().map(|h| h.join(".config")))
        .context("cannot determine config directory")?;
    Ok(dir.join("gg").join("config.toml"))
}

/// The user's home directory. On Windows `%USERPROFILE%` wins, as it does for OpenSSH and Git.
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(p) = std::env::var_os("USERPROFILE").filter(|p| !p.is_empty()) {
        return Some(p.into());
    }
    dirs::home_dir()
}

pub fn expand_tilde(p: &str) -> PathBuf {
    if p == "~" {
        return home_dir().unwrap_or_default();
    }
    if let Some(rest) = p
        .strip_prefix("~/")
        .or_else(|| p.strip_prefix("~\\").filter(|_| cfg!(windows)))
        && let Some(home) = home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(p)
}

pub fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        bail!("profile name '{name}' may only contain letters, digits, '-' and '_'");
    }
    Ok(())
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let text = toml::to_string_pretty(self)?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn ensure_profiles(&self) -> Result<()> {
        if self.profiles.is_empty() {
            bail!("no profiles configured yet. Run `gg setup` or `gg profile add <name>`.");
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<&Profile> {
        self.profiles.get(name).with_context(|| {
            let known: Vec<_> = self.profiles.keys().map(String::as_str).collect();
            format!(
                "unknown profile '{name}' (known: {})",
                if known.is_empty() {
                    "none".into()
                } else {
                    known.join(", ")
                }
            )
        })
    }

    /// The explicit profile, else the default, else the only profile.
    pub fn resolve(&self, explicit: Option<&str>) -> Result<(String, &Profile)> {
        self.ensure_profiles()?;
        let name = match explicit.or(self.default.as_deref()) {
            Some(n) => n.to_string(),
            None if self.profiles.len() == 1 => self.profiles.keys().next().unwrap().clone(),
            None => bail!("no default profile set. Use `gg default <profile>` or pass --profile."),
        };
        let p = self.get(&name)?;
        Ok((name, p))
    }

    pub fn profile_by_alias(&self, alias: &str) -> Option<(&String, &Profile)> {
        self.profiles.iter().find(|(_, p)| p.ssh_alias == alias)
    }

    pub fn profile_by_email(&self, email: &str) -> Option<(&String, &Profile)> {
        self.profiles.iter().find(|(_, p)| p.email.eq_ignore_ascii_case(email))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_resolve() {
        let mut c = Config::default();
        let mut p = Profile::with_defaults("personal");
        p.name = "A".into();
        p.email = "a@x.io".into();
        c.profiles.insert("personal".into(), p.clone());
        let text = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.profiles["personal"], p);
        assert_eq!(back.resolve(None).unwrap().0, "personal");

        c.profiles.insert("work".into(), p);
        assert!(c.resolve(None).is_err());
        c.default = Some("work".into());
        assert_eq!(c.resolve(None).unwrap().0, "work");
        assert_eq!(c.resolve(Some("personal")).unwrap().0, "personal");
        assert!(c.resolve(Some("nope")).is_err());
    }

    #[test]
    fn profile_validation() {
        assert!(validate_profile_name("work-2").is_ok());
        assert!(validate_profile_name("a b").is_err());
        let mut p = Profile::with_defaults("x");
        p.name = "X".into();
        p.email = "bad".into();
        assert!(p.validate().is_err());
        p.email = "x@y.z".into();
        assert!(p.validate().is_ok());
        p.ssh_alias = "a:b".into();
        assert!(p.validate().is_err());
    }
}
