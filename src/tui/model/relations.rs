//! Relationships shown in the detail view (spec §9.2). Pure: an issue and a
//! snapshot in, ordered sections out.

use crate::tui::data::Snapshot;
use crate::types::{DependencyType, Issue, Status};
use std::collections::BTreeSet;

/// A relationship section, in display order (spec §9.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Parent,
    BlockedBy,
    Children,
    Blocks,
    Related,
    DiscoveredFrom,
    Discoveries,
    Tracks,
    TrackedBy,
}

impl SectionKind {
    /// Every section in display order.
    pub const ALL: [SectionKind; 9] = [
        SectionKind::Parent,
        SectionKind::BlockedBy,
        SectionKind::Children,
        SectionKind::Blocks,
        SectionKind::Related,
        SectionKind::DiscoveredFrom,
        SectionKind::Discoveries,
        SectionKind::Tracks,
        SectionKind::TrackedBy,
    ];

    /// Heading text.
    pub fn title(self) -> &'static str {
        match self {
            SectionKind::Parent => "Parent",
            SectionKind::BlockedBy => "Blocked by",
            SectionKind::Children => "Children",
            SectionKind::Blocks => "Blocks",
            SectionKind::Related => "Related",
            SectionKind::DiscoveredFrom => "Discovered from",
            SectionKind::Discoveries => "Discoveries",
            SectionKind::Tracks => "Tracks",
            SectionKind::TrackedBy => "Tracked by",
        }
    }
}

/// One related issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The target's issue ID.
    pub id: String,
    /// `None` when the target does not exist (shown as `(missing)`).
    pub target: Option<Target>,
}

/// What the snapshot knows about an existing target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub status: Status,
    pub title: String,
}

impl Entry {
    pub fn is_missing(&self) -> bool {
        self.target.is_none()
    }

    /// Whether `Enter` can open this entry (false for a missing target).
    pub fn followable(&self) -> bool {
        self.target.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub kind: SectionKind,
    /// Sorted by ID; never empty.
    pub entries: Vec<Entry>,
}

/// All non-empty sections of one issue, in display order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Relationships {
    pub sections: Vec<Section>,
}

impl Relationships {
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    /// Every entry in display order (sections top to bottom): the order `Tab`
    /// walks through.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.sections.iter().flat_map(|s| s.entries.iter())
    }

    /// Number of entries over all sections.
    pub fn len(&self) -> usize {
        self.sections.iter().map(|s| s.entries.len()).sum()
    }

    /// The `index`th entry in display order.
    pub fn entry(&self, index: usize) -> Option<&Entry> {
        self.entries().nth(index)
    }

    /// The section holding the `index`th entry in display order.
    pub fn section_of(&self, index: usize) -> Option<SectionKind> {
        let mut rest = index;
        for s in &self.sections {
            if rest < s.entries.len() {
                return Some(s.kind);
            }
            rest -= s.entries.len();
        }
        None
    }
}

/// Build the relationship sections for `issue` (spec §9.2).
///
/// Outgoing edges come from the issue's own `depends_on`; reverse edges from
/// its `dependents` (both normalised by the parser/loader, so the legacy
/// `discovered-from` spelling is already `discovered_from`).
pub fn relationships(issue: &Issue, snapshot: &Snapshot) -> Relationships {
    let outgoing = |ty: DependencyType| -> BTreeSet<&str> {
        issue
            .depends_on
            .iter()
            .filter(|(_, t)| **t == ty)
            .map(|(id, _)| id.as_str())
            .collect()
    };
    let reverse = |ty: DependencyType| -> BTreeSet<&str> {
        issue
            .dependents
            .iter()
            .filter(|d| d.dep_type.parse::<DependencyType>().ok() == Some(ty))
            .map(|d| d.id.as_str())
            .collect()
    };

    use DependencyType::*;
    let related: BTreeSet<&str> = outgoing(RelatedTo)
        .union(&reverse(RelatedTo))
        .copied()
        .collect();

    let sections = SectionKind::ALL
        .into_iter()
        .filter_map(|kind| {
            let ids = match kind {
                SectionKind::Parent => outgoing(ChildOf),
                SectionKind::BlockedBy => outgoing(BlockedBy),
                SectionKind::Children => reverse(ChildOf),
                SectionKind::Blocks => reverse(BlockedBy),
                SectionKind::Related => related.clone(),
                SectionKind::DiscoveredFrom => outgoing(DiscoveredFrom),
                SectionKind::Discoveries => reverse(DiscoveredFrom),
                SectionKind::Tracks => outgoing(Tracks),
                SectionKind::TrackedBy => reverse(Tracks),
            };
            (!ids.is_empty()).then(|| Section {
                kind,
                entries: ids.into_iter().map(|id| entry(id, snapshot)).collect(),
            })
        })
        .collect();
    Relationships { sections }
}

fn entry(id: &str, snapshot: &Snapshot) -> Entry {
    Entry {
        id: id.to_string(),
        target: snapshot.issue(id).map(|t| Target {
            status: t.status,
            title: t.title.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing::*;

    fn rels(issues: Vec<IssueBuilder>, id: &str) -> Relationships {
        let snap = snapshot_of(issues);
        relationships(snap.issue(id).unwrap(), &snap)
    }

    fn section(r: &Relationships, kind: SectionKind) -> Vec<&str> {
        r.sections
            .iter()
            .find(|s| s.kind == kind)
            .map(|s| s.entries.iter().map(|e| e.id.as_str()).collect())
            .unwrap_or_default()
    }

    fn kinds(r: &Relationships) -> Vec<SectionKind> {
        r.sections.iter().map(|s| s.kind).collect()
    }

    #[test]
    fn no_relationships_gives_no_sections() {
        let r = rels(vec![issue("a", "A"), issue("b", "B")], "a");
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
        assert!(r.entry(0).is_none());
    }

    #[test]
    fn every_section_is_populated_in_display_order() {
        let r = rels(
            vec![
                issue("me", "Me")
                    .child_of("parent")
                    .blocked_by("blocker")
                    .related_to("rel-out")
                    .dep("origin", DependencyType::DiscoveredFrom)
                    .dep("tracked", DependencyType::Tracks),
                issue("parent", "Parent"),
                issue("blocker", "Blocker"),
                issue("rel-out", "Rel"),
                issue("origin", "Origin"),
                issue("tracked", "Tracked"),
                issue("kid", "Kid").child_of("me"),
                issue("waiter", "Waiter").blocked_by("me"),
                issue("found", "Found").dep("me", DependencyType::DiscoveredFrom),
                issue("tracker", "Tracker").dep("me", DependencyType::Tracks),
            ],
            "me",
        );
        use SectionKind::*;
        assert_eq!(
            kinds(&r),
            [
                Parent,
                BlockedBy,
                Children,
                Blocks,
                Related,
                DiscoveredFrom,
                Discoveries,
                Tracks,
                TrackedBy
            ]
        );
        assert_eq!(section(&r, Parent), ["parent"]);
        assert_eq!(section(&r, BlockedBy), ["blocker"]);
        assert_eq!(section(&r, Children), ["kid"]);
        assert_eq!(section(&r, Blocks), ["waiter"]);
        assert_eq!(section(&r, Related), ["rel-out"]);
        assert_eq!(section(&r, DiscoveredFrom), ["origin"]);
        assert_eq!(section(&r, Discoveries), ["found"]);
        assert_eq!(section(&r, Tracks), ["tracked"]);
        assert_eq!(section(&r, TrackedBy), ["tracker"]);
        assert_eq!(r.len(), 9);
    }

    #[test]
    fn empty_sections_are_omitted() {
        let r = rels(vec![issue("a", "A").blocked_by("b"), issue("b", "B")], "a");
        assert_eq!(kinds(&r), [SectionKind::BlockedBy]);
        // The other side of the edge only has "Blocks".
        let r = rels(vec![issue("a", "A").blocked_by("b"), issue("b", "B")], "b");
        assert_eq!(kinds(&r), [SectionKind::Blocks]);
    }

    #[test]
    fn entries_are_sorted_by_id() {
        let r = rels(
            vec![
                issue("me", "Me")
                    .child_of("p-3")
                    .child_of("p-1")
                    .child_of("p-2"),
                issue("z", "Z").child_of("me"),
                issue("c", "C").child_of("me"),
                issue("m", "M").child_of("me"),
            ],
            "me",
        );
        assert_eq!(section(&r, SectionKind::Parent), ["p-1", "p-2", "p-3"]);
        assert_eq!(section(&r, SectionKind::Children), ["c", "m", "z"]);
    }

    #[test]
    fn entries_carry_status_and_title() {
        let r = rels(
            vec![
                issue("a", "A").blocked_by("b").blocked_by("c"),
                issue("b", "Open one"),
                issue("c", "Done").closed(),
            ],
            "a",
        );
        let e: Vec<_> = r.entries().collect();
        assert_eq!(
            e[0].target,
            Some(Target {
                status: Status::Open,
                title: "Open one".into()
            })
        );
        assert_eq!(e[1].target.as_ref().unwrap().status, Status::Closed);
        assert!(e.iter().all(|e| e.followable() && !e.is_missing()));
    }

    #[test]
    fn missing_target_is_marked_and_not_followable() {
        let r = rels(
            vec![
                issue("a", "A").child_of("gone").blocked_by("b"),
                issue("b", "B"),
            ],
            "a",
        );
        let gone = &r.sections[0].entries[0];
        assert_eq!(gone.id, "gone");
        assert!(gone.is_missing());
        assert!(!gone.followable());
        assert!(r.sections[1].entries[0].followable());
    }

    #[test]
    fn related_to_in_both_directions_appears_once() {
        // a -> b and b -> a, plus a one-way edge from c and one to d.
        let r = rels(
            vec![
                issue("a", "A").related_to("b").related_to("d"),
                issue("b", "B").related_to("a"),
                issue("c", "C").related_to("a"),
                issue("d", "D"),
            ],
            "a",
        );
        assert_eq!(section(&r, SectionKind::Related), ["b", "c", "d"]);
        // From the other end the single edge shows up too.
        let r = rels(vec![issue("a", "A").related_to("d"), issue("d", "D")], "d");
        assert_eq!(section(&r, SectionKind::Related), ["a"]);
    }

    #[test]
    fn flat_list_follows_display_order() {
        let r = rels(
            vec![
                issue("me", "Me")
                    .blocked_by("x")
                    .child_of("p")
                    .child_of("gone"),
                issue("x", "X"),
                issue("p", "P"),
                issue("k", "K").child_of("me"),
            ],
            "me",
        );
        let ids: Vec<_> = r.entries().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["gone", "p", "x", "k"]);
        assert_eq!(r.entry(2).unwrap().id, "x");
        assert!(r.entry(4).is_none());
        let secs: Vec<_> = (0..4).map(|i| r.section_of(i).unwrap()).collect();
        use SectionKind::*;
        assert_eq!(secs, [Parent, Parent, BlockedBy, Children]);
        assert_eq!(r.section_of(4), None);
        // Missing entries stay in the list (so Tab can land on them).
        assert!(!r.entry(0).unwrap().followable());
    }

    #[test]
    fn legacy_discovered_from_spelling_lands_in_discovered_from() {
        use crate::format::markdown_to_issue;
        let text = "---\ntitle: Found\nstatus: open\npriority: 2\nissue_type: task\ndependencies:\n  tz-1: discovered-from\ncreated_at: 2025-01-01T00:00:00Z\nupdated_at: 2025-01-01T00:00:00Z\n---\n";
        let mut legacy = markdown_to_issue("tz-2", text).expect("parse");
        legacy.created_at = base_time();
        legacy.updated_at = base_time();
        let snap = snapshot_of(vec![issue("tz-1", "Origin").build(), legacy]);
        let r = relationships(snap.issue("tz-2").unwrap(), &snap);
        assert_eq!(section(&r, SectionKind::DiscoveredFrom), ["tz-1"]);
        let r = relationships(snap.issue("tz-1").unwrap(), &snap);
        assert_eq!(section(&r, SectionKind::Discoveries), ["tz-2"]);
    }
}
