//! `Snapshot` (issues including closed, dependents, `by_id` index, triage items,
//! skipped files) and loading from storage.

use crate::storage::triage::TriageItem;
use crate::storage::{SkippedFile, Storage};
use crate::types::Issue;
use anyhow::Result;
use std::collections::HashMap;

/// An immutable view of everything on disk at one moment (spec §3.1).
///
/// The app replaces the whole snapshot on every reload.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// Every issue, including closed ones, with `dependents` populated.
    pub issues: Vec<Issue>,
    /// Issue ID -> index into `issues`.
    pub by_id: HashMap<String, usize>,
    /// Triage items, sorted by file name.
    pub triage: Vec<TriageItem>,
    /// Files that failed to load (spec §3.2).
    pub skipped: Vec<SkippedFile>,
}

impl Snapshot {
    /// Build a snapshot (and its `by_id` index) from already-loaded parts.
    /// No filesystem access.
    pub fn new(issues: Vec<Issue>, triage: Vec<TriageItem>, skipped: Vec<SkippedFile>) -> Self {
        let by_id = issues
            .iter()
            .enumerate()
            .map(|(i, issue)| (issue.id.clone(), i))
            .collect();
        Self {
            issues,
            by_id,
            triage,
            skipped,
        }
    }

    /// Load everything with the tolerant loaders: bad files are skipped and
    /// reported, never fatal. Only takes short read locks.
    pub fn load(storage: &Storage) -> Result<Self> {
        let issues = storage.list_all_issues_tolerant()?;
        let triage = storage.list_triage_items_tolerant()?;
        let mut skipped = issues.skipped;
        skipped.extend(triage.skipped);
        Ok(Self::new(issues.items, triage.items, skipped))
    }

    pub fn issue(&self, id: &str) -> Option<&Issue> {
        self.by_id.get(id).map(|&i| &self.issues[i])
    }
}

/// Fixture builders for tests (no filesystem access).
#[cfg(test)]
pub mod fixtures {
    use super::*;
    use crate::types::IssueType;

    pub fn issue(id: &str, title: &str) -> Issue {
        Issue::new(id.to_string(), title.to_string(), 2, IssueType::Task)
    }

    pub fn triage_item(name: &str, body: &str) -> TriageItem {
        TriageItem {
            name: name.to_string(),
            body: body.to_string(),
        }
    }

    /// Two issues and one triage item.
    pub fn snapshot() -> Snapshot {
        Snapshot::new(
            vec![issue("tz-1", "First"), issue("tz-2", "Second")],
            vec![triage_item("20250101T000000-aaaa.md", "idea")],
            Vec::new(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;

    #[test]
    fn new_builds_index() {
        let s = snapshot();
        assert_eq!(s.by_id["tz-2"], 1);
        assert_eq!(s.issue("tz-1").unwrap().title, "First");
        assert!(s.issue("nope").is_none());
        assert_eq!(s.triage.len(), 1);
    }
}
