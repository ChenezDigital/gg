//! Shared console styling. `console` disables colors automatically when output is not a terminal.

use std::sync::OnceLock;

use console::{Color, style};

use crate::config::Config;

const PALETTE: [Color; 6] = [
    Color::Green,
    Color::Magenta,
    Color::Yellow,
    Color::Blue,
    Color::Cyan,
    Color::Red,
];

/// Stable per-profile color index (profiles are kept sorted by name).
pub fn profile_index(cfg: &Config, name: &str) -> usize {
    cfg.profiles.keys().position(|k| k == name).unwrap_or(0) % PALETTE.len()
}

pub fn badge(cfg: &Config, name: &str) -> String {
    style(format!("● {name}"))
        .fg(PALETTE[profile_index(cfg, name)])
        .bold()
        .to_string()
}

/// Symbols that need a font beyond the classic Windows console's (Consolas, Lucida Console).
pub struct Glyphs {
    pub star: &'static str,
    pub check: &'static str,
    pub cross: &'static str,
    pub warn: &'static str,
    pub pointer: &'static str,
    pub enter: &'static str,
    pub branch: &'static str,
    pub edit: &'static str,
}

const FANCY: Glyphs = Glyphs {
    star: "★",
    check: "✓",
    cross: "✗",
    warn: "⚠",
    pointer: "▶",
    enter: "⏎",
    branch: "⎇",
    edit: "±",
};

/// Stand-ins from the WGL4 set, which every Windows console font covers.
const PLAIN: Glyphs = Glyphs {
    star: "*",
    check: "√",
    cross: "×",
    warn: "!",
    pointer: "►",
    enter: "enter",
    branch: "»",
    edit: "±",
};

/// `GG_GLYPHS=plain|fancy` overrides detection. On Windows, only terminals that announce
/// themselves (Windows Terminal, VS Code, ...) get the fancy set; the classic console window
/// draws the missing symbols as boxes and misaligns the rest of the line.
pub fn glyphs() -> &'static Glyphs {
    static G: OnceLock<&'static Glyphs> = OnceLock::new();
    G.get_or_init(|| {
        let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
        let fancy = match std::env::var("GG_GLYPHS").as_deref() {
            Ok("plain") => false,
            Ok("fancy") => true,
            _ => !cfg!(windows) || set("WT_SESSION") || set("TERM_PROGRAM") || set("ConEmuANSI"),
        };
        if fancy { &FANCY } else { &PLAIN }
    })
}

pub fn ok(msg: impl std::fmt::Display) -> String {
    format!("{} {msg}", style(glyphs().check).green().bold())
}

pub fn warn(msg: impl std::fmt::Display) -> String {
    format!("{} {msg}", style("!").yellow().bold())
}

pub fn fail(msg: impl std::fmt::Display) -> String {
    format!("{} {msg}", style(glyphs().cross).red().bold())
}

pub fn dim(msg: impl std::fmt::Display) -> String {
    style(msg).dim().to_string()
}

pub fn bold(msg: impl std::fmt::Display) -> String {
    style(msg).bold().to_string()
}

pub fn heading(msg: &str) -> String {
    style(msg).cyan().bold().to_string()
}

pub fn banner(msg: &str) -> String {
    format!(
        "{}{}",
        style(" gg ").black().on_cyan().bold(),
        style(format!(" {msg} ")).cyan().bold()
    )
}
