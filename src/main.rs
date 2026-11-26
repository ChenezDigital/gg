use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{CommandFactory, Parser, Subcommand};

use gg::commands::{self, ProfileFields};
use gg::config::Config;
use gg::{git, interactive, ssh, update};

#[derive(Parser)]
#[command(
    name = "gg",
    version,
    about = "Switch between Git accounts (identity + SSH key) and run git with the right one",
    after_help = "Any other command is passed straight to git, e.g. `gg commit -m msg`, `gg push`, `gg log`.\n\
                  Use `gg git <args>` for git commands whose names gg uses itself.\n\
                  Run `gg` with no arguments for the interactive dashboard."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show the current identity, remotes and profiles
    Whoami,
    /// In a repository: set its author identity and remote URLs to a profile. Elsewhere: set the default profile
    Use {
        /// Profile name (prompts if omitted)
        profile: Option<String>,
        /// Also make it the default for new clones
        #[arg(long)]
        default: bool,
    },
    /// Set the default profile used for new clones
    Default {
        /// Profile name (prompts if omitted)
        profile: Option<String>,
    },
    /// Clone via a profile's SSH alias and configure the new repository (accepts owner/repo, SSH or HTTPS URLs)
    #[command(disable_help_flag = true)]
    Clone {
        /// Profile to clone as [default: the default profile]
        #[arg(short, long)]
        profile: Option<String>,
        /// <repo> [dir] plus any `git clone` options
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        args: Vec<String>,
    },
    /// Manage remotes; URLs (incl. owner/repo shorthand) are routed through the repository's profile
    #[command(args_conflicts_with_subcommands = true)]
    Remote {
        #[command(subcommand)]
        cmd: Option<RemoteCmd>,
        /// Accepted for `git remote -v` compatibility (URLs are always shown)
        #[arg(short, long)]
        verbose: bool,
    },
    /// Manage profiles
    #[command(subcommand)]
    Profile(ProfileCmd),
    /// Manage SSH keys and ~/.ssh/config host aliases
    #[command(subcommand)]
    Ssh(SshCmd),
    /// Guided setup: import existing ~/.ssh/config hosts, create profiles, keys and SSH aliases
    Setup,
    /// Print shell completions, e.g. `gg completions bash > ~/.local/share/bash-completion/completions/gg`
    Completions { shell: clap_complete::Shell },
    /// Update gg to the latest release (verifies the SHA-256 checksum before replacing the binary)
    SelfUpdate {
        /// Only report whether a newer release exists
        #[arg(long)]
        check: bool,
    },
    #[command(name = update::BACKGROUND_CHECK_ARG, hide = true)]
    UpdateCheck,
    /// Run git directly with the given arguments
    Git {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Subcommand)]
enum RemoteCmd {
    /// List remotes and the account each one authenticates as (default)
    List,
    /// Add a remote, e.g. `gg remote add origin octocat/repo`
    Add {
        name: String,
        /// owner/repo, SSH or HTTPS URL
        url: String,
        /// Profile whose SSH alias to use [default: the repository's profile]
        #[arg(short, long)]
        profile: Option<String>,
        /// Fetch right after adding
        #[arg(short, long)]
        fetch: bool,
    },
    /// Change a remote's URL (prompts if URL is omitted)
    #[command(visible_alias = "set-url")]
    Edit {
        name: String,
        url: Option<String>,
        #[arg(short, long)]
        profile: Option<String>,
        /// Change the push URL only
        #[arg(long)]
        push: bool,
    },
    /// Delete a remote
    #[command(visible_aliases = ["remove", "rm"])]
    Delete { name: String },
    #[command(external_subcommand)]
    Other(Vec<String>),
}

#[derive(Subcommand)]
enum ProfileCmd {
    /// List profiles (★ or * marks the default)
    List,
    /// Add a profile (prompts for anything not given as a flag)
    Add {
        name: String,
        #[command(flatten)]
        fields: ProfileFields,
    },
    /// Edit a profile (interactive when no flags are given)
    Edit {
        name: String,
        #[command(flatten)]
        fields: ProfileFields,
    },
    /// Remove a profile (keeps SSH keys)
    Remove { name: String },
}

#[derive(Subcommand)]
enum SshCmd {
    /// Show how ~/.ssh/config matches the profiles (gg keeps it in sync automatically; your own entries are never modified)
    Sync,
    /// Generate an ed25519 key for a profile
    Keygen {
        profile: String,
        /// Create the key without a passphrase
        #[arg(long)]
        no_passphrase: bool,
    },
    /// Print a profile's public key
    Pubkey { profile: String },
    /// Check which account each profile's SSH alias authenticates as
    Test { profile: Option<String> },
}

fn pick(cfg: &Config, given: Option<String>, prompt: &str) -> Result<Option<String>> {
    if given.is_some() {
        return Ok(given);
    }
    if !interactive::is_tty() {
        bail!("missing profile name");
    }
    let current = git::get(None, &["config", "--local", "gg.profile"]).or(cfg.default.clone());
    interactive::pick_profile(cfg, prompt, current.as_deref())
}

fn run(cli: Cli) -> Result<i32> {
    let Some(cmd) = cli.command else {
        if interactive::is_tty() {
            return interactive::menu();
        }
        Cli::command().print_help()?;
        return Ok(0);
    };
    let mut cfg = Config::load()?;
    // `gg ssh sync` and `gg setup` report the sync themselves.
    if !matches!(cmd, Cmd::Ssh(SshCmd::Sync) | Cmd::Setup) {
        ssh::auto_sync(&cfg, false);
    }
    match cmd {
        Cmd::Whoami => commands::whoami(&cfg)?,
        Cmd::Use { profile, default } => {
            if let Some(p) = pick(&cfg, profile, "Use profile")? {
                commands::use_profile(&mut cfg, &p, default)?;
            }
        }
        Cmd::Default { profile } => {
            if let Some(p) = pick(&cfg, profile, "Default profile for new clones")? {
                commands::set_default(&mut cfg, &p)?;
            }
        }
        Cmd::Clone { profile, args } => {
            if args.iter().any(|a| a == "-h" || a == "--help") {
                Cli::command().find_subcommand_mut("clone").unwrap().print_help()?;
                return Ok(0);
            }
            return commands::clone(&cfg, profile.as_deref(), &args);
        }
        Cmd::Remote { cmd, .. } => match cmd.unwrap_or(RemoteCmd::List) {
            RemoteCmd::List => commands::remote_list(&cfg)?,
            RemoteCmd::Add {
                name,
                url,
                profile,
                fetch,
            } => {
                return commands::remote_add(&cfg, profile.as_deref(), &name, &url, fetch);
            }
            RemoteCmd::Edit {
                name,
                url,
                profile,
                push,
            } => {
                return commands::remote_edit(&cfg, profile.as_deref(), &name, url.as_deref(), push);
            }
            RemoteCmd::Delete { name } => return commands::remote_delete(&name),
            RemoteCmd::Other(args) => {
                let mut full = vec!["remote".to_string()];
                full.extend(args);
                return git::passthrough(&full);
            }
        },
        Cmd::Profile(c) => match c {
            ProfileCmd::List => commands::list_profiles(&cfg),
            ProfileCmd::Add { name, fields } => commands::profile_add(&mut cfg, &name, &fields)?,
            ProfileCmd::Edit { name, fields } => commands::profile_edit(&mut cfg, &name, &fields)?,
            ProfileCmd::Remove { name } => commands::profile_remove(&mut cfg, &name)?,
        },
        Cmd::Ssh(c) => match c {
            SshCmd::Sync => {
                cfg.ensure_profiles()?;
                ssh::sync(&cfg)?
            }
            SshCmd::Keygen { profile, no_passphrase } => {
                let p = cfg.get(&profile)?;
                ssh::keygen(p, no_passphrase)?;
                interactive::share_public_key(&profile, p)?;
                println!("Then run: gg ssh test {profile}");
            }
            SshCmd::Pubkey { profile } => ssh::print_public_key(cfg.get(&profile)?)?,
            SshCmd::Test { profile } => return commands::ssh_test(&cfg, profile.as_deref()),
        },
        Cmd::Setup => interactive::setup(&mut cfg)?,
        Cmd::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "gg", &mut std::io::stdout());
        }
        Cmd::SelfUpdate { check } => update::self_update(check)?,
        Cmd::UpdateCheck => update::background_check(),
        Cmd::Git { args } => return git::passthrough(&args),
        Cmd::External(args) => return commands::git_passthrough(&cfg, &args),
    }
    Ok(0)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let notice = !matches!(
        cli.command,
        Some(Cmd::SelfUpdate { .. } | Cmd::UpdateCheck | Cmd::Completions { .. })
    );
    match run(cli) {
        Ok(code) => {
            if notice {
                update::notify(Config::load().map_or(true, |c| c.update_check != Some(false)));
            }
            ExitCode::from(code.clamp(0, 255) as u8)
        }
        // A Ctrl-C can fail the pending read before the signal handler exits; report it the same way.
        Err(e) if interactive::is_interrupt(&e) => ExitCode::from(130),
        Err(e) => {
            eprintln!("gg: {e:#}");
            ExitCode::from(1)
        }
    }
}
