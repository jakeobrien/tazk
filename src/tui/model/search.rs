//! Search query matching (spec §7). Pure logic.
//!
//! Plain, case-insensitive substring match. Issues are matched on ID, title
//! and all four body sections (description, design, acceptance criteria,
//! notes); triage items on their full text. An empty query matches everything.
//! Combining with the filters (AND) is done by
//! [`visible_issues`](super::filter::visible_issues).

use crate::storage::triage::TriageItem;
use crate::tui::data::Snapshot;
use crate::types::Issue;

/// A prepared (lower-cased) search query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    needle: String,
}

impl Query {
    pub fn new(query: &str) -> Self {
        Self {
            needle: query.to_lowercase(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.needle.is_empty()
    }

    fn found_in(&self, text: &str) -> bool {
        text.to_lowercase().contains(&self.needle)
    }

    /// ID, title, description, design, acceptance criteria or notes contain the query.
    pub fn matches_issue(&self, issue: &Issue) -> bool {
        self.is_empty()
            || [
                &issue.id,
                &issue.title,
                &issue.description,
                &issue.design,
                &issue.acceptance_criteria,
                &issue.notes,
            ]
            .into_iter()
            .any(|field| self.found_in(field))
    }

    /// The item's full text contains the query.
    pub fn matches_triage(&self, item: &TriageItem) -> bool {
        self.is_empty() || self.found_in(&item.body)
    }
}

/// Whether `issue` matches `query`.
pub fn issue_matches(issue: &Issue, query: &str) -> bool {
    Query::new(query).matches_issue(issue)
}

/// Whether the triage `item` matches `query`.
pub fn triage_matches(item: &TriageItem, query: &str) -> bool {
    Query::new(query).matches_triage(item)
}

/// Triage items matching `query`, in snapshot order.
pub fn visible_triage<'a>(snapshot: &'a Snapshot, query: &str) -> Vec<&'a TriageItem> {
    let q = Query::new(query);
    snapshot
        .triage
        .iter()
        .filter(|t| q.matches_triage(t))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::*;

    fn full() -> Issue {
        issue("tz-42", "Fix Login Crash")
            .description("The quick brown fox")
            .design("Use a Mutex")
            .acceptance("Never panics again")
            .notes("Seen on Windows")
            .build()
    }

    #[test]
    fn title_and_id_case_insensitive() {
        let i = full();
        assert!(issue_matches(&i, "login"));
        assert!(issue_matches(&i, "LOGIN CRASH"));
        assert!(issue_matches(&i, "TZ-42"));
        assert!(issue_matches(&i, "z-4"));
    }

    #[test]
    fn each_body_section() {
        let i = full();
        assert!(issue_matches(&i, "BROWN"), "description");
        assert!(issue_matches(&i, "mutex"), "design");
        assert!(issue_matches(&i, "panics"), "acceptance criteria");
        assert!(issue_matches(&i, "windows"), "notes");
    }

    #[test]
    fn sections_are_matched_individually_only() {
        let only_notes = issue("a", "b").notes("needle").build();
        assert!(issue_matches(&only_notes, "needle"));
        let only_design = issue("a", "b").design("needle").build();
        assert!(issue_matches(&only_design, "needle"));
        let only_ac = issue("a", "b").acceptance("needle").build();
        assert!(issue_matches(&only_ac, "needle"));
        let only_desc = issue("a", "b").description("needle").build();
        assert!(issue_matches(&only_desc, "needle"));
    }

    #[test]
    fn non_match_and_empty_query() {
        let i = full();
        assert!(!issue_matches(&i, "zebra"));
        assert!(issue_matches(&i, ""));
        // Not a regex or token search.
        assert!(!issue_matches(&i, "quick  brown"));
        assert!(!issue_matches(&i, "fox.*"));
        // Other fields (assignee, labels) are not searched.
        let j = issue("a", "b").assignee("zed").labels(&["lbl"]).build();
        assert!(!issue_matches(&j, "zed"));
        assert!(!issue_matches(&j, "lbl"));
    }

    #[test]
    fn triage_full_text() {
        let t = triage_item(
            "20250101T000000-aaaa.md",
            "First line\n\nSecond Paragraph about Cats\n",
        );
        assert!(triage_matches(&t, "cats"));
        assert!(triage_matches(&t, "FIRST"));
        assert!(triage_matches(&t, "line\n\nsecond"));
        assert!(triage_matches(&t, ""));
        assert!(!triage_matches(&t, "dogs"));
    }

    #[test]
    fn visible_triage_filters_items() {
        let snap = snapshot_with(
            Vec::<IssueBuilder>::new(),
            vec![
                triage_item("a.md", "apples"),
                triage_item("b.md", "bananas"),
                triage_item("c.md", "Apple pie"),
            ],
        );
        let names: Vec<_> = visible_triage(&snap, "apple")
            .into_iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(names, ["a.md", "c.md"]);
        assert_eq!(visible_triage(&snap, "").len(), 3);
    }
}
