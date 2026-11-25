//! Remote URL parsing and account rewriting (`gg use`, `gg clone`, push repair).
#![no_main]

mod common;

use gg::git;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|url: &str| {
    let parsed = git::parse_remote(url);
    git::is_shorthand(url);
    let cfg = common::config();
    for profile in cfg.profiles.values() {
        let Some(new) = git::rewrite_url(url, profile, &cfg) else {
            continue;
        };
        // A rewritten URL points at the profile alias, keeps the repository path and is stable.
        let again = git::parse_remote(&new).expect("rewritten URL must parse");
        assert_eq!(again.host, profile.ssh_alias);
        assert_eq!(Some(&again.path), parsed.as_ref().map(|p| &p.path));
        assert_eq!(git::rewrite_url(&new, profile, &cfg).as_deref(), Some(new.as_str()));
    }
});
