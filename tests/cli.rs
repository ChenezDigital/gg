use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

struct Env {
    home: TempDir,
}

impl Env {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        fs::write(
            home.path().join("gitconfig"),
            "[user]\n\tname = Global\n\temail = global@example.com\n[init]\n\tdefaultBranch = main\n",
        )
        .unwrap();
        Env { home }
    }

    fn apply(&self, cmd: &mut Command) {
        let h = self.home.path();
        cmd.env("HOME", h)
            .env("USERPROFILE", h)
            .env("GG_GLYPHS", "fancy")
            .env("XDG_CONFIG_HOME", h.join(".config"))
            .env("XDG_CACHE_HOME", h.join(".cache"))
            .env("GIT_CONFIG_GLOBAL", h.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_SSH_COMMAND", "false")
            .env_remove("GG_CONFIG");
    }

    fn gg(&self, dir: &Path, args: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_gg"));
        cmd.current_dir(dir).args(args);
        self.apply(&mut cmd);
        cmd.output().unwrap()
    }

    fn ok(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.gg(dir, args);
        assert!(
            out.status.success(),
            "gg {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        cmd.current_dir(dir).args(args);
        self.apply(&mut cmd);
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn with_profiles() -> Self {
        let e = Self::new();
        let h = e.home.path();
        e.ok(
            h,
            &[
                "profile",
                "add",
                "personal",
                "--author",
                "Jane Doe",
                "--email",
                "jane@example.com",
                "--username",
                "octocat",
            ],
        );
        e.ok(
            h,
            &[
                "profile",
                "add",
                "work",
                "--author",
                "John Doe",
                "--email",
                "john@work.example",
                "--key",
                "~/.ssh/github_work",
            ],
        );
        e
    }
}

#[test]
fn profiles_and_default() {
    let e = Env::with_profiles();
    let h = e.home.path();
    let list = e.ok(h, &["profile", "list"]);
    assert!(list.contains("★ ● personal"), "{list}");
    assert!(
        list.contains("git@github-work -> ~/.ssh/github_work  ✗ key missing"),
        "{list}"
    );

    e.ok(h, &["use", "work"]);
    assert!(e.ok(h, &["profile", "list"]).contains("★ ● work"));
    let cfg = fs::read_to_string(h.join(".config/gg/config.toml")).unwrap();
    assert!(cfg.contains("default = \"work\""));

    assert!(
        !e.gg(h, &["profile", "add", "work", "--author", "x", "--email", "x@y"])
            .status
            .success()
    );
    assert!(!e.gg(h, &["use", "nope"]).status.success());
    // Global git identity is never touched.
    assert_eq!(e.git(h, &["config", "--global", "user.email"]), "global@example.com");
}

#[test]
fn ssh_sync_preserves_user_entries() {
    let e = Env::with_profiles();
    let h = e.home.path();
    fs::create_dir_all(h.join(".ssh")).unwrap();
    let mine = "Host github-personal\n    HostName github.com\n    User git\n    IdentityFile ~/.ssh/github_personal\n    IdentitiesOnly yes\n";
    fs::write(h.join(".ssh/config"), mine).unwrap();

    // Any gg command adds the missing alias on its own.
    let out = e.gg(h, &["whoami"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("+ github-work"), "{err}");
    let cfg = fs::read_to_string(h.join(".ssh/config")).unwrap();
    assert!(cfg.contains(mine));
    assert_eq!(cfg.matches("Host github-personal").count(), 1);
    assert!(
        cfg.contains("Host github-work\n    HostName github.com\n    User git\n    IdentityFile ~/.ssh/github_work")
    );
    assert_eq!(fs::read_to_string(h.join(".ssh/config.gg-backup")).unwrap(), mine);

    let out = e.gg(h, &["ssh", "sync"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("up to date"));
    assert_eq!(fs::read_to_string(h.join(".ssh/config")).unwrap(), cfg);

    // Removing a profile drops its managed entry without an explicit sync.
    e.ok(h, &["profile", "remove", "work"]);
    assert_eq!(fs::read_to_string(h.join(".ssh/config")).unwrap(), mine);
}

#[test]
fn use_in_repo_sets_identity_and_remotes() {
    let e = Env::with_profiles();
    let repo = e.home.path().join("repo");
    fs::create_dir(&repo).unwrap();
    e.git(&repo, &["init", "-q"]);
    e.git(
        &repo,
        &["remote", "add", "origin", "https://github.com/octocat/hello-world"],
    );
    e.git(&repo, &["remote", "add", "other", "git@gitlab.com:x/y.git"]);

    let out = e.ok(&repo, &["use", "personal"]);
    assert!(out.contains("now uses ● personal"), "{out}");
    assert_eq!(
        e.git(&repo, &["remote", "get-url", "origin"]),
        "git@github-personal:octocat/hello-world.git"
    );
    assert_eq!(e.git(&repo, &["remote", "get-url", "other"]), "git@gitlab.com:x/y.git");
    assert_eq!(e.git(&repo, &["config", "--local", "user.email"]), "jane@example.com");
    assert_eq!(e.git(&repo, &["config", "--local", "gg.profile"]), "personal");

    e.ok(&repo, &["use", "work"]);
    assert_eq!(
        e.git(&repo, &["remote", "get-url", "origin"]),
        "git@github-work:octocat/hello-world.git"
    );
    assert_eq!(e.git(&repo, &["config", "--local", "user.name"]), "John Doe");
    // Using a profile inside a repo does not change the default.
    assert!(e.ok(&repo, &["profile", "list"]).contains("★ ● personal"));

    let who = e.ok(&repo, &["whoami"]);
    assert!(
        who.contains("Profile    ● work") && who.contains("[ssh: ● work]"),
        "{who}"
    );
    assert_eq!(
        e.git(e.home.path(), &["config", "--global", "user.email"]),
        "global@example.com"
    );
}

#[test]
fn passthrough_and_commit() {
    let e = Env::with_profiles();
    let repo = e.home.path().join("repo");
    fs::create_dir(&repo).unwrap();
    e.git(&repo, &["init", "-q"]);
    fs::write(repo.join("a.txt"), "hi").unwrap();
    e.ok(&repo, &["add", "a.txt"]);
    let out = e.gg(&repo, &["commit", "-q", "-m", "first"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no profile"));

    e.ok(&repo, &["use", "personal"]);
    fs::write(repo.join("b.txt"), "hi").unwrap();
    e.ok(&repo, &["add", "."]);
    let out = e.gg(&repo, &["commit", "-q", "-m", "second"]);
    assert!(
        out.status.success() && out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        e.git(&repo, &["log", "-1", "--format=%an <%ae>"]),
        "Jane Doe <jane@example.com>"
    );
    assert_eq!(
        e.ok(&repo, &["git", "rev-parse", "--is-inside-work-tree"]).trim(),
        "true"
    );
    assert_eq!(e.gg(&repo, &["definitely-not-a-git-command"]).status.code(), Some(1));
}

#[test]
fn clone_configures_repository() {
    let e = Env::with_profiles();
    let h = e.home.path();
    let src = h.join("src");
    fs::create_dir(&src).unwrap();
    e.git(&src, &["init", "-q"]);
    e.git(&src, &["commit", "-q", "--allow-empty", "-m", "init"]);

    e.ok(h, &["clone", "-q", src.to_str().unwrap(), "dest", "-p", "work"]);
    assert_eq!(
        e.git(&h.join("dest"), &["config", "--local", "user.email"]),
        "john@work.example"
    );
    assert_eq!(e.git(&h.join("dest"), &["config", "--local", "gg.profile"]), "work");

    // Remote clones are rewritten to the profile's alias (network blocked via GIT_SSH_COMMAND=false).
    let out = e.gg(h, &["clone", "octocat/hello-world"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .contains("Cloning git@github-personal:octocat/hello-world.git as 'personal'")
    );
    let out = e.gg(
        h,
        &[
            "clone",
            "--depth",
            "1",
            "git@github.com:octocat/hello-world.git",
            "-p",
            "work",
        ],
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("Cloning git@github-work:octocat/hello-world.git as 'work'"));
}

#[test]
fn remote_add_edit_delete() {
    let e = Env::with_profiles();
    let repo = e.home.path().join("repo");
    fs::create_dir(&repo).unwrap();
    e.git(&repo, &["init", "-q"]);
    e.ok(&repo, &["use", "work"]);

    // Shorthand and GitHub URLs go through the repository's profile.
    e.ok(&repo, &["remote", "add", "origin", "octocat/gg"]);
    assert_eq!(
        e.git(&repo, &["remote", "get-url", "origin"]),
        "git@github-work:octocat/gg.git"
    );
    e.ok(
        &repo,
        &["remote", "add", "up", "https://github.com/x/y", "-p", "personal"],
    );
    assert_eq!(
        e.git(&repo, &["remote", "get-url", "up"]),
        "git@github-personal:x/y.git"
    );
    e.ok(&repo, &["remote", "add", "lab", "git@gitlab.com:x/y.git"]);
    assert_eq!(e.git(&repo, &["remote", "get-url", "lab"]), "git@gitlab.com:x/y.git");

    e.ok(&repo, &["remote", "edit", "up", "git@github.com:x/z.git"]);
    assert_eq!(e.git(&repo, &["remote", "get-url", "up"]), "git@github-work:x/z.git");
    e.ok(&repo, &["remote", "set-url", "up", "--push", "x/p"]);
    assert_eq!(
        e.git(&repo, &["remote", "get-url", "--push", "up"]),
        "git@github-work:x/p.git"
    );

    let list = e.ok(&repo, &["remote"]);
    assert!(
        list.contains("origin  git@github-work:octocat/gg.git [ssh: ● work]"),
        "{list}"
    );
    assert!(list.contains("[not managed by gg]"), "{list}");
    assert!(e.ok(&repo, &["remote", "-v"]).contains("[ssh: ● work]"));

    e.ok(&repo, &["remote", "delete", "lab"]);
    e.ok(&repo, &["remote", "rm", "up"]);
    e.ok(&repo, &["remote", "rename", "origin", "main"]); // passed through to git
    assert_eq!(e.git(&repo, &["remote"]), "main");
    assert!(!e.gg(&repo, &["remote", "delete", "nope"]).status.success());
}

#[test]
fn push_repairs_direct_github_remote() {
    let e = Env::with_profiles();
    let repo = e.home.path().join("repo");
    fs::create_dir(&repo).unwrap();
    e.git(&repo, &["init", "-q"]);
    e.ok(&repo, &["use", "personal"]);
    // Added with plain git after `gg use`: would authenticate with no account key.
    e.git(&repo, &["remote", "add", "origin", "git@github.com:octocat/gg.git"]);
    e.git(&repo, &["remote", "add", "lab", "git@gitlab.com:x/y.git"]);
    assert!(e.ok(&repo, &["remote"]).contains("github.com directly"));

    let out = e.gg(&repo, &["push", "origin", "main"]); // fails: GIT_SSH_COMMAND=false
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("origin: git@github.com:octocat/gg.git -> git@github-personal:octocat/gg.git"),
        "{err}"
    );
    assert_eq!(
        e.git(&repo, &["remote", "get-url", "origin"]),
        "git@github-personal:octocat/gg.git"
    );
    assert_eq!(e.git(&repo, &["remote", "get-url", "lab"]), "git@gitlab.com:x/y.git");

    // A deliberate other-account alias is not changed silently.
    e.git(
        &repo,
        &["remote", "set-url", "origin", "git@github-work:octocat/gg.git"],
    );
    let out = e.gg(&repo, &["fetch", "origin"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("Permission") || !out.status.success());
    assert_eq!(
        e.git(&repo, &["remote", "get-url", "origin"]),
        "git@github-work:octocat/gg.git"
    );
}

#[test]
fn self_update_leaves_development_builds_alone() {
    let env = Env::new();
    let out = env.gg(env.home.path(), &["self-update"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("development build"), "{err}");
    // Output is not a terminal, so no background check and no cache file.
    env.ok(env.home.path(), &["profile", "list"]);
    assert!(!env.home.path().join(".cache/gg").exists());
}
