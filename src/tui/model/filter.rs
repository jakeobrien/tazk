//! Role/status/type/priority/effort filters (spec §6) and the combined
//! filter + search helper [`visible_issues`].
//! Pure logic (no rendering, no terminal access).
//!
//! Each property has a [`FilterSet`]: the set of values that are allowed. An
//! issue is visible when every property allows its value ([`Filters::matches`]).
//!
//! # Representation
//!
//! A set is either "everything except these" or "only these". The first form
//! is what makes the default work for values nobody has listed yet: the
//! default priority filter allows *every* priority, including an out-of-range
//! one (say `7`) that shows up after a reload. [`FilterSet::from_selection`]
//! (used when a picker is applied) keeps the "everything except" form when all
//! or all-but-one of the listed options are selected and otherwise stores the
//! chosen values.
//!
//! # Summary strings
//!
//! [`FilterSet::summary`] renders the filter bar text: `all` when every listed
//! value is allowed, `none selected` when none is (not `none`, which is the
//! label of the unset role/effort value), `!v` when exactly one is excluded,
//! and otherwise the allowed values joined by `,`.

use super::search;
use crate::tui::app::FilterKind;
use crate::tui::data::Snapshot;
use crate::types::{Issue, IssueType, Role, Status};
use std::collections::{BTreeSet, HashSet};
use std::hash::Hash;

/// Priorities that are always listed (P0–P4), whether or not the data has them.
pub const STANDARD_PRIORITIES: std::ops::RangeInclusive<i32> = 0..=4;

/// A value a filter can select.
pub trait FilterValue: Clone + Eq + Hash {
    /// Short text for the filter bar summary (`none`, `P1`, `in_progress`).
    fn label(&self) -> String;
    /// Text for the picker row. Defaults to [`label`](Self::label).
    fn picker_label(&self) -> String {
        self.label()
    }
}

impl FilterValue for Role {
    fn label(&self) -> String {
        self.as_str().to_string()
    }
}

impl FilterValue for Status {
    fn label(&self) -> String {
        self.as_str().to_string()
    }
}

impl FilterValue for IssueType {
    fn label(&self) -> String {
        self.as_str().to_string()
    }
}

impl FilterValue for i32 {
    fn label(&self) -> String {
        format!("P{self}")
    }
}

impl FilterValue for u8 {
    fn label(&self) -> String {
        self.to_string()
    }
}

/// `None` is the "unset" value of role and effort.
impl<T: FilterValue> FilterValue for Option<T> {
    fn label(&self) -> String {
        match self {
            Some(v) => v.label(),
            None => "none".to_string(),
        }
    }
    fn picker_label(&self) -> String {
        match self {
            Some(v) => v.picker_label(),
            None => "(none)".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Selection<T: Eq + Hash> {
    AllExcept(HashSet<T>),
    Only(HashSet<T>),
}

/// The set of allowed values of one property. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterSet<T: FilterValue>(Selection<T>);

impl<T: FilterValue> FilterSet<T> {
    /// Every value allowed, including ones not seen yet.
    pub fn all() -> Self {
        Self(Selection::AllExcept(HashSet::new()))
    }

    /// Every value allowed except `excluded`.
    pub fn all_except(excluded: impl IntoIterator<Item = T>) -> Self {
        Self(Selection::AllExcept(excluded.into_iter().collect()))
    }

    /// Only `allowed` values allowed. Empty means nothing is visible.
    pub fn only(allowed: impl IntoIterator<Item = T>) -> Self {
        Self(Selection::Only(allowed.into_iter().collect()))
    }

    /// Whether `value` is allowed.
    pub fn allows(&self, value: &T) -> bool {
        match &self.0 {
            Selection::AllExcept(excluded) => !excluded.contains(value),
            Selection::Only(allowed) => allowed.contains(value),
        }
    }

    /// Build the set a picker produces: `selected[i]` says whether `options[i]`
    /// is ticked. All selected → [`all`](Self::all); exactly one unticked →
    /// [`all_except`](Self::all_except) that value; otherwise only the ticked
    /// values.
    pub fn from_selection(options: &[T], selected: &[bool]) -> Self {
        let ticked = |i: usize| selected.get(i).copied().unwrap_or(false);
        let unticked: Vec<T> = options
            .iter()
            .enumerate()
            .filter(|(i, _)| !ticked(*i))
            .map(|(_, v)| v.clone())
            .collect();
        match unticked.len() {
            0 | 1 => Self::all_except(unticked),
            _ => Self::only(
                options
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| ticked(*i))
                    .map(|(_, v)| v.clone()),
            ),
        }
    }

    /// For each of `options`, whether it is allowed (the picker's checkboxes).
    pub fn selected(&self, options: &[T]) -> Vec<bool> {
        options.iter().map(|v| self.allows(v)).collect()
    }

    /// The filter bar text for this set over the listed `options`.
    pub fn summary(&self, options: &[T]) -> String {
        let allowed: Vec<&T> = options.iter().filter(|v| self.allows(v)).collect();
        let excluded: Vec<&T> = options.iter().filter(|v| !self.allows(v)).collect();
        if excluded.is_empty() {
            "all".to_string()
        } else if allowed.is_empty() {
            "none selected".to_string()
        } else if excluded.len() == 1 {
            format!("!{}", excluded[0].label())
        } else {
            allowed
                .iter()
                .map(|v| v.label())
                .collect::<Vec<_>>()
                .join(",")
        }
    }
}

/// All statuses in picker order. Built by an exhaustive `match` (no wildcard
/// arm), so adding a status is a compile error here.
pub fn status_options() -> Vec<Status> {
    fn next(s: Status) -> Option<Status> {
        match s {
            Status::Open => Some(Status::InProgress),
            Status::InProgress => Some(Status::Blocked),
            Status::Blocked => Some(Status::Deferred),
            Status::Deferred => Some(Status::Closed),
            Status::Closed => None,
        }
    }
    chain(Status::Open, next)
}

/// All issue types in spec order (§1): epic, spec, feature, bug, task, gate,
/// chore. Built by an exhaustive `match` (no wildcard arm), so adding a type
/// is a compile error here.
pub fn type_options() -> Vec<IssueType> {
    fn next(t: IssueType) -> Option<IssueType> {
        match t {
            IssueType::Epic => Some(IssueType::Spec),
            IssueType::Spec => Some(IssueType::Feature),
            IssueType::Feature => Some(IssueType::Bug),
            IssueType::Bug => Some(IssueType::Task),
            IssueType::Task => Some(IssueType::Gate),
            IssueType::Gate => Some(IssueType::Chore),
            IssueType::Chore => None,
        }
    }
    chain(IssueType::Epic, next)
}

/// Roles: human, agent, then unset.
pub fn role_options() -> Vec<Option<Role>> {
    vec![Some(Role::Human), Some(Role::Agent), None]
}

/// Efforts: the valid values in order, then unset.
pub fn effort_options() -> Vec<Option<u8>> {
    (crate::types::MIN_EFFORT..=crate::types::MAX_EFFORT)
        .map(Some)
        .chain(std::iter::once(None))
        .collect()
}

/// Priorities: P0–P4 plus any other value present in the data, ascending.
pub fn priority_options(snapshot: &Snapshot) -> Vec<i32> {
    let set: BTreeSet<i32> = STANDARD_PRIORITIES
        .chain(snapshot.issues.iter().map(|i| i.priority))
        .collect();
    set.into_iter().collect()
}

fn chain<T: Copy>(first: T, next: impl Fn(T) -> Option<T>) -> Vec<T> {
    std::iter::successors(Some(first), |&v| next(v)).collect()
}

/// One row of the picker popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerOption {
    /// Row text without the checkbox (`open`, `(none)`, `P7`).
    pub label: String,
    pub selected: bool,
}

/// All five filters (spec §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filters {
    pub role: FilterSet<Option<Role>>,
    pub status: FilterSet<Status>,
    pub issue_type: FilterSet<IssueType>,
    pub priority: FilterSet<i32>,
    pub effort: FilterSet<Option<u8>>,
}

impl Default for Filters {
    /// Everything allowed except status `closed`.
    fn default() -> Self {
        Self {
            role: FilterSet::all(),
            status: FilterSet::all_except([Status::Closed]),
            issue_type: FilterSet::all(),
            priority: FilterSet::all(),
            effort: FilterSet::all(),
        }
    }
}

impl Filters {
    /// Restore the default (`c`).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// AND across properties.
    pub fn matches(&self, issue: &Issue) -> bool {
        self.role.allows(&issue.role)
            && self.status.allows(&issue.status)
            && self.issue_type.allows(&issue.issue_type)
            && self.priority.allows(&issue.priority)
            && self.effort.allows(&issue.effort)
    }

    /// Filter bar text for one filter (e.g. `!closed`).
    pub fn summary(&self, kind: FilterKind, snapshot: &Snapshot) -> String {
        match kind {
            FilterKind::Role => self.role.summary(&role_options()),
            FilterKind::Status => self.status.summary(&status_options()),
            FilterKind::Type => self.issue_type.summary(&type_options()),
            FilterKind::Priority => self.priority.summary(&priority_options(snapshot)),
            FilterKind::Effort => self.effort.summary(&effort_options()),
        }
    }

    /// Rows for the picker of `kind`, in display order.
    pub fn picker_options(&self, kind: FilterKind, snapshot: &Snapshot) -> Vec<PickerOption> {
        fn rows<T: FilterValue>(set: &FilterSet<T>, options: &[T]) -> Vec<PickerOption> {
            options
                .iter()
                .map(|v| PickerOption {
                    label: v.picker_label(),
                    selected: set.allows(v),
                })
                .collect()
        }
        match kind {
            FilterKind::Role => rows(&self.role, &role_options()),
            FilterKind::Status => rows(&self.status, &status_options()),
            FilterKind::Type => rows(&self.issue_type, &type_options()),
            FilterKind::Priority => rows(&self.priority, &priority_options(snapshot)),
            FilterKind::Effort => rows(&self.effort, &effort_options()),
        }
    }

    /// Apply a picker result: `selected[i]` is the checkbox of row `i` of
    /// [`picker_options`](Self::picker_options) (same `snapshot`).
    pub fn apply_picker(&mut self, kind: FilterKind, snapshot: &Snapshot, selected: &[bool]) {
        match kind {
            FilterKind::Role => {
                self.role = FilterSet::from_selection(&role_options(), selected);
            }
            FilterKind::Status => {
                self.status = FilterSet::from_selection(&status_options(), selected);
            }
            FilterKind::Type => {
                self.issue_type = FilterSet::from_selection(&type_options(), selected);
            }
            FilterKind::Priority => {
                self.priority = FilterSet::from_selection(&priority_options(snapshot), selected);
            }
            FilterKind::Effort => {
                self.effort = FilterSet::from_selection(&effort_options(), selected);
            }
        }
    }
}

/// Issues passing both `filters` and the search `query` (AND), in snapshot
/// order. An empty query matches everything.
pub fn visible_issues<'a>(
    snapshot: &'a Snapshot,
    filters: &Filters,
    query: &str,
) -> Vec<&'a Issue> {
    let needle = search::Query::new(query);
    snapshot
        .issues
        .iter()
        .filter(|i| filters.matches(i) && needle.matches_issue(i))
        .collect()
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::tui::testing::*;

    fn ids(v: Vec<&Issue>) -> Vec<&str> {
        v.into_iter().map(|i| i.id.as_str()).collect()
    }

    fn labels<T: FilterValue>(v: &[T]) -> Vec<String> {
        v.iter().map(|x| x.label()).collect()
    }

    #[test]
    fn default_hides_closed_only() {
        let snap = snapshot_of(vec![
            issue("a", "").status(Status::Open),
            issue("b", "").status(Status::InProgress),
            issue("c", "").status(Status::Blocked),
            issue("d", "").status(Status::Deferred),
            issue("e", "").closed(),
        ]);
        assert_eq!(
            ids(visible_issues(&snap, &Filters::default(), "")),
            ["a", "b", "c", "d"]
        );
    }

    #[test]
    fn status_options_order() {
        assert_eq!(
            labels(&status_options()),
            ["open", "in_progress", "blocked", "deferred", "closed"]
        );
    }

    #[test]
    fn none_role_and_effort() {
        let snap = snapshot_of(vec![
            issue("a", "").role(Role::Agent).effort(2),
            issue("b", ""),
            issue("c", "").role(Role::Human),
        ]);
        let mut f = Filters::default();
        // Default allows unset.
        assert_eq!(ids(visible_issues(&snap, &f, "")), ["a", "b", "c"]);

        f.role = FilterSet::only([None]);
        assert_eq!(ids(visible_issues(&snap, &f, "")), ["b"]);
        f.role = FilterSet::only([Some(Role::Agent), None]);
        assert_eq!(ids(visible_issues(&snap, &f, "")), ["a", "b"]);
        f.role = FilterSet::all_except([None]);
        assert_eq!(ids(visible_issues(&snap, &f, "")), ["a", "c"]);

        f.role = FilterSet::all();
        f.effort = FilterSet::only([None]);
        assert_eq!(ids(visible_issues(&snap, &f, "")), ["b", "c"]);
        f.effort = FilterSet::only([Some(2)]);
        assert_eq!(ids(visible_issues(&snap, &f, "")), ["a"]);
    }

    #[test]
    fn priority_options_include_out_of_range_from_data() {
        let empty = snapshot_of(Vec::<IssueBuilder>::new());
        assert_eq!(priority_options(&empty), [0, 1, 2, 3, 4]);

        let snap = snapshot_of(vec![
            issue("a", "").priority(7),
            issue("b", "").priority(2),
            issue("c", "").priority(-1),
            issue("d", "").priority(7),
        ]);
        assert_eq!(priority_options(&snap), [-1, 0, 1, 2, 3, 4, 7]);
        assert_eq!(
            labels(&priority_options(&snap)),
            ["P-1", "P0", "P1", "P2", "P3", "P4", "P7"]
        );
        // The default allows the out-of-range value.
        assert_eq!(
            ids(visible_issues(&snap, &Filters::default(), "")),
            ["a", "b", "c", "d"]
        );
        let mut f = Filters::default();
        f.priority = FilterSet::only([7]);
        assert_eq!(ids(visible_issues(&snap, &f, "")), ["a", "d"]);
    }

    #[test]
    fn type_options_are_the_seven_types_in_spec_order() {
        assert_eq!(
            labels(&type_options()),
            ["epic", "spec", "feature", "bug", "task", "gate", "chore"]
        );
    }

    #[test]
    fn filtering_on_gate_and_spec() {
        let snap = snapshot_of(vec![
            issue("a", "").ty(IssueType::Gate),
            issue("b", "").ty(IssueType::Spec),
            issue("c", "").ty(IssueType::Task),
            issue("d", "").ty(IssueType::Epic),
        ]);
        let mut f = Filters::default();
        f.issue_type = FilterSet::only([IssueType::Gate]);
        assert_eq!(ids(visible_issues(&snap, &f, "")), ["a"]);
        f.issue_type = FilterSet::only([IssueType::Spec, IssueType::Task]);
        assert_eq!(ids(visible_issues(&snap, &f, "")), ["b", "c"]);
        f.issue_type = FilterSet::all_except([IssueType::Gate, IssueType::Spec]);
        assert_eq!(ids(visible_issues(&snap, &f, "")), ["c", "d"]);
    }

    #[test]
    fn properties_combine_with_and() {
        let snap = snapshot_of(vec![
            issue("a", "").role(Role::Agent).priority(0),
            issue("b", "").role(Role::Agent).priority(1),
            issue("c", "").role(Role::Human).priority(0),
        ]);
        let mut f = Filters::default();
        f.role = FilterSet::only([Some(Role::Agent)]);
        f.priority = FilterSet::only([0]);
        assert_eq!(ids(visible_issues(&snap, &f, "")), ["a"]);
    }

    #[test]
    fn summaries() {
        let snap = snapshot_of(vec![issue("a", "").priority(7)]);
        let f = Filters::default();
        assert_eq!(f.summary(FilterKind::Status, &snap), "!closed");
        assert_eq!(f.summary(FilterKind::Role, &snap), "all");
        assert_eq!(f.summary(FilterKind::Type, &snap), "all");
        assert_eq!(f.summary(FilterKind::Priority, &snap), "all");
        assert_eq!(f.summary(FilterKind::Effort, &snap), "all");

        let mut f = Filters::default();
        f.status = FilterSet::all();
        assert_eq!(f.summary(FilterKind::Status, &snap), "all");
        f.status = FilterSet::only([Status::Closed]);
        assert_eq!(f.summary(FilterKind::Status, &snap), "closed");
        f.status = FilterSet::only([Status::Open, Status::Blocked]);
        assert_eq!(f.summary(FilterKind::Status, &snap), "open,blocked");
        f.status = FilterSet::only([]);
        assert_eq!(f.summary(FilterKind::Status, &snap), "none selected");

        f.role = FilterSet::only([Some(Role::Agent), None]);
        assert_eq!(f.summary(FilterKind::Role, &snap), "!human");
        f.role = FilterSet::only([Some(Role::Human), None]);
        assert_eq!(f.summary(FilterKind::Role, &snap), "!agent");
        f.effort = FilterSet::only([Some(1), Some(2), None]);
        assert_eq!(f.summary(FilterKind::Effort, &snap), "1,2,none");
        f.role = FilterSet::all_except([None]);
        assert_eq!(f.summary(FilterKind::Role, &snap), "!none");
        f.role = FilterSet::only([None]);
        assert_eq!(f.summary(FilterKind::Role, &snap), "none");
        f.role = FilterSet::only([]);
        assert_eq!(f.summary(FilterKind::Role, &snap), "none selected");

        f.priority = FilterSet::only([0, 1, 7]);
        assert_eq!(f.summary(FilterKind::Priority, &snap), "P0,P1,P7");
        f.priority = FilterSet::all_except([7]);
        assert_eq!(f.summary(FilterKind::Priority, &snap), "!P7");

        f.effort = FilterSet::only([Some(1), None]);
        assert_eq!(f.summary(FilterKind::Effort, &snap), "1,none");

        f.issue_type = FilterSet::all_except([IssueType::Gate]);
        assert_eq!(f.summary(FilterKind::Type, &snap), "!gate");
    }

    #[test]
    fn reset_restores_default() {
        let mut f = Filters::default();
        f.status = FilterSet::only([Status::Closed]);
        f.role = FilterSet::only([]);
        f.priority = FilterSet::only([0]);
        f.reset();
        assert_eq!(f, Filters::default());
    }

    #[test]
    fn picker_round_trip() {
        let snap = snapshot_of(vec![issue("a", "").priority(7)]);
        let mut f = Filters::default();

        let rows = f.picker_options(FilterKind::Status, &snap);
        let sel: Vec<_> = rows
            .iter()
            .map(|r| (r.label.as_str(), r.selected))
            .collect();
        assert_eq!(
            sel,
            [
                ("open", true),
                ("in_progress", true),
                ("blocked", true),
                ("deferred", true),
                ("closed", false)
            ]
        );

        // Role rows use the "(none)" label.
        let rows = f.picker_options(FilterKind::Role, &snap);
        assert_eq!(rows[2].label, "(none)");
        assert_eq!(rows.len(), 3);

        // Unchanged selection keeps the default.
        let selected: Vec<bool> = rows_selected(&f, FilterKind::Status, &snap);
        f.apply_picker(FilterKind::Status, &snap, &selected);
        assert_eq!(f, Filters::default());

        // Nothing selected is allowed.
        f.apply_picker(FilterKind::Status, &snap, &[false; 5]);
        assert_eq!(f.summary(FilterKind::Status, &snap), "none selected");
        assert!(ids(visible_issues(&snap, &f, "")).is_empty());

        // Select all.
        f.apply_picker(FilterKind::Status, &snap, &[true; 5]);
        assert_eq!(f.summary(FilterKind::Status, &snap), "all");

        // Priority picker includes the extra value.
        let rows = f.picker_options(FilterKind::Priority, &snap);
        assert_eq!(rows.last().unwrap().label, "P7");
        f.apply_picker(
            FilterKind::Priority,
            &snap,
            &[true, true, false, false, false, false],
        );
        assert_eq!(f.summary(FilterKind::Priority, &snap), "P0,P1");
    }

    fn rows_selected(f: &Filters, kind: FilterKind, snap: &Snapshot) -> Vec<bool> {
        f.picker_options(kind, snap)
            .into_iter()
            .map(|r| r.selected)
            .collect()
    }

    #[test]
    fn all_but_one_keeps_unlisted_values_allowed() {
        // Unticking one status leaves statuses added later allowed.
        let set = FilterSet::from_selection(&[1, 2, 3], &[true, false, true]);
        assert_eq!(set, FilterSet::all_except([2]));
        assert!(set.allows(&99));
        let set = FilterSet::from_selection(&[1, 2, 3], &[true, false, false]);
        assert_eq!(set, FilterSet::only([1]));
        assert!(!set.allows(&99));
    }

    #[test]
    fn filter_and_search_combine_with_and() {
        let snap = snapshot_of(vec![
            issue("tz-1", "Login page").role(Role::Agent),
            issue("tz-2", "Login bug").role(Role::Human),
            issue("tz-3", "Logout").role(Role::Agent),
            issue("tz-4", "Login old").role(Role::Agent).closed(),
        ]);
        let mut f = Filters::default();
        assert_eq!(ids(visible_issues(&snap, &f, "login")), ["tz-1", "tz-2"]);
        f.role = FilterSet::only([Some(Role::Agent)]);
        assert_eq!(ids(visible_issues(&snap, &f, "login")), ["tz-1"]);
        assert_eq!(ids(visible_issues(&snap, &f, "")), ["tz-1", "tz-3"]);
        assert!(visible_issues(&snap, &f, "zzz").is_empty());
        // Closed issue matches the query but not the status filter.
        assert!(!ids(visible_issues(&snap, &f, "old")).contains(&"tz-4"));
        f.status = FilterSet::all();
        assert_eq!(ids(visible_issues(&snap, &f, "old")), ["tz-4"]);
    }
}
