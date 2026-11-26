//! Library behind the `gg` binary. It is split out so fuzz targets and tests can reach the parsers;
//! it is not a stable API.

pub mod commands;
pub mod config;
pub mod gh;
pub mod git;
pub mod interactive;
pub mod ssh;
pub mod tui;
pub mod ui;
pub mod update;
