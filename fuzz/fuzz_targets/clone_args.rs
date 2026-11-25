//! `gg clone` argument handling: `-p/--profile` anywhere, everything else passed to git.
#![no_main]

use gg::commands::parse_clone_args;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|args: Vec<String>| {
    let Ok(ca) = parse_clone_args(&args, None) else { return };
    assert!((1..=2).contains(&ca.positionals.len()));
    assert!(ca.positionals.iter().all(|&i| i < ca.args.len()));
    assert!(ca.args.len() <= args.len());
});
