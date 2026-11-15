//! Grouping of issues into grid columns (epic/status/role/type/priority),
//! spec §5.1–§5.3. Pure: snapshot + visible set + grouping in, columns out.

use super::filter::{status_options, type_options, STANDARD_PRIORITIES};
use super::sort::cell_order;
use crate::tui::data::Snapshot;
use crate::tui::GroupByArg;
use crate::types::{DependencyType, Issue, IssueType, Role, Status};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Identifies a column independently of its position, so selection can find
/// "the same column" after a reload or filter change.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ColumnKey {
    NoEpic,
    Epic(String),
    Status(Status),
    Role(Option<Role>),
    Type(IssueType),
    Priority(i32),
}

/// The epic cell at the top of an epic column (spec §5.2). The cell content is
/// looked up from the full snapshot by `epic_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub epic_id: String,
    /// The epic itself fails the current filters/search.
    pub dimmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub key: ColumnKey,
    /// Title bar text (epic columns: ID and title).
    pub title: String,
    /// Number of cells, header excluded.
    pub count: usize,
    pub header: Option<Header>,
    /// Issue IDs in cell order.
    pub cells: Vec<String>,
}

impl Column {
    fn new(key: ColumnKey, title: String, header: Option<Header>, cells: Vec<String>) -> Self {
        Self {
            key,
            title,
            count: cells.len(),
            header,
            cells,
        }
    }

    /// Number of selectable rows: the header (if any) is row 0, then the cells.
    pub fn row_count(&self) -> usize {
        self.cells.len() + usize::from(self.header.is_some())
    }

    /// The issue ID at `row` (the epic's ID for the header row).
    pub fn id_at(&self, row: usize) -> Option<&str> {
        match (&self.header, row) {
            (Some(h), 0) => Some(h.epic_id.as_str()),
            (Some(_), r) => self.cells.get(r - 1).map(String::as_str),
            (None, r) => self.cells.get(r).map(String::as_str),
        }
    }

    /// The row holding issue `id`, header included.
    pub fn row_of(&self, id: &str) -> Option<usize> {
        if self.header.as_ref().is_some_and(|h| h.epic_id == id) {
            return Some(0);
        }
        let offset = usize::from(self.header.is_some());
        self.cells.iter().position(|c| c == id).map(|i| i + offset)
    }
}

/// Build the columns for `grouping` from the `visible` issues (those passing
/// filters and search). Empty columns are hidden, except an epic column whose
/// epic is visible (it holds only its header).
pub fn build_columns(snapshot: &Snapshot, visible: &[&Issue], grouping: GroupByArg) -> Vec<Column> {
    match grouping {
        GroupByArg::Epic => epic_columns(snapshot, visible),
        GroupByArg::Status => fixed_columns(
            visible,
            |i| i.status,
            status_options(),
            ColumnKey::Status,
            |s| s.as_str().to_string(),
        ),
        GroupByArg::Role => fixed_columns(
            visible,
            |i| i.role,
            vec![Some(Role::Agent), Some(Role::Human), None],
            ColumnKey::Role,
            |r| r.map_or("(none)".to_string(), |r| r.as_str().to_string()),
        ),
        GroupByArg::Type => fixed_columns(
            visible,
            |i| i.issue_type,
            type_options(),
            ColumnKey::Type,
            |t| t.as_str().to_string(),
        ),
        GroupByArg::Priority => priority_columns(visible),
    }
}

fn sorted_ids(mut issues: Vec<&Issue>) -> Vec<String> {
    issues.sort_by(|a, b| cell_order(a, b));
    issues.into_iter().map(|i| i.id.clone()).collect()
}

/// One column per value in `order`; values with no issues produce no column.
fn fixed_columns<V: Copy + PartialEq>(
    visible: &[&Issue],
    value_of: impl Fn(&Issue) -> V,
    order: Vec<V>,
    key: impl Fn(V) -> ColumnKey,
    title: impl Fn(V) -> String,
) -> Vec<Column> {
    order
        .into_iter()
        .filter_map(|v| {
            let members: Vec<&Issue> = visible
                .iter()
                .copied()
                .filter(|i| value_of(i) == v)
                .collect();
            (!members.is_empty()).then(|| Column::new(key(v), title(v), None, sorted_ids(members)))
        })
        .collect()
}

/// P0..P4, then any other values in numeric order.
fn priority_columns(visible: &[&Issue]) -> Vec<Column> {
    let mut groups: BTreeMap<(bool, i32), Vec<&Issue>> = BTreeMap::new();
    for i in visible {
        let p = i.priority;
        groups
            .entry((!STANDARD_PRIORITIES.contains(&p), p))
            .or_default()
            .push(i);
    }
    groups
        .into_iter()
        .map(|((_, p), members)| {
            Column::new(
                ColumnKey::Priority(p),
                format!("P{p}"),
                None,
                sorted_ids(members),
            )
        })
        .collect()
}

/// The issue's parent: its lowest `child_of` target ID. A self-reference is
/// ignored (an issue is never its own parent).
fn parent_of(issue: &Issue) -> Option<&str> {
    issue
        .depends_on
        .iter()
        .filter(|(target, ty)| **ty == DependencyType::ChildOf && **target != issue.id)
        .map(|(target, _)| target.as_str())
        .min()
}

fn epic_columns(snapshot: &Snapshot, visible: &[&Issue]) -> Vec<Column> {
    let visible_ids: HashSet<&str> = visible.iter().map(|i| i.id.as_str()).collect();

    let mut no_epic: Vec<&Issue> = Vec::new();
    let mut by_epic: HashMap<&str, Vec<&Issue>> = HashMap::new();
    for &i in visible {
        match parent_of(i)
            .and_then(|p| snapshot.issue(p))
            .filter(|p| p.issue_type == IssueType::Epic)
        {
            Some(parent) => by_epic.entry(parent.id.as_str()).or_default().push(i),
            // Top-level epics are already represented by their own column.
            None if i.issue_type == IssueType::Epic => {}
            None => no_epic.push(i),
        }
    }
    // Every visible epic gets a column, even without visible children.
    for &i in visible {
        if i.issue_type == IssueType::Epic {
            by_epic.entry(i.id.as_str()).or_default();
        }
    }

    let mut epics: Vec<(&Issue, Vec<&Issue>)> = by_epic
        .into_iter()
        .filter_map(|(id, members)| snapshot.issue(id).map(|e| (e, members)))
        .collect();
    epics.sort_by(|a, b| cell_order(a.0, b.0));

    let mut columns = Vec::new();
    if !no_epic.is_empty() {
        columns.push(Column::new(
            ColumnKey::NoEpic,
            "No epic".to_string(),
            None,
            sorted_ids(no_epic),
        ));
    }
    columns.extend(epics.into_iter().map(|(epic, members)| {
        Column::new(
            ColumnKey::Epic(epic.id.clone()),
            format!("{} {}", epic.id, epic.title),
            Some(Header {
                epic_id: epic.id.clone(),
                dimmed: !visible_ids.contains(epic.id.as_str()),
            }),
            sorted_ids(members),
        )
    }));
    columns
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::filter::{visible_issues, FilterSet, Filters};
    use crate::tui::testing::*;

    fn cols(snap: &Snapshot, g: GroupByArg) -> Vec<Column> {
        let visible = visible_issues(snap, &Filters::default(), "");
        build_columns(snap, &visible, g)
    }

    fn all_cols(snap: &Snapshot, g: GroupByArg) -> Vec<Column> {
        let visible: Vec<&Issue> = snap.issues.iter().collect();
        build_columns(snap, &visible, g)
    }

    fn summary(cols: &[Column]) -> Vec<(String, Vec<&str>)> {
        cols.iter()
            .map(|c| {
                (
                    c.title.clone(),
                    c.cells.iter().map(String::as_str).collect(),
                )
            })
            .collect()
    }

    fn titles(cols: &[Column]) -> Vec<&str> {
        cols.iter().map(|c| c.title.as_str()).collect()
    }

    /// The column with this key (panics if absent).
    fn col<'a>(cols: &'a [Column], key: &ColumnKey) -> &'a Column {
        cols.iter()
            .find(|c| &c.key == key)
            .unwrap_or_else(|| panic!("no column {key:?}"))
    }

    fn epic_key(id: &str) -> ColumnKey {
        ColumnKey::Epic(id.to_string())
    }

    // ---- epic grouping ----

    #[test]
    fn multi_parent_picks_lowest_id() {
        let snap = snapshot_of(vec![
            epic("e-1", "One"),
            epic("e-2", "Two"),
            issue("t", "x").child_of("e-2").child_of("e-1"),
        ]);
        let c = cols(&snap, GroupByArg::Epic);
        assert_eq!(col(&c, &epic_key("e-1")).cells, ["t"]);
        assert!(col(&c, &epic_key("e-2")).cells.is_empty());
    }

    #[test]
    fn missing_parent_goes_to_no_epic() {
        let snap = snapshot_of(vec![issue("t", "x").child_of("nope")]);
        let c = cols(&snap, GroupByArg::Epic);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].key, ColumnKey::NoEpic);
        assert_eq!(c[0].title, "No epic");
        assert!(c[0].header.is_none());
        assert_eq!(c[0].cells, ["t"]);
    }

    #[test]
    fn non_epic_parent_goes_to_no_epic_and_gets_no_column() {
        let snap = snapshot_of(vec![
            issue("p", "parent"),
            issue("c", "child").child_of("p"),
        ]);
        let c = cols(&snap, GroupByArg::Epic);
        assert_eq!(summary(&c), [("No epic".to_string(), vec!["c", "p"])]);
    }

    #[test]
    fn other_dependency_types_are_not_parents() {
        let snap = snapshot_of(vec![
            epic("e", "E").closed(),
            issue("t", "x").blocked_by("e").related_to("e"),
        ]);
        let f = Filters {
            status: FilterSet::all(),
            ..Default::default()
        };
        let visible = visible_issues(&snap, &f, "");
        let c = build_columns(&snap, &visible, GroupByArg::Epic);
        assert_eq!(c[0].key, ColumnKey::NoEpic);
        assert_eq!(c[0].cells, ["t"]);
        assert_eq!(c.len(), 2);
        assert!(col(&c, &epic_key("e")).cells.is_empty());
    }

    #[test]
    fn self_parent_is_ignored() {
        let snap = snapshot_of(vec![epic("e", "E").child_of("e")]);
        let c = cols(&snap, GroupByArg::Epic);
        // The epic is not its own parent: it has its own (empty) column and,
        // being an epic, is not repeated in "No epic" (which is then omitted).
        assert_eq!(c.len(), 1);
        assert!(col(&c, &epic_key("e")).cells.is_empty());
    }

    #[test]
    fn closed_epic_with_open_children_has_dimmed_header() {
        let snap = snapshot_of(vec![
            epic("e", "Done").closed(),
            issue("t", "x").child_of("e"),
        ]);
        // Default filters hide closed: the epic is not visible.
        let c = cols(&snap, GroupByArg::Epic);
        assert_eq!(c.len(), 1);
        assert_eq!(
            c[0].header,
            Some(Header {
                epic_id: "e".into(),
                dimmed: true
            })
        );
        assert_eq!(c[0].cells, ["t"]);
        assert_eq!(c[0].count, 1);
    }

    #[test]
    fn visible_epic_header_is_not_dimmed() {
        let snap = snapshot_of(vec![epic("e", "E"), issue("t", "x").child_of("e")]);
        let c = cols(&snap, GroupByArg::Epic);
        let h = col(&c, &epic_key("e")).header.as_ref();
        assert_eq!(h.map(|h| h.dimmed), Some(false));
    }

    #[test]
    fn epic_failing_search_is_dimmed_but_shown_for_visible_children() {
        let snap = snapshot_of(vec![epic("e", "Alpha"), issue("t", "needle").child_of("e")]);
        let visible = visible_issues(&snap, &Filters::default(), "needle");
        let c = build_columns(&snap, &visible, GroupByArg::Epic);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].header.as_ref().map(|h| h.dimmed), Some(true));
    }

    #[test]
    fn visible_epic_without_visible_children_is_header_only() {
        let snap = snapshot_of(vec![epic("e", "E"), issue("t", "x").child_of("e").closed()]);
        let c = cols(&snap, GroupByArg::Epic);
        let e = col(&c, &epic_key("e"));
        assert!(e.cells.is_empty());
        assert_eq!(e.count, 0);
        assert!(e.header.is_some());
    }

    #[test]
    fn nested_epic_is_cell_in_parent_and_own_column() {
        let snap = snapshot_of(vec![
            epic("e-1", "Outer"),
            epic("e-2", "Inner").child_of("e-1"),
            issue("t", "x").child_of("e-2"),
        ]);
        let c = cols(&snap, GroupByArg::Epic);
        // The outer epic is only a column header (not in "No epic"); the inner
        // epic is a cell of the outer epic's column and has its own column.
        assert_eq!(c.len(), 2);
        assert_eq!(col(&c, &epic_key("e-1")).cells, ["e-2"]);
        let inner = col(&c, &epic_key("e-2"));
        assert_eq!(inner.header.as_ref().unwrap().epic_id, "e-2");
        assert_eq!(inner.cells, ["t"]);
    }

    #[test]
    fn epic_columns_ordered_by_priority_updated_id() {
        let snap = snapshot_of(vec![
            epic("e-a", "A").priority(2).updated_after(5),
            epic("e-b", "B").priority(1).updated_after(0),
            epic("e-c", "C").priority(2).updated_after(10),
            epic("e-d", "D").priority(2).updated_after(10),
            issue("t", "orphan"),
        ]);
        let c = cols(&snap, GroupByArg::Epic);
        let keys: Vec<_> = c.iter().map(|c| c.key.clone()).collect();
        assert_eq!(
            keys,
            [
                ColumnKey::NoEpic,
                epic_key("e-b"),
                epic_key("e-c"),
                epic_key("e-d"),
                epic_key("e-a"),
            ]
        );
        assert_eq!(c[1].title, "e-b B");
    }

    #[test]
    fn top_level_epics_are_not_listed_in_no_epic() {
        let snap = snapshot_of(vec![epic("e", "E"), issue("t", "orphan")]);
        let c = cols(&snap, GroupByArg::Epic);
        assert_eq!(col(&c, &ColumnKey::NoEpic).cells, ["t"]);
        assert_eq!(
            col(&c, &epic_key("e")).header.as_ref().unwrap().epic_id,
            "e"
        );
    }

    #[test]
    fn no_epic_omitted_when_empty() {
        // Everything is under a (hidden, closed) epic: no top-level issues.
        let snap = snapshot_of(vec![
            epic("e0", "E0").closed(),
            epic("e", "E").child_of("e0"),
            issue("t", "x").child_of("e"),
        ]);
        let c = cols(&snap, GroupByArg::Epic);
        assert!(c.iter().all(|c| c.key != ColumnKey::NoEpic), "{c:?}");
        assert!(cols(&snapshot_of(Vec::<IssueBuilder>::new()), GroupByArg::Epic).is_empty());
    }

    #[test]
    fn epic_cells_sorted_with_cell_order() {
        let snap = snapshot_of(vec![
            epic("e", "E"),
            issue("a", "").child_of("e").priority(3),
            issue("b", "").child_of("e").priority(0),
            issue("c", "").child_of("e").priority(3).updated_after(9),
        ]);
        let c = cols(&snap, GroupByArg::Epic);
        let e = col(&c, &epic_key("e"));
        assert_eq!(e.cells, ["b", "c", "a"]);
        assert_eq!(e.count, 3);
    }

    // ---- other groupings ----

    #[test]
    fn status_value_order_and_closed_only_with_filter() {
        let snap = snapshot_of(vec![
            issue("a", "").closed(),
            issue("b", "").status(Status::Deferred),
            issue("c", "").status(Status::Blocked),
            issue("d", "").status(Status::InProgress),
            issue("e", ""),
        ]);
        let c = cols(&snap, GroupByArg::Status);
        assert_eq!(titles(&c), ["open", "in_progress", "blocked", "deferred"]);
        let c = all_cols(&snap, GroupByArg::Status);
        assert_eq!(
            titles(&c),
            ["open", "in_progress", "blocked", "deferred", "closed"]
        );
        assert_eq!(c[4].key, ColumnKey::Status(Status::Closed));
    }

    #[test]
    fn role_order_none_last_and_empty_hidden() {
        let snap = snapshot_of(vec![
            issue("a", ""),
            issue("b", "").role(Role::Human),
            issue("c", "").role(Role::Agent),
        ]);
        let c = cols(&snap, GroupByArg::Role);
        assert_eq!(titles(&c), ["agent", "human", "(none)"]);
        assert_eq!(c[2].key, ColumnKey::Role(None));
        assert_eq!(c[2].cells, ["a"]);

        let snap = snapshot_of(vec![issue("a", "").role(Role::Human), issue("b", "")]);
        assert_eq!(titles(&cols(&snap, GroupByArg::Role)), ["human", "(none)"]);
        let snap = snapshot_of(vec![issue("a", "").role(Role::Agent)]);
        assert_eq!(titles(&cols(&snap, GroupByArg::Role)), ["agent"]);
    }

    #[test]
    fn type_grouping_has_all_seven_in_order() {
        use IssueType::*;
        let types = [Chore, Gate, Task, Bug, Feature, Spec, Epic];
        let snap = snapshot_of(
            types
                .iter()
                .map(|t| issue(&format!("i-{}", t.as_str()), "").ty(*t))
                .collect::<Vec<_>>(),
        );
        let c = cols(&snap, GroupByArg::Type);
        assert_eq!(
            titles(&c),
            ["epic", "spec", "feature", "bug", "task", "gate", "chore"]
        );
        assert_eq!(c[0].key, ColumnKey::Type(Epic));
        assert!(c.iter().all(|c| c.count == 1 && c.header.is_none()));
    }

    #[test]
    fn priority_standard_then_others_numeric() {
        let snap = snapshot_of(vec![
            issue("a", "").priority(10),
            issue("b", "").priority(2),
            issue("c", "").priority(-1),
            issue("d", "").priority(0),
            issue("e", "").priority(7),
            issue("f", "").priority(4),
        ]);
        let c = cols(&snap, GroupByArg::Priority);
        assert_eq!(titles(&c), ["P0", "P2", "P4", "P-1", "P7", "P10"]);
        assert_eq!(c[3].key, ColumnKey::Priority(-1));
    }

    #[test]
    fn empty_columns_hidden_and_cells_sorted_in_every_grouping() {
        let snap = snapshot_of(vec![
            issue("a", "").priority(2).updated_after(1),
            issue("b", "").priority(2).updated_after(2),
            issue("c", "").priority(1),
        ]);
        let c = cols(&snap, GroupByArg::Status);
        assert_eq!(summary(&c), [("open".to_string(), vec!["c", "b", "a"])]);
        let c = cols(&snap, GroupByArg::Priority);
        assert_eq!(
            summary(&c),
            [
                ("P1".to_string(), vec!["c"]),
                ("P2".to_string(), vec!["b", "a"])
            ]
        );
    }

    #[test]
    fn grouping_cycle_order() {
        let mut g = GroupByArg::Epic;
        let mut seen = vec![g];
        for _ in 0..5 {
            g = g.next();
            seen.push(g);
        }
        assert_eq!(
            seen,
            [
                GroupByArg::Epic,
                GroupByArg::Status,
                GroupByArg::Role,
                GroupByArg::Type,
                GroupByArg::Priority,
                GroupByArg::Epic,
            ]
        );
    }
}
