//! Open-blockers detection (spec §5.4).

use crate::storage::is_blocked_with_ancestors;
use crate::tui::data::Snapshot;
use crate::types::Issue;

/// True if `issue` has at least one `blocked_by` target that is not closed
/// (a missing target counts as unresolved), or inherits such a blocker (or
/// `status: blocked`) from a `child_of` ancestor. The issue's own
/// `status: blocked` is not an open blocker.
///
/// This is the rule `tz ready` uses ([`is_blocked_with_ancestors`]), evaluated
/// against the snapshot's full issue set.
pub fn has_open_blockers(issue: &Issue, snapshot: &Snapshot) -> bool {
    let by_id = snapshot
        .by_id
        .iter()
        .map(|(id, &i)| (id.as_str(), &snapshot.issues[i]))
        .collect();
    is_blocked_with_ancestors(issue, &by_id, &snapshot.closed_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::*;
    use crate::types::Status;

    fn blocked(issues: Vec<IssueBuilder>) -> bool {
        let snap = snapshot_of(issues);
        has_open_blockers(snap.issue("tz-1").unwrap(), &snap)
    }

    #[test]
    fn open_target_blocks() {
        assert!(blocked(vec![
            issue("tz-1", "a").blocked_by("tz-2"),
            issue("tz-2", "b"),
        ]));
        assert!(blocked(vec![
            issue("tz-1", "a").blocked_by("tz-2"),
            issue("tz-2", "b").status(Status::InProgress),
        ]));
    }

    #[test]
    fn closed_target_does_not_block() {
        assert!(!blocked(vec![
            issue("tz-1", "a").blocked_by("tz-2"),
            issue("tz-2", "b").closed(),
        ]));
    }

    #[test]
    fn missing_target_blocks() {
        assert!(blocked(vec![issue("tz-1", "a").blocked_by("tz-404")]));
        // Even when other closed issues exist.
        assert!(blocked(vec![
            issue("tz-1", "a").blocked_by("tz-404"),
            issue("tz-2", "b").closed(),
        ]));
    }

    #[test]
    fn one_closed_one_open_blocks() {
        assert!(blocked(vec![
            issue("tz-1", "a").blocked_by("tz-2").blocked_by("tz-3"),
            issue("tz-2", "b").closed(),
            issue("tz-3", "c"),
        ]));
    }

    #[test]
    fn related_to_only_does_not_block() {
        assert!(!blocked(vec![
            issue("tz-1", "a").related_to("tz-2").child_of("tz-3"),
            issue("tz-2", "b"),
        ]));
    }

    #[test]
    fn blocked_status_without_deps_is_not_open_blockers() {
        assert!(!blocked(vec![issue("tz-1", "a").status(Status::Blocked)]));
    }
}

#[cfg(test)]
mod inherited_tests {
    use super::*;
    use crate::tui::testing::*;
    use crate::types::Status;

    fn blocked(issues: Vec<IssueBuilder>, id: &str) -> bool {
        let snap = snapshot_of(issues);
        has_open_blockers(snap.issue(id).unwrap(), &snap)
    }

    #[test]
    fn ancestor_blocker_marks_descendants() {
        let issues = || {
            vec![
                epic("tz-1", "e").blocked_by("tz-9"),
                epic("tz-2", "sub").child_of("tz-1"),
                issue("tz-3", "leaf").child_of("tz-2"),
                issue("tz-9", "ask"),
            ]
        };
        assert!(blocked(issues(), "tz-2"));
        assert!(blocked(issues(), "tz-3"));
        assert!(!blocked(issues(), "tz-9"));
    }

    #[test]
    fn closed_ancestor_blocker_does_not_mark() {
        assert!(!blocked(
            vec![
                epic("tz-1", "e").blocked_by("tz-9"),
                issue("tz-3", "leaf").child_of("tz-1"),
                issue("tz-9", "ask").closed(),
            ],
            "tz-3"
        ));
    }

    #[test]
    fn blocked_status_ancestor_marks_but_own_status_does_not() {
        let issues = || {
            vec![
                epic("tz-1", "e").status(Status::Blocked),
                issue("tz-3", "leaf").child_of("tz-1"),
            ]
        };
        assert!(blocked(issues(), "tz-3"));
        assert!(!blocked(issues(), "tz-1"));
    }

    #[test]
    fn cycle_terminates() {
        assert!(!blocked(
            vec![
                issue("tz-1", "a").child_of("tz-2"),
                issue("tz-2", "b").child_of("tz-1"),
            ],
            "tz-1"
        ));
    }
}
