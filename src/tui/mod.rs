//! `tz tui`: read-only full-screen terminal UI (see `docs/tui-spec.md`).
//!
//! Rule: state and update logic never touch the terminal.

// Skeleton modules are filled in by later issues.
#![allow(dead_code)]

mod actions;
mod app;
mod data;
#[cfg(test)]
mod e2e_tests;
mod keymap;
mod model;
mod terminal;
#[cfg(test)]
mod testing;
mod ui;

use crate::storage::Storage;
use anyhow::Result;
use clap::{Args, ValueEnum};
use model::cli::FilterFlags;
use model::filter::Filters;

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

impl GroupByArg {
    /// The grouping `g` switches to: epic → status → role → type → priority → epic.
    pub fn next(self) -> Self {
        match self {
            GroupByArg::Epic => GroupByArg::Status,
            GroupByArg::Status => GroupByArg::Role,
            GroupByArg::Role => GroupByArg::Type,
            GroupByArg::Type => GroupByArg::Priority,
            GroupByArg::Priority => GroupByArg::Epic,
        }
    }
}

/// Command-line flags. They only set the initial view state; nothing is
/// persisted.
///
/// `VALUES` flags are repeatable and/or comma-separated (`-p 0,1 -p 3`), like
/// `tz list -p 0,1`. The literal `none` selects issues with the property
/// unset (role, effort).
#[derive(Debug, Args)]
pub struct TuiArgs {
    /// Starting mode
    #[arg(long, value_enum, default_value = "issues")]
    pub mode: ModeArg,

    /// Starting grouping
    #[arg(long, value_enum, default_value = "epic")]
    pub group_by: GroupByArg,

    /// Initial role filter: human, agent, none (unset). Repeatable and/or
    /// comma-separated. Default: all
    #[arg(short, long, value_name = "VALUES", value_delimiter = ',')]
    pub role: Vec<String>,

    /// Initial status filter: open, in_progress, blocked, deferred, closed.
    /// Repeatable and/or comma-separated. Default: all except closed
    #[arg(short, long, value_name = "VALUES", value_delimiter = ',')]
    pub status: Vec<String>,

    /// Initial type filter: epic, spec, feature, bug, task, gate, chore.
    /// Repeatable and/or comma-separated. Default: all
    #[arg(
        short = 't',
        long = "type",
        value_name = "VALUES",
        value_delimiter = ','
    )]
    pub r#type: Vec<String>,

    /// Initial priority filter, e.g. 0,1 (repeatable and/or comma-separated).
    /// Default: all
    #[arg(short, long, value_name = "VALUES", value_delimiter = ',')]
    pub priority: Vec<String>,

    /// Initial effort filter: 1-5 or none (unset). Repeatable and/or
    /// comma-separated. Default: all
    #[arg(long, value_name = "VALUES", value_delimiter = ',')]
    pub effort: Vec<String>,

    /// Initial search query (issues mode)
    #[arg(long, value_name = "TEXT")]
    pub search: Option<String>,
}

impl TuiArgs {
    /// The initial filters from `-r -s -t -p --effort`; an error names the
    /// bad flag value.
    pub fn initial_filters(&self) -> Result<Filters> {
        Filters::from_flags(FilterFlags {
            role: &self.role,
            status: &self.status,
            issue_type: &self.r#type,
            priority: &self.priority,
            effort: &self.effort,
        })
    }
}

/// Entry point. The storage has already been opened (so a missing `.tazk`
/// has failed before the terminal is touched).
pub fn run(storage: Storage, args: TuiArgs) -> Result<()> {
    // Validate the flags first: bad values must fail before the terminal is touched.
    let filters = args.initial_filters()?;
    let snapshot = data::Snapshot::load(&storage)?;
    let storage = std::sync::Arc::new(storage);
    terminal::run(
        app::App::with_snapshot(&args, filters, snapshot),
        data::Reloader::for_shared(storage.clone()),
        storage,
    )
}
