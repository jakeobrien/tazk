//! Triage items: an inbox of reports kept apart from issues.
//!
//! A triage item is a plain Markdown file in `.tazk/triage/` with no
//! frontmatter: no ID, title, status, priority, role, assignee, labels,
//! dependencies or comments. Being in that folder is what makes it a triage
//! item. Reporters (code review, validation, testing, ad hoc feedback) write
//! one without having to think about planning. A triager later sweeps the
//! folder and, for each item, promotes it to an issue (several items may become
//! one issue), attaches it to an existing issue as a comment, or removes it.
//! Promoting or attaching deletes the item; git history keeps the original.
//!
//! Item names are `<UTC timestamp>-<4 random base36 chars>.md`, e.g.
//! `20251104T001800-k3f9.md`. They sort by creation time and are unique without
//! coordination, so items reported on different branches never collide. Any
//! other `*.md` file dropped into the folder by hand is also an item.

use super::{
    append_paragraph, new_comment, search_replace_text, serialize_comments, skipped_file, Loaded,
    NewIssue, Storage,
};
use crate::format::issue_to_markdown;
use crate::paths::{ensure_contained, IssueId};
use crate::transaction::{atomic_write, FileTransaction};
use crate::types::{Comment, Issue};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Name of the triage folder inside the `.tazk` directory.
pub const TRIAGE_DIR: &str = "triage";

const EXTENSION: &str = ".md";

/// Separator placed between item bodies when several items are promoted to one
/// issue and no description is given.
const PROMOTE_SEPARATOR: &str = "\n\n---\n\n";

/// One triage item: its file name and contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TriageItem {
    /// File name within `.tazk/triage/`, e.g. `20251104T001800-k3f9.md`.
    pub name: String,
    /// The file's contents.
    pub body: String,
}

/// Join text fragments into a stored item body: each fragment loses its
/// leading blank lines and trailing whitespace, empty fragments are dropped,
/// the rest are separated by a blank line, and the result ends in one newline.
/// Returns an empty string when every fragment is blank.
pub fn compose_body(parts: &[&str]) -> String {
    let parts: Vec<&str> = parts
        .iter()
        .map(|part| part.trim_end())
        .map(|part| part.trim_start_matches(['\n', '\r']))
        .filter(|part| !part.trim().is_empty())
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!("{}\n", parts.join("\n\n"))
    }
}

/// A fresh item name for `now`: `YYYYMMDDTHHMMSS-xxxx.md` (UTC). Lowercase
/// only after the timestamp, so names stay distinct on case-insensitive file
/// systems.
fn generate_name(now: DateTime<Utc>) -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut rng = rand::thread_rng();
    let suffix: String = (0..4)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect();
    format!("{}-{}{}", now.format("%Y%m%dT%H%M%S"), suffix, EXTENSION)
}

/// Whether a directory entry name is a triage item file.
fn is_item_file_name(name: &str) -> bool {
    name.ends_with(EXTENSION) && !name.starts_with('.') && IssueId::parse(name).is_ok()
}

/// Turn a user-supplied item name (with or without `.md`) into a file name.
fn normalize_name(name: &str) -> Result<String> {
    let file_name = if name.ends_with(EXTENSION) {
        name.to_string()
    } else {
        format!("{name}{EXTENSION}")
    };
    anyhow::ensure!(
        is_item_file_name(&file_name),
        "Invalid triage item name {name:?}: expected a file name such as 20251104T001800-k3f9.md"
    );
    Ok(file_name)
}

impl Storage {
    fn triage_dir(&self) -> PathBuf {
        self.tazk_dir.join(TRIAGE_DIR)
    }

    /// Resolve a user-supplied item reference to its file name and path. Takes
    /// a bare name (`NAME` or `NAME.md`) or a path to a file in the triage
    /// folder, so a tab-completed `.tazk/triage/NAME.md` works too. Fails if the
    /// item does not exist.
    pub fn triage_item_path(&self, reference: &str) -> Result<(String, PathBuf)> {
        let dir = self.triage_dir();
        let name = if reference.contains('/') || reference.contains(std::path::MAIN_SEPARATOR) {
            let path = Path::new(reference);
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .with_context(|| format!("Invalid triage item reference {reference:?}"))?;
            let parent = path.parent().unwrap_or(Path::new("."));
            let in_triage_dir = match (parent.canonicalize(), dir.canonicalize()) {
                (Ok(parent), Ok(dir)) => parent == dir,
                _ => false,
            };
            anyhow::ensure!(
                in_triage_dir,
                "{reference} is not in the triage folder ({})",
                dir.display()
            );
            file_name.to_string()
        } else {
            reference.to_string()
        };
        let name = normalize_name(&name)?;
        let path = dir.join(&name);
        ensure_contained(&self.tazk_dir, &path)?;
        anyhow::ensure!(
            path.is_file(),
            "Triage item not found: {name}. Run `tz triage list` to see item names."
        );
        Ok((name, path))
    }

    fn read_triage_item_no_lock(&self, reference: &str) -> Result<(TriageItem, PathBuf)> {
        let (name, path) = self.triage_item_path(reference)?;
        let body = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read triage item {}", path.display()))?;
        Ok((TriageItem { name, body }, path))
    }

    /// Read every referenced item, in order, skipping repeats. Fails before
    /// anything changes if any item is missing.
    fn read_triage_items_no_lock(
        &self,
        references: &[String],
    ) -> Result<Vec<(TriageItem, PathBuf)>> {
        anyhow::ensure!(!references.is_empty(), "No triage items given");
        let mut items: Vec<(TriageItem, PathBuf)> = Vec::new();
        for reference in references {
            let (item, path) = self.read_triage_item_no_lock(reference)?;
            if !items.iter().any(|(seen, _)| seen.name == item.name) {
                items.push((item, path));
            }
        }
        Ok(items)
    }

    fn triage_names_no_lock(&self) -> Result<Vec<String>> {
        let dir = self.triage_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        ensure_contained(&self.tazk_dir, &dir)?;
        let mut names = Vec::new();
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("Failed to read triage folder: {}", dir.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                eprintln!(
                    "Warning: skipping triage file with a non-UTF-8 name: {}",
                    entry.path().display()
                );
                continue;
            };
            if name.starts_with('.') || !name.ends_with(EXTENSION) {
                continue;
            }
            if !is_item_file_name(&name) {
                eprintln!("Warning: skipping triage file with an unusable name: {name}");
                continue;
            }
            names.push(name);
        }
        names.sort();
        Ok(names)
    }

    /// Store a new triage item. `body` is normalized with [`compose_body`] and
    /// must not be blank.
    pub fn add_triage_item(&self, body: &str) -> Result<TriageItem> {
        let body = compose_body(&[body]);
        anyhow::ensure!(!body.is_empty(), "A triage item needs some text");
        let _lock = self.lock_for_write()?;

        let dir = self.triage_dir();
        ensure_contained(&self.tazk_dir, &dir)?;
        fs::create_dir_all(&dir).context("Failed to create triage folder")?;

        let now = Utc::now();
        for _ in 0..32 {
            let name = generate_name(now);
            let path = dir.join(&name);
            if path.exists() {
                continue;
            }
            atomic_write(&path, body.as_bytes())
                .with_context(|| format!("Failed to write triage item {}", path.display()))?;
            return Ok(TriageItem { name, body });
        }
        anyhow::bail!("Could not find a free triage item name; try again")
    }

    /// Every triage item, sorted by name (creation time, to the second).
    pub fn list_triage_items(&self) -> Result<Vec<TriageItem>> {
        let _lock = self.lock_for_read()?;
        self.triage_names_no_lock()?
            .into_iter()
            .map(|name| Ok(self.read_triage_item_no_lock(&name)?.0))
            .collect()
    }

    /// Every triage item that can be read, sorted by name; unreadable files
    /// (e.g. non-UTF-8) are reported in `skipped` instead of failing.
    pub fn list_triage_items_tolerant(&self) -> Result<Loaded<TriageItem>> {
        let _lock = self.lock_for_read()?;
        let dir = self.triage_dir();
        let mut items = Vec::new();
        let mut skipped = Vec::new();
        for name in self.triage_names_no_lock()? {
            let path = dir.join(&name);
            match fs::read_to_string(&path) {
                Ok(body) => items.push(TriageItem { name, body }),
                Err(err) => {
                    let err = anyhow::Error::from(err)
                        .context(format!("Failed to read triage item {}", path.display()));
                    skipped.push(skipped_file(path, &err));
                }
            }
        }
        Ok(Loaded { items, skipped })
    }

    /// One triage item.
    pub fn get_triage_item(&self, reference: &str) -> Result<TriageItem> {
        let _lock = self.lock_for_read()?;
        Ok(self.read_triage_item_no_lock(reference)?.0)
    }

    fn edit_triage_item(
        &self,
        reference: &str,
        edit: impl FnOnce(&TriageItem) -> Result<String>,
    ) -> Result<TriageItem> {
        let _lock = self.lock_for_write()?;
        let (item, path) = self.read_triage_item_no_lock(reference)?;
        let body = compose_body(&[&edit(&item)?]);
        anyhow::ensure!(
            !body.is_empty(),
            "The edit would leave triage item {} empty. Use `tz triage rm {}` to delete it.",
            item.name,
            item.name
        );
        atomic_write(&path, body.as_bytes())
            .with_context(|| format!("Failed to write triage item {}", path.display()))?;
        Ok(TriageItem {
            name: item.name,
            body,
        })
    }

    /// Replace `search` with `replace` in an item's body, with the same rules as
    /// `tz update --search/--replace`: the text must match exactly once unless
    /// `replace_all` is set.
    pub fn search_replace_triage_item(
        &self,
        reference: &str,
        search: &str,
        replace: &str,
        replace_all: bool,
    ) -> Result<TriageItem> {
        anyhow::ensure!(!search.is_empty(), "--search text must not be empty");
        self.edit_triage_item(reference, |item| {
            search_replace_text(
                &item.body,
                search,
                replace,
                replace_all,
                &format!("triage item {}", item.name),
            )
        })
    }

    /// Append `text` to an item's body as its own paragraph.
    pub fn append_to_triage_item(&self, reference: &str, text: &str) -> Result<TriageItem> {
        anyhow::ensure!(!text.is_empty(), "--append text must not be empty");
        self.edit_triage_item(reference, |item| Ok(append_paragraph(&item.body, text)))
    }

    /// Delete triage items. All of them must exist; otherwise nothing is
    /// deleted. Returns the deleted names.
    pub fn remove_triage_items(&self, references: &[String]) -> Result<Vec<String>> {
        let _lock = self.lock_for_write()?;
        let items = self.read_triage_items_no_lock(references)?;
        let mut transaction = FileTransaction::new(&self.tazk_dir);
        for (_, path) in &items {
            transaction.remove(path.clone());
        }
        transaction.commit()?;
        Ok(items.into_iter().map(|(item, _)| item.name).collect())
    }

    /// Create one issue from one or more triage items and delete the items, as
    /// a single transaction.
    ///
    /// When `fields.description` is blank, the issue's description is the
    /// items' bodies, in the order given, separated by `---` lines. The issue's
    /// notes record which items it came from, so the originals can be found in
    /// git history.
    pub fn promote_triage_items(&self, references: &[String], fields: NewIssue) -> Result<Issue> {
        let mut fields = fields;
        let _lock = self.lock_for_write()?;
        let items = self.read_triage_items_no_lock(references)?;

        if fields.description.trim().is_empty() {
            fields.description = items
                .iter()
                .map(|(item, _)| item.body.trim_end())
                .collect::<Vec<_>>()
                .join(PROMOTE_SEPARATOR);
        }
        let mut issue = self.build_new_issue_no_lock(fields)?;
        let names: Vec<&str> = items.iter().map(|(item, _)| item.name.as_str()).collect();
        issue.notes = format!("Promoted from triage: {}", names.join(", "));

        anyhow::ensure!(
            !self.issue_exists_any_layout(&issue.id)?,
            "Issue already exists: {}",
            issue.id
        );
        let issue_path = self.configured_issue_path(&issue.id)?;

        let mut transaction = FileTransaction::new(&self.tazk_dir);
        transaction.write(issue_path, issue_to_markdown(&issue)?.into_bytes());
        for (_, path) in &items {
            transaction.remove(path.clone());
        }
        transaction.commit()?;
        Ok(issue)
    }

    /// Add each triage item as a comment on `issue_id` (one comment per item,
    /// in the order given) and delete the items, as a single transaction.
    /// Returns each item's name with the comment made from it.
    pub fn attach_triage_items(
        &self,
        references: &[String],
        issue_id: &str,
        author: &str,
    ) -> Result<Vec<(String, Comment)>> {
        let _lock = self.lock_for_write()?;
        if !self.issue_exists_any_layout(issue_id)? {
            anyhow::bail!("Issue not found: {}", issue_id);
        }
        let items = self.read_triage_items_no_lock(references)?;

        let mut comments = self.read_comments_no_lock(issue_id)?;
        let now = Utc::now();
        let mut attached = Vec::new();
        for (offset, (item, _)) in items.iter().enumerate() {
            // Distinct timestamps keep the comments in order and their IDs
            // distinct even when two items have the same text.
            let created_at = now + chrono::Duration::microseconds(offset as i64);
            let comment = new_comment(issue_id, author, item.body.trim_end(), created_at);
            comments.push(comment.clone());
            attached.push((item.name.clone(), comment));
        }

        let comments_path = self.comment_path(issue_id)?;
        let mut transaction = FileTransaction::new(&self.tazk_dir);
        transaction.write(comments_path, serialize_comments(&comments)?.into_bytes());
        for (_, path) in &items {
            transaction.remove(path.clone());
        }
        transaction.commit()?;
        Ok(attached)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::IssueStorageLayout;
    use crate::types::{IssueType, Role, Status};

    fn storage() -> (tempfile::TempDir, Storage) {
        let tmp = tempfile::tempdir().unwrap();
        let storage =
            Storage::init(tmp.path().join(".tazk"), false, IssueStorageLayout::Flat).unwrap();
        (tmp, storage)
    }

    fn fields(title: &str) -> NewIssue {
        NewIssue {
            title: title.to_string(),
            description: String::new(),
            design: None,
            acceptance: None,
            priority: 1,
            issue_type: IssueType::Bug,
            assignee: None,
            labels: Vec::new(),
            external_ref: None,
            id: None,
            deps: Vec::new(),
            role: Some(Role::Agent),
            effort: None,
        }
    }

    fn create_issue(storage: &Storage) -> String {
        storage
            .create_issue(
                "Existing".into(),
                String::new(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                None,
                Vec::new(),
                Some(Role::Agent),
                None,
            )
            .unwrap()
            .id
    }

    fn triage_files(storage: &Storage) -> Vec<String> {
        let dir = storage.triage_dir();
        if !dir.exists() {
            return Vec::new();
        }
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn compose_body_joins_and_trims() {
        assert_eq!(compose_body(&["one  \n\n", "\n\ntwo\n"]), "one\n\ntwo\n");
        assert_eq!(compose_body(&["  indented\n"]), "  indented\n");
        assert_eq!(compose_body(&["", "  \n", "x"]), "x\n");
        assert_eq!(compose_body(&["", " \n"]), "");
    }

    #[test]
    fn generated_names_have_the_documented_shape() {
        let now = DateTime::parse_from_rfc3339("2025-11-04T00:18:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let name = generate_name(now);
        assert!(name.starts_with("20251104T001800-"), "{name}");
        assert!(name.ends_with(".md"), "{name}");
        let suffix = &name["20251104T001800-".len()..name.len() - 3];
        assert_eq!(suffix.len(), 4);
        assert!(suffix
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()));
    }

    #[test]
    fn add_writes_a_plain_markdown_file() {
        let (_tmp, storage) = storage();
        let item = storage.add_triage_item("It breaks\n\n").unwrap();
        assert_eq!(item.body, "It breaks\n");
        let content = fs::read_to_string(storage.triage_dir().join(&item.name)).unwrap();
        assert_eq!(content, "It breaks\n");
        assert!(!content.starts_with("---"));
        // No issue was created.
        assert!(storage
            .list_issues(None, None, None, None, None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn add_rejects_blank_text() {
        let (_tmp, storage) = storage();
        assert!(storage.add_triage_item(" \n ").is_err());
        assert!(triage_files(&storage).is_empty());
    }

    #[test]
    fn tolerant_triage_skips_unreadable_files() {
        let (_tmp, storage) = storage();
        let dir = storage.get_tazk_dir().join(TRIAGE_DIR);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("b.md"), "bee\n").unwrap();
        fs::write(dir.join("a.md"), "ay\n").unwrap();
        fs::write(dir.join("bad.md"), [0xff, 0xfe, 0x00]).unwrap();
        let loaded = storage.list_triage_items_tolerant().unwrap();
        let names: Vec<_> = loaded.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["a.md", "b.md"]);
        assert_eq!(loaded.skipped.len(), 1);
        assert_eq!(loaded.skipped[0].path, dir.join("bad.md"));
        assert!(!loaded.skipped[0].error.contains('\n'));
    }

    #[test]
    fn list_and_show_items() {
        let (_tmp, storage) = storage();
        assert!(storage.list_triage_items().unwrap().is_empty());
        let a = storage.add_triage_item("a").unwrap();
        let b = storage.add_triage_item("b").unwrap();
        // A hand-dropped file is an item too; dotfiles and other extensions are not.
        fs::write(storage.triage_dir().join("by-hand.md"), "manual\n").unwrap();
        fs::write(storage.triage_dir().join(".hidden.md"), "x").unwrap();
        fs::write(storage.triage_dir().join("notes.txt"), "x").unwrap();

        let names: Vec<String> = storage
            .list_triage_items()
            .unwrap()
            .into_iter()
            .map(|item| item.name)
            .collect();
        let mut expected = vec![a.name.clone(), b.name.clone(), "by-hand.md".to_string()];
        expected.sort();
        assert_eq!(names, expected);

        assert_eq!(storage.get_triage_item(&a.name).unwrap().body, "a\n");
        let stem = a.name.trim_end_matches(".md");
        assert_eq!(storage.get_triage_item(stem).unwrap(), a);
        assert_eq!(storage.get_triage_item("by-hand").unwrap().body, "manual\n");
    }

    #[test]
    fn items_can_be_referenced_by_path() {
        let (_tmp, storage) = storage();
        let item = storage.add_triage_item("a").unwrap();
        let path = storage.triage_dir().join(&item.name);
        assert_eq!(
            storage.get_triage_item(path.to_str().unwrap()).unwrap(),
            item
        );
        // A path to a same-named file elsewhere is refused.
        let elsewhere = storage.get_tazk_dir().join(&item.name);
        fs::write(&elsewhere, "x").unwrap();
        assert!(storage
            .get_triage_item(elsewhere.to_str().unwrap())
            .is_err());
    }

    #[test]
    fn bad_and_missing_names_are_errors() {
        let (_tmp, storage) = storage();
        let err = storage.get_triage_item("nope").unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
        for bad in ["", ".md", "a|b", "..", ".hidden"] {
            assert!(storage.get_triage_item(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn search_replace_and_append_edit_the_body() {
        let (_tmp, storage) = storage();
        let item = storage.add_triage_item("one two two").unwrap();

        let err = storage
            .search_replace_triage_item(&item.name, "two", "2", false)
            .unwrap_err();
        assert!(err.to_string().contains("matches 2 times"), "{err}");
        let err = storage
            .search_replace_triage_item(&item.name, "three", "3", false)
            .unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
        assert_eq!(
            storage.get_triage_item(&item.name).unwrap().body,
            "one two two\n"
        );

        let edited = storage
            .search_replace_triage_item(&item.name, "one", "1", false)
            .unwrap();
        assert_eq!(edited.body, "1 two two\n");
        let edited = storage
            .search_replace_triage_item(&item.name, "two", "2", true)
            .unwrap();
        assert_eq!(edited.body, "1 2 2\n");

        let edited = storage
            .append_to_triage_item(&item.name, "Ref: GH-1")
            .unwrap();
        assert_eq!(edited.body, "1 2 2\n\nRef: GH-1\n");
        assert_eq!(storage.get_triage_item(&item.name).unwrap(), edited);
    }

    #[test]
    fn an_edit_cannot_empty_an_item() {
        let (_tmp, storage) = storage();
        let item = storage.add_triage_item("only").unwrap();
        let err = storage
            .search_replace_triage_item(&item.name, "only", "", false)
            .unwrap_err();
        assert!(err.to_string().contains("tz triage rm"), "{err}");
        assert_eq!(storage.get_triage_item(&item.name).unwrap().body, "only\n");
    }

    #[test]
    fn remove_is_all_or_nothing() {
        let (_tmp, storage) = storage();
        let a = storage.add_triage_item("a").unwrap();
        let b = storage.add_triage_item("b").unwrap();
        assert!(storage
            .remove_triage_items(&[a.name.clone(), "missing".to_string()])
            .is_err());
        assert_eq!(triage_files(&storage).len(), 2);

        let removed = storage.remove_triage_items(&[a.name.clone()]).unwrap();
        assert_eq!(removed, vec![a.name]);
        assert_eq!(triage_files(&storage), vec![b.name]);
    }

    #[test]
    fn promote_one_item_uses_its_body_and_deletes_it() {
        let (_tmp, storage) = storage();
        let item = storage.add_triage_item("Login fails on Safari").unwrap();
        let issue = storage
            .promote_triage_items(std::slice::from_ref(&item.name), fields("Fix Safari login"))
            .unwrap();
        assert_eq!(issue.title, "Fix Safari login");
        assert_eq!(issue.status, Status::Open);
        assert_eq!(issue.priority, 1);
        assert_eq!(issue.issue_type, IssueType::Bug);
        assert_eq!(issue.role, Some(Role::Agent));
        assert_eq!(issue.description, "Login fails on Safari");
        assert_eq!(issue.notes, format!("Promoted from triage: {}", item.name));
        assert!(triage_files(&storage).is_empty());

        let stored = storage.get_issue(&issue.id).unwrap().unwrap();
        assert_eq!(stored.description, issue.description);
        assert_eq!(stored.notes, issue.notes);
        assert!(storage
            .get_ready(None, None, None, "priority")
            .unwrap()
            .iter()
            .any(|ready| ready.id == issue.id));
    }

    #[test]
    fn promote_many_items_into_one_issue() {
        let (_tmp, storage) = storage();
        let a = storage.add_triage_item("first").unwrap();
        let b = storage.add_triage_item("second").unwrap();
        let other = storage.add_triage_item("untouched").unwrap();
        let issue = storage
            .promote_triage_items(
                &[b.name.clone(), a.name.clone(), b.name.clone()],
                fields("Both"),
            )
            .unwrap();
        assert_eq!(issue.description, "second\n\n---\n\nfirst");
        assert_eq!(
            issue.notes,
            format!("Promoted from triage: {}, {}", b.name, a.name)
        );
        assert_eq!(triage_files(&storage), vec![other.name]);
        assert_eq!(
            storage.get_issue(&issue.id).unwrap().unwrap().description,
            issue.description
        );
    }

    #[test]
    fn promote_with_a_description_replaces_the_bodies() {
        let (_tmp, storage) = storage();
        let item = storage.add_triage_item("raw report").unwrap();
        let mut f = fields("Titled");
        f.description = "Rewritten".to_string();
        let issue = storage.promote_triage_items(&[item.name], f).unwrap();
        assert_eq!(issue.description, "Rewritten");
    }

    #[test]
    fn failed_promote_changes_nothing() {
        let (_tmp, storage) = storage();
        let item = storage.add_triage_item("report").unwrap();
        assert!(storage
            .promote_triage_items(&[item.name.clone(), "missing".to_string()], fields("T"))
            .is_err());
        let mut f = fields("T");
        f.effort = Some(9);
        assert!(storage
            .promote_triage_items(std::slice::from_ref(&item.name), f)
            .is_err());
        let existing = create_issue(&storage);
        let mut f = fields("T");
        f.id = Some(existing);
        assert!(storage
            .promote_triage_items(std::slice::from_ref(&item.name), f)
            .is_err());

        assert_eq!(triage_files(&storage), vec![item.name]);
        assert_eq!(
            storage
                .list_issues(None, None, None, None, None)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn attach_adds_one_comment_per_item_and_deletes_them() {
        let (_tmp, storage) = storage();
        let id = create_issue(&storage);
        let a = storage.add_triage_item("same").unwrap();
        let b = storage.add_triage_item("same").unwrap();
        let attached = storage
            .attach_triage_items(&[a.name.clone(), b.name.clone()], &id, "alice")
            .unwrap();
        assert_eq!(attached.len(), 2);
        assert_eq!(attached[0].0, a.name);
        assert_eq!(attached[1].0, b.name);
        assert_ne!(attached[0].1.id, attached[1].1.id);

        let comments = storage.list_comments(&id).unwrap();
        assert_eq!(comments.len(), 2);
        assert!(comments
            .iter()
            .all(|c| c.body == "same" && c.author == "alice"));
        assert_eq!(comments[0].id, attached[0].1.id);
        assert!(triage_files(&storage).is_empty());
    }

    #[test]
    fn failed_attach_changes_nothing() {
        let (_tmp, storage) = storage();
        let id = create_issue(&storage);
        let item = storage.add_triage_item("report").unwrap();
        let err = storage
            .attach_triage_items(std::slice::from_ref(&item.name), "tz-99", "a")
            .unwrap_err();
        assert!(err.to_string().contains("Issue not found"), "{err}");
        assert!(storage
            .attach_triage_items(&[item.name.clone(), "missing".into()], &id, "a")
            .is_err());
        assert_eq!(triage_files(&storage), vec![item.name]);
        assert!(storage.list_comments(&id).unwrap().is_empty());
    }

    #[test]
    fn readonly_storage_can_read_but_not_write_items() {
        let (_tmp, storage) = storage();
        let item = storage.add_triage_item("a").unwrap();
        let readonly = Storage::open_readonly(storage.get_tazk_dir()).unwrap();
        assert_eq!(readonly.list_triage_items().unwrap(), vec![item.clone()]);
        assert!(readonly.add_triage_item("b").is_err());
        assert!(readonly.remove_triage_items(&[item.name]).is_err());
        assert_eq!(triage_files(&storage).len(), 1);
    }
}
