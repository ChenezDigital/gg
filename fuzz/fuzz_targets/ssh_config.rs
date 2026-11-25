//! `~/.ssh/config` parsing and the managed block written by `gg ssh sync`.
#![no_main]

mod common;

use gg::ssh;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|content: &str| {
    ssh::parse_hosts(content);
    let cfg = common::config();
    let plan = ssh::plan_sync(content, &cfg);
    // After a sync every profile alias resolves, and syncing again changes nothing.
    let hosts = ssh::parse_hosts(&plan.content);
    for p in cfg.profiles.values() {
        assert!(hosts.iter().any(|h| h.alias == p.ssh_alias), "{} missing", p.ssh_alias);
    }
    assert_eq!(ssh::plan_sync(&plan.content, &cfg).content, plan.content);
    // Without gg markers, every line the user wrote is kept as is.
    if !content.contains("gg managed hosts") {
        let out: Vec<&str> = plan.content.split('\n').collect();
        for line in content.split('\n').filter(|l| !l.trim().is_empty()) {
            assert!(out.contains(&line), "lost line {line:?}");
        }
    }
});
