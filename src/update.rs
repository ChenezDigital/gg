//! `gg self-update` and the once-a-day "new version available" notice.
//!
//! Downloads go through the system `curl` (or `wget`), checksums through `sha256sum` (or `shasum`) and
//! unpacking through `tar`, the same tools `install.sh` uses, so no HTTP or crypto crates are needed.

use std::fmt;
use std::fs;
use std::io::{ErrorKind, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::ui;

pub const REPO: &str = "ChenezDigital/gg";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    /// `1.2.3` or `v1.2.3`. Pre-releases and build metadata are rejected.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let mut parts = s.strip_prefix('v').unwrap_or(s).split('.').map(|p| {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                p.parse().ok()
            }
        });
        let v = Version(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(v)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// Whether `latest` should replace `current` (a pre-release such as `0.2.0-rc.1` is older than `0.2.0`).
pub fn is_newer(latest: Version, current: &str) -> bool {
    let (base, pre) = current.split_once('-').unwrap_or((current, ""));
    match Version::parse(base) {
        Some(cur) => latest > cur || (latest == cur && !pre.is_empty()),
        None => false,
    }
}

/// The `tag_name` of a GitHub "latest release" API response.
pub fn tag_from_release_json(json: &str) -> Option<&str> {
    let rest = &json[json.find("\"tag_name\"")? + "\"tag_name\"".len()..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    Some(&rest[..rest.find('"')?])
}

/// The release archive target for this build, if one is published.
pub fn target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-musl"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        _ => None,
    }
}

/// For binaries that another tool owns, the command that updates them instead.
pub fn managed_by(exe: &Path) -> Option<String> {
    let s = exe.to_string_lossy();
    if s.contains("/.cargo/bin/") {
        Some(format!(
            "cargo install --locked --force --git https://github.com/{REPO}"
        ))
    } else if s.contains("/Cellar/") || s.contains("/linuxbrew/") || s.contains("/homebrew/") {
        Some("brew upgrade gg".into())
    } else if s.contains("/nix/store/") {
        Some("your Nix configuration".into())
    } else if is_cargo_build_dir(exe) {
        Some("git pull && cargo build (development build)".into())
    } else {
        None
    }
}

/// `…/target/{debug,release}/gg` or `…/target/<triple>/{debug,release}/gg`.
fn is_cargo_build_dir(exe: &Path) -> bool {
    let mut dirs = exe.ancestors().skip(1).filter_map(|p| p.file_name()?.to_str());
    if !matches!(dirs.next(), Some("debug" | "release")) {
        return false;
    }
    dirs.take(2).any(|d| d == "target")
}

fn update_command() -> String {
    std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .ok()
        .and_then(|p| managed_by(&p))
        .unwrap_or_else(|| "gg self-update".into())
}

/// Downloads `url` over HTTPS into `dest`, or returns the body when `dest` is `None`.
fn fetch(url: &str, dest: Option<&Path>, max_secs: u32, api: bool) -> Result<Vec<u8>> {
    let out = dest.map(|d| d.as_os_str().to_owned()).unwrap_or_else(|| "-".into());
    let mut curl = Command::new("curl");
    curl.args([
        "--proto",
        "=https",
        "--tlsv1.2",
        "-sSL",
        "--connect-timeout",
        "15",
        "--max-time",
    ])
    .arg(max_secs.to_string());
    if api {
        // API errors come back as JSON with a message, which `latest` reports.
        curl.args(["-H", "Accept: application/vnd.github+json"]);
    } else {
        curl.arg("-f");
    }
    curl.arg("-o").arg(&out).arg(url);
    let mut wget = Command::new("wget");
    wget.args(["--https-only", "-q", "-T", "15"]);
    if api {
        wget.args(["--content-on-error", "--header=Accept: application/vnd.github+json"]);
    }
    wget.arg("-O").arg(&out).arg(url);
    for mut cmd in [curl, wget] {
        let name = cmd.get_program().to_string_lossy().into_owned();
        match cmd.stdin(Stdio::null()).stderr(Stdio::inherit()).output() {
            Ok(o) if o.status.success() || (api && !o.stdout.is_empty()) => return Ok(o.stdout),
            Ok(o) => bail!("downloading {url} failed ({name} {})", o.status),
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("running {name}")),
        }
    }
    bail!("curl or wget is required")
}

/// The newest published release (pre-releases are never returned by this endpoint).
pub fn latest() -> Result<(String, Version)> {
    let body = fetch(
        &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        None,
        20,
        true,
    )?;
    let body = String::from_utf8_lossy(&body);
    let Some(tag) = tag_from_release_json(&body) else {
        if body.contains("Not Found") {
            bail!("no release of {REPO} has been published yet");
        }
        if body.contains("rate limit") {
            bail!("GitHub API rate limit reached; try again later");
        }
        bail!("unexpected response from the GitHub API");
    };
    let v = Version::parse(tag).with_context(|| format!("unexpected release tag '{tag}'"))?;
    Ok((tag.to_string(), v))
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self> {
        let p = std::env::temp_dir().join(format!("gg-update-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).with_context(|| format!("creating {}", p.display()))?;
        Ok(TempDir(p))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_in(dir: &Path, program: &str, args: &[&str]) -> std::io::Result<bool> {
    Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
}

/// Verifies `gg-<target>.tar.gz` in `dir` against its `.sha256`, unpacks it, checks that the new binary
/// reports version `expect`, then atomically replaces `exe`.
pub fn install_archive(dir: &Path, target: &str, exe: &Path, expect: Version) -> Result<()> {
    let archive = format!("gg-{target}.tar.gz");
    let sums = format!("{archive}.sha256");
    let verified = match run_in(dir, "sha256sum", &["-c", &sums]) {
        Err(e) if e.kind() == ErrorKind::NotFound => run_in(dir, "shasum", &["-a", "256", "-c", &sums]),
        r => r,
    }
    .context("sha256sum or shasum is required to verify the download")?;
    if !verified {
        bail!("checksum mismatch for {archive}; nothing was changed");
    }
    if !run_in(dir, "tar", &["-xzf", &archive]).context("running tar")? {
        bail!("could not unpack {archive}");
    }
    let new = dir.join(format!("gg-{target}")).join("gg");
    let out = Command::new(&new)
        .arg("--version")
        .output()
        .with_context(|| format!("running {}", new.display()))?;
    let reported = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if reported != format!("gg {expect}") {
        bail!("the downloaded binary reports '{reported}', expected 'gg {expect}'; nothing was changed");
    }

    // Stage next to the old binary so the final rename stays on one filesystem and is atomic.
    let staged = exe.with_file_name(format!(".gg-update-{}", std::process::id()));
    let result = (|| {
        fs::copy(&new, &staged)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
        }
        fs::rename(&staged, exe)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&staged);
        if e.kind() == ErrorKind::PermissionDenied {
            bail!(
                "no permission to replace {}; run `sudo gg self-update` or reinstall to a directory you own",
                exe.display()
            );
        }
        return Err(e).with_context(|| format!("replacing {}", exe.display()));
    }
    Ok(())
}

pub fn self_update(check_only: bool) -> Result<()> {
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .context("cannot locate the gg binary")?;
    if !check_only && let Some(how) = managed_by(&exe) {
        bail!("gg cannot replace {} itself; update it with: {how}", exe.display());
    }
    let (tag, latest) = latest().context("checking for the latest release")?;
    if let Some(path) = cache_path() {
        write_cache(&path, now(), Some(latest));
    }
    if !is_newer(latest, CURRENT) {
        println!("{}", ui::ok(format!("gg {CURRENT} is the latest version")));
        return Ok(());
    }
    if check_only {
        println!(
            "{}",
            ui::warn(format!(
                "gg {latest} is available (you have {CURRENT}). Update with: {}",
                update_command()
            ))
        );
        return Ok(());
    }
    let target = target().context("no prebuilt binary for this platform; build from source instead")?;
    let dir = TempDir::new()?;
    let base = format!("https://github.com/{REPO}/releases/download/{tag}");
    let archive = format!("gg-{target}.tar.gz");
    println!("{}", ui::dim(format!("Downloading gg {latest} for {target}…")));
    for name in [archive.clone(), format!("{archive}.sha256")] {
        fetch(&format!("{base}/{name}"), Some(&dir.0.join(&name)), 300, false)?;
    }
    install_archive(&dir.0, target, &exe, latest)?;
    println!(
        "{}",
        ui::ok(format!("Updated gg {CURRENT} → {latest} ({})", exe.display()))
    );
    println!(
        "{}",
        ui::dim(format!("Release notes: https://github.com/{REPO}/releases/tag/{tag}"))
    );
    Ok(())
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `$XDG_CACHE_HOME/gg/latest-version` (`~/Library/Caches/gg/…` on macOS).
fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("gg").join("latest-version"))
}

/// Cache format: `<unix time of the last check> <latest version or ->`.
fn read_cache(path: &Path) -> Option<(u64, Option<Version>)> {
    let text = fs::read_to_string(path).ok()?;
    let (t, v) = text.trim().split_once(' ')?;
    Some((t.parse().ok()?, Version::parse(v)))
}

fn write_cache(path: &Path, checked: u64, latest: Option<Version>) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let v = latest.map_or("-".to_string(), |v| v.to_string());
    let _ = fs::write(path, format!("{checked} {v}\n"));
}

fn notices_enabled(config_enabled: bool) -> bool {
    config_enabled
        && std::env::var_os("GG_NO_UPDATE_CHECK").is_none_or(|v| v.is_empty() || v == "0")
        && std::env::var_os("CI").is_none()
        && std::io::stderr().is_terminal()
}

/// Runs after a command: prints a one-line hint when a newer release is known, and refreshes that
/// knowledge in a detached background process at most once a day. It never blocks or fails the command.
pub fn notify(config_enabled: bool) {
    if !notices_enabled(config_enabled) {
        return;
    }
    let Some(path) = cache_path() else { return };
    let cached = read_cache(&path);
    let known = cached.and_then(|(_, v)| v);
    if let Some(latest) = known
        && is_newer(latest, CURRENT)
    {
        eprintln!(
            "\n{}",
            ui::warn(format!(
                "gg {latest} is available (you have {CURRENT}). Update with: {}",
                update_command()
            ))
        );
    }
    let now = now();
    if cached.is_none_or(|(t, _)| now.saturating_sub(t) >= CHECK_INTERVAL_SECS || t > now) {
        // Record the attempt first so an offline machine does not retry on every command.
        write_cache(&path, now, known);
        spawn_background_check();
    }
}

fn spawn_background_check() {
    let Ok(exe) = std::env::current_exe() else { return };
    let mut cmd = Command::new(exe);
    cmd.arg(BACKGROUND_CHECK_ARG)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group: Ctrl-C in the terminal does not reach it.
        cmd.process_group(0);
    }
    let _ = cmd.spawn();
}

/// Hidden subcommand run by `spawn_background_check`.
pub const BACKGROUND_CHECK_ARG: &str = "__update-check";

pub fn background_check() {
    if let (Ok((_, v)), Some(path)) = (latest(), cache_path()) {
        write_cache(&path, now(), Some(v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(Version::parse("v1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(Version::parse("0.10.0"), Some(Version(0, 10, 0)));
        for bad in ["1.2", "1.2.3.4", "v1.2.3-rc.1", "1..3", "x.y.z", "", "+1.2.3"] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        assert!(is_newer(Version(0, 2, 0), "0.1.9"));
        assert!(!is_newer(Version(0, 2, 0), "0.2.0"));
        assert!(!is_newer(Version(0, 1, 0), "0.2.0"));
        assert!(is_newer(Version(0, 2, 0), "0.2.0-rc.1"));
    }

    #[test]
    fn release_json() {
        let json = r#"{"url":"x","tag_name" : "v0.3.1","name":"gg v0.3.1"}"#;
        assert_eq!(tag_from_release_json(json), Some("v0.3.1"));
        assert_eq!(tag_from_release_json(r#"{"message":"Not Found"}"#), None);
        assert_eq!(tag_from_release_json(r#"{"tag_name": 1}"#), None);
    }

    #[test]
    fn managed_installs() {
        assert!(
            managed_by(Path::new("/home/a/.cargo/bin/gg"))
                .unwrap()
                .contains("cargo install")
        );
        assert!(managed_by(Path::new("/home/linuxbrew/.linuxbrew/Cellar/gg/0.1.0/bin/gg")).is_some());
        assert!(managed_by(Path::new("/src/gg/target/release/gg")).is_some());
        assert!(managed_by(Path::new("/src/gg/target/x86_64-unknown-linux-musl/debug/gg")).is_some());
        assert_eq!(managed_by(Path::new("/opt/release/bin/gg")), None);
        assert_eq!(managed_by(Path::new("/home/a/.local/bin/gg")), None);
        assert_eq!(managed_by(Path::new("/usr/local/bin/gg")), None);
    }

    #[test]
    fn cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gg").join("latest-version");
        assert_eq!(read_cache(&path), None);
        write_cache(&path, 42, Some(Version(1, 2, 3)));
        assert_eq!(read_cache(&path), Some((42, Some(Version(1, 2, 3)))));
        write_cache(&path, 43, None);
        assert_eq!(read_cache(&path), Some((43, None)));
    }

    #[cfg(unix)]
    #[test]
    fn installs_only_verified_archives() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let target = "test-target";
        let make_release = |version: &str| {
            let dl = tmp.path().join(format!("dl-{version}"));
            let pkg = dl.join(format!("gg-{target}"));
            fs::create_dir_all(&pkg).unwrap();
            let bin = pkg.join("gg");
            fs::write(&bin, format!("#!/bin/sh\necho 'gg {version}'\n")).unwrap();
            fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
            let archive = format!("gg-{target}.tar.gz");
            assert!(run_in(&dl, "tar", &["-czf", &archive, &format!("gg-{target}")]).unwrap());
            fs::remove_dir_all(&pkg).unwrap();
            let sums = Command::new("sh")
                .arg("-c")
                .arg(format!("sha256sum {archive} 2>/dev/null || shasum -a 256 {archive}"))
                .current_dir(&dl)
                .output()
                .unwrap();
            fs::write(dl.join(format!("{archive}.sha256")), sums.stdout).unwrap();
            dl
        };
        let bin_dir = tmp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let exe = bin_dir.join("gg");
        fs::write(&exe, "old").unwrap();

        // Wrong version inside the archive.
        let dl = make_release("9.9.8");
        let err = install_archive(&dl, target, &exe, Version(9, 9, 9)).unwrap_err();
        assert!(err.to_string().contains("expected 'gg 9.9.9'"), "{err:#}");
        assert_eq!(fs::read_to_string(&exe).unwrap(), "old");

        // Tampered archive.
        let dl = make_release("9.9.9");
        let archive = dl.join(format!("gg-{target}.tar.gz"));
        let mut bytes = fs::read(&archive).unwrap();
        bytes.push(0);
        fs::write(&archive, &bytes).unwrap();
        let err = install_archive(&dl, target, &exe, Version(9, 9, 9)).unwrap_err();
        assert!(err.to_string().contains("checksum mismatch"), "{err:#}");
        assert_eq!(fs::read_to_string(&exe).unwrap(), "old");

        // Good archive.
        let dl = make_release("9.9.9");
        install_archive(&dl, target, &exe, Version(9, 9, 9)).unwrap();
        assert!(fs::read_to_string(&exe).unwrap().contains("gg 9.9.9"));
        assert_eq!(fs::metadata(&exe).unwrap().permissions().mode() & 0o777, 0o755);
        assert_eq!(
            fs::read_dir(&bin_dir).unwrap().count(),
            1,
            "no staging file left behind"
        );
    }
}
