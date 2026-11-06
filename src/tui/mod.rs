//! `tz tui`: read-only full-screen terminal UI (see `docs/tui-spec.md`).
//!
//! Rule: state and update logic never touch the terminal.

// Skeleton modules are filled in by later issues.
#![allow(dead_code)]

mod actions;
mod app;
mod data;
mod keymap;
mod model;
mod terminal;
mod ui;

use crate::storage::Storage;
use anyhow::Result;
use clap::{Args, ValueEnum};

/// Starting mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ModeArg {
    Issues,
    Triage,
}

/// Starting grouping for the issues grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GroupByArg {
    Epic,
    Status,
    Role,
    Type,
    Priority,
}

/// Command-line flags. They only set the initial view state.
///
/// `VALUES` flags are repeatable and/or comma-separated; `none` selects
/// issues with the property unset (role, effort).
#[derive(Debug, Args)]
pub struct TuiArgs {
    /// Starting mode
    #[arg(long, value_enum, default_value = "issues")]
    pub mode: ModeArg,

    /// Starting grouping
    #[arg(long, value_enum, default_value = "epic")]
    pub group_by: GroupByArg,

    /// Initial role filter (human, agent, none)
    #[arg(short, long, value_name = "VALUES", value_delimiter = ',')]
    pub role: Vec<String>,

    /// Initial status filter (default: all except closed)
    #[arg(short, long, value_name = "VALUES", value_delimiter = ',')]
    pub status: Vec<String>,

    /// Initial type filter
    #[arg(
        short = 't',
        long = "type",
        value_name = "VALUES",
        value_delimiter = ','
    )]
    pub r#type: Vec<String>,

    /// Initial priority filter
    #[arg(short, long, value_name = "VALUES", value_delimiter = ',')]
    pub priority: Vec<String>,

    /// Initial effort filter (values or none)
    #[arg(long, value_name = "VALUES", value_delimiter = ',')]
    pub effort: Vec<String>,

    /// Initial search query (issues mode)
    #[arg(long, value_name = "TEXT")]
    pub search: Option<String>,
}

/// Entry point. The storage has already been opened (so a missing `.tazk`
/// has failed before the terminal is touched).
pub fn run(_storage: Storage, _args: TuiArgs) -> Result<()> {
    anyhow::bail!("tz tui is not implemented yet")
}
