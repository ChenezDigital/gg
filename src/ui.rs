//! Shared console styling. `console` disables colors automatically when output is not a terminal.

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

pub fn ok(msg: impl std::fmt::Display) -> String {
    format!("{} {msg}", style("✓").green().bold())
}

pub fn warn(msg: impl std::fmt::Display) -> String {
    format!("{} {msg}", style("!").yellow().bold())
}

pub fn fail(msg: impl std::fmt::Display) -> String {
    format!("{} {msg}", style("✗").red().bold())
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
