//! Fixed sort order for grid cells (spec §5.3).

use crate::types::Issue;
use std::cmp::Ordering;

/// Cell order within every column: priority ascending (0 first), then
/// `updated_at` newest first, then ID.
pub fn cell_order(a: &Issue, b: &Issue) -> Ordering {
    a.priority
        .cmp(&b.priority)
        .then_with(|| b.updated_at.cmp(&a.updated_at))
        .then_with(|| a.id.cmp(&b.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::*;

    fn sorted(issues: Vec<IssueBuilder>) -> Vec<String> {
        let mut v: Vec<Issue> = issues.into_iter().map(IssueBuilder::build).collect();
        v.sort_by(cell_order);
        v.into_iter().map(|i| i.id).collect()
    }

    #[test]
    fn priority_ascending_first() {
        let order = sorted(vec![
            issue("c", "").priority(3),
            issue("a", "").priority(0).updated_after(-100),
            issue("b", "").priority(-1),
            issue("d", "").priority(10),
        ]);
        assert_eq!(order, ["b", "a", "c", "d"]);
    }

    #[test]
    fn ties_in_priority_break_by_newest_updated() {
        let order = sorted(vec![
            issue("a", "").updated_after(1),
            issue("b", "").updated_after(30),
            issue("c", "").updated_after(10),
        ]);
        assert_eq!(order, ["b", "c", "a"]);
    }

    #[test]
    fn ties_in_updated_at_break_by_id() {
        let order = sorted(vec![
            issue("tz-2", "").updated_after(5),
            issue("tz-1", "").updated_after(5),
            issue("tz-3", "").updated_after(5),
        ]);
        assert_eq!(order, ["tz-1", "tz-2", "tz-3"]);
    }

    #[test]
    fn all_levels_together_and_equal_is_equal() {
        let order = sorted(vec![
            issue("e", "").priority(1).updated_after(1),
            issue("d", "").priority(1).updated_after(2),
            issue("c", "").priority(0).updated_after(1),
            issue("b", "").priority(0).updated_after(1),
        ]);
        assert_eq!(order, ["b", "c", "d", "e"]);
        let a = issue("x", "").build();
        assert_eq!(cell_order(&a, &a.clone()), Ordering::Equal);
    }
}
