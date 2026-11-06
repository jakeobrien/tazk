//! Open-blockers detection (spec §5.4).

use crate::storage::has_unresolved_dependencies;
use crate::tui::data::Snapshot;
use crate::types::Issue;

/// True if `issue` has at least one `blocked_by` target that is not closed.
/// A missing target counts as unresolved. Independent of `status: blocked`.
///
/// This is the rule `tz ready` uses ([`has_unresolved_dependencies`]), evaluated
/// against the snapshot's full issue set.
pub fn has_open_blockers(issue: &Issue, snapshot: &Snapshot) -> bool {
    has_unresolved_dependencies(issue, &snapshot.closed_ids)
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
