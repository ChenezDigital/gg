use gg::config::{Config, Profile};

/// Two accounts on github.com plus one on a self-hosted GitLab.
pub fn config() -> Config {
    let mut cfg = Config::default();
    for (name, host) in [
        ("personal", "github.com"),
        ("work", "github.com"),
        ("lab", "gitlab.example.com"),
    ] {
        let mut p = Profile::with_defaults(name);
        p.name = format!("{name} user");
        p.email = format!("{name}@example.com");
        p.hostname = host.to_string();
        cfg.profiles.insert(name.to_string(), p);
    }
    cfg
}
