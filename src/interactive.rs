use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail};
use console::style;
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input, Select};

use crate::commands;
use crate::config::{Config, Profile, config_path, validate_profile_name};
use crate::{gh, git, ssh};

pub fn is_tty() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn require_tty(what: &str) -> Result<()> {
    if !is_tty() {
        bail!("{what} needs an interactive terminal; see `gg --help` for non-interactive options");
    }
    install_interrupt_handler();
    Ok(())
}

/// Terminal settings (echo, line mode, ...) that child programs or a Ctrl-C may leave changed.
#[cfg(unix)]
pub type TtyState = libc::termios;
#[cfg(windows)]
pub type TtyState = u32;

/// Snapshot of the terminal settings, if stdin is a terminal.
#[cfg(unix)]
pub fn save_tty() -> Option<TtyState> {
    // SAFETY: tcgetattr only writes into the zeroed struct we own.
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        (libc::tcgetattr(libc::STDIN_FILENO, &mut t) == 0).then_some(t)
    }
}

#[cfg(unix)]
pub fn restore_tty(saved: Option<TtyState>) {
    if let Some(t) = saved {
        // SAFETY: applies attributes previously read from the same descriptor.
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &t) };
    }
}

/// Console input mode of `CONIN$`, if there is a console.
#[cfg(windows)]
pub fn save_tty() -> Option<TtyState> {
    use crossterm_winapi::{ConsoleMode, Handle};
    Handle::current_in_handle()
        .and_then(|h| ConsoleMode::from(h).mode())
        .ok()
}

#[cfg(windows)]
pub fn restore_tty(saved: Option<TtyState>) {
    use crossterm_winapi::{ConsoleMode, Handle};
    if let (Some(mode), Ok(h)) = (saved, Handle::current_in_handle()) {
        let _ = ConsoleMode::from(h).set_mode(mode);
    }
}

pub static DASHBOARD: AtomicBool = AtomicBool::new(false);
static PROMPTING: AtomicBool = AtomicBool::new(false);
static CANCELLED: AtomicBool = AtomicBool::new(false);
/// The dashboard moved into a repository cloned from it (see `Action::Clone`).
pub static SWITCHED: AtomicBool = AtomicBool::new(false);
/// A shell started by `open_shell` owns the terminal; its Ctrl-C is not ours.
static CHILD_SHELL: AtomicBool = AtomicBool::new(false);

/// Runs a dialoguer prompt so that a Ctrl-C in the dashboard cancels it as `ErrorKind::Interrupted`.
/// On Unix the read itself fails that way. On Windows the console turns Ctrl-C into a control
/// event and the read keeps blocking, so the handler marks the prompt cancelled and feeds it an
/// Enter key, which ends every prompt kind we use.
fn prompt<T>(f: impl FnOnce() -> dialoguer::Result<T>) -> Result<T> {
    CANCELLED.store(false, Ordering::SeqCst);
    PROMPTING.store(true, Ordering::SeqCst);
    let r = f();
    PROMPTING.store(false, Ordering::SeqCst);
    if CANCELLED.swap(false, Ordering::SeqCst) {
        return Err(std::io::Error::from(std::io::ErrorKind::Interrupted).into());
    }
    Ok(r?)
}

#[cfg(windows)]
fn send_enter_key() {
    use windows_sys::Win32::System::Console::{
        GetStdHandle, INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0, STD_INPUT_HANDLE,
        WriteConsoleInputW,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;

    let key = |down| INPUT_RECORD {
        EventType: KEY_EVENT as u16,
        Event: INPUT_RECORD_0 {
            KeyEvent: KEY_EVENT_RECORD {
                bKeyDown: down,
                wRepeatCount: 1,
                wVirtualKeyCode: VK_RETURN,
                wVirtualScanCode: 0x1C,
                uChar: KEY_EVENT_RECORD_0 {
                    UnicodeChar: u16::from(b'\r'),
                },
                dwControlKeyState: 0,
            },
        },
    };
    let records = [key(1), key(0)];
    let mut written = 0;
    // SAFETY: `records` outlives the call and its length matches the count passed.
    unsafe {
        WriteConsoleInputW(
            GetStdHandle(STD_INPUT_HANDLE),
            records.as_ptr(),
            records.len() as u32,
            &mut written,
        );
    }
}

/// On Ctrl-C (prompts raise SIGINT even in raw mode), put the terminal back the way we found it:
/// original termios (echo, line mode) and a visible cursor.
fn install_interrupt_handler() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let saved = save_tty();
        let _ = ctrlc::set_handler(move || {
            if CHILD_SHELL.load(Ordering::SeqCst) {
                return;
            }
            // In the dashboard a Ctrl-C only cancels the current prompt (see `run_action`).
            if DASHBOARD.load(Ordering::SeqCst) {
                if PROMPTING.load(Ordering::SeqCst) {
                    CANCELLED.store(true, Ordering::SeqCst);
                    #[cfg(windows)]
                    send_enter_key();
                }
                return;
            }
            restore_tty(saved);
            let _ = ratatui::crossterm::execute!(std::io::stderr(), ratatui::crossterm::cursor::Show);
            eprintln!();
            std::process::exit(130);
        });
    });
}

/// Prompt prefixes use the same glyphs as `ui::ok` / `ui::fail`, so answered prompts and
/// status lines share one check/cross style.
/// Asks whether to continue working inside a freshly cloned repository.
pub fn offer_switch(dir: &Path) -> Result<bool> {
    install_interrupt_handler();
    println!();
    confirm(&format!("Switch to '{}' to work in it?", dir.display()), true)
}

/// A process can't change its parent shell's directory, so this starts a new shell in `dir`
/// (`$SHELL` on Unix, PowerShell on Windows) and returns its exit code once the user leaves it.
pub fn open_shell(dir: &Path) -> Result<i32> {
    let shell = shell_program();
    println!(
        "{}",
        crate::ui::dim(format!(
            "Starting {} in {} (type `exit` to return to the previous directory)",
            shell.display(),
            dir.display()
        ))
    );
    let mut cmd = std::process::Command::new(&shell);
    if cfg!(windows) {
        cmd.arg("-NoLogo");
    }
    CHILD_SHELL.store(true, Ordering::SeqCst);
    let status = cmd.current_dir(dir).status();
    CHILD_SHELL.store(false, Ordering::SeqCst);
    let status = status.map_err(|e| anyhow::anyhow!("could not start {}: {e}", shell.display()))?;
    Ok(status.code().unwrap_or(0))
}

fn shell_program() -> PathBuf {
    if cfg!(windows) {
        let on_path = |exe: &str| {
            std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(exe).is_file()))
        };
        return PathBuf::from(if on_path("pwsh.exe") {
            "pwsh.exe"
        } else {
            "powershell.exe"
        });
    }
    std::env::var_os("SHELL")
        .filter(|s| !s.is_empty())
        .map_or_else(|| PathBuf::from("/bin/sh"), PathBuf::from)
}

fn theme() -> ColorfulTheme {
    let g = crate::ui::glyphs();
    ColorfulTheme {
        success_prefix: style(g.check.to_string()).for_stderr().green().bold(),
        error_prefix: style(g.cross.to_string()).for_stderr().red().bold(),
        ..ColorfulTheme::default()
    }
}

pub fn input_text(prompt: &str, default: &str) -> Result<String> {
    require_tty(prompt)?;
    input(prompt, default, false)
}

fn input(label: &str, default: &str, allow_empty: bool) -> Result<String> {
    let t = theme();
    // Empty answers always reach the validator (rather than dialoguer silently re-asking), so the
    // Enter key sent on a Windows Ctrl-C ends the prompt even for required fields.
    let mut i = Input::<String>::with_theme(&t)
        .with_prompt(label)
        .allow_empty(true)
        .validate_with(|v: &String| {
            if allow_empty || !v.trim().is_empty() || CANCELLED.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err("a value is required")
            }
        });
    if !default.is_empty() {
        i = i.default(default.to_string());
    }
    Ok(prompt(|| i.interact_text())?.trim().to_string())
}

fn confirm(label: &str, default: bool) -> Result<bool> {
    prompt(|| {
        Confirm::with_theme(&theme())
            .with_prompt(label)
            .default(default)
            .interact()
    })
}

fn select(label: &str, items: &[String], default: usize) -> Result<Option<usize>> {
    prompt(|| {
        Select::with_theme(&theme())
            .with_prompt(label)
            .items(items)
            .default(default)
            .interact_opt()
    })
}

pub fn prompt_profile(name: &str, mut p: Profile) -> Result<Profile> {
    require_tty("entering profile details")?;
    println!("Profile '{name}':");
    loop {
        p.name = input("Commit author name", &p.name, false)?;
        p.email = input("Commit author email", &p.email, false)?;
        let user = input(
            "Account username (optional, verifies SSH login)",
            p.username.as_deref().unwrap_or(""),
            true,
        )?;
        p.username = Some(user).filter(|u| !u.is_empty());
        p.hostname = input("Git host", &p.hostname, false)?;
        p.ssh_alias = input("SSH host alias", &p.ssh_alias, false)?;
        p.ssh_key = input("SSH private key", &p.ssh_key, false)?;
        match p.validate() {
            Ok(()) => return Ok(p),
            Err(e) => eprintln!("error: {e}. Please try again."),
        }
    }
}

pub fn pick_profile(cfg: &Config, prompt: &str, preselect: Option<&str>) -> Result<Option<String>> {
    cfg.ensure_profiles()?;
    let names: Vec<&String> = cfg.profiles.keys().collect();
    let items: Vec<String> = cfg
        .profiles
        .iter()
        .map(|(n, p)| format!("{n:<12} {} <{}>  via {}", p.name, p.email, p.ssh_alias))
        .collect();
    let default = preselect
        .and_then(|d| names.iter().position(|n| n.as_str() == d))
        .unwrap_or(0);
    Ok(select(prompt, &items, default)?.map(|i| names[i].clone()))
}

fn ask_new_profile_name(cfg: &Config, suggested: &str) -> Result<String> {
    loop {
        let name = input("Profile name", suggested, false)?;
        match validate_profile_name(&name) {
            Err(e) => eprintln!("error: {e}"),
            Ok(()) if cfg.profiles.contains_key(&name) => eprintln!("error: profile '{name}' already exists"),
            Ok(()) => return Ok(name),
        }
    }
}

/// Asks for a profile name and the details of a `gh` account, pre-filled from its GitHub profile.
fn profile_from_gh(cfg: &Config, acc: &gh::Account) -> Result<(String, Profile)> {
    let name = ask_new_profile_name(cfg, &gh::profile_name_for(&acc.user))?;
    let mut base = Profile::with_defaults(&name);
    fill_from_gh(&mut base, acc);
    let p = prompt_profile(&name, base)?;
    Ok((name, p))
}

/// Copies a `gh` account's host and username into `p`, plus the name and email its GitHub
/// profile reports (left as they are when it can't be read).
fn fill_from_gh(p: &mut Profile, acc: &gh::Account) {
    println!(
        "{}",
        crate::ui::dim(format!("Reading {}'s profile on {} with gh…", acc.user, acc.host))
    );
    let d = gh::details(acc).unwrap_or_else(|e| {
        eprintln!(
            "{}",
            crate::ui::warn(format!("could not read the account ({e:#}); enter the details below"))
        );
        gh::Details::default()
    });
    p.hostname = acc.host.clone();
    p.username = Some(acc.user.clone());
    if let Some(n) = d.name {
        p.name = n;
    }
    if let Some(e) = d.email {
        p.email = e;
    }
}

/// Offers `candidates` plus a final "none" entry. `Ok(None)` means Esc was pressed.
fn pick_gh_account<'a>(
    prompt: &str,
    candidates: &'a [gh::Account],
    none: &str,
) -> Result<Option<Option<&'a gh::Account>>> {
    let mut items: Vec<String> = candidates
        .iter()
        .map(|a| format!("{} on {}  (from GitHub CLI)", a.user, a.host))
        .collect();
    items.push(none.to_string());
    Ok(select(prompt, &items, 0)?.map(|i| candidates.get(i)))
}

/// For `gg profile add <name>` without author and email: offers the `gh` accounts that have no
/// profile yet as a starting point. Returns the base profile to prompt with.
pub fn new_profile_base(cfg: &Config, name: &str) -> Result<Profile> {
    let mut p = Profile::with_defaults(name);
    let candidates = if is_tty() { gh::unimported(cfg) } else { Vec::new() };
    if candidates.is_empty() {
        return Ok(p);
    }
    install_interrupt_handler();
    match pick_gh_account("Start from", &candidates, "Enter the details myself")? {
        Some(Some(acc)) => fill_from_gh(&mut p, acc),
        Some(None) => {}
        None => return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled").into()),
    }
    Ok(p)
}

/// Shows a profile's public key. When `gh` is logged in to the profile's account, offers to add
/// the key there (asking for gh's key permission if needed); otherwise says where to paste it.
pub fn share_public_key(name: &str, p: &Profile) -> Result<()> {
    ssh::show_public_key(p)?;
    let acc = if is_tty() { gh::account_for(p) } else { None };
    let Some(acc) = acc else {
        ssh::print_key_hint(p);
        return Ok(());
    };
    install_interrupt_handler();
    println!();
    if !confirm(
        &format!("Add this key to the {} account '{}' with gh?", acc.host, acc.user),
        true,
    )? {
        ssh::print_key_hint(p);
        return Ok(());
    }
    let title = format!("gg {name} ({})", p.ssh_alias);
    let key = ssh::public_key_path(p);
    let mut granted = false;
    loop {
        match gh::add_key(&acc, &key, &title) {
            Ok(gh::Upload::Added) => {
                println!(
                    "{}",
                    crate::ui::ok(format!("Key added to {} on {}", acc.user, acc.host))
                );
                return Ok(());
            }
            Ok(gh::Upload::NeedsScope) if !granted => {
                println!(
                    "gh's sign-in for '{}' may not add SSH keys yet (it needs the {} permission).",
                    acc.user,
                    gh::KEY_SCOPE
                );
                if !confirm("Grant it now? gh opens a browser sign-in", true)? {
                    break;
                }
                if let Err(e) = gh::grant_key_scope(&acc) {
                    eprintln!("{}", crate::ui::fail(format!("{e:#}")));
                    break;
                }
                granted = true;
            }
            Ok(gh::Upload::NeedsScope) => {
                eprintln!(
                    "{}",
                    crate::ui::fail(format!("gh still lacks the {} permission", gh::KEY_SCOPE))
                );
                break;
            }
            Err(e) => {
                eprintln!("{}", crate::ui::fail(format!("{e:#}")));
                break;
            }
        }
    }
    ssh::print_key_hint(p);
    Ok(())
}

fn insert_profile(cfg: &mut Config, name: String, p: Profile) -> Result<()> {
    if let Some((other, _)) = cfg.profile_by_alias(&p.ssh_alias) {
        bail!("SSH alias '{}' is already used by profile '{other}'", p.ssh_alias);
    }
    cfg.profiles.insert(name.clone(), p);
    if cfg.default.is_none() {
        cfg.default = Some(name.clone());
    }
    cfg.save()?;
    println!("Saved profile '{name}'.");
    Ok(())
}

pub fn setup(cfg: &mut Config) -> Result<()> {
    require_tty("gg setup")?;
    println!("gg setup — profiles are stored in {}\n", config_path()?.display());

    for h in ssh::detect_hosts() {
        if cfg.profile_by_alias(&h.alias).is_some() {
            continue;
        }
        let (Some(hostname), Some(key)) = (&h.hostname, &h.identity_file) else {
            continue;
        };
        if !confirm(
            &format!(
                "Found SSH host '{}' ({hostname}, key {key}). Create a profile for it?",
                h.alias
            ),
            true,
        )? {
            continue;
        }
        let suggested = h.alias.split_once('-').map_or(h.alias.as_str(), |(_, rest)| rest);
        let name = ask_new_profile_name(cfg, suggested)?;
        let mut base = Profile {
            hostname: hostname.clone(),
            ssh_alias: h.alias.clone(),
            ssh_key: key.clone(),
            ..Profile::with_defaults(&name)
        };
        // Linking the host to its gh account fills in the details and keeps that account from
        // being offered again as a separate profile below.
        let candidates: Vec<gh::Account> = gh::unimported(cfg)
            .into_iter()
            .filter(|a| a.host.eq_ignore_ascii_case(hostname))
            .collect();
        if !candidates.is_empty()
            && let Some(Some(acc)) = pick_gh_account(
                &format!("Which GitHub CLI account does '{}' sign in as?", h.alias),
                &candidates,
                "None of these / not sure",
            )?
        {
            fill_from_gh(&mut base, acc);
            base.hostname = hostname.clone();
        }
        let p = prompt_profile(&name, base)?;
        insert_profile(cfg, name, p)?;
    }

    for acc in gh::unimported(cfg) {
        if !confirm(
            &format!(
                "Found GitHub CLI account '{}' ({}). Create a profile for it?",
                acc.user, acc.host
            ),
            true,
        )? {
            continue;
        }
        let (name, p) = profile_from_gh(cfg, &acc)?;
        if let Err(e) = insert_profile(cfg, name, p) {
            eprintln!("error: {e}");
        }
    }

    loop {
        let prompt = if cfg.profiles.is_empty() {
            "Add a profile?"
        } else {
            "Add another profile?"
        };
        if !confirm(prompt, cfg.profiles.is_empty())? {
            break;
        }
        let name = ask_new_profile_name(cfg, if cfg.profiles.is_empty() { "personal" } else { "work" })?;
        let p = prompt_profile(&name, Profile::with_defaults(&name))?;
        if let Err(e) = insert_profile(cfg, name, p) {
            eprintln!("error: {e}");
        }
    }
    if cfg.profiles.is_empty() {
        println!("No profiles configured.");
        return Ok(());
    }

    if cfg.profiles.len() > 1
        && let Some(d) = pick_profile(cfg, "Default profile for new clones", cfg.default.clone().as_deref())?
    {
        cfg.default = Some(d);
        cfg.save()?;
    }

    let missing: Vec<(String, Profile)> = cfg
        .profiles
        .iter()
        .filter(|(_, p)| !p.key_path().exists())
        .map(|(n, p)| (n.clone(), p.clone()))
        .collect();
    for (name, p) in missing {
        if confirm(
            &format!("SSH key {} for '{name}' does not exist. Generate it?", p.ssh_key),
            true,
        )? {
            ssh::keygen(&p, false)?;
            share_public_key(&name, &p)?;
        }
    }

    ssh::sync(cfg)?;

    if confirm("Test SSH connections now?", true)? {
        commands::ssh_test(cfg, None)?;
    }
    println!("\nDone. Try `gg whoami`, `gg clone owner/repo`, or just `gg`.");
    Ok(())
}

/// Everything the dashboard can run. Returns whether output was produced that the user should read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Status,
    Commit,
    Pull,
    Push,
    Branch,
    Log,
    UseHere,
    RemoteAdd,
    RemoteEdit,
    RemoteDelete,
    Whoami,
    Clone,
    Default,
    ProfileAdd,
    ProfileEdit,
    ProfileRemove,
    SshTest,
    SshKeygen,
    SshPubkey,
    Setup,
    Quit,
}

pub fn menu() -> Result<i32> {
    require_tty("The interactive menu")?;
    let mut cfg = Config::load()?;
    ssh::auto_sync(&cfg, false);
    if cfg.profiles.is_empty() {
        println!("Welcome to gg! No profiles are configured yet.");
        if !confirm("Run setup now?", true)? {
            return Ok(0);
        }
        setup(&mut cfg)?;
        if cfg.profiles.is_empty() {
            return Ok(0);
        }
    }
    crate::tui::run()?;
    if SWITCHED.load(Ordering::SeqCst) {
        return open_shell(&std::env::current_dir()?);
    }
    Ok(0)
}

fn pick_remote(prompt: &str) -> Result<Option<String>> {
    let remotes = git::remotes(None);
    if remotes.is_empty() {
        println!("No remotes.");
        return Ok(None);
    }
    Ok(select(prompt, &remotes, 0)?.map(|i| remotes[i].clone()))
}

/// Runs an action in the normal (non-TUI) terminal. Ok(false) means it was cancelled.
pub fn run_action(cfg: &mut Config, action: Action) -> Result<bool> {
    let _ = require_tty("gg");
    match run_action_inner(cfg, action) {
        Err(e) if is_interrupt(&e) => {
            println!();
            Ok(false)
        }
        other => other,
    }
}

pub fn is_interrupt(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        let io = match c.downcast_ref::<dialoguer::Error>() {
            Some(dialoguer::Error::IO(io)) => Some(io),
            _ => c.downcast_ref::<std::io::Error>(),
        };
        io.is_some_and(|io| io.kind() == std::io::ErrorKind::Interrupted)
    })
}

fn run_action_inner(cfg: &mut Config, action: Action) -> Result<bool> {
    match action {
        Action::Quit => return Ok(false),
        Action::Whoami => commands::whoami(cfg)?,
        Action::UseHere => {
            let current = git::get(None, &["config", "--local", "gg.profile"]).or(cfg.default.clone());
            let Some(n) = pick_profile(cfg, "Use profile for this repository", current.as_deref())? else {
                return Ok(false);
            };
            commands::use_profile(cfg, &n, false)?;
        }
        Action::Default => {
            let Some(n) = pick_profile(cfg, "Default profile for new clones", cfg.default.clone().as_deref())? else {
                return Ok(false);
            };
            commands::set_default(cfg, &n)?;
        }
        Action::Clone => {
            let repo = input("Repository (owner/repo or URL)", "", false)?;
            let Some(profile) = pick_profile(cfg, "Clone as", cfg.default.clone().as_deref())? else {
                return Ok(false);
            };
            let dir = input("Directory (empty for default)", "", true)?;
            let mut args = vec![repo];
            if !dir.is_empty() {
                args.push(dir);
            }
            if let (_, Some(dir)) = commands::clone_repo(cfg, Some(&profile), &args)?
                && offer_switch(&dir)?
            {
                std::env::set_current_dir(&dir)?;
                SWITCHED.store(true, Ordering::SeqCst);
                println!(
                    "{}",
                    crate::ui::dim("gg now works in this repository; leaving gg opens a shell here.")
                );
            }
        }
        Action::Status => {
            git::passthrough(&["status"])?;
        }
        Action::Pull => {
            commands::git_passthrough(cfg, &["pull".into()])?;
        }
        Action::Push => {
            let has_upstream = git::get(None, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).is_some();
            let args: Vec<String> = if !has_upstream && git::remotes(None).iter().any(|r| r == "origin") {
                ["push", "-u", "origin", "HEAD"].map(String::from).to_vec()
            } else {
                vec!["push".into()]
            };
            commands::git_passthrough(cfg, &args)?;
        }
        Action::Commit => {
            if git::get(None, &["status", "--porcelain"]).is_none() {
                println!("Nothing to commit, working tree clean.");
                return Ok(true);
            }
            git::passthrough(&["status", "--short"])?;
            println!();
            let msg = input("Commit message", "", false)?;
            if confirm("Stage all changes first (git add -A)?", true)? && git::passthrough(&["add", "-A"])? != 0 {
                bail!("git add failed");
            }
            commands::git_passthrough(cfg, &["commit".into(), "-m".into(), msg])?;
        }
        Action::Branch => {
            let current = git::get(None, &["branch", "--show-current"]).unwrap_or_default();
            let mut branches: Vec<String> = git::get(None, &["branch", "--format=%(refname:short)"])
                .map(|s| s.lines().map(String::from).collect())
                .unwrap_or_default();
            let default = branches.iter().position(|b| *b == current).unwrap_or(0);
            branches.push("+ Create new branch".into());
            let Some(i) = select("Switch to branch", &branches, default)? else {
                return Ok(false);
            };
            if i == branches.len() - 1 {
                let name = input("New branch name", "", false)?;
                git::passthrough(&["switch", "-c", &name])?;
            } else if branches[i] == current {
                return Ok(false);
            } else {
                git::passthrough(&["switch", &branches[i]])?;
            }
        }
        Action::Log => {
            git::passthrough(&[
                "--no-pager",
                "log",
                "--oneline",
                "--graph",
                "--decorate",
                "--color=always",
                "-n",
                "25",
            ])?;
        }
        Action::RemoteAdd => {
            let has_origin = git::remotes(None).iter().any(|r| r == "origin");
            let name = input("Remote name", if has_origin { "" } else { "origin" }, false)?;
            let url = input("Repository (owner/repo or URL)", "", false)?;
            let current = git::get(None, &["config", "--local", "gg.profile"]).or(cfg.default.clone());
            let profile = if cfg.profiles.is_empty() {
                None
            } else {
                let Some(p) = pick_profile(cfg, "Via profile", current.as_deref())? else {
                    return Ok(false);
                };
                Some(p)
            };
            commands::remote_add(cfg, profile.as_deref(), &name, &url, false)?;
        }
        Action::RemoteEdit => {
            let Some(r) = pick_remote("Edit remote")? else {
                return Ok(false);
            };
            commands::remote_edit(cfg, None, &r, None, false)?;
        }
        Action::RemoteDelete => {
            let Some(r) = pick_remote("Delete remote")? else {
                return Ok(false);
            };
            if !confirm(&format!("Really delete remote '{r}'?"), false)? {
                return Ok(false);
            }
            commands::remote_delete(&r)?;
        }
        Action::ProfileAdd => {
            let candidates = gh::unimported(cfg);
            let choice = if candidates.is_empty() {
                None
            } else {
                let Some(c) = pick_gh_account("Start from", &candidates, "Enter the details myself")? else {
                    return Ok(false);
                };
                c
            };
            match choice {
                Some(acc) => {
                    let (name, p) = profile_from_gh(cfg, acc)?;
                    commands::profile_insert(cfg, &name, p)?;
                }
                None => {
                    let name = ask_new_profile_name(cfg, "")?;
                    commands::profile_add(cfg, &name, &Default::default())?;
                }
            }
        }
        Action::ProfileEdit => {
            let Some(n) = pick_profile(cfg, "Edit profile", None)? else {
                return Ok(false);
            };
            commands::profile_edit(cfg, &n, &Default::default())?;
        }
        Action::ProfileRemove => {
            let Some(n) = pick_profile(cfg, "Remove profile", None)? else {
                return Ok(false);
            };
            if !confirm(&format!("Really remove profile '{n}'?"), false)? {
                return Ok(false);
            }
            commands::profile_remove(cfg, &n)?;
        }
        Action::SshTest => {
            commands::ssh_test(cfg, None)?;
        }
        Action::SshKeygen => {
            let Some(n) = pick_profile(cfg, "Generate key for", None)? else {
                return Ok(false);
            };
            let p = cfg.get(&n)?;
            ssh::keygen(p, false)?;
            share_public_key(&n, p)?;
        }
        Action::SshPubkey => {
            let Some(n) = pick_profile(cfg, "Show public key of", None)? else {
                return Ok(false);
            };
            share_public_key(&n, cfg.get(&n)?)?;
        }
        Action::Setup => setup(cfg)?,
    }
    Ok(true)
}
