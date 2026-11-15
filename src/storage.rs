use crate::format::{issue_to_markdown, markdown_to_issue};
use crate::hash;
use crate::lock::Lock;
use crate::paths::{ensure_contained, numeric_id_suffix, IdKind, IssueId};
use crate::transaction::{atomic_write, FileTransaction};
use crate::types::{
    parse_effort, validate_effort, BlockedIssue, Comment, DependencyType, EditField,
    InheritedBlocker, Issue, IssueType, Role, Stats, Status, ID_PREFIX,
};
use anyhow::{Context, Result};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
enum StorageAccess {
    ReadWrite,
    ReadOnly,
}

pub mod triage;

/// Items loaded by a tolerant loader, plus the files that could not be loaded.
#[derive(Debug, Clone)]
pub struct Loaded<T> {
    pub items: Vec<T>,
    pub skipped: Vec<SkippedFile>,
}

/// A file that failed to read or parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFile {
    pub path: PathBuf,
    /// Full error chain on one line.
    pub error: String,
}

pub(crate) fn skipped_file(path: PathBuf, err: &anyhow::Error) -> SkippedFile {
    let error = format!("{err:#}")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    SkippedFile { path, error }
}

/// Every field of a new issue, for callers (such as `tz triage promote`) that
/// build one in code rather than through [`Storage::create_issue`]'s argument
/// list.
pub struct NewIssue {
    pub title: String,
    pub description: String,
    pub design: Option<String>,
    pub acceptance: Option<String>,
    pub priority: i32,
    pub issue_type: IssueType,
    pub assignee: Option<String>,
    pub labels: Vec<String>,
    pub external_ref: Option<String>,
    pub deps: Vec<(String, DependencyType)>,
    pub role: Option<Role>,
    pub effort: Option<u8>,
}

pub struct Storage {
    tazk_dir: PathBuf,
    issues_dir: PathBuf,
    access: StorageAccess,
}

/// Replace issue ID references in text fields using word boundaries
///
/// This function replaces all occurrences of issue IDs in text, but only when they appear
/// as standalone tokens (delimited by non-alphanumeric characters or word boundaries).
/// This prevents matching IDs that are embedded in longer strings.
///
/// Uses a HashMap for O(1) lookup of replacement mappings.
/// Ordering for `list`: numeric IDs first (ascending, so the most recent
/// appear last), then hash-based IDs ordered by creation date (oldest first).
fn compare_for_list(a: &Issue, b: &Issue) -> std::cmp::Ordering {
    match (numeric_id_suffix(&a.id), numeric_id_suffix(&b.id)) {
        (Some(an), Some(bn)) => an.cmp(&bn),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.created_at.cmp(&b.created_at),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueStorageLayout {
    Flat,
    Sharded,
}

impl IssueStorageLayout {
    pub fn as_str(self) -> &'static str {
        match self {
            IssueStorageLayout::Flat => "flat",
            IssueStorageLayout::Sharded => "sharded",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "flat" => Ok(IssueStorageLayout::Flat),
            "sharded" => Ok(IssueStorageLayout::Sharded),
            _ => anyhow::bail!(
                "Invalid issue-storage-layout: '{}'. Valid values are: flat, sharded",
                value
            ),
        }
    }
}

fn shard_char(c: Option<char>) -> String {
    match c {
        Some(c) if c.is_ascii_alphanumeric() => c.to_ascii_lowercase().to_string(),
        _ => "_".to_string(),
    }
}

fn sharded_relative_path(id: IssueId<'_>) -> PathBuf {
    let suffix = id.suffix();

    let mut path = PathBuf::new();
    match id.kind() {
        IdKind::Numeric => {
            let mut chars = suffix.chars().rev();
            let ones = chars.next();
            let tens = chars.next().or(Some('0'));
            path.push("n");
            path.push(shard_char(tens).as_str());
            path.push(shard_char(ones).as_str());
        }
        IdKind::Hash => {
            let mut chars = suffix.chars();
            path.push("h");
            path.push(shard_char(chars.next()).as_str());
            path.push(shard_char(chars.next()).as_str());
        }
    }

    path.push(format!("{}.md", id.as_str()));
    path
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("md")
}

fn replace_issue_ids_in_text(text: &str, id_mapping: &HashMap<String, String>) -> String {
    if text.is_empty() || id_mapping.is_empty() {
        return text.to_string();
    }

    // Build a regex pattern that matches any issue ID with word boundaries
    // Pattern: \b(prefix1-suffix1|prefix2-suffix2|...)\b
    let mut patterns: Vec<String> = id_mapping.keys().map(|id| regex::escape(id)).collect();

    if patterns.is_empty() {
        return text.to_string();
    }

    // Sort by length (longest first) to avoid partial matches
    patterns.sort_by_key(|b| std::cmp::Reverse(b.len()));

    let pattern = format!(r"\b({})\b", patterns.join("|"));

    // Compile regex (note: in production code, this could be cached)
    let re = match Regex::new(&pattern) {
        Ok(r) => r,
        Err(_) => return text.to_string(), // Fallback: return original text
    };

    // Replace all matches using the mapping
    re.replace_all(text, |caps: &regex::Captures| {
        let matched_id = &caps[1];
        id_mapping
            .get(matched_id)
            .cloned()
            .unwrap_or_else(|| matched_id.to_string())
    })
    .to_string()
}

impl Storage {
    /// Get the tazk directory path
    pub fn get_tazk_dir(&self) -> PathBuf {
        self.tazk_dir.clone()
    }

    fn config_path(&self) -> PathBuf {
        self.tazk_dir.join("config.yaml")
    }
}

impl Storage {
    /// Commit one complete ID permutation, preserving comment identity and
    /// updating all stores that use the issue ID as ownership metadata.
    fn apply_id_mapping(
        &self,
        all_issues: Vec<Issue>,
        mapping: &HashMap<String, String>,
        config_updates: &[(&str, &str, &str)],
    ) -> Result<()> {
        let mut destinations = HashSet::new();
        for issue in &all_issues {
            let destination = mapping.get(&issue.id).unwrap_or(&issue.id);
            IssueId::parse(destination)?;
            anyhow::ensure!(
                destinations.insert(destination),
                "ID mapping would overwrite issue {destination}"
            );
        }

        let mut transaction = FileTransaction::new(&self.tazk_dir);
        let comment_sources: HashSet<PathBuf> = mapping
            .keys()
            .map(|id| self.comment_path(id))
            .collect::<Result<_>>()?;
        // Schedule removals first. Destination writes below take precedence in
        // a swap or longer cycle; no iteration can delete an earlier result.
        for (old_id, new_id) in mapping {
            if old_id == new_id {
                continue;
            }
            if let Some(path) = self.existing_issue_path(old_id)? {
                transaction.remove(path);
            }
            let source = self.comment_path(old_id)?;
            let destination = self.comment_path(new_id)?;
            anyhow::ensure!(
                !destination.exists() || comment_sources.contains(&destination),
                "Renaming would overwrite orphaned comments for {new_id}"
            );
            if source.exists() {
                transaction.remove(source);
            }
        }

        for mut issue in all_issues {
            let old_id = issue.id.clone();
            if remap_issue(&mut issue, mapping) {
                let source = self
                    .existing_issue_path(&old_id)?
                    .context("Migration source issue disappeared")?;
                let modified = fs::metadata(source)?.modified()?;
                let path = if old_id != issue.id {
                    self.configured_issue_path(&issue.id)?
                } else {
                    self.existing_or_configured_issue_path(&issue.id)?
                };
                transaction.write_with_mtime(
                    path,
                    issue_to_markdown(&issue)?.into_bytes(),
                    modified,
                );
            }
            let mut comments = self.read_comments_no_lock(&old_id)?;
            let mut comments_changed = old_id != issue.id && !comments.is_empty();
            for comment in &mut comments {
                if comment.issue_id != issue.id {
                    comment.issue_id.clone_from(&issue.id);
                    comments_changed = true;
                }
                // Comment IDs are stable identities; do not regenerate them
                // just because their owning issue has a different ID.
                let body = replace_issue_ids_in_text(&comment.body, mapping);
                if body != comment.body {
                    comment.body = body;
                    comment.updated_at = chrono::Utc::now();
                    comments_changed = true;
                }
            }
            if comments_changed {
                transaction.write(
                    self.comment_path(&issue.id)?,
                    serde_json::to_vec_pretty(&comments)?,
                );
            }
        }

        // Triage items often name the issue they were found during. Rewrite
        // only the items whose text changes; names are kept.
        for (_, path, body) in self.triage_id_rewrites(mapping)? {
            transaction.write(path, body.into_bytes());
        }

        // Keep the default JSONL store from resurrecting old IDs at the next
        // sync. Preserve its independently edited fields and timestamps.
        let jsonl_path = self.tazk_dir.join("issues.jsonl");
        ensure_contained(&self.tazk_dir, &jsonl_path)?;
        if jsonl_path.exists() {
            let mut issues = crate::sync::load_jsonl_issues(&jsonl_path)?;
            let mut seen = HashSet::new();
            let mut changed = false;
            let mut lines = Vec::with_capacity(issues.len());
            for record in issues.values_mut() {
                changed |= remap_issue(&mut record.issue, mapping);
                anyhow::ensure!(
                    seen.insert(record.issue.id.as_str()),
                    "ID mapping would overwrite a JSONL issue"
                );
                lines.push(serde_json::to_string(&record.issue)?);
            }
            if changed {
                lines.sort();
                transaction.write(jsonl_path, (lines.join("\n") + "\n").into_bytes());
            }
        }

        for (file, key, value) in config_updates {
            transaction.config(file, key, value)?;
        }
        transaction.commit()
    }

    /// Open storage at the given tazk directory
    pub fn open(tazk_dir: PathBuf) -> Result<Self> {
        let issues_dir = tazk_dir.join("issues");

        // Create directories if they don't exist
        fs::create_dir_all(&issues_dir).context("Failed to create issues directory")?;

        // Validate and ensure config.yaml exists
        let config_path = tazk_dir.join("config.yaml");
        if !config_path.exists() {
            fs::write(&config_path, "").context("Failed to create config.yaml")?;
        } else {
            // Validate that config.yaml parses.
            let content = fs::read_to_string(&config_path).context("Failed to read config.yaml")?;
            serde_yaml::from_str::<HashMap<String, String>>(&content)
                .context("Failed to parse config.yaml")?;
        }

        // Fold a legacy config-tazk.yaml (from before the two files were merged)
        // into config.yaml. Options absent from config.yaml use their defaults.
        merge_legacy_tazk_config(&tazk_dir)?;
        reject_removed_config_keys(&config_path)?;

        // Ensure .gitignore exists and has required entries
        ensure_gitignore(&tazk_dir)?;

        Ok(Self {
            tazk_dir,
            issues_dir,
            access: StorageAccess::ReadWrite,
        })
    }

    /// Open existing storage without bootstrapping config or recovering writes.
    /// Reads still take the transient database lock; this is a logical write
    /// guard, not support for a filesystem mounted without write permission.
    pub fn open_readonly(tazk_dir: PathBuf) -> Result<Self> {
        let issues_dir = tazk_dir.join("issues");
        anyhow::ensure!(
            issues_dir.is_dir(),
            "Existing issues directory required for --readonly"
        );
        ensure_contained(&tazk_dir, &issues_dir)?;
        let storage = Self {
            tazk_dir,
            issues_dir,
            access: StorageAccess::ReadOnly,
        };
        anyhow::ensure!(
            !storage.tazk_dir.join(LEGACY_TAZK_CONFIG).exists(),
            "{LEGACY_TAZK_CONFIG} is obsolete; run any command without --readonly to merge it into config.yaml"
        );
        let _lock = storage.lock_for_read()?;
        storage.list_config_values()?;
        reject_removed_config_keys(&storage.config_path())?;
        storage.issue_storage_layout()?;
        Ok(storage)
    }

    fn ensure_writable(&self) -> Result<()> {
        anyhow::ensure!(
            self.access == StorageAccess::ReadWrite,
            "Storage is read-only"
        );
        Ok(())
    }

    fn lock_for_write(&self) -> Result<Lock> {
        self.ensure_writable()?;
        Lock::acquire(&self.tazk_dir)
    }

    fn lock_for_read(&self) -> Result<Lock> {
        match self.access {
            StorageAccess::ReadWrite => Lock::acquire(&self.tazk_dir),
            StorageAccess::ReadOnly => Lock::acquire_without_recovery(&self.tazk_dir),
        }
    }

    /// Initialize a new tazk database
    pub fn init(
        tazk_dir: PathBuf,
        tz_hash_ids: bool,
        issue_layout: IssueStorageLayout,
    ) -> Result<Self> {
        // Create tazk directory
        fs::create_dir_all(&tazk_dir).context("Failed to create tazk directory")?;

        // Create issues directory
        let issues_dir = tazk_dir.join("issues");
        fs::create_dir_all(&issues_dir).context("Failed to create issues directory")?;

        // Create config.yaml with the tazk options
        write_initial_config(&tazk_dir, tz_hash_ids, issue_layout)?;

        // Ensure .gitignore exists and has required entries
        ensure_gitignore(&tazk_dir)?;

        Ok(Self {
            tazk_dir,
            issues_dir,
            access: StorageAccess::ReadWrite,
        })
    }

    /// Get a config value from config.yaml.
    pub fn get_config_value(&self, key: &str) -> Result<Option<String>> {
        let config_path = self.config_path();

        if !config_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&config_path).context("Failed to read config.yaml")?;
        let config: HashMap<String, serde_yaml::Value> =
            serde_yaml::from_str(&content).context("Failed to parse config.yaml")?;

        Ok(config.get(key).map(yaml_value_to_string))
    }

    /// Set a config value in config.yaml.
    pub fn set_config_value(&self, key: &str, value: &str) -> Result<()> {
        let _lock = self.lock_for_write()?;
        reject_removed_config_key(key)?;
        let config_path = self.config_path();

        if !config_path.exists() {
            fs::write(&config_path, "").context("Failed to create config.yaml")?;
        }

        upsert_yaml_key_value(&config_path, key, value)
    }

    /// Return all config.yaml scalar values for `tz config list`.
    pub fn list_config_values(&self) -> Result<HashMap<String, String>> {
        let config_path = self.config_path();

        if !config_path.exists() {
            return Ok(HashMap::new());
        }

        let content = fs::read_to_string(&config_path).context("Failed to read config.yaml")?;
        let config: HashMap<String, serde_yaml::Value> =
            serde_yaml::from_str(&content).context("Failed to parse config.yaml")?;

        Ok(config
            .into_iter()
            .map(|(key, value)| (key, yaml_value_to_string(&value)))
            .collect())
    }

    /// Read config.yaml as string key/value pairs (empty if the file is missing).
    fn read_config_map(&self) -> Result<HashMap<String, String>> {
        let config_path = self.config_path();

        if !config_path.exists() {
            return Ok(HashMap::new());
        }

        let content = fs::read_to_string(&config_path).context("Failed to read config.yaml")?;
        serde_yaml::from_str(&content).context("Failed to parse config.yaml")
    }

    /// Check if hash-based IDs are enabled in config.yaml
    fn use_hash_ids(&self) -> Result<bool> {
        let config = self.read_config_map()?;

        // Parse tz-hash-ids field (default to false if not present)
        match config.get("tz-hash-ids") {
            Some(value) => Ok(value == "true"),
            None => Ok(false),
        }
    }

    /// Return the configured issue file layout.
    pub fn issue_storage_layout(&self) -> Result<IssueStorageLayout> {
        let config = self.read_config_map()?;

        match config.get("issue-storage-layout") {
            Some(value) => IssueStorageLayout::parse(value),
            None => Ok(IssueStorageLayout::Flat),
        }
    }

    fn flat_issue_path(&self, id: IssueId<'_>) -> PathBuf {
        self.issues_dir.join(format!("{}.md", id.as_str()))
    }

    fn sharded_issue_path(&self, id: IssueId<'_>) -> PathBuf {
        self.issues_dir.join(sharded_relative_path(id))
    }

    fn issue_path_for_layout(&self, id: IssueId<'_>, layout: IssueStorageLayout) -> PathBuf {
        match layout {
            IssueStorageLayout::Flat => self.flat_issue_path(id),
            IssueStorageLayout::Sharded => self.sharded_issue_path(id),
        }
    }

    fn configured_issue_path(&self, id: &str) -> Result<PathBuf> {
        let id = IssueId::parse(id)?;
        let path = self.issue_path_for_layout(id, self.issue_storage_layout()?);
        ensure_contained(&self.tazk_dir, &path)?;
        Ok(path)
    }

    fn existing_issue_path(&self, id: &str) -> Result<Option<PathBuf>> {
        let configured_path = self.configured_issue_path(id)?;
        if configured_path.exists() {
            return Ok(Some(configured_path));
        }

        let id = IssueId::parse(id)?;
        let flat_path = self.flat_issue_path(id);
        if flat_path.exists() {
            ensure_contained(&self.tazk_dir, &flat_path)?;
            return Ok(Some(flat_path));
        }

        let sharded_path = self.sharded_issue_path(id);
        if sharded_path.exists() {
            ensure_contained(&self.tazk_dir, &sharded_path)?;
            return Ok(Some(sharded_path));
        }

        Ok(None)
    }

    fn issue_exists_any_layout(&self, id: &str) -> Result<bool> {
        Ok(self.existing_issue_path(id)?.is_some())
    }

    /// Path of the issue's Markdown file in whichever layout it is stored
    /// (flat or sharded), or `None` if there is no such file.
    pub fn issue_file_path(&self, id: &str) -> Result<Option<PathBuf>> {
        self.existing_issue_path(id)
    }

    pub(crate) fn existing_or_configured_issue_path(&self, id: &str) -> Result<PathBuf> {
        match self.existing_issue_path(id)? {
            Some(path) => Ok(path),
            None => self.configured_issue_path(id),
        }
    }

    fn write_issue_to_path(&self, path: &Path, issue: &Issue) -> Result<()> {
        IssueId::parse(&issue.id)?;
        ensure_contained(&self.tazk_dir, path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create issue directory: {}", parent.display())
            })?;
        }
        let markdown = issue_to_markdown(issue)?;
        atomic_write(path, markdown.as_bytes())
            .with_context(|| format!("Failed to write issue file: {}", path.display()))?;
        Ok(())
    }

    /// Write a newly-created issue without ever replacing an existing file.
    ///
    /// ID allocation can observe stale or otherwise unexpected state. A
    /// normal `fs::write` would then replace an unrelated issue and let the
    /// caller print a fabricated "Created issue" success. `create_new(true)`
    /// makes that collision an explicit error instead.
    fn write_new_issue_to_path(&self, path: &Path, issue: &Issue) -> Result<()> {
        IssueId::parse(&issue.id)?;
        ensure_contained(&self.tazk_dir, path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create issue directory: {}", parent.display())
            })?;
        }
        let markdown = issue_to_markdown(issue)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| format!("Failed to create issue file: {}", path.display()))?;
        if let Err(error) = file.write_all(markdown.as_bytes()) {
            drop(file);
            let _ = fs::remove_file(path);
            return Err(error).context("Failed to write newly-created issue file");
        }
        Ok(())
    }

    fn issue_file_paths(&self) -> Result<Vec<(String, PathBuf)>> {
        issue_file_paths_in(&self.issues_dir)
    }

    /// Get the next issue number: the highest numeric suffix + 1.
    fn get_next_number(&self) -> Result<u32> {
        let mut max_num = 0;
        for (issue_id, _) in self.issue_file_paths()? {
            if let Some(num) = numeric_id_suffix(&issue_id) {
                max_num = max_num.max(num);
            }
        }

        max_num
            .checked_add(1)
            .context("Numeric issue IDs are exhausted")
    }

    /// Generate a hash-based ID with adaptive length and collision handling
    fn generate_hash_id(&self, title: &str, description: &str) -> Result<String> {
        use chrono::Utc;

        let timestamp = Utc::now();

        // Count existing issues to determine adaptive length
        let issue_count = self.issue_file_paths()?.len();

        // Use hash::generate_hash_id_with_collision_check with filesystem checker
        hash::generate_hash_id_with_collision_check(
            title,
            description,
            timestamp,
            issue_count,
            |candidate| self.issue_exists_any_layout(candidate).unwrap_or(false),
        )
    }

    /// Create a new issue
    #[allow(clippy::too_many_arguments)]
    pub fn create_issue(
        &self,
        title: String,
        description: String,
        design: Option<String>,
        acceptance: Option<String>,
        priority: i32,
        issue_type: IssueType,
        assignee: Option<String>,
        labels: Vec<String>,
        external_ref: Option<String>,
        deps: Vec<(String, DependencyType)>,
        role: Option<Role>,
        effort: Option<u8>,
    ) -> Result<Issue> {
        let fields = NewIssue {
            title,
            description,
            design,
            acceptance,
            priority,
            issue_type,
            assignee,
            labels,
            external_ref,
            deps,
            role,
            effort,
        };
        let _lock = self.lock_for_write()?;

        let issue = self.build_new_issue_no_lock(fields)?;

        // Write to file
        let issue_path = self.configured_issue_path(&issue.id)?;
        self.write_new_issue_to_path(&issue_path, &issue)?;

        Ok(issue)
    }

    /// Allocate an ID and build the issue, without
    /// writing it. The caller must hold the write lock, so the allocated ID
    /// stays free until the caller writes the file.
    fn build_new_issue_no_lock(&self, fields: NewIssue) -> Result<Issue> {
        let NewIssue {
            title,
            description,
            design,
            acceptance,
            priority,
            issue_type,
            assignee,
            labels,
            external_ref,
            deps,
            role,
            effort,
        } = fields;
        issue_type.ensure_role_allowed(role)?;
        if let Some(effort) = effort {
            validate_effort(effort)?;
        }

        // Every section is new, so check them all (tz-51 §4) before an ID is
        // allocated or anything is written.
        let design = design.unwrap_or_default();
        let mut acceptance = acceptance.unwrap_or_default();
        if acceptance.trim().is_empty() {
            if let Some(default) = issue_type.default_acceptance() {
                acceptance = default.to_string();
            }
        }
        issue_type.ensure_new_sections(&description, &design, &acceptance, "")?;

        let issue_id = if self.use_hash_ids()? {
            // Use hash-based ID generation
            self.generate_hash_id(&title, &description)?
        } else {
            // Use sequential numbering
            let num = self.get_next_number()?;
            format!("{ID_PREFIX}{num}")
        };

        // Create issue
        let mut issue = Issue::new(issue_id, title, priority, issue_type);
        issue.description = description;
        issue.design = design;
        issue.acceptance_criteria = acceptance;
        issue.assignee = assignee.unwrap_or_default();
        issue.role = role;
        issue.effort = effort;
        issue.labels = labels;
        issue.external_ref = external_ref;

        // Add dependencies (with validation)
        for (dep_id, dep_type) in deps {
            ensure_single_parent(&issue.id, &issue.depends_on, &dep_id, dep_type)?;
            // Validate dependency target exists (warn if not)
            self.validate_dependency_exists(&dep_id);
            issue.depends_on.insert(dep_id, dep_type);
        }

        Ok(issue)
    }

    /// Get an issue by ID
    pub fn get_issue(&self, id: &str) -> Result<Option<Issue>> {
        let _lock = self.lock_for_read()?;

        let Some(issue_path) = self.existing_issue_path(id)? else {
            return Ok(None);
        };

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(id, &content)?;

        // Populate dependents by scanning all issues
        let all_issues = self.list_all_issues_no_dependents()?;
        Self::populate_dependents_for_one(&all_issues, &mut issue);

        Ok(Some(issue))
    }

    fn comments_dir(&self) -> PathBuf {
        self.tazk_dir.join("comments")
    }

    fn comment_path(&self, issue_id: &str) -> Result<PathBuf> {
        let id = IssueId::parse(issue_id)?;
        let path = self.comments_dir().join(format!("{}.json", id.as_str()));
        ensure_contained(&self.tazk_dir, &path)?;
        Ok(path)
    }

    fn read_comments_no_lock(&self, issue_id: &str) -> Result<Vec<Comment>> {
        let path = self.comment_path(issue_id)?;
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if content.trim().is_empty() {
            return Ok(Vec::new());
        }

        let mut comments: Vec<Comment> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        comments.sort_by_key(|c| c.created_at);
        Ok(comments)
    }

    fn write_comments_no_lock(&self, issue_id: &str, comments: &[Comment]) -> Result<()> {
        let comments_dir = self.comments_dir();
        ensure_contained(&self.tazk_dir, &comments_dir)?;
        fs::create_dir_all(&comments_dir).context("Failed to create comments directory")?;

        let content = serialize_comments(comments)?;
        let path = self.comment_path(issue_id)?;
        atomic_write(&path, content.as_bytes())
            .with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    /// Add a local comment to an issue.
    pub fn add_comment(&self, issue_id: &str, author: &str, body: &str) -> Result<Comment> {
        let _lock = self.lock_for_write()?;

        if !self.issue_exists_any_layout(issue_id)? {
            anyhow::bail!("Issue not found: {}", issue_id);
        }

        let mut comments = self.read_comments_no_lock(issue_id)?;
        let comment = new_comment(issue_id, author, body, chrono::Utc::now());
        comments.push(comment.clone());
        self.write_comments_no_lock(issue_id, &comments)?;
        Ok(comment)
    }

    /// List comments for an issue.
    pub fn list_comments(&self, issue_id: &str) -> Result<Vec<Comment>> {
        let _lock = self.lock_for_read()?;
        self.read_comments_no_lock(issue_id)
    }

    /// Delete a single comment from an issue by its comment ID.
    ///
    /// Returns the removed [`Comment`]. Errors (leaving the store untouched) when
    /// no comment on `issue_id` has the given `comment_id`, so a typo or stale ID
    /// never silently succeeds.
    pub fn delete_comment(&self, issue_id: &str, comment_id: &str) -> Result<Comment> {
        let _lock = self.lock_for_write()?;

        let mut comments = self.read_comments_no_lock(issue_id)?;
        let pos = comments
            .iter()
            .position(|c| c.id == comment_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Comment not found: {} (on issue {}). Run `tz comments list {}` to see valid comment IDs.",
                    comment_id,
                    issue_id,
                    issue_id
                )
            })?;
        let removed = comments.remove(pos);
        self.write_comments_no_lock(issue_id, &comments)?;
        Ok(removed)
    }

    /// Helper to load all issues without computing dependents (to avoid recursion)
    fn list_all_issues_no_dependents(&self) -> Result<Vec<Issue>> {
        let mut issues = Vec::new();
        for (issue_id, path) in self.issue_file_paths()? {
            let content = fs::read_to_string(path)?;
            let issue = parse_issue_file(&issue_id, &content)?;
            issues.push(issue);
        }

        Ok(issues)
    }

    /// Load every issue (any status, any layout), skipping files that fail to
    /// read or parse. `dependents` is populated over the parsed set. Files are
    /// read under the lock, which is released before parsing.
    pub fn list_all_issues_tolerant(&self) -> Result<Loaded<Issue>> {
        let raw: Vec<(String, PathBuf, std::io::Result<String>)> = {
            let _lock = self.lock_for_read()?;
            self.issue_file_paths()?
                .into_iter()
                .map(|(id, path)| {
                    let content = fs::read_to_string(&path);
                    (id, path, content)
                })
                .collect()
        };

        let mut items = Vec::new();
        let mut skipped = Vec::new();
        for (id, path, content) in raw {
            let parsed = content
                .map_err(anyhow::Error::from)
                .and_then(|c| parse_issue_file(&id, &c));
            match parsed {
                Ok(issue) => items.push(issue),
                Err(err) => skipped.push(skipped_file(path, &err)),
            }
        }
        items.sort_by(compare_for_list);
        Self::populate_dependents(&mut items);
        Ok(Loaded { items, skipped })
    }

    /// Populate dependents for a single issue given all issues
    fn populate_dependents_for_one(all_issues: &[Issue], target_issue: &mut Issue) {
        use crate::types::Dependency;

        let mut dependents = Vec::new();
        for issue in all_issues {
            if issue.depends_on.contains_key(&target_issue.id) {
                if let Some(dep_type) = issue.depends_on.get(&target_issue.id) {
                    dependents.push(Dependency {
                        id: issue.id.clone(),
                        dep_type: dep_type.to_string(),
                    });
                }
            }
        }

        target_issue.dependents = dependents;
    }

    /// Update an issue
    pub fn update_issue(&self, id: &str, updates: HashMap<String, String>) -> Result<Issue> {
        let _lock = self.lock_for_write()?;

        let Some(issue_path) = self.existing_issue_path(id)? else {
            anyhow::bail!("Issue not found: {}", id);
        };

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(id, &content)?;

        apply_issue_updates(&mut issue, updates)?;

        self.write_issue_to_path(&issue_path, &issue)?;

        Ok(issue)
    }

    /// Add a label to an issue, returning the updated issue.
    pub fn add_label(&self, id: &str, label: &str) -> Result<Issue> {
        let _lock = self.lock_for_write()?;

        let Some(issue_path) = self.existing_issue_path(id)? else {
            anyhow::bail!("Issue not found: {}", id);
        };

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(id, &content)?;

        if !issue.labels.iter().any(|existing| existing == label) {
            issue.labels.push(label.to_string());
            issue.labels.sort();
        }
        issue.updated_at = chrono::Utc::now();

        self.write_issue_to_path(&issue_path, &issue)?;

        Ok(issue)
    }

    /// Remove a label from an issue, returning the updated issue.
    pub fn remove_label(&self, id: &str, label: &str) -> Result<Issue> {
        let _lock = self.lock_for_write()?;

        let Some(issue_path) = self.existing_issue_path(id)? else {
            anyhow::bail!("Issue not found: {}", id);
        };

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(id, &content)?;

        issue.labels.retain(|existing| existing != label);
        issue.updated_at = chrono::Utc::now();

        self.write_issue_to_path(&issue_path, &issue)?;

        Ok(issue)
    }

    /// Replace all labels on an issue.
    pub fn set_labels(&self, id: &str, labels: Vec<String>) -> Result<Issue> {
        let _lock = self.lock_for_write()?;

        let Some(issue_path) = self.existing_issue_path(id)? else {
            anyhow::bail!("Issue not found: {}", id);
        };

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(id, &content)?;

        issue.labels = normalize_labels(labels);
        issue.updated_at = chrono::Utc::now();

        self.write_issue_to_path(&issue_path, &issue)?;

        Ok(issue)
    }

    /// List all unique labels across issues.
    pub fn list_all_labels(&self) -> Result<Vec<String>> {
        let issues = self.list_issues(None, None, None, None, None)?;
        let labels = issues
            .into_iter()
            .flat_map(|issue| issue.labels)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(labels)
    }

    /// Apply a targeted search/replace edit to one free-text field of an issue.
    ///
    /// This is the safer alternative to overwriting a whole field via
    /// [`update_issue`]: the caller supplies an exact `search` substring and its
    /// `replace`ment, mirroring the "aider" edit pattern that LLM agents handle
    /// far more reliably than wholesale rewrites or line-numbered diffs.
    ///
    /// By default the edit is refused unless `search` occurs **exactly once** in
    /// the field, so an ambiguous or stale search cannot silently corrupt the
    /// wrong location. Pass `replace_all = true` to rewrite every occurrence.
    /// An empty `search`, a `search` that is not found, or (without
    /// `replace_all`) a `search` that matches more than once is an error and the
    /// file is left untouched.
    pub fn search_replace_issue(
        &self,
        id: &str,
        field: EditField,
        search: &str,
        replace: &str,
        replace_all: bool,
    ) -> Result<Issue> {
        let _lock = self.lock_for_write()?;

        if search.is_empty() {
            anyhow::bail!("--search text must not be empty");
        }

        let Some(issue_path) = self.existing_issue_path(id)? else {
            anyhow::bail!("Issue not found: {}", id);
        };

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(id, &content)?;

        let target = issue.text_field_mut(field);
        let new = search_replace_text(
            target,
            search,
            replace,
            replace_all,
            &format!("{field} field of {id}"),
        )?;
        let (ty, old) = (issue.issue_type, issue.text_field_mut(field).clone());
        ty.ensure_section_change(id, field, &old, &new)?;
        *issue.text_field_mut(field) = new;

        issue.updated_at = chrono::Utc::now();

        self.write_issue_to_path(&issue_path, &issue)?;

        Ok(issue)
    }

    /// Append `text` to a text field, inserting a blank line before it when the
    /// field already has content so the appended text becomes its own paragraph.
    ///
    /// A safer alternative to overwriting a whole field with `--description` when
    /// you only want to add to the end, and simpler than a search/replace that
    /// has to reproduce the field's trailing lines exactly.
    pub fn append_to_issue(&self, id: &str, field: EditField, text: &str) -> Result<Issue> {
        let _lock = self.lock_for_write()?;

        if text.is_empty() {
            anyhow::bail!("--append text must not be empty");
        }

        let Some(issue_path) = self.existing_issue_path(id)? else {
            anyhow::bail!("Issue not found: {}", id);
        };

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(id, &content)?;

        let new = append_paragraph(issue.text_field_mut(field), text);
        let (ty, old) = (issue.issue_type, issue.text_field_mut(field).clone());
        ty.ensure_section_change(id, field, &old, &new)?;
        *issue.text_field_mut(field) = new;

        issue.updated_at = chrono::Utc::now();

        self.write_issue_to_path(&issue_path, &issue)?;

        Ok(issue)
    }

    /// Claim an issue for `actor`.
    ///
    /// This is the local half of tazk' git-as-coordination model: the claim
    /// records `assignee` and `status=in_progress` (and bumps `updated_at`) in
    /// the markdown file. The cross-machine "compare-and-swap" is the subsequent
    /// `git push` (a losing push is rejected, the loser pulls and sees the issue
    /// is taken). Within a single working copy the [`Lock`] serializes writers.
    ///
    /// Fails if the issue is closed, or if it is already `in_progress` (whoever
    /// holds it, including `actor`) unless `force` is set. `force` overrides the
    /// lock and reassigns the issue to `actor` (it never overrides closed).
    pub fn claim_issue(&self, id: &str, actor: &str, force: bool) -> Result<Issue> {
        let _lock = self.lock_for_write()?;

        let Some(issue_path) = self.existing_issue_path(id)? else {
            anyhow::bail!("Issue not found: {}", id);
        };

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(id, &content)?;

        if issue.status == Status::Closed {
            anyhow::bail!("Cannot claim {}: issue is closed", id);
        }
        // Compare-and-swap precondition: an in-progress issue is already taken.
        if issue.status == Status::InProgress && !force {
            let holder = if issue.assignee.is_empty() {
                "an unknown worker"
            } else {
                issue.assignee.as_str()
            };
            anyhow::bail!(
                "Issue {} is already in progress (claimed by '{}'). Use --force to take it anyway.",
                id,
                holder
            );
        }

        issue.assignee = actor.to_string();
        issue.status = Status::InProgress;
        issue.updated_at = chrono::Utc::now();

        self.write_issue_to_path(&issue_path, &issue)?;

        Ok(issue)
    }

    /// Release a claim, returning the issue to the backlog.
    ///
    /// Clears `assignee` and, if the issue was `in_progress`, flips it back to `open` so `tz ready` surfaces it again.
    /// Refuses to release a claim held by a different worker unless `force` is
    /// set.
    pub fn release_issue(&self, id: &str, actor: &str, force: bool) -> Result<Issue> {
        let _lock = self.lock_for_write()?;

        let Some(issue_path) = self.existing_issue_path(id)? else {
            anyhow::bail!("Issue not found: {}", id);
        };

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(id, &content)?;

        if !issue.assignee.is_empty() && issue.assignee != actor && !force {
            anyhow::bail!(
                "Issue {} is claimed by '{}', not '{}'. Use --force to release it anyway.",
                id,
                issue.assignee,
                actor
            );
        }

        issue.assignee = String::new();
        if issue.status == Status::InProgress {
            issue.status = Status::Open;
        }
        issue.updated_at = chrono::Utc::now();

        self.write_issue_to_path(&issue_path, &issue)?;

        Ok(issue)
    }

    /// Put an issue on hold (`hold` = `Blocked` or `Deferred`), or take it off
    /// hold when `release` is set (back to `open`).
    ///
    /// A closed issue cannot be put on hold. Releasing requires the issue to
    /// currently have the `hold` status. A non-empty `reason` is appended to the
    /// issue's notes as its own paragraph, framed with a UTC timestamp, e.g.
    /// `Blocked 2025-01-02T03:04:05Z: waiting on API keys` (on release the
    /// frame is `Unblocked` / `Undeferred`).
    pub fn set_hold(
        &self,
        id: &str,
        hold: Status,
        release: bool,
        reason: Option<&str>,
    ) -> Result<Issue> {
        let (verb, release_verb) = match hold {
            Status::Blocked => ("Blocked", "Unblocked"),
            Status::Deferred => ("Deferred", "Undeferred"),
            other => anyhow::bail!("Invalid hold status: {other}"),
        };

        let _lock = self.lock_for_write()?;

        let Some(issue_path) = self.existing_issue_path(id)? else {
            anyhow::bail!("Issue not found: {}", id);
        };

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(id, &content)?;

        if release {
            if issue.status != hold {
                anyhow::bail!(
                    "Cannot release {}: issue is {}, not {}",
                    id,
                    issue.status,
                    hold
                );
            }
            issue.status = Status::Open;
        } else {
            if issue.status == Status::Closed {
                anyhow::bail!("Cannot mark {} as {}: issue is closed", id, hold);
            }
            issue.status = hold;
        }

        let now = chrono::Utc::now();
        if let Some(reason) = reason.map(str::trim).filter(|r| !r.is_empty()) {
            let verb = if release { release_verb } else { verb };
            append_framed_note(&mut issue, verb, now, reason);
        }
        issue.updated_at = now;

        self.write_issue_to_path(&issue_path, &issue)?;

        Ok(issue)
    }

    /// Close an issue. A non-empty `reason` is appended to the notes as
    /// `Closed <timestamp>: <reason>`.
    pub fn close_issue(&self, id: &str, reason: Option<&str>) -> Result<Issue> {
        let _lock = self.lock_for_write()?;

        let Some(issue_path) = self.existing_issue_path(id)? else {
            anyhow::bail!("Issue not found: {}", id);
        };

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(id, &content)?;

        let now = chrono::Utc::now();
        issue.status = Status::Closed;
        issue.closed_at = Some(now);
        issue.updated_at = now;
        if let Some(reason) = reason {
            append_framed_note(&mut issue, "Closed", now, reason);
        }

        self.write_issue_to_path(&issue_path, &issue)?;

        Ok(issue)
    }

    /// Reopen an issue.
    ///
    /// A work issue (not an epic or spec) without a role cannot be reopened: the
    /// caller must supply one (`role`). A supplied `role` always replaces the
    /// stored one. Epics and specs take no role: reopening needs none and
    /// rejects one. A non-empty
    /// `reason` is appended to the notes as `Reopened <timestamp>: <reason>`.
    pub fn reopen_issue(
        &self,
        id: &str,
        role: Option<Role>,
        reason: Option<&str>,
    ) -> Result<Issue> {
        let _lock = self.lock_for_write()?;

        let Some(issue_path) = self.existing_issue_path(id)? else {
            anyhow::bail!("Issue not found: {}", id);
        };

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(id, &content)?;

        if issue.issue_type.takes_role() {
            match role.or(issue.role) {
                Some(role) => issue.role = Some(role),
                None => anyhow::bail!(
                    "Cannot reopen {}: it has no role. Resubmit with --role human|agent",
                    id
                ),
            }
        } else {
            // Epics and specs take no role; a legacy stored role is left alone.
            issue.issue_type.ensure_role_allowed(role)?;
        }

        let now = chrono::Utc::now();
        issue.status = Status::Open;
        issue.closed_at = None;
        issue.updated_at = now;
        if let Some(reason) = reason {
            append_framed_note(&mut issue, "Reopened", now, reason);
        }

        self.write_issue_to_path(&issue_path, &issue)?;

        Ok(issue)
    }

    /// Give an issue a new canonical ID (see `tz renumber`).
    ///
    /// `new_id` must be a free ID in the store's current format (numeric or
    /// hash); without it the next free number (numeric mode) or a freshly
    /// generated hash (hash mode) is used. Returns the new ID and the list
    /// of changes (applied unless `dry_run`).
    ///
    /// The rewrite:
    /// - Renames the markdown file and updates the ID in its frontmatter
    /// - Updates all dependency references in other issues
    /// - Updates text mentions in issues, comments and triage items
    /// - Is atomic (all updates succeed or none)
    pub fn renumber_issue(
        &self,
        old_id: &str,
        new_id: Option<&str>,
        dry_run: bool,
    ) -> Result<(String, Vec<String>)> {
        let _lock = self.lock_for_write()?;
        IssueId::parse(old_id)?;
        let path = self
            .existing_issue_path(old_id)?
            .with_context(|| format!("Issue not found: {old_id}"))?;
        let issue = markdown_to_issue(
            old_id,
            &fs::read_to_string(&path).context("Failed to read issue file")?,
        )?;
        let hash_mode = self.use_hash_ids()?;
        let new_id = match new_id {
            Some(new_id) => {
                let parsed = IssueId::parse(new_id)?;
                match (hash_mode, parsed.kind()) {
                    (false, IdKind::Numeric) => anyhow::ensure!(
                        parsed.numeric_suffix().is_some(),
                        "Numeric issue ID is too large: {new_id}"
                    ),
                    (true, IdKind::Hash) => {}
                    (false, IdKind::Hash) => anyhow::bail!(
                        "Cannot renumber to {new_id}: this store uses numeric IDs (tz-<n>)"
                    ),
                    (true, IdKind::Numeric) => anyhow::bail!(
                        "Cannot renumber to {new_id}: this store uses hash IDs (tz-<hash>)"
                    ),
                }
                new_id.to_owned()
            }
            None if hash_mode => self.generate_hash_id(&issue.title, &issue.description)?,
            None => format!("{ID_PREFIX}{}", self.get_next_number()?),
        };
        anyhow::ensure!(old_id != new_id, "{old_id} already has that ID");
        let changes = self.rename_issue_no_lock(old_id, &new_id, dry_run)?;
        Ok((new_id, changes))
    }

    /// Rewrite `old_id` to `new_id` everywhere. The caller holds the write
    /// lock and has validated `new_id`'s format.
    fn rename_issue_no_lock(
        &self,
        old_id: &str,
        new_id: &str,
        dry_run: bool,
    ) -> Result<Vec<String>> {
        anyhow::ensure!(
            self.issue_exists_any_layout(old_id)?,
            "Issue not found: {old_id}"
        );
        anyhow::ensure!(
            !self.issue_exists_any_layout(new_id)?,
            "Target issue ID already exists: {new_id}"
        );
        let all_issues = self.list_all_issues_no_dependents()?;
        let mapping = HashMap::from([(old_id.to_owned(), new_id.to_owned())]);
        let mut changes = vec![format!("Rename file: {old_id}.md -> {new_id}.md")];
        for issue in &all_issues {
            if issue.depends_on.contains_key(old_id) {
                changes.push(format!(
                    "Update dependency in {}: {old_id} -> {new_id}",
                    issue.id
                ));
            }
        }
        changes.push("Preserve comments and update synchronization ownership".to_owned());
        changes.extend(self.triage_rewrite_changes(&mapping)?);
        if !dry_run {
            self.apply_id_mapping(all_issues, &mapping, &[])?;
        }
        Ok(changes)
    }

    /// Repair broken references by scanning all issues and fixing stale references
    ///
    /// This scans all issues and removes references to nonexistent issues
    pub fn repair_references(&self, dry_run: bool) -> Result<Vec<String>> {
        let _lock = self.lock_for_write()?;

        let mut changes = Vec::new();
        let all_issues = self.list_all_issues_no_dependents()?;

        // Build a set of all valid issue IDs
        let valid_ids: std::collections::HashSet<String> =
            all_issues.iter().map(|i| i.id.clone()).collect();

        // Find issues with broken references
        for issue in all_issues {
            let mut broken_refs = Vec::new();

            for dep_id in issue.depends_on.keys() {
                if !valid_ids.contains(dep_id) {
                    broken_refs.push(dep_id.clone());
                }
            }

            if !broken_refs.is_empty() {
                for broken_ref in &broken_refs {
                    changes.push(format!(
                        "Remove broken reference in {}: {} (does not exist)",
                        issue.id, broken_ref
                    ));
                }

                // If not dry-run, apply the fix
                if !dry_run {
                    let mut updated_issue = issue.clone();
                    for broken_ref in &broken_refs {
                        updated_issue.depends_on.remove(broken_ref);
                    }
                    updated_issue.updated_at = chrono::Utc::now();

                    let issue_path = self.existing_or_configured_issue_path(&updated_issue.id)?;
                    self.write_issue_to_path(&issue_path, &updated_issue)
                        .context(format!("Failed to update issue: {}", updated_issue.id))?;
                }
            }
        }

        if changes.is_empty() {
            changes.push("No broken references found".to_string());
        }

        Ok(changes)
    }

    /// Validate that a dependency target exists (warns if not)
    fn validate_dependency_exists(&self, dep_id: &str) -> bool {
        let exists = self.issue_exists_any_layout(dep_id).unwrap_or(false);

        if !exists {
            eprintln!("Warning: Dependency target does not exist: {}", dep_id);
            eprintln!("  This issue will be blocked until {} is created.", dep_id);
        }

        exists
    }

    /// Add a dependency between issues
    pub fn add_dependency(
        &self,
        from_id: &str,
        to_id: &str,
        dep_type: DependencyType,
    ) -> Result<()> {
        let _lock = self.lock_for_write()?;

        let Some(issue_path) = self.existing_issue_path(from_id)? else {
            anyhow::bail!("Issue not found: {}", from_id);
        };

        // Validate dependency target exists (warn if not)
        self.validate_dependency_exists(to_id);

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(from_id, &content)?;

        // Add dependency
        ensure_single_parent(from_id, &issue.depends_on, to_id, dep_type)?;
        issue.depends_on.insert(to_id.to_string(), dep_type);
        issue.updated_at = chrono::Utc::now();

        self.write_issue_to_path(&issue_path, &issue)?;

        Ok(())
    }

    pub fn remove_dependency(&self, from_id: &str, to_id: &str) -> Result<()> {
        let _lock = self.lock_for_write()?;

        let Some(issue_path) = self.existing_issue_path(from_id)? else {
            anyhow::bail!("Issue not found: {}", from_id);
        };

        let content = fs::read_to_string(&issue_path).context("Failed to read issue file")?;
        let mut issue = markdown_to_issue(from_id, &content)?;

        // Remove dependency
        if issue.depends_on.remove(to_id).is_none() {
            anyhow::bail!("Dependency not found: {} -> {}", from_id, to_id);
        }
        issue.updated_at = chrono::Utc::now();

        self.write_issue_to_path(&issue_path, &issue)?;

        Ok(())
    }

    /// Get dependency tree starting from a given issue
    pub fn get_dependency_tree(
        &self,
        issue_id: &str,
        max_depth: usize,
        show_all_paths: bool,
    ) -> Result<crate::types::TreeNode> {
        use std::collections::HashSet;

        // Load all issues to build the tree
        let issues = self.list_issues(None, None, None, None, None)?;
        let issues_map: HashMap<String, Issue> = issues
            .into_iter()
            .map(|issue| (issue.id.clone(), issue))
            .collect();

        // Find the root issue
        let root_issue = issues_map
            .get(issue_id)
            .ok_or_else(|| anyhow::anyhow!("Issue not found: {}", issue_id))?;

        // Track visited nodes for cycle detection (if not showing all paths)
        let mut visited = HashSet::new();

        // Build tree recursively
        build_tree_node(
            root_issue,
            &issues_map,
            &mut visited,
            0,
            max_depth,
            show_all_paths,
            None,
        )
    }

    /// Detect dependency cycles in the issue graph
    pub fn detect_dependency_cycles(&self) -> Result<Vec<Vec<String>>> {
        use std::collections::{HashMap, HashSet};

        // Load all issues
        let issues = self.list_issues(None, None, None, None, None)?;
        let issues_map: HashMap<String, Issue> = issues
            .into_iter()
            .map(|issue| (issue.id.clone(), issue))
            .collect();

        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        // Try to find cycles starting from each unvisited node
        for issue_id in issues_map.keys() {
            if !visited.contains(issue_id) {
                find_cycles_dfs(
                    issue_id,
                    &issues_map,
                    &mut visited,
                    &mut rec_stack,
                    &mut path,
                    &mut cycles,
                );
            }
        }

        Ok(cycles)
    }
}

/// A new comment on `issue_id`. The ID hashes the issue, author, body and
/// time, so two comments differ unless all four match.
fn new_comment(
    issue_id: &str,
    author: &str,
    body: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Comment {
    let hash = Sha256::digest(format!(
        "{}\n{}\n{}\n{}",
        issue_id,
        author,
        body,
        now.to_rfc3339()
    ));
    let id = format!("{}-c{:x}", issue_id, hash)
        .chars()
        .take(issue_id.len() + 12)
        .collect::<String>();
    Comment {
        id,
        issue_id: issue_id.to_string(),
        author: author.to_string(),
        body: body.to_string(),
        created_at: now,
        updated_at: now,
    }
}

/// The comment-file JSON for `comments`, sorted by creation time.
fn serialize_comments(comments: &[Comment]) -> Result<String> {
    let mut sorted = comments.to_vec();
    sorted.sort_by_key(|c| c.created_at);
    serde_json::to_string_pretty(&sorted).context("Failed to serialize comments")
}

/// Replace `search` with `replace` in `text`. `search` must match exactly once
/// unless `replace_all` is set; a missing or (without `replace_all`) ambiguous
/// match is an error naming `location` (e.g. "description field of tz-1").
pub(crate) fn search_replace_text(
    text: &str,
    search: &str,
    replace: &str,
    replace_all: bool,
    location: &str,
) -> Result<String> {
    if search.is_empty() {
        anyhow::bail!("--search text must not be empty");
    }
    let occurrences = text.matches(search).count();
    if occurrences == 0 {
        anyhow::bail!(
            "Search text not found in {}. The --search text must match the current contents exactly (including whitespace and newlines).",
            location
        );
    }
    if occurrences > 1 && !replace_all {
        anyhow::bail!(
            "Search text matches {} times in {}. Add surrounding context so it matches exactly once, or pass --replace-all to replace every occurrence.",
            occurrences,
            location
        );
    }
    // replacen with the full count is equivalent to replace-all; the
    // single-match path has already been bounded to one occurrence above.
    let limit = if replace_all { occurrences } else { 1 };
    Ok(text.replacen(search, replace, limit))
}

/// `text` with `addition` appended as its own paragraph (after a blank line),
/// or just `addition` when `text` is blank.
/// Append `<verb> <UTC timestamp>: <reason>` to the issue's notes as its own
/// paragraph. Blank reasons are ignored.
fn append_framed_note(
    issue: &mut Issue,
    verb: &str,
    now: chrono::DateTime<chrono::Utc>,
    reason: &str,
) {
    let reason = reason.trim();
    if reason.is_empty() {
        return;
    }
    let stamp = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    issue.notes = append_paragraph(&issue.notes, &format!("{verb} {stamp}: {reason}"));
}

pub(crate) fn append_paragraph(text: &str, addition: &str) -> String {
    let existing = text.trim_end();
    if existing.is_empty() {
        addition.to_string()
    } else {
        format!("{existing}\n\n{addition}")
    }
}

fn yaml_value_to_string(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Null => String::new(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

fn normalize_labels(labels: Vec<String>) -> Vec<String> {
    labels
        .into_iter()
        .flat_map(|label| {
            label
                .split(',')
                .map(|part| part.trim().to_string())
                .collect::<Vec<_>>()
        })
        .filter(|label| !label.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Recursively build a tree node
fn build_tree_node(
    issue: &Issue,
    issues_map: &HashMap<String, Issue>,
    visited: &mut std::collections::HashSet<String>,
    current_depth: usize,
    max_depth: usize,
    show_all_paths: bool,
    dep_type: Option<String>,
) -> Result<crate::types::TreeNode> {
    let mut node = crate::types::TreeNode {
        id: issue.id.clone(),
        title: issue.title.clone(),
        status: issue.status,
        priority: issue.priority,
        dep_type,
        children: Vec::new(),
        is_cycle: false,
        depth_exceeded: false,
    };

    // Check for cycle
    if !show_all_paths && visited.contains(&issue.id) {
        node.is_cycle = true;
        return Ok(node);
    }

    // Check depth limit
    if current_depth >= max_depth {
        node.depth_exceeded = true;
        return Ok(node);
    }

    // Mark as visited (if not showing all paths)
    if !show_all_paths {
        visited.insert(issue.id.clone());
    }

    // Add children (dependencies)
    for (dep_id, dep_type_val) in &issue.depends_on {
        if let Some(dep_issue) = issues_map.get(dep_id) {
            let child = build_tree_node(
                dep_issue,
                issues_map,
                visited,
                current_depth + 1,
                max_depth,
                show_all_paths,
                Some(dep_type_val.to_string()),
            )?;
            node.children.push(child);
        }
    }

    // Unmark as visited (for backtracking if not showing all paths)
    if !show_all_paths {
        visited.remove(&issue.id);
    }

    Ok(node)
}

/// DFS helper function to find cycles in the dependency graph
fn find_cycles_dfs(
    current_id: &str,
    issues_map: &HashMap<String, Issue>,
    visited: &mut std::collections::HashSet<String>,
    rec_stack: &mut std::collections::HashSet<String>,
    path: &mut Vec<String>,
    cycles: &mut Vec<Vec<String>>,
) {
    // Mark current node as visited and add to recursion stack
    visited.insert(current_id.to_string());
    rec_stack.insert(current_id.to_string());
    path.push(current_id.to_string());

    // Get the current issue
    if let Some(issue) = issues_map.get(current_id) {
        // Check all dependencies
        for dep_id in issue.depends_on.keys() {
            if !visited.contains(dep_id) {
                // Recursively visit unvisited dependencies
                find_cycles_dfs(dep_id, issues_map, visited, rec_stack, path, cycles);
            } else if rec_stack.contains(dep_id) {
                // Found a cycle! Extract the cycle from the path
                if let Some(cycle_start_idx) = path.iter().position(|id| id == dep_id) {
                    let cycle: Vec<String> = path[cycle_start_idx..].to_vec();
                    // Only add if this exact cycle hasn't been found before
                    if !cycles.iter().any(|c| cycles_equal(c, &cycle)) {
                        cycles.push(cycle);
                    }
                }
            }
        }
    }

    // Remove from recursion stack and path
    rec_stack.remove(current_id);
    path.pop();
}

/// Check if two cycles are equal (considering rotation)
fn cycles_equal(cycle1: &[String], cycle2: &[String]) -> bool {
    if cycle1.len() != cycle2.len() {
        return false;
    }

    // Check all rotations
    for i in 0..cycle1.len() {
        let mut match_found = true;
        for j in 0..cycle1.len() {
            if cycle1[(i + j) % cycle1.len()] != cycle2[j] {
                match_found = false;
                break;
            }
        }
        if match_found {
            return true;
        }
    }
    false
}

impl Storage {
    /// Populate dependents field for a vector of issues
    fn populate_dependents(issues: &mut [Issue]) {
        use crate::types::Dependency;
        use std::collections::HashMap;

        // Build a reverse dependency map: issue_id -> [(dependent_id, dep_type), ...]
        let mut reverse_deps: HashMap<String, Vec<Dependency>> = HashMap::new();

        for issue in issues.iter() {
            for (dep_id, dep_type) in &issue.depends_on {
                reverse_deps
                    .entry(dep_id.clone())
                    .or_default()
                    .push(Dependency {
                        id: issue.id.clone(),
                        dep_type: dep_type.to_string(),
                    });
            }
        }

        // Populate dependents for each issue (zero-copy: take ownership from HashMap)
        for issue in issues.iter_mut() {
            issue.dependents = reverse_deps.remove(&issue.id).unwrap_or_default();
        }
    }

    /// List all issues
    pub fn list_issues(
        &self,
        status: Option<Status>,
        priority: Option<Vec<i32>>,
        issue_type: Option<IssueType>,
        assignee: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Issue>> {
        let _lock = self.lock_for_read()?;

        let mut issues = Vec::new();
        for (issue_id, path) in self.issue_file_paths()? {
            let content = fs::read_to_string(path)?;
            let issue = parse_issue_file(&issue_id, &content)?;

            // Apply filters
            if let Some(s) = status {
                if issue.status != s {
                    continue;
                }
            }
            if let Some(ref priorities) = priority {
                if !priorities.contains(&issue.priority) {
                    continue;
                }
            }
            if let Some(t) = issue_type {
                if issue.issue_type != t {
                    continue;
                }
            }
            if let Some(a) = assignee {
                if issue.assignee != a {
                    continue;
                }
            }

            issues.push(issue);
        }

        // Sort so that numeric IDs are clustered first, in ascending numeric
        // order (1..N) so the most recent ones appear at the end, followed by
        // hash-based IDs ordered by creation date (oldest first).
        issues.sort_by(compare_for_list);

        // Apply limit
        if let Some(limit) = limit {
            if limit > 0 {
                issues.truncate(limit);
            }
        }

        // Populate dependents
        Self::populate_dependents(&mut issues);

        Ok(issues)
    }

    /// Get statistics
    pub fn get_stats(&self) -> Result<Stats> {
        let issues = self.list_issues(None, None, None, None, None)?;

        let closed_ids = closed_issue_ids(&issues);
        let by_id = issues_by_id(&issues);
        let is_blocked = |i: &Issue| is_blocked_with_ancestors(i, &by_id, &closed_ids);
        let total = issues.len();
        let open = issues.iter().filter(|i| i.status == Status::Open).count();
        let in_progress = issues
            .iter()
            .filter(|i| i.status == Status::InProgress)
            .count();
        let closed = issues.iter().filter(|i| i.status == Status::Closed).count();

        // Calculate blocked issues (those with blocking dependencies)
        let blocked = issues
            .iter()
            .filter(|i| i.status != Status::Closed && is_blocked(i))
            .count();

        // Calculate ready issues
        let ready = issues
            .iter()
            .filter(|i| i.status == Status::Open && !is_blocked(i))
            .count();

        // Calculate average lead time for closed issues
        let mut lead_times = Vec::new();
        for issue in &issues {
            if issue.status == Status::Closed {
                if let Some(closed_at) = issue.closed_at {
                    let duration = closed_at.signed_duration_since(issue.created_at);
                    lead_times.push(duration.num_hours() as f64);
                }
            }
        }

        let avg_lead_time_hours = if lead_times.is_empty() {
            0.0
        } else {
            lead_times.iter().sum::<f64>() / lead_times.len() as f64
        };

        Ok(Stats {
            total_issues: total,
            open_issues: open,
            in_progress_issues: in_progress,
            blocked_issues: blocked,
            closed_issues: closed,
            ready_issues: ready,
            average_lead_time_hours: avg_lead_time_hours,
        })
    }

    /// Get blocked issues
    pub fn get_blocked(&self) -> Result<Vec<BlockedIssue>> {
        let issues = self.list_issues(None, None, None, None, None)?;

        let closed_ids = closed_issue_ids(&issues);
        // Inherited blockers are computed up front: the lookup borrows
        // `issues`, which is then consumed below.
        let mut inherited: HashMap<String, Vec<InheritedBlocker>> = {
            let by_id = issues_by_id(&issues);
            issues
                .iter()
                .filter(|i| i.status != Status::Closed)
                .map(|i| (i.id.clone(), inherited_blockers(i, &by_id, &closed_ids)))
                .filter(|(_, found)| !found.is_empty())
                .collect()
        };
        let mut blocked = Vec::new();
        for issue in issues {
            if issue.status == Status::Closed {
                continue;
            }

            // Zero-copy: collect blocking dependencies directly without intermediate Vec
            let blocked_by: Vec<String> = issue
                .get_blocking_dependencies()
                .filter(|id| !closed_ids.contains(id.as_str()))
                .cloned()
                .collect();

            let inherited_blocked_by = inherited.remove(&issue.id).unwrap_or_default();
            if !blocked_by.is_empty() || !inherited_blocked_by.is_empty() {
                let blocked_by_count = blocked_by.len()
                    + inherited_blocked_by
                        .iter()
                        .map(InheritedBlocker::count)
                        .sum::<usize>();
                blocked.push(BlockedIssue {
                    issue,
                    blocked_by,
                    blocked_by_count,
                    inherited_blocked_by,
                });
            }
        }

        Ok(blocked)
    }

    /// Get ready work
    ///
    /// The caller is responsible for any post-query in-memory filtering (labels,
    /// title, etc.) and for applying a result limit afterwards, so that limiting
    /// happens after every filter has run.
    pub fn get_ready(
        &self,
        assignee: Option<&str>,
        priority: Option<Vec<i32>>,
        issue_type: Option<IssueType>,
        sort_policy: &str,
    ) -> Result<Vec<Issue>> {
        // Resolve dependency status from the full snapshot before filtering:
        // an assignee/priority/type filter must not hide a live prerequisite.
        let issues = self.list_issues(None, None, None, None, None)?;
        let closed_ids = closed_issue_ids(&issues);
        let blocked_ids: HashSet<String> = {
            let by_id = issues_by_id(&issues);
            issues
                .iter()
                .filter(|i| i.status == Status::Open)
                .filter(|i| is_blocked_with_ancestors(i, &by_id, &closed_ids))
                .map(|i| i.id.clone())
                .collect()
        };
        let mut ready: Vec<Issue> = issues
            .into_iter()
            .filter(|issue| {
                issue.status == Status::Open
                    && priority
                        .as_ref()
                        .is_none_or(|priorities| priorities.contains(&issue.priority))
                    && issue_type.is_none_or(|kind| issue.issue_type == kind)
                    && assignee.is_none_or(|actor| issue.assignee == actor)
                    && !blocked_ids.contains(&issue.id)
            })
            .collect();

        // Apply sorting based on policy
        match sort_policy {
            "priority" => {
                // Sort by priority (0 is highest priority, so ascending order)
                ready.sort_by_key(|i| i.priority);
            }
            "oldest" => {
                // Sort by creation date (oldest first)
                ready.sort_by_key(|i| i.created_at);
            }
            "hybrid" => {
                // Hybrid: Sort by priority first, then by creation date (oldest first) for same priority
                ready.sort_by(|a, b| {
                    a.priority
                        .cmp(&b.priority)
                        .then_with(|| a.created_at.cmp(&b.created_at))
                });
            }
            "random" => {
                // No ordering here; the caller shuffles after post-query
                // filtering so the randomization spans the whole filtered set.
            }
            _ => {
                // Default to hybrid if invalid (shouldn't happen due to CLI validation)
                ready.sort_by(|a, b| {
                    a.priority
                        .cmp(&b.priority)
                        .then_with(|| a.created_at.cmp(&b.created_at))
                });
            }
        }

        Ok(ready)
    }

    /// Export issues to JSONL format
    pub fn export_to_jsonl(
        &self,
        output_path: &Path,
        status: Option<Status>,
        priority: Option<i32>,
        issue_type: Option<IssueType>,
        assignee: Option<&str>,
        role: Option<Role>,
    ) -> Result<usize> {
        use std::io::Write;

        self.ensure_writable()?;
        // Convert single priority to vector for list_issues
        let priority_list = priority.map(|p| vec![p]);

        // Get issues with filters (list_issues acquires its own lock)
        let mut issues = self.list_issues(status, priority_list, issue_type, assignee, None)?;
        if let Some(role) = role {
            issues.retain(|issue| issue.role == Some(role));
        }

        // Open output file
        let mut file = fs::File::create(output_path)
            .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;

        // Write each issue as a JSON line
        for issue in &issues {
            let json =
                serde_json::to_string(&issue).context("Failed to serialize issue to JSON")?;
            writeln!(file, "{}", json).context("Failed to write to output file")?;
        }

        Ok(issues.len())
    }

    /// Import issues from JSONL format
    ///
    /// Returns: (imported_count, skipped_count, errors)
    #[allow(dead_code)] // Used by sync command (not yet implemented)
    pub fn import_from_jsonl(
        &self,
        input_path: &Path,
        overwrite: bool,
    ) -> Result<(usize, usize, Vec<String>)> {
        use std::io::{BufRead, BufReader};

        let _lock = self.lock_for_write()?;

        // Open input file
        let file = fs::File::open(input_path)
            .with_context(|| format!("Failed to open input file: {}", input_path.display()))?;
        let reader = BufReader::new(file);

        let mut imported = 0;
        let mut skipped = 0;
        let mut errors = Vec::new();

        // Read and parse each line
        for (line_num, line_result) in reader.lines().enumerate() {
            let line = match line_result {
                Ok(l) => l,
                Err(e) => {
                    errors.push(format!("Line {}: Failed to read line: {}", line_num + 1, e));
                    continue;
                }
            };

            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }

            // Parse JSON
            let issue: Issue = match serde_json::from_str(&line) {
                Ok(i) => i,
                Err(e) => {
                    errors.push(format!(
                        "Line {}: Failed to parse JSON: {}",
                        line_num + 1,
                        e
                    ));
                    continue;
                }
            };

            // Only canonical IDs can be imported; anything else is an error
            // for this record.
            if let Err(e) = IssueId::parse(&issue.id) {
                errors.push(format!(
                    "Line {}: Issue {:?}: {}",
                    line_num + 1,
                    issue.id,
                    e
                ));
                continue;
            }

            // Check if markdown file already exists
            let issue_path = self.existing_or_configured_issue_path(&issue.id)?;
            if issue_path.exists() && !overwrite {
                skipped += 1;
                continue;
            }

            // Convert to markdown and write
            match issue_to_markdown(&issue) {
                Ok(markdown) => {
                    if let Some(parent) = issue_path.parent() {
                        if let Err(e) = fs::create_dir_all(parent) {
                            errors.push(format!(
                                "Issue {}: Failed to create issue directory: {}",
                                issue.id, e
                            ));
                            continue;
                        }
                    }

                    if let Err(e) = fs::write(&issue_path, &markdown) {
                        errors.push(format!(
                            "Issue {}: Failed to write markdown file: {}",
                            issue.id, e
                        ));
                        continue;
                    }

                    // Set file mtime to match issue's updated_at timestamp (preserve timestamp)
                    if let Err(e) = set_file_mtime_from_issue(&issue_path, &issue) {
                        // Non-fatal: log warning but don't fail the import
                        eprintln!("Warning: Failed to set mtime for {}: {}", issue.id, e);
                    }

                    imported += 1;
                }
                Err(e) => {
                    errors.push(format!(
                        "Issue {}: Failed to convert to markdown: {}",
                        issue.id, e
                    ));
                }
            }
        }

        Ok((imported, skipped, errors))
    }

    /// Move issue markdown files between flat and sharded on-disk layouts.
    pub fn migrate_issue_storage_layout(
        &self,
        target_layout: IssueStorageLayout,
        dry_run: bool,
        update_config: bool,
    ) -> Result<Vec<String>> {
        let _lock = self.lock_for_write()?;

        let current_layout = self.issue_storage_layout()?;
        let issue_paths = self.issue_file_paths()?;
        let mut moves = Vec::new();

        for (issue_id, old_path) in issue_paths {
            let new_path = self.issue_path_for_layout(IssueId::parse(&issue_id)?, target_layout);
            if old_path == new_path {
                continue;
            }

            if new_path.exists() {
                anyhow::bail!(
                    "Cannot migrate layout: target path already exists for {}: {}",
                    issue_id,
                    new_path.display()
                );
            }

            moves.push((issue_id, old_path, new_path));
        }

        let mut changes = Vec::new();
        if update_config && current_layout != target_layout {
            changes.push(format!(
                "Update config.yaml: issue-storage-layout: {} -> {}",
                current_layout.as_str(),
                target_layout.as_str()
            ));
        }
        for (issue_id, old_path, new_path) in &moves {
            changes.push(format!(
                "Move issue {}: {} -> {}",
                issue_id,
                old_path
                    .strip_prefix(&self.tazk_dir)
                    .unwrap_or(old_path)
                    .display(),
                new_path
                    .strip_prefix(&self.tazk_dir)
                    .unwrap_or(new_path)
                    .display()
            ));
        }

        if changes.is_empty() {
            changes.push(format!(
                "No changes needed - issues are already in {} layout",
                target_layout.as_str()
            ));
        }

        if dry_run {
            return Ok(changes);
        }

        let mut transaction = FileTransaction::new(&self.tazk_dir);
        for (_issue_id, old_path, new_path) in &moves {
            ensure_contained(&self.tazk_dir, old_path)?;
            ensure_contained(&self.tazk_dir, new_path)?;
            transaction.remove(old_path.clone());
            transaction.write_with_mtime(
                new_path.clone(),
                fs::read(old_path)?,
                fs::metadata(old_path)?.modified()?,
            );
        }
        if update_config {
            transaction.config(
                "config.yaml",
                "issue-storage-layout",
                target_layout.as_str(),
            )?;
        }
        transaction.commit()?;

        self.remove_empty_issue_dirs()?;

        Ok(changes)
    }

    fn remove_empty_issue_dirs(&self) -> Result<()> {
        fn remove_empty_below(root: &Path, dir: &Path) -> Result<bool> {
            let mut is_empty = true;

            for entry in fs::read_dir(dir)
                .with_context(|| format!("Failed to read directory: {}", dir.display()))?
            {
                let entry = entry?;
                let path = entry.path();
                let file_type = entry.file_type()?;

                if file_type.is_dir() {
                    if !remove_empty_below(root, &path)? {
                        is_empty = false;
                    }
                } else {
                    is_empty = false;
                }
            }

            if is_empty && dir != root {
                fs::remove_dir(dir).with_context(|| {
                    format!("Failed to remove empty directory: {}", dir.display())
                })?;
                Ok(true)
            } else {
                Ok(false)
            }
        }

        if self.issues_dir.exists() {
            remove_empty_below(&self.issues_dir, &self.issues_dir)?;
        }
        Ok(())
    }

    /// Migrate from numeric to hash-based IDs
    ///
    /// This operation:
    /// - Generates hash-based IDs for all issues with numeric IDs
    /// - Renames all issue files from tz-N to tz-hash
    /// - Updates all dependency references
    /// - Updates all text mentions of old IDs in all issues (title, description, design, notes, acceptance_criteria)
    /// - Updates config.yaml to set tz-hash-ids: true (unless update_config is false)
    /// - Is atomic (all updates succeed or none)
    pub fn migrate_to_hash_ids(
        &self,
        dry_run: bool,
        update_config: bool,
    ) -> Result<(Vec<String>, HashMap<String, String>)> {
        let _lock = self.lock_for_write()?;

        // Check if already using hash IDs
        if self.use_hash_ids()? {
            anyhow::bail!(
                "Database is already using hash-based IDs (tz-hash-ids: true in config.yaml)"
            );
        }

        // Load all issues
        let all_issues = self.list_all_issues_no_dependents()?;

        // Build mapping from old numeric ID to new hash ID
        let mut id_mapping = HashMap::new();
        for issue in &all_issues {
            // Only migrate numeric IDs
            if numeric_id_suffix(&issue.id).is_some() {
                // Generate hash-based ID
                let hash_id = self.generate_hash_id(&issue.title, &issue.description)?;

                // Check if new ID would conflict with existing issue
                if self.issue_exists_any_layout(&hash_id)? {
                    anyhow::bail!(
                        "Cannot migrate: generated hash ID '{}' already exists. This is a collision - please report this bug.",
                        hash_id
                    );
                }

                id_mapping.insert(issue.id.clone(), hash_id);
            }
        }

        if id_mapping.is_empty() {
            anyhow::bail!(
                "No numeric IDs found to migrate. All issues already use hash-based or custom IDs."
            );
        }

        // Track all changes for dry-run mode
        let mut changes = Vec::new();
        if update_config {
            changes.push("Update config.yaml: tz-hash-ids: false -> true".to_string());
        }

        // Plan all file renames and content updates
        for issue in &all_issues {
            if let Some(new_id) = id_mapping.get(&issue.id) {
                changes.push(format!("Rename file: {}.md -> {}.md", issue.id, new_id));
                changes.push(format!(
                    "Update ID in frontmatter: {} -> {}",
                    issue.id, new_id
                ));

                // Check if this issue has dependencies that will be renamed
                for dep_id in issue.depends_on.keys() {
                    if id_mapping.contains_key(dep_id) {
                        changes.push(format!(
                            "Update dependency in {}: {} -> {}",
                            new_id,
                            dep_id,
                            id_mapping.get(dep_id).unwrap()
                        ));
                    }
                }
            }
        }

        changes.extend(self.triage_rewrite_changes(&id_mapping)?);

        // If dry-run, return changes without applying (return empty mapping for dry-run)
        if dry_run {
            return Ok((changes, HashMap::new()));
        }

        let configs = if update_config {
            vec![("config.yaml", "tz-hash-ids", "true")]
        } else {
            Vec::new()
        };
        self.apply_id_mapping(all_issues, &id_mapping, &configs)?;

        Ok((changes, id_mapping))
    }

    /// Migrate from hash-based or mixed IDs to pure numeric IDs
    ///
    /// This operation:
    /// - Identifies hash-based IDs (non-numeric chars OR length >= 4)
    /// - Sorts them by created timestamp
    /// - Assigns sequential numbers starting from MAX_ID+1
    /// - Updates all dependency references
    /// - Updates all text mentions of old IDs in all issues (title, description, design, notes, acceptance_criteria)
    /// - Updates config.yaml to set tz-hash-ids: false (unless update_config is false)
    /// - Is atomic (all updates succeed or none)
    pub fn migrate_to_numeric_ids(
        &self,
        dry_run: bool,
        update_config: bool,
    ) -> Result<(Vec<String>, HashMap<String, String>)> {
        let _lock = self.lock_for_write()?;

        // Load all issues
        let all_issues = self.list_all_issues_no_dependents()?;

        // Classify by pattern: numeric IDs stay, hash IDs get renumbered.
        let mut hash_issues = Vec::new();
        let mut max_numeric_id = 0;
        for issue in &all_issues {
            match numeric_id_suffix(&issue.id) {
                Some(num) => max_numeric_id = max_numeric_id.max(num),
                None => hash_issues.push(issue.clone()),
            }
        }

        if hash_issues.is_empty() {
            anyhow::bail!(
                "No hash-based IDs found to migrate. All issues already use numeric IDs."
            );
        }

        // Sort hash issues by created timestamp
        hash_issues.sort_by_key(|issue| issue.created_at);

        // Build mapping from old hash ID to new numeric ID
        let mut id_mapping = HashMap::new();

        for (next_id, issue) in (max_numeric_id + 1..).zip(hash_issues.iter()) {
            let new_id = format!("{ID_PREFIX}{next_id}");

            // Check if new ID would conflict with existing issue
            if self.issue_exists_any_layout(&new_id)? {
                anyhow::bail!(
                    "Cannot migrate: numeric ID '{}' already exists. This should not happen - please report this bug.",
                    new_id
                );
            }

            id_mapping.insert(issue.id.clone(), new_id);
        }

        // Track all changes for dry-run mode
        let mut changes = Vec::new();
        if update_config {
            changes.push("Update config.yaml: tz-hash-ids: true -> false".to_string());
        }

        // Plan all file renames and content updates
        for issue in &all_issues {
            if let Some(new_id) = id_mapping.get(&issue.id) {
                changes.push(format!("Rename file: {}.md -> {}.md", issue.id, new_id));
                changes.push(format!(
                    "Update ID in frontmatter: {} -> {}",
                    issue.id, new_id
                ));

                // Check if this issue has dependencies that will be renamed
                for dep_id in issue.depends_on.keys() {
                    if id_mapping.contains_key(dep_id) {
                        changes.push(format!(
                            "Update dependency in {}: {} -> {}",
                            new_id,
                            dep_id,
                            id_mapping.get(dep_id).unwrap()
                        ));
                    }
                }
            }
        }

        changes.extend(self.triage_rewrite_changes(&id_mapping)?);

        // If dry-run, return changes without applying (return empty mapping for dry-run)
        if dry_run {
            return Ok((changes, HashMap::new()));
        }

        let configs = if update_config {
            vec![("config.yaml", "tz-hash-ids", "false")]
        } else {
            Vec::new()
        };
        self.apply_id_mapping(all_issues, &id_mapping, &configs)?;

        Ok((changes, id_mapping))
    }

    /// Repack numeric IDs to fill gaps (make them contiguous)
    ///
    /// This operation:
    /// - Collects all issues with numeric IDs
    /// - Sorts them by creation timestamp (preserves chronological order)
    /// - Renumbers them as 1, 2, 3, ... (fills gaps)
    /// - If closed_issue_start is provided, numbers open and closed issues separately:
    ///   - Open issues: 1, 2, 3, ...
    ///   - Closed issues: closed_issue_start, closed_issue_start+1, ...
    /// - Updates all dependency references
    /// - Updates all text mentions of old IDs in all issues (title, description, design, notes, acceptance_criteria)
    /// - Is atomic (all updates succeed or none)
    ///
    /// Example: If you have issues 1, 3, 5, 7, they become 1, 2, 3, 4
    /// Example with --closed-issue-start=1000:
    ///   - Open issues 1, 3, 5 -> 1, 2, 3
    ///   - Closed issues 2, 4, 6 -> 1000, 1001, 1002
    pub fn repack_numeric_ids(
        &self,
        dry_run: bool,
        closed_issue_start: Option<u32>,
    ) -> Result<(Vec<String>, HashMap<String, String>)> {
        let _lock = self.lock_for_write()?;

        // Load all issues
        let all_issues = self.list_all_issues_no_dependents()?;

        // Collect all numeric IDs
        let mut numeric_issues = Vec::new();
        for issue in &all_issues {
            if numeric_id_suffix(&issue.id).is_some() {
                numeric_issues.push(issue.clone());
            }
        }

        if numeric_issues.is_empty() {
            anyhow::bail!("No numeric IDs found to repack. Use 'tz migrate --to numeric' first.");
        }

        // Sort by creation timestamp to preserve chronological order
        numeric_issues.sort_by_key(|issue| issue.created_at);

        // Build mapping from old ID to new contiguous ID
        let mut id_mapping = HashMap::new();

        // If closed_issue_start is provided, number open and closed issues separately
        if let Some(closed_start) = closed_issue_start {
            // Separate open and closed issues (maintaining chronological order within each group)
            let mut open_issues = Vec::new();
            let mut closed_issues = Vec::new();

            for issue in &numeric_issues {
                if issue.status == Status::Closed {
                    closed_issues.push(issue.clone());
                } else {
                    open_issues.push(issue.clone());
                }
            }

            // Validate that we have room for all open issues before the closed_issue_start
            if open_issues.len() as u32 >= closed_start {
                anyhow::bail!(
                    "Cannot repack: {} open issues found, but --closed-issue-start is set to {}. \
                     Open issues would collide with closed issue numbering. \
                     Use a larger value for --closed-issue-start (e.g., {}).",
                    open_issues.len(),
                    closed_start,
                    open_issues.len() as u32 + 1
                );
            }

            // Number open issues: 1, 2, 3, ...
            for (next_open_id, issue) in (1u32..).zip(open_issues.iter()) {
                let new_id = format!("{ID_PREFIX}{next_open_id}");

                // Only add to mapping if ID actually changes
                if issue.id != new_id {
                    id_mapping.insert(issue.id.clone(), new_id);
                }
            }

            // Number closed issues: closed_issue_start, closed_issue_start+1, ...
            for (next_closed_id, issue) in (closed_start..).zip(closed_issues.iter()) {
                let new_id = format!("{ID_PREFIX}{next_closed_id}");

                // Only add to mapping if ID actually changes
                if issue.id != new_id {
                    id_mapping.insert(issue.id.clone(), new_id);
                }
            }
        } else {
            // Original behavior: number all issues sequentially
            for (next_id, issue) in (1u32..).zip(numeric_issues.iter()) {
                let new_id = format!("{ID_PREFIX}{next_id}");

                // Only add to mapping if ID actually changes
                if issue.id != new_id {
                    id_mapping.insert(issue.id.clone(), new_id);
                }
            }
        }

        if id_mapping.is_empty() {
            eprintln!("No gaps found - IDs are already contiguous (1, 2, 3, ...)");
            return Ok((
                vec!["No changes needed - IDs already contiguous".to_string()],
                HashMap::new(),
            ));
        }

        // Track all changes for dry-run mode
        let mut changes = Vec::new();
        changes.push(format!(
            "Repacking {} numeric IDs to fill gaps",
            id_mapping.len()
        ));

        // Plan all file renames and content updates
        for issue in &all_issues {
            if let Some(new_id) = id_mapping.get(&issue.id) {
                changes.push(format!("Rename file: {}.md -> {}.md", issue.id, new_id));
                changes.push(format!(
                    "Update ID in frontmatter: {} -> {}",
                    issue.id, new_id
                ));

                // Check if this issue has dependencies that will be renamed
                for dep_id in issue.depends_on.keys() {
                    if id_mapping.contains_key(dep_id) {
                        changes.push(format!(
                            "Update dependency in {}: {} -> {}",
                            new_id,
                            dep_id,
                            id_mapping.get(dep_id).unwrap()
                        ));
                    }
                }
            }
        }

        changes.extend(self.triage_rewrite_changes(&id_mapping)?);

        // If dry-run, return changes without applying (return empty mapping for dry-run)
        if dry_run {
            return Ok((changes, HashMap::new()));
        }

        self.apply_id_mapping(all_issues, &id_mapping, &[])?;

        Ok((changes, id_mapping))
    }
}

/// An issue has at most one parent (`child_of` dependency). Fail if adding
/// `new_type` on `new_target` would give `issue_id` a second one. Re-adding
/// the existing parent, or retyping a dependency on the target, is fine.
pub(crate) fn ensure_single_parent(
    issue_id: &str,
    depends_on: &HashMap<String, DependencyType>,
    new_target: &str,
    new_type: DependencyType,
) -> Result<()> {
    if new_type != DependencyType::ChildOf {
        return Ok(());
    }
    if let Some(existing) = depends_on
        .iter()
        .find(|(id, t)| **t == DependencyType::ChildOf && id.as_str() != new_target)
        .map(|(id, _)| id)
    {
        anyhow::bail!(
            "{issue_id} already has a parent ({existing}); an issue can have only one. \
             Remove it first with `tz dep remove {issue_id} --target {existing}`"
        );
    }
    Ok(())
}

/// Parse the contents of an issue file named `<issue_id>.md`. A name outside
/// the canonical ID format makes the file an invalid issue, like any other
/// file that fails to parse.
fn parse_issue_file(issue_id: &str, content: &str) -> Result<Issue> {
    IssueId::parse(issue_id).context("Invalid issue filename")?;
    markdown_to_issue(issue_id, content)
}

/// Recursively walk `issues_dir` (flat or sharded layout) and return every
/// issue's ID and path. Free function so it can be used before a `Storage` is
/// fully constructed (`Storage::issue_file_paths` delegates here).
pub(crate) fn issue_file_paths_in(issues_dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    if !issues_dir.exists() {
        return Ok(Vec::new());
    }

    let mut stack = vec![issues_dir.to_path_buf()];
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir)
            .with_context(|| format!("Failed to read issues directory: {}", dir.display()))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;

            if file_type.is_dir() {
                stack.push(path);
                continue;
            }

            if !file_type.is_file() || !is_markdown_path(&path) {
                continue;
            }

            let issue_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow::anyhow!("Invalid issue filename: {}", path.display()))?
                .to_string();

            if !seen.insert(issue_id.clone()) {
                anyhow::bail!("Duplicate issue file found for ID: {}", issue_id);
            }

            paths.push((issue_id, path));
        }
    }

    paths.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(paths)
}

/// Legacy file that used to hold the tazk options before they moved into config.yaml.
const LEGACY_TAZK_CONFIG: &str = "config-tazk.yaml";

/// Write a fresh config.yaml: the documented tazk options.
fn write_initial_config(
    tazk_dir: &Path,
    tz_hash_ids: bool,
    issue_layout: IssueStorageLayout,
) -> Result<()> {
    use std::io::Write;

    let config_path = tazk_dir.join("config.yaml");
    let mut file = fs::File::create(&config_path).context("Failed to write config.yaml")?;

    writeln!(file, "# Tazk configuration")?;
    writeln!(file)?;

    writeln!(
        file,
        "# Use hash-based issue IDs instead of sequential numbers"
    )?;
    writeln!(
        file,
        "# When true, issues are named like: tz-a1b2c3 (based on content hash)"
    )?;
    writeln!(
        file,
        "# When false, issues are named like: tz-1, tz-2, ... (sequential)"
    )?;
    writeln!(file, "# Default: false")?;
    writeln!(
        file,
        "tz-hash-ids: {}",
        if tz_hash_ids { "true" } else { "false" }
    )?;
    writeln!(file)?;

    // Issue file layout
    writeln!(file, "# Issue markdown file layout")?;
    writeln!(file, "# flat: issues are stored directly under issues/")?;
    writeln!(
        file,
        "# sharded: issues are stored under issues/n/<tens>/<ones>/ for numeric IDs"
    )?;
    writeln!(
        file,
        "#          and issues/h/<first>/<second>/ for hash/custom IDs"
    )?;
    writeln!(file, "# Default: flat")?;
    writeln!(file, "issue-storage-layout: {}", issue_layout.as_str())?;

    Ok(())
}

/// Fold a legacy `config-tazk.yaml` into `config.yaml` and delete it.
/// Keys already present in config.yaml are kept.
/// config.yaml keys that used to exist and are now rejected.
const REMOVED_CONFIG_KEYS: &[&str] = &["hash-encoding", "issue-prefix"];

fn reject_removed_config_key(key: &str) -> Result<()> {
    anyhow::ensure!(
        !REMOVED_CONFIG_KEYS.contains(&key),
        "config.yaml: '{key}' is no longer supported (IDs are always tz-…); remove it"
    );
    Ok(())
}

/// Fail if config.yaml still contains a removed key.
fn reject_removed_config_keys(config_path: &Path) -> Result<()> {
    if !config_path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(config_path).context("Failed to read config.yaml")?;
    let config: HashMap<String, serde_yaml::Value> =
        serde_yaml::from_str(&content).context("Failed to parse config.yaml")?;
    for key in REMOVED_CONFIG_KEYS {
        reject_removed_config_key_if_present(&config, key)?;
    }
    Ok(())
}

fn reject_removed_config_key_if_present(
    config: &HashMap<String, serde_yaml::Value>,
    key: &str,
) -> Result<()> {
    if config.contains_key(key) {
        reject_removed_config_key(key)?;
    }
    Ok(())
}

fn merge_legacy_tazk_config(tazk_dir: &Path) -> Result<()> {
    let legacy_path = tazk_dir.join(LEGACY_TAZK_CONFIG);
    if !legacy_path.exists() {
        return Ok(());
    }
    let config_path = tazk_dir.join("config.yaml");

    let legacy: HashMap<String, String> = serde_yaml::from_str(
        &fs::read_to_string(&legacy_path)
            .with_context(|| format!("Failed to read {LEGACY_TAZK_CONFIG}"))?,
    )
    .with_context(|| format!("Failed to parse {LEGACY_TAZK_CONFIG}"))?;
    let current: HashMap<String, String> = serde_yaml::from_str(
        &fs::read_to_string(&config_path).context("Failed to read config.yaml")?,
    )
    .context("Failed to parse config.yaml")?;

    let mut missing: Vec<_> = legacy
        .iter()
        .filter(|(key, _)| !current.contains_key(*key))
        .collect();
    missing.sort();
    for (key, value) in missing {
        upsert_yaml_key_value(&config_path, key, value)?;
    }

    fs::remove_file(&legacy_path).with_context(|| format!("Failed to remove {LEGACY_TAZK_CONFIG}"))
}

/// Ensure .gitignore exists and contains required entries
fn ensure_gitignore(tazk_dir: &Path) -> Result<()> {
    use std::io::{BufRead, BufReader, Write};

    let gitignore_path = tazk_dir.join(".gitignore");
    let required_entries = [
        "tazk.lock",
        "command_history.log",
        "tz-transaction.json",
        ".tz-write-*",
        "sync_ancestors/",
    ];

    // Read existing content if file exists
    let mut existing_lines = Vec::new();
    if gitignore_path.exists() {
        let file = fs::File::open(&gitignore_path).context("Failed to read .gitignore")?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            existing_lines.push(line?);
        }
    }

    // Check which entries are missing
    let mut missing_entries = Vec::new();
    for entry in &required_entries {
        if !existing_lines.iter().any(|line| line.trim() == *entry) {
            missing_entries.push(*entry);
        }
    }

    // Append missing entries if any
    if !missing_entries.is_empty() {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&gitignore_path)
            .context("Failed to open .gitignore for writing")?;

        // Add a newline before our entries if file already had content
        if !existing_lines.is_empty() && !existing_lines.last().unwrap().is_empty() {
            writeln!(file)?;
        }

        // Write missing entries
        for entry in missing_entries {
            writeln!(file, "{}", entry)?;
        }
    }

    Ok(())
}

/// Set file mtime to match issue's updated_at timestamp
/// This preserves timestamps when importing from JSONL
#[allow(dead_code)] // Used by import and sync (not yet fully wired up)
fn set_file_mtime_from_issue(file_path: &Path, issue: &Issue) -> Result<()> {
    use filetime::{set_file_mtime, FileTime};
    use std::time::SystemTime;

    // Convert DateTime<Utc> to SystemTime
    let system_time: SystemTime = issue.updated_at.into();

    // Convert to FileTime
    let file_time = FileTime::from_system_time(system_time);

    // Set the file's modification time
    set_file_mtime(file_path, file_time)
        .with_context(|| format!("Failed to set mtime for {}", file_path.display()))?;

    Ok(())
}

/// Get file mtime as DateTime<Utc>
/// This is used for comparison during sync
#[allow(dead_code)] // Used by sync command (not yet implemented)
pub fn get_file_mtime(file_path: &Path) -> Result<chrono::DateTime<chrono::Utc>> {
    use chrono::{DateTime, Utc};
    use std::time::SystemTime;

    let metadata = fs::metadata(file_path)
        .with_context(|| format!("Failed to get metadata for {}", file_path.display()))?;

    let mtime: SystemTime = metadata
        .modified()
        .with_context(|| format!("Failed to get modified time for {}", file_path.display()))?;

    // Convert SystemTime to DateTime<Utc>
    let datetime: DateTime<Utc> = mtime.into();

    Ok(datetime)
}

fn upsert_yaml_key_value(file_path: &Path, key: &str, new_value: &str) -> Result<()> {
    use std::io::{BufRead, BufReader};

    if !file_path.exists() {
        fs::write(file_path, "")
            .with_context(|| format!("Failed to create config file: {}", file_path.display()))?;
    }

    let file = fs::File::open(file_path)
        .with_context(|| format!("Failed to open config file: {}", file_path.display()))?;
    let reader = BufReader::new(file);
    let mut lines: Vec<String> = reader
        .lines()
        .collect::<Result<_, _>>()
        .with_context(|| format!("Failed to read config file: {}", file_path.display()))?;

    let key_prefix = format!("{}:", key);
    let rendered_value = serde_yaml::to_string(new_value)
        .unwrap_or_else(|_| format!("{:?}\n", new_value))
        .trim()
        .trim_start_matches("---")
        .trim()
        .to_string();
    let mut found = false;

    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.starts_with(&key_prefix) {
            let indent = line.len() - line.trim_start().len();
            *line = format!("{}{}: {}", " ".repeat(indent), key, rendered_value);
            found = true;
            break;
        }
    }

    if !found {
        lines.push(format!("{}: {}", key, rendered_value));
    }

    let content = lines.join("\n") + "\n";
    fs::write(file_path, content)
        .with_context(|| format!("Failed to write config file: {}", file_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod create_integrity_tests {
    use super::*;

    #[test]
    fn writing_a_new_issue_never_replaces_an_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        let storage = Storage::init(tazk_dir.clone(), false, IssueStorageLayout::Flat).unwrap();

        let original = storage
            .create_issue(
                "Existing spec".to_string(),
                "Keep this issue unchanged.".to_string(),
                None,
                None,
                0,
                IssueType::Spec,
                None,
                Vec::new(),
                None,
                Vec::new(),
                None,
                None,
            )
            .unwrap();
        let path = tazk_dir.join(format!("issues/{}.md", original.id));
        let original_markdown = fs::read_to_string(&path).unwrap();

        let replacement = Issue::new(
            original.id.clone(),
            "Accidental replacement".to_string(),
            2,
            IssueType::Task,
        );
        let result = storage.write_new_issue_to_path(&path, &replacement);

        assert!(result.is_err(), "duplicate creation must fail loudly");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            original_markdown,
            "a failed create must preserve the existing issue"
        );
    }
}

#[cfg(test)]
mod list_order_tests {
    use super::*;
    use crate::types::IssueType;
    use chrono::{DateTime, Duration};

    fn issue_at(id: &str, created: DateTime<chrono::Utc>) -> Issue {
        let mut issue = Issue::new(id.to_string(), "t".to_string(), 3, IssueType::Task);
        issue.created_at = created;
        issue
    }

    #[test]
    fn numeric_suffix_parsing() {
        assert_eq!(numeric_id_suffix("tz-42"), Some(42));
        assert_eq!(numeric_id_suffix("tz-1"), Some(1));
        assert_eq!(numeric_id_suffix("tz-a3f9"), None);
        assert_eq!(numeric_id_suffix("tz-01"), None);
    }

    #[test]
    fn numeric_cluster_first_then_hash() {
        let base = chrono::Utc::now();
        let mut issues = [
            issue_at("tz-a3f9", base + Duration::seconds(1)),
            issue_at("tz-10", base + Duration::seconds(2)),
            issue_at("tz-2", base + Duration::seconds(3)),
            issue_at("tz-b001", base),
            issue_at("tz-1", base + Duration::seconds(4)),
        ];
        issues.sort_by(compare_for_list);
        let order: Vec<&str> = issues.iter().map(|i| i.id.as_str()).collect();
        // Numeric ascending first (1, 2, 10), then hash-based by creation date.
        assert_eq!(order, vec!["tz-1", "tz-2", "tz-10", "tz-b001", "tz-a3f9",]);
    }
}

#[cfg(test)]
mod ready_tests {
    use super::*;

    fn storage_with_open_issues(count: usize) -> (tempfile::TempDir, Storage) {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        let storage =
            Storage::init(tazk_dir, false, IssueStorageLayout::Flat).expect("init storage");
        for i in 0..count {
            storage
                .create_issue(
                    format!("Issue {i}"),
                    String::new(),
                    None,
                    None,
                    2,
                    IssueType::Task,
                    None,
                    Vec::new(),
                    None,
                    Vec::new(),
                    None,
                    None,
                )
                .expect("create issue");
        }
        (tmp, storage)
    }

    #[test]
    fn no_limit_returns_all_ready_issues() {
        let (_tmp, storage) = storage_with_open_issues(15);
        let ready = storage.get_ready(None, None, None, "hybrid").unwrap();
        assert_eq!(ready.len(), 15);
    }

    #[test]
    fn explicit_limit_truncates() {
        let (_tmp, storage) = storage_with_open_issues(15);
        // Limiting is now the caller's responsibility (applied after filtering).
        let mut ready = storage.get_ready(None, None, None, "hybrid").unwrap();
        ready.truncate(5);
        assert_eq!(ready.len(), 5);
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn init_writes_all_options_to_config_yaml_only() {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        let storage = Storage::init(tazk_dir.clone(), true, IssueStorageLayout::Sharded).unwrap();

        assert!(!tazk_dir.join("config-tazk.yaml").exists());
        assert!(storage.use_hash_ids().unwrap());
        assert_eq!(
            storage.issue_storage_layout().unwrap(),
            IssueStorageLayout::Sharded
        );
        let config = fs::read_to_string(tazk_dir.join("config.yaml")).unwrap();
        assert!(!config.contains("hash-encoding"));
        assert!(!config.contains("issue-prefix"));
    }

    #[test]
    fn open_merges_legacy_tazk_config() {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        fs::create_dir_all(tazk_dir.join("issues")).unwrap();
        fs::write(
            tazk_dir.join("config.yaml"),
            "# options live in the legacy file\n",
        )
        .unwrap();
        fs::write(
            tazk_dir.join("config-tazk.yaml"),
            "# comment\ntz-hash-ids: true\nissue-storage-layout: sharded\n",
        )
        .unwrap();

        let storage = Storage::open(tazk_dir.clone()).unwrap();
        assert!(!tazk_dir.join("config-tazk.yaml").exists());
        assert!(storage.use_hash_ids().unwrap());
        assert_eq!(
            storage.issue_storage_layout().unwrap(),
            IssueStorageLayout::Sharded
        );

        // Read-only open refuses to ignore an unmerged legacy file.
        fs::write(tazk_dir.join("config-tazk.yaml"), "tz-hash-ids: false\n").unwrap();
        assert!(Storage::open_readonly(tazk_dir).is_err());
    }

    #[test]
    fn open_rejects_removed_hash_encoding_key() {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        fs::create_dir_all(tazk_dir.join("issues")).unwrap();
        fs::write(tazk_dir.join("config.yaml"), "hash-encoding: base36\n").unwrap();

        let expected =
            "config.yaml: 'hash-encoding' is no longer supported (IDs are always tz-…); remove it";
        let err = Storage::open(tazk_dir.clone()).err().unwrap();
        assert_eq!(err.to_string(), expected);
        let err = Storage::open_readonly(tazk_dir).err().unwrap();
        assert_eq!(err.to_string(), expected);
    }

    #[test]
    fn config_set_rejects_hash_encoding() {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        let storage = Storage::init(tazk_dir.clone(), false, IssueStorageLayout::Flat).unwrap();
        let err = storage
            .set_config_value("hash-encoding", "base36")
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("'hash-encoding' is no longer supported"));
        let config = fs::read_to_string(tazk_dir.join("config.yaml")).unwrap();
        assert!(!config.contains("hash-encoding"));
    }

    #[test]
    fn open_rejects_removed_issue_prefix_key() {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        fs::create_dir_all(tazk_dir.join("issues")).unwrap();
        fs::write(tazk_dir.join("config.yaml"), "issue-prefix: tz\n").unwrap();

        let expected =
            "config.yaml: 'issue-prefix' is no longer supported (IDs are always tz-…); remove it";
        let err = Storage::open(tazk_dir.clone()).err().unwrap();
        assert_eq!(err.to_string(), expected);
        let err = Storage::open_readonly(tazk_dir).err().unwrap();
        assert_eq!(err.to_string(), expected);
    }

    #[test]
    fn config_set_rejects_issue_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        let storage = Storage::init(tazk_dir.clone(), false, IssueStorageLayout::Flat).unwrap();
        let err = storage.set_config_value("issue-prefix", "x").unwrap_err();
        assert!(err
            .to_string()
            .contains("'issue-prefix' is no longer supported"));
        let config = fs::read_to_string(tazk_dir.join("config.yaml")).unwrap();
        assert!(!config.contains("issue-prefix"));
    }

    /// A missing config.yaml is recreated without any prefix key, and new IDs
    /// always use the fixed prefix whatever the existing issue files are called.
    #[test]
    fn open_without_config_creates_one_and_uses_fixed_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join("acme-project").join(".tazk");
        fs::create_dir_all(&tazk_dir).unwrap();

        let storage = Storage::open(tazk_dir.clone()).expect("open must succeed");
        let config = fs::read_to_string(tazk_dir.join("config.yaml")).unwrap();
        assert!(!config.contains("issue-prefix"));
        let issue = storage
            .create_issue(
                "t".into(),
                "d".into(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                vec![],
                None,
                vec![],
                Some(Role::Agent),
                None,
            )
            .unwrap();
        assert_eq!(issue.id, format!("{ID_PREFIX}1"));
    }
}

#[cfg(test)]
mod issue_layout_tests {
    use super::*;
    use crate::types::IssueType;

    #[test]
    fn sharded_paths_use_numeric_suffix_or_hash_prefix() {
        assert_eq!(
            sharded_relative_path(IssueId::parse("tz-1").unwrap()),
            PathBuf::from("n").join("0").join("1").join("tz-1.md")
        );
        assert_eq!(
            sharded_relative_path(IssueId::parse("tz-12").unwrap()),
            PathBuf::from("n").join("1").join("2").join("tz-12.md")
        );
        assert_eq!(
            sharded_relative_path(IssueId::parse("tz-123").unwrap()),
            PathBuf::from("n").join("2").join("3").join("tz-123.md")
        );
        assert_eq!(
            sharded_relative_path(IssueId::parse("tz-a3f9c2").unwrap()),
            PathBuf::from("h").join("a").join("3").join("tz-a3f9c2.md")
        );
    }

    #[test]
    fn create_uses_configured_sharded_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        let storage = Storage::init(tazk_dir.clone(), false, IssueStorageLayout::Sharded).unwrap();

        let issue = storage
            .create_issue(
                "Task".to_string(),
                String::new(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                Vec::new(),
                None,
                None,
            )
            .unwrap();

        assert_eq!(issue.id, "tz-1");
        assert!(tazk_dir.join("issues/n/0/1/tz-1.md").exists());
        assert!(!tazk_dir.join("issues/tz-1.md").exists());
        assert!(storage.get_issue("tz-1").unwrap().is_some());
    }

    #[test]
    fn migrate_layout_moves_existing_files_and_updates_config() {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        let storage = Storage::init(tazk_dir.clone(), false, IssueStorageLayout::Flat).unwrap();

        storage
            .create_issue(
                "One".to_string(),
                String::new(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                Vec::new(),
                None,
                None,
            )
            .unwrap();
        storage
            .create_issue(
                "Two".to_string(),
                String::new(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                Vec::new(),
                None,
                None,
            )
            .unwrap();

        let changes = storage
            .migrate_issue_storage_layout(IssueStorageLayout::Sharded, false, true)
            .unwrap();

        assert!(changes.iter().any(|c| c.contains("issue-storage-layout")));
        assert!(tazk_dir.join("issues/n/0/1/tz-1.md").exists());
        assert!(tazk_dir.join("issues/n/0/2/tz-2.md").exists());
        assert!(!tazk_dir.join("issues/tz-1.md").exists());
        assert_eq!(
            storage.issue_storage_layout().unwrap(),
            IssueStorageLayout::Sharded
        );

        let next = storage
            .create_issue(
                "Three".to_string(),
                String::new(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                Vec::new(),
                None,
                None,
            )
            .unwrap();
        assert_eq!(next.id, "tz-3");
        assert!(tazk_dir.join("issues/n/0/3/tz-3.md").exists());
    }
}

#[cfg(test)]
mod claim_tests {
    use super::*;

    /// Create an initialized storage in a temp dir with a single open issue.
    fn storage_with_one_issue() -> (tempfile::TempDir, Storage, String) {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        let storage =
            Storage::init(tazk_dir, false, IssueStorageLayout::Flat).expect("init storage");
        let issue = storage
            .create_issue(
                "A task".to_string(),
                String::new(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                Vec::new(),
                None,
                None,
            )
            .expect("create issue");
        (tmp, storage, issue.id)
    }

    #[test]
    fn claim_sets_assignee_and_status() {
        let (_tmp, storage, id) = storage_with_one_issue();
        let before = storage.get_issue(&id).unwrap().unwrap().updated_at;

        let issue = storage.claim_issue(&id, "host-a", false).unwrap();
        assert_eq!(issue.assignee, "host-a");
        assert_eq!(issue.status, Status::InProgress);
        assert!(issue.updated_at >= before);
    }

    #[test]
    fn claim_rejected_when_in_progress() {
        let (_tmp, storage, id) = storage_with_one_issue();
        storage.claim_issue(&id, "host-a", false).unwrap();

        let err = storage.claim_issue(&id, "host-b", false).unwrap_err();
        assert!(
            err.to_string().contains("already in progress"),
            "unexpected error: {err}"
        );
        // The original holder must be untouched.
        let issue = storage.get_issue(&id).unwrap().unwrap();
        assert_eq!(issue.assignee, "host-a");
    }

    #[test]
    fn same_actor_cannot_reclaim_without_force() {
        let (_tmp, storage, id) = storage_with_one_issue();
        storage.claim_issue(&id, "host-a", false).unwrap();
        assert!(storage.claim_issue(&id, "host-a", false).is_err());
    }

    #[test]
    fn force_overrides_in_progress_claim() {
        let (_tmp, storage, id) = storage_with_one_issue();
        storage.claim_issue(&id, "host-a", false).unwrap();

        let issue = storage.claim_issue(&id, "host-b", true).unwrap();
        assert_eq!(issue.assignee, "host-b");
        assert_eq!(issue.status, Status::InProgress);
    }

    #[test]
    fn claiming_a_closed_issue_fails_even_with_force() {
        let (_tmp, storage, id) = storage_with_one_issue();
        storage.close_issue(&id, Some("done")).unwrap();
        for force in [false, true] {
            let err = storage.claim_issue(&id, "host-a", force).unwrap_err();
            assert!(err.to_string().contains("closed"), "unexpected: {err}");
        }
    }

    #[test]
    fn release_clears_claim_and_reopens() {
        let (_tmp, storage, id) = storage_with_one_issue();
        storage.claim_issue(&id, "host-a", false).unwrap();

        let issue = storage.release_issue(&id, "host-a", false).unwrap();
        assert!(issue.assignee.is_empty());
        assert_eq!(issue.status, Status::Open);

        // Released issues can be claimed again.
        storage.claim_issue(&id, "host-b", false).unwrap();
    }

    #[test]
    fn release_by_other_requires_force() {
        let (_tmp, storage, id) = storage_with_one_issue();
        storage.claim_issue(&id, "host-a", false).unwrap();

        assert!(storage.release_issue(&id, "host-b", false).is_err());
        let issue = storage.release_issue(&id, "host-b", true).unwrap();
        assert!(issue.assignee.is_empty());
    }

    #[test]
    fn update_issue_cannot_set_status() {
        let (_tmp, storage, id) = storage_with_one_issue();
        let mut updates = HashMap::new();
        updates.insert("status".to_string(), "blocked".to_string());
        assert!(storage.update_issue(&id, updates).is_err());
        let issue = storage.get_issue(&id).unwrap().unwrap();
        assert_eq!(issue.status, Status::Open);
    }
}

#[cfg(test)]
mod reopen_tests {
    use super::*;

    fn storage() -> (tempfile::TempDir, Storage) {
        let tmp = tempfile::tempdir().unwrap();
        let storage =
            Storage::init(tmp.path().join(".tazk"), false, IssueStorageLayout::Flat).unwrap();
        (tmp, storage)
    }

    fn make_open_issue(storage: &Storage, role: Option<Role>) -> String {
        storage
            .create_issue(
                "x".into(),
                String::new(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                Vec::new(),
                role,
                None,
            )
            .unwrap()
            .id
    }

    fn status(storage: &Storage, id: &str) -> Status {
        storage.get_issue(id).unwrap().unwrap().status
    }

    #[test]
    fn reopen_requires_a_role() {
        let (_tmp, storage) = storage();
        let id = make_open_issue(&storage, None);
        storage.close_issue(&id, Some("done")).unwrap();

        let err = storage.reopen_issue(&id, None, None).unwrap_err();
        assert!(err.to_string().contains("--role"), "{err}");
        assert_eq!(status(&storage, &id), Status::Closed);

        let issue = storage.reopen_issue(&id, Some(Role::Human), None).unwrap();
        assert_eq!(issue.status, Status::Open);
        assert_eq!(issue.role, Some(Role::Human));
    }

    #[test]
    fn reopen_keeps_existing_role_without_flag() {
        let (_tmp, storage) = storage();
        let id = make_open_issue(&storage, Some(Role::Agent));
        storage.close_issue(&id, Some("done")).unwrap();
        let issue = storage.reopen_issue(&id, None, None).unwrap();
        assert_eq!(issue.role, Some(Role::Agent));
    }
}

#[cfg(test)]
mod role_effort_tests {
    use super::*;

    fn storage() -> (tempfile::TempDir, Storage) {
        let tmp = tempfile::tempdir().unwrap();
        let storage =
            Storage::init(tmp.path().join(".tazk"), false, IssueStorageLayout::Flat).unwrap();
        (tmp, storage)
    }

    fn create(storage: &Storage, role: Option<Role>, effort: Option<u8>) -> Result<Issue> {
        storage.create_issue(
            "T".to_string(),
            String::new(),
            None,
            None,
            2,
            IssueType::Task,
            None,
            Vec::new(),
            None,
            Vec::new(),
            role,
            effort,
        )
    }

    #[test]
    fn create_stores_role_and_effort() {
        let (_tmp, storage) = storage();
        let issue = create(&storage, Some(Role::Human), Some(2)).unwrap();
        let stored = storage.get_issue(&issue.id).unwrap().unwrap();
        assert_eq!(stored.role, Some(Role::Human));
        assert_eq!(stored.effort, Some(2));
    }

    #[test]
    fn create_rejects_out_of_range_effort() {
        let (_tmp, storage) = storage();
        assert!(create(&storage, None, Some(0)).is_err());
        assert!(create(&storage, None, Some(6)).is_err());
    }

    #[test]
    fn update_sets_and_clears_role_and_effort() {
        let (_tmp, storage) = storage();
        let issue = create(&storage, None, None).unwrap();
        let mut updates = HashMap::new();
        updates.insert("role".to_string(), "agent".to_string());
        updates.insert("effort".to_string(), "5".to_string());
        let updated = storage.update_issue(&issue.id, updates).unwrap();
        assert_eq!(updated.role, Some(Role::Agent));
        assert_eq!(updated.effort, Some(5));

        let mut updates = HashMap::new();
        updates.insert("effort".to_string(), String::new());
        let cleared = storage.update_issue(&issue.id, updates).unwrap();
        assert_eq!(cleared.role, Some(Role::Agent));
        assert_eq!(cleared.effort, None);
    }

    fn create_typed(storage: &Storage, issue_type: IssueType, role: Option<Role>) -> Result<Issue> {
        // Specs and gates need a description, and gates need acceptance criteria.
        let acceptance = (issue_type == IssueType::Gate).then(|| "Check it.".to_string());
        storage.create_issue(
            "T".to_string(),
            "Body".to_string(),
            None,
            acceptance,
            2,
            issue_type,
            None,
            Vec::new(),
            None,
            Vec::new(),
            role,
            None,
        )
    }

    fn update_role(storage: &Storage, id: &str, role: &str) -> Result<Issue> {
        let mut updates = HashMap::new();
        updates.insert("role".to_string(), role.to_string());
        storage.update_issue(id, updates)
    }

    /// An epic or spec that has a role stored, as written before the role rule.
    fn legacy_with_role(storage: &Storage, issue_type: IssueType) -> String {
        let id = create_typed(storage, issue_type, None).unwrap().id;
        let path = storage.get_tazk_dir().join(format!("issues/{id}.md"));
        let text = fs::read_to_string(&path).unwrap();
        let marker = format!("issue_type: {issue_type}\n");
        assert!(text.contains(&marker));
        fs::write(
            &path,
            text.replace(&marker, &format!("{marker}role: agent\n")),
        )
        .unwrap();
        assert_eq!(
            storage.get_issue(&id).unwrap().unwrap().role,
            Some(Role::Agent)
        );
        id
    }

    #[test]
    fn epics_and_specs_take_no_role() {
        let (_tmp, storage) = storage();
        for ty in [IssueType::Epic, IssueType::Spec] {
            let err = create_typed(&storage, ty, Some(Role::Agent)).unwrap_err();
            assert!(err.to_string().contains("take no role"), "{err}");
            let issue = create_typed(&storage, ty, None).unwrap();
            assert_eq!(issue.role, None);

            let err = update_role(&storage, &issue.id, "human").unwrap_err();
            assert!(err.to_string().contains("take no role"), "{err}");
            assert_eq!(update_role(&storage, &issue.id, "").unwrap().role, None);
        }
        for ty in [
            IssueType::Task,
            IssueType::Bug,
            IssueType::Feature,
            IssueType::Chore,
            IssueType::Gate,
        ] {
            let issue = create_typed(&storage, ty, Some(Role::Human)).unwrap();
            let err = update_role(&storage, &issue.id, "").unwrap_err();
            assert!(err.to_string().contains("cannot be cleared"), "{err}");
            let issue = update_role(&storage, &issue.id, "agent").unwrap();
            assert_eq!(issue.role, Some(Role::Agent));
        }
    }

    // Section rules on update (tz-51 §4, §5.3).

    fn update_body(storage: &Storage, id: &str, key: &str, value: &str) -> Result<Issue> {
        let mut updates = HashMap::new();
        updates.insert(key.to_string(), value.to_string());
        storage.update_issue(id, updates)
    }

    fn issue_file(storage: &Storage, id: &str) -> String {
        fs::read_to_string(storage.get_tazk_dir().join(format!("issues/{id}.md"))).unwrap()
    }

    /// Rewrite an issue file directly, bypassing the checks.
    fn rewrite_file(storage: &Storage, id: &str, from: &str, to: &str) {
        let path = storage.get_tazk_dir().join(format!("issues/{id}.md"));
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(from), "{from:?} not in {text}");
        fs::write(&path, text.replacen(from, to, 1)).unwrap();
    }

    #[test]
    fn update_rejects_forbidden_and_emptied_required_sections() {
        let (_tmp, storage) = storage();
        // (type, key, field, rejected value)
        let cases = [
            (IssueType::Epic, "design", EditField::Design, "approach"),
            (IssueType::Spec, "design", EditField::Design, "approach"),
            (
                IssueType::Spec,
                "acceptance_criteria",
                EditField::Acceptance,
                "c",
            ),
            (IssueType::Gate, "design", EditField::Design, "approach"),
            (IssueType::Spec, "description", EditField::Description, "  "),
            (IssueType::Gate, "description", EditField::Description, ""),
            (
                IssueType::Gate,
                "acceptance_criteria",
                EditField::Acceptance,
                " ",
            ),
        ];
        for (ty, key, field, value) in cases {
            let id = create_typed(&storage, ty, (ty == IssueType::Gate).then_some(Role::Agent))
                .unwrap()
                .id;
            let before = issue_file(&storage, &id);

            let err = update_body(&storage, &id, key, value).unwrap_err();
            assert!(
                err.to_string().contains(&id) || err.to_string().contains("needs"),
                "{ty} {key}: {err}"
            );
            assert_eq!(issue_file(&storage, &id), before, "{ty} {key} update");

            if field == EditField::Design
                || (ty == IssueType::Spec && field == EditField::Acceptance)
            {
                // Append to an empty forbidden section; replace is impossible.
                let err = storage.append_to_issue(&id, field, "more").unwrap_err();
                assert!(err.to_string().contains(&id), "{ty} {key}: {err}");
                assert!(!err.to_string().contains("clearing"), "{err}");
                assert_eq!(issue_file(&storage, &id), before, "{ty} {key} append");
            } else {
                // Replace the whole body with nothing.
                let mut body = storage.get_issue(&id).unwrap().unwrap();
                let text = body.text_field_mut(field).clone();
                let err = storage
                    .search_replace_issue(&id, field, &text, "", false)
                    .unwrap_err();
                assert!(err.to_string().contains("needs"), "{ty} {key}: {err}");
                assert_eq!(issue_file(&storage, &id), before, "{ty} {key} replace");
            }
        }
    }

    #[test]
    fn update_accepts_valid_section_changes_and_unchecked_fields() {
        let (_tmp, storage) = storage();
        let epic = create_typed(&storage, IssueType::Epic, None).unwrap().id;
        update_body(&storage, &epic, "acceptance_criteria", "").unwrap();
        update_body(&storage, &epic, "design", "  ").unwrap();
        storage
            .append_to_issue(&epic, EditField::Notes, "n")
            .unwrap();
        storage
            .append_to_issue(&epic, EditField::Title, "t")
            .unwrap();

        let spec = create_typed(&storage, IssueType::Spec, None).unwrap().id;
        update_body(&storage, &spec, "description", "New spec").unwrap();
        update_body(&storage, &spec, "notes", "later").unwrap();
        update_body(&storage, &spec, "design", "").unwrap();
        storage
            .search_replace_issue(&spec, EditField::Description, "New", "Newer", false)
            .unwrap();
        storage
            .search_replace_issue(&spec, EditField::Title, "T", "U", true)
            .unwrap();
        storage
            .append_to_issue(&spec, EditField::Description, "more")
            .unwrap();

        let gate = create_typed(&storage, IssueType::Gate, Some(Role::Human))
            .unwrap()
            .id;
        update_body(&storage, &gate, "acceptance_criteria", "Look").unwrap();
        update_body(&storage, &gate, "priority", "1").unwrap();
        storage
            .append_to_issue(&gate, EditField::Acceptance, "and see")
            .unwrap();
        storage
            .search_replace_issue(&gate, EditField::Acceptance, "Look", "Peek", false)
            .unwrap();
        let gate = storage.get_issue(&gate).unwrap().unwrap();
        assert_eq!(gate.acceptance_criteria, "Peek\n\nand see");
    }

    #[test]
    fn update_checks_each_issue_against_its_own_type() {
        let (_tmp, storage) = storage();
        let task = create_typed(&storage, IssueType::Task, Some(Role::Agent))
            .unwrap()
            .id;
        let epic = create_typed(&storage, IssueType::Epic, None).unwrap().id;
        update_body(&storage, &task, "design", "ok").unwrap();
        let err = update_body(&storage, &epic, "design", "no").unwrap_err();
        assert!(err.to_string().contains("epics have no Design"), "{err}");
    }

    #[test]
    fn issues_that_break_the_rules_still_work_and_can_be_cleared() {
        let (_tmp, storage) = storage();

        let epic = create_typed(&storage, IssueType::Epic, None).unwrap().id;
        rewrite_file(
            &storage,
            &epic,
            "# Description",
            "# Design\n\nOld approach\n\n# Description",
        );
        let spec = create_typed(&storage, IssueType::Spec, None).unwrap().id;
        rewrite_file(
            &storage,
            &spec,
            "# Description",
            "# Acceptance Criteria\n\nOld\n\n# Description",
        );
        let gate = create_typed(&storage, IssueType::Gate, Some(Role::Agent))
            .unwrap()
            .id;
        rewrite_file(&storage, &gate, "Check it.", "");

        assert_eq!(
            storage.get_issue(&epic).unwrap().unwrap().design,
            "Old approach"
        );
        assert_eq!(
            storage
                .get_issue(&spec)
                .unwrap()
                .unwrap()
                .acceptance_criteria,
            "Old"
        );
        assert_eq!(
            storage
                .get_issue(&gate)
                .unwrap()
                .unwrap()
                .acceptance_criteria,
            ""
        );

        for id in [&epic, &spec, &gate] {
            update_body(&storage, id, "title", "Renamed").unwrap();
            update_body(&storage, id, "notes", "note").unwrap();
            update_body(&storage, id, "description", "Body 2").unwrap();
            storage.append_to_issue(id, EditField::Title, "x").unwrap();
            storage.close_issue(id, None).unwrap();
            let role = (*id == gate).then_some(Role::Agent);
            storage.reopen_issue(id, role, Some("again")).unwrap();
        }

        // The forbidden sections cannot be changed, only cleared.
        let err = update_body(&storage, &epic, "design", "New").unwrap_err();
        assert!(err.to_string().contains("clearing it is allowed"), "{err}");
        let err = storage
            .append_to_issue(&epic, EditField::Design, "more")
            .unwrap_err();
        assert!(err.to_string().contains("clearing it is allowed"), "{err}");
        let err = storage
            .search_replace_issue(&epic, EditField::Design, "Old", "New", false)
            .unwrap_err();
        assert!(err.to_string().contains("clearing it is allowed"), "{err}");
        let err = storage
            .search_replace_issue(&spec, EditField::Acceptance, "Old", "New", true)
            .unwrap_err();
        assert!(err.to_string().contains("--acceptance"), "{err}");

        // A gate's empty Acceptance Criteria only fails when it is touched.
        let err = update_body(&storage, &gate, "acceptance_criteria", "").unwrap_err();
        assert!(err.to_string().contains("A gate needs Acceptance"), "{err}");

        update_body(&storage, &epic, "design", "").unwrap();
        update_body(&storage, &spec, "acceptance_criteria", "").unwrap();
        assert_eq!(storage.get_issue(&epic).unwrap().unwrap().design, "");
        assert_eq!(
            storage
                .get_issue(&spec)
                .unwrap()
                .unwrap()
                .acceptance_criteria,
            ""
        );
    }

    #[test]
    fn legacy_epic_or_spec_role_still_loads_and_takes_other_changes() {
        let (_tmp, storage) = storage();
        for ty in [IssueType::Epic, IssueType::Spec] {
            let id = legacy_with_role(&storage, ty);
            let mut updates = HashMap::new();
            updates.insert("title".to_string(), "Renamed".to_string());
            let updated = storage.update_issue(&id, updates).unwrap();
            assert_eq!(updated.title, "Renamed");
            assert_eq!(updated.role, Some(Role::Agent));
            assert!(update_role(&storage, &id, "human").is_err());

            // Closing and reopening need no role, and reopening rejects one.
            storage.close_issue(&id, None).unwrap();
            let err = storage
                .reopen_issue(&id, Some(Role::Human), None)
                .unwrap_err();
            assert!(err.to_string().contains("take no role"), "{err}");
            assert_eq!(
                storage.get_issue(&id).unwrap().unwrap().status,
                Status::Closed
            );
            storage.reopen_issue(&id, None, None).unwrap();

            assert_eq!(update_role(&storage, &id, "").unwrap().role, None);
        }
    }

    #[test]
    fn reopen_epic_or_spec_without_role() {
        let (_tmp, storage) = storage();
        for ty in [IssueType::Epic, IssueType::Spec] {
            let id = create_typed(&storage, ty, None).unwrap().id;
            storage.close_issue(&id, None).unwrap();
            let issue = storage.reopen_issue(&id, None, None).unwrap();
            assert_eq!(issue.status, Status::Open);
            assert_eq!(issue.role, None);
        }
    }

    #[test]
    fn update_rejects_invalid_values_and_leaves_issue_untouched() {
        let (_tmp, storage) = storage();
        let issue = create(&storage, Some(Role::Human), Some(1)).unwrap();
        for (key, value) in [("role", "robot"), ("effort", "9"), ("effort", "abc")] {
            let mut updates = HashMap::new();
            updates.insert(key.to_string(), value.to_string());
            assert!(storage.update_issue(&issue.id, updates).is_err());
        }
        let stored = storage.get_issue(&issue.id).unwrap().unwrap();
        assert_eq!(stored.role, Some(Role::Human));
        assert_eq!(stored.effort, Some(1));
    }
}

#[cfg(test)]
mod comment_tests {
    use super::*;

    fn storage() -> (tempfile::TempDir, Storage) {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        let storage =
            Storage::init(tazk_dir, false, IssueStorageLayout::Flat).expect("init storage");
        (tmp, storage)
    }

    #[test]
    fn comments_round_trip_in_created_order() {
        let (_tmp, storage) = storage();
        let issue = storage
            .create_issue(
                "commented".to_string(),
                String::new(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                Vec::new(),
                None,
                None,
            )
            .unwrap();

        let first = storage
            .add_comment(&issue.id, "alice", "first comment")
            .unwrap();
        let second = storage
            .add_comment(&issue.id, "bob", "second comment")
            .unwrap();

        let comments = storage.list_comments(&issue.id).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].id, first.id);
        assert_eq!(comments[0].body, "first comment");
        assert_eq!(comments[1].id, second.id);
        assert_eq!(comments[1].body, "second comment");
    }

    #[test]
    fn delete_comment_removes_only_the_targeted_comment() {
        let (_tmp, storage) = storage();
        let issue = storage
            .create_issue(
                "commented".to_string(),
                String::new(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                Vec::new(),
                None,
                None,
            )
            .unwrap();

        let first = storage.add_comment(&issue.id, "alice", "first").unwrap();
        let second = storage.add_comment(&issue.id, "bob", "second").unwrap();

        let removed = storage.delete_comment(&issue.id, &first.id).unwrap();
        assert_eq!(removed.id, first.id);

        let comments = storage.list_comments(&issue.id).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].id, second.id);
    }

    #[test]
    fn delete_missing_comment_errors_and_leaves_others() {
        let (_tmp, storage) = storage();
        let issue = storage
            .create_issue(
                "commented".to_string(),
                String::new(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                Vec::new(),
                None,
                None,
            )
            .unwrap();
        let only = storage.add_comment(&issue.id, "alice", "keep me").unwrap();

        let err = storage
            .delete_comment(&issue.id, "does-not-exist")
            .unwrap_err();
        assert!(err.to_string().contains("not found"), "unexpected: {err}");

        // The real comment is untouched.
        let comments = storage.list_comments(&issue.id).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].id, only.id);
    }
}

#[cfg(test)]
mod search_replace_tests {
    use super::*;

    /// Create an initialized storage with one issue whose description is `desc`.
    fn storage_with_description(desc: &str) -> (tempfile::TempDir, Storage, String) {
        let tmp = tempfile::tempdir().unwrap();
        let tazk_dir = tmp.path().join(".tazk");
        let storage =
            Storage::init(tazk_dir, false, IssueStorageLayout::Flat).expect("init storage");
        let issue = storage
            .create_issue(
                "A task".to_string(),
                desc.to_string(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                Vec::new(),
                None,
                None,
            )
            .expect("create issue");
        (tmp, storage, issue.id)
    }

    #[test]
    fn unique_match_is_replaced() {
        let (_tmp, storage, id) = storage_with_description("hello world, hello there");
        let issue = storage
            .search_replace_issue(&id, EditField::Description, "world", "rust", false)
            .unwrap();
        assert_eq!(issue.description, "hello rust, hello there");
    }

    #[test]
    fn missing_search_text_errors_and_leaves_file_untouched() {
        let (_tmp, storage, id) = storage_with_description("unchanged body");
        let err = storage
            .search_replace_issue(&id, EditField::Description, "absent", "x", false)
            .unwrap_err();
        assert!(err.to_string().contains("not found"), "unexpected: {err}");
        let issue = storage.get_issue(&id).unwrap().unwrap();
        assert_eq!(issue.description, "unchanged body");
    }

    #[test]
    fn ambiguous_match_errors_without_replace_all() {
        let (_tmp, storage, id) = storage_with_description("a a a");
        let err = storage
            .search_replace_issue(&id, EditField::Description, "a", "b", false)
            .unwrap_err();
        assert!(err.to_string().contains("3 times"), "unexpected: {err}");
        // Nothing was written.
        let issue = storage.get_issue(&id).unwrap().unwrap();
        assert_eq!(issue.description, "a a a");
    }

    #[test]
    fn replace_all_rewrites_every_occurrence() {
        let (_tmp, storage, id) = storage_with_description("a a a");
        let issue = storage
            .search_replace_issue(&id, EditField::Description, "a", "b", true)
            .unwrap();
        assert_eq!(issue.description, "b b b");
    }

    #[test]
    fn empty_search_is_rejected() {
        let (_tmp, storage, id) = storage_with_description("body");
        let err = storage
            .search_replace_issue(&id, EditField::Description, "", "x", false)
            .unwrap_err();
        assert!(err.to_string().contains("empty"), "unexpected: {err}");
    }

    #[test]
    fn multiline_search_targets_selected_field() {
        let (_tmp, storage, id) = storage_with_description("body");
        storage
            .update_issue(
                &id,
                HashMap::from([(
                    "design".to_string(),
                    "line one\nline two\nline three".to_string(),
                )]),
            )
            .unwrap();
        let issue = storage
            .search_replace_issue(
                &id,
                EditField::Design,
                "line two\nline three",
                "line two only",
                false,
            )
            .unwrap();
        assert_eq!(issue.design, "line one\nline two only");
        // The description field is untouched by a design-targeted edit.
        assert_eq!(issue.description, "body");
    }

    #[test]
    fn append_adds_a_paragraph_after_existing_content() {
        let (_tmp, storage, id) = storage_with_description("First paragraph.");
        let issue = storage
            .append_to_issue(&id, EditField::Description, "Second paragraph.")
            .unwrap();
        assert_eq!(issue.description, "First paragraph.\n\nSecond paragraph.");
    }

    #[test]
    fn append_normalizes_trailing_whitespace_before_the_blank_line() {
        let (_tmp, storage, id) = storage_with_description("First paragraph.\n\n");
        let issue = storage
            .append_to_issue(&id, EditField::Description, "Second paragraph.")
            .unwrap();
        // Exactly one blank line separates the two, regardless of prior trailing
        // newlines.
        assert_eq!(issue.description, "First paragraph.\n\nSecond paragraph.");
    }

    #[test]
    fn append_to_empty_field_has_no_leading_blank_line() {
        let (_tmp, storage, id) = storage_with_description("");
        let issue = storage
            .append_to_issue(&id, EditField::Design, "Design note.")
            .unwrap();
        assert_eq!(issue.design, "Design note.");
        // The default (description) field stays empty.
        assert_eq!(issue.description, "");
    }

    #[test]
    fn append_empty_text_is_rejected() {
        let (_tmp, storage, id) = storage_with_description("body");
        let err = storage
            .append_to_issue(&id, EditField::Description, "")
            .unwrap_err();
        assert!(err.to_string().contains("empty"), "unexpected: {err}");
        // Nothing was written.
        let issue = storage.get_issue(&id).unwrap().unwrap();
        assert_eq!(issue.description, "body");
    }
}

fn remap_issue(issue: &mut Issue, mapping: &HashMap<String, String>) -> bool {
    let mut changed = false;
    if let Some(new_id) = mapping.get(&issue.id) {
        if new_id != &issue.id {
            issue.id.clone_from(new_id);
            changed = true;
        }
    }
    issue.depends_on = std::mem::take(&mut issue.depends_on)
        .into_iter()
        .map(|(id, kind)| {
            if let Some(new_id) = mapping.get(&id) {
                changed |= new_id != &id;
                (new_id.clone(), kind)
            } else {
                (id, kind)
            }
        })
        .collect();
    for field in [
        &mut issue.title,
        &mut issue.description,
        &mut issue.design,
        &mut issue.acceptance_criteria,
        &mut issue.notes,
    ] {
        let replaced = replace_issue_ids_in_text(field, mapping);
        if replaced != *field {
            *field = replaced;
            changed = true;
        }
    }
    changed
}

// Own only the closed IDs so callers can move Issue values into their results.
pub(crate) fn closed_issue_ids(issues: &[Issue]) -> HashSet<String> {
    issues
        .iter()
        .filter(|issue| issue.status == Status::Closed)
        .map(|issue| issue.id.clone())
        .collect()
}

/// The "open blockers" rule used by `tz ready`: true if any `blocked_by`
/// target of `issue` is not in `closed_ids` (a missing target is unresolved).
pub fn has_unresolved_dependencies(issue: &Issue, closed_ids: &HashSet<String>) -> bool {
    if closed_ids.is_empty() {
        issue.has_blocking_dependencies()
    } else {
        // A missing target stays unresolved; only explicitly closed issues
        // discharge a blocking dependency.
        issue
            .get_blocking_dependencies()
            .any(|id| !closed_ids.contains(id.as_str()))
    }
}

/// Index issues by ID for ancestor lookups.
pub fn issues_by_id(issues: &[Issue]) -> HashMap<&str, &Issue> {
    issues.iter().map(|i| (i.id.as_str(), i)).collect()
}

/// Blockers `issue` inherits from its `child_of` ancestors.
///
/// Walks every ancestor (epics nest, and merges or hand edits can leave an
/// issue with several parents). Each ancestor is visited once, so a `child_of`
/// cycle terminates. An ancestor blocks when it is in `blocked` status or has
/// an unresolved `blocked_by` (target open or missing: the rule of
/// [`has_unresolved_dependencies`]). A closed ancestor no longer blocks, and a
/// parent missing from `by_id` contributes nothing. Ancestors are reported
/// nearest first, sorted by ID within a level.
pub fn inherited_blockers(
    issue: &Issue,
    by_id: &HashMap<&str, &Issue>,
    closed_ids: &HashSet<String>,
) -> Vec<InheritedBlocker> {
    let parents = |i: &Issue| -> Vec<String> {
        let mut ids: Vec<String> = i
            .depends_on
            .iter()
            .filter(|(_, ty)| **ty == DependencyType::ChildOf)
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    };
    let mut seen: HashSet<String> = HashSet::from([issue.id.clone()]);
    let mut level = parents(issue);
    let mut found = Vec::new();
    while !level.is_empty() {
        let mut next = Vec::new();
        for id in level {
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(ancestor) = by_id.get(id.as_str()) else {
                continue;
            };
            next.extend(parents(ancestor));
            if ancestor.status == Status::Closed {
                continue;
            }
            let mut blocked_by: Vec<String> = ancestor
                .get_blocking_dependencies()
                .filter(|t| !closed_ids.contains(t.as_str()))
                .cloned()
                .collect();
            blocked_by.sort();
            let blocked_status = ancestor.status == Status::Blocked;
            if !blocked_by.is_empty() || blocked_status {
                found.push(InheritedBlocker {
                    ancestor: id,
                    blocked_by,
                    blocked_status,
                });
            }
        }
        level = next;
    }
    found
}

/// The one "blocked" rule shared by `tz ready`, `tz blocked`, `tz stats` and
/// the TUI: the issue's own unresolved `blocked_by` links, or a blocker
/// inherited from an ancestor ([`inherited_blockers`]).
pub fn is_blocked_with_ancestors(
    issue: &Issue,
    by_id: &HashMap<&str, &Issue>,
    closed_ids: &HashSet<String>,
) -> bool {
    has_unresolved_dependencies(issue, closed_ids)
        || !inherited_blockers(issue, by_id, closed_ids).is_empty()
}

fn apply_issue_updates(issue: &mut Issue, updates: HashMap<String, String>) -> Result<()> {
    let now = chrono::Utc::now();
    // Role rule: only commands that change the role are checked, so a legacy
    // epic/spec that still has a role accepts every other update.
    if let Some(role) = updates.get("role") {
        if role.is_empty() {
            anyhow::ensure!(
                !issue.issue_type.takes_role(),
                "{}s need a role, so it cannot be cleared. Set another with --role human|agent",
                issue.issue_type
            );
        } else {
            role.parse::<Role>()?;
            anyhow::ensure!(
                issue.issue_type.takes_role(),
                "{}s take no role. To remove a stored role, use --clear-role",
                issue.issue_type
            );
        }
    }
    // Section rule (tz-51 §4): only the body sections this update changes are
    // checked, against the values they will have after it.
    for (key, field) in [
        ("description", EditField::Description),
        ("design", EditField::Design),
        ("acceptance_criteria", EditField::Acceptance),
        ("notes", EditField::Notes),
    ] {
        if let Some(value) = updates.get(key) {
            let old = issue.text_field_mut(field).clone();
            issue
                .issue_type
                .ensure_section_change(&issue.id, field, &old, value)?;
        }
    }
    for (key, value) in updates {
        match key.as_str() {
            "title" => issue.title = value,
            "description" => issue.description = value,
            "design" => issue.design = value,
            "notes" => issue.notes = value,
            "acceptance_criteria" => issue.acceptance_criteria = value,
            // Status changes go through dedicated commands (claim, close,
            // reopen) so each transition can be validated.
            "status" => anyhow::bail!("status cannot be set through update"),
            "priority" => issue.priority = value.parse()?,
            "issue_type" => issue.issue_type = value.parse()?,
            "assignee" => issue.assignee = value,
            "role" => {
                issue.role = if value.is_empty() {
                    None
                } else {
                    Some(value.parse()?)
                }
            }
            "effort" => {
                issue.effort = if value.is_empty() {
                    None
                } else {
                    Some(parse_effort(&value)?)
                }
            }
            "external_ref" => {
                issue.external_ref = if value.is_empty() { None } else { Some(value) }
            }
            _ => {}
        }
    }
    issue.updated_at = now;
    Ok(())
}

#[cfg(test)]
mod hold_tests {
    use super::*;

    fn storage_with_issue() -> (tempfile::TempDir, Storage, String) {
        let tmp = tempfile::tempdir().unwrap();
        let storage =
            Storage::init(tmp.path().join(".tazk"), false, IssueStorageLayout::Flat).unwrap();
        let id = storage
            .create_issue(
                "x".into(),
                String::new(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                Vec::new(),
                None,
                None,
            )
            .unwrap()
            .id;
        (tmp, storage, id)
    }

    #[test]
    fn block_and_release_with_reason() {
        let (_tmp, storage, id) = storage_with_issue();
        let issue = storage
            .set_hold(&id, Status::Blocked, false, Some("need keys"))
            .unwrap();
        assert_eq!(issue.status, Status::Blocked);
        assert!(issue.notes.starts_with("Blocked 20"), "{}", issue.notes);
        assert!(issue.notes.ends_with("Z: need keys"), "{}", issue.notes);

        let issue = storage
            .set_hold(&id, Status::Blocked, true, Some("got keys"))
            .unwrap();
        assert_eq!(issue.status, Status::Open);
        assert_eq!(issue.notes.matches("\n\n").count(), 1);
        assert!(issue.notes.contains("Unblocked 20"));
    }

    #[test]
    fn defer_without_reason_leaves_notes() {
        let (_tmp, storage, id) = storage_with_issue();
        let issue = storage
            .set_hold(&id, Status::Deferred, false, None)
            .unwrap();
        assert_eq!(issue.status, Status::Deferred);
        assert!(issue.notes.is_empty());
    }

    #[test]
    fn release_requires_matching_status_and_closed_refused() {
        let (_tmp, storage, id) = storage_with_issue();
        assert!(storage.set_hold(&id, Status::Blocked, true, None).is_err());
        storage
            .set_hold(&id, Status::Deferred, false, None)
            .unwrap();
        assert!(storage.set_hold(&id, Status::Blocked, true, None).is_err());
        storage.close_issue(&id, Some("done")).unwrap();
        assert!(storage.set_hold(&id, Status::Blocked, false, None).is_err());
    }
}

#[cfg(test)]
mod close_reason_tests {
    use super::*;

    #[test]
    fn close_and_reopen_reasons_append_to_notes() {
        let tmp = tempfile::tempdir().unwrap();
        let storage =
            Storage::init(tmp.path().join(".tazk"), false, IssueStorageLayout::Flat).unwrap();
        let id = storage
            .create_issue(
                "x".into(),
                String::new(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                Vec::new(),
                Some(Role::Agent),
                None,
            )
            .unwrap()
            .id;

        let issue = storage.close_issue(&id, None).unwrap();
        assert!(issue.notes.is_empty());
        let issue = storage.reopen_issue(&id, None, Some("  ")).unwrap();
        assert!(issue.notes.is_empty());

        storage.close_issue(&id, Some("fixed in #42")).unwrap();
        let issue = storage.reopen_issue(&id, None, Some("regressed")).unwrap();
        assert!(issue.notes.starts_with("Closed 20"), "{}", issue.notes);
        assert!(issue.notes.contains("Z: fixed in #42\n\nReopened 20"));
        assert!(issue.notes.ends_with("Z: regressed"));
    }
}

#[cfg(test)]
mod tolerant_loader_tests {
    use super::*;
    use crate::types::{DependencyType, IssueType};

    fn setup() -> (tempfile::TempDir, Storage) {
        let tmp = tempfile::tempdir().unwrap();
        let storage =
            Storage::init(tmp.path().join(".tazk"), false, IssueStorageLayout::Flat).unwrap();
        (tmp, storage)
    }

    fn write_issue(storage: &Storage, issue: &Issue) {
        let path = storage
            .get_tazk_dir()
            .join(format!("issues/{}.md", issue.id));
        fs::write(path, issue_to_markdown(issue).unwrap()).unwrap();
    }

    #[test]
    fn skips_bad_files_and_reports_them() {
        let (_tmp, storage) = setup();
        let dir = storage.get_tazk_dir().join("issues");
        write_issue(
            &storage,
            &Issue::new("tz-1".into(), "ok".into(), 2, IssueType::Task),
        );

        fs::write(dir.join("tz-2.md"), "---\ntitle: [unclosed\n---\n").unwrap();
        let good =
            issue_to_markdown(&Issue::new("tz-3".into(), "c".into(), 2, IssueType::Task)).unwrap();
        fs::write(
            dir.join("tz-3.md"),
            format!("{good}\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> other\n"),
        )
        .unwrap();
        let bad_effort =
            issue_to_markdown(&Issue::new("tz-4".into(), "e".into(), 2, IssueType::Task))
                .unwrap()
                .replacen("issue_type: task", "issue_type: task\neffort: 99", 1);
        fs::write(dir.join("tz-4.md"), bad_effort).unwrap();

        let loaded = storage.list_all_issues_tolerant().unwrap();
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.items[0].id, "tz-1");
        assert_eq!(loaded.skipped.len(), 3);
        let mut skipped = loaded.skipped.clone();
        skipped.sort_by(|a, b| a.path.cmp(&b.path));
        for (s, n) in skipped.iter().zip(["tz-2", "tz-3", "tz-4"]) {
            assert_eq!(s.path, dir.join(format!("{n}.md")));
            assert!(
                !s.error.is_empty() && !s.error.contains('\n'),
                "{}",
                s.error
            );
        }
        assert!(
            skipped[1].error.contains("merge conflict"),
            "{}",
            skipped[1].error
        );
        assert!(
            skipped[2].error.to_lowercase().contains("effort"),
            "{}",
            skipped[2].error
        );
    }

    #[test]
    fn dependents_cover_all_parsed_issues_including_closed() {
        let (_tmp, storage) = setup();
        let parent = Issue::new("tz-1".into(), "p".into(), 2, IssueType::Epic);
        let mut child = Issue::new("tz-2".into(), "c".into(), 2, IssueType::Task);
        child
            .depends_on
            .insert("tz-1".into(), DependencyType::ChildOf);
        let mut blocked = Issue::new("tz-3".into(), "b".into(), 2, IssueType::Task);
        blocked
            .depends_on
            .insert("tz-2".into(), DependencyType::BlockedBy);
        blocked.status = Status::Closed;
        for i in [&parent, &child, &blocked] {
            write_issue(&storage, i);
        }

        let loaded = storage.list_all_issues_tolerant().unwrap();
        assert!(loaded.skipped.is_empty());
        assert_eq!(loaded.items.len(), 3);
        let get = |id: &str| loaded.items.iter().find(|i| i.id == id).unwrap();
        assert_eq!(get("tz-1").dependents.len(), 1);
        assert_eq!(get("tz-1").dependents[0].id, "tz-2");
        assert_eq!(get("tz-2").dependents[0].id, "tz-3");
        assert_eq!(get("tz-3").status, Status::Closed);

        let closed: HashSet<String> = ["tz-2".to_string()].into();
        assert!(has_unresolved_dependencies(get("tz-3"), &HashSet::new()));
        assert!(!has_unresolved_dependencies(get("tz-3"), &closed));
    }

    #[test]
    fn works_on_readonly_storage_and_leaves_no_lock_behind() {
        let (_tmp, storage) = setup();
        write_issue(
            &storage,
            &Issue::new("tz-1".into(), "ok".into(), 2, IssueType::Task),
        );
        let ro = Storage::open_readonly(storage.get_tazk_dir()).unwrap();
        assert_eq!(ro.list_all_issues_tolerant().unwrap().items.len(), 1);
        assert!(!storage.get_tazk_dir().join("tazk.lock").exists());
    }
}

#[cfg(test)]
mod single_parent_tests {
    use super::*;

    fn storage_with(count: usize) -> (tempfile::TempDir, Storage, Vec<String>) {
        let tmp = tempfile::tempdir().unwrap();
        let storage =
            Storage::init(tmp.path().join(".tazk"), false, IssueStorageLayout::Flat).unwrap();
        let ids = (0..count)
            .map(|i| {
                storage
                    .create_issue(
                        format!("issue {i}"),
                        String::new(),
                        None,
                        None,
                        2,
                        IssueType::Task,
                        None,
                        Vec::new(),
                        None,
                        Vec::new(),
                        Some(Role::Agent),
                        None,
                    )
                    .unwrap()
                    .id
            })
            .collect();
        (tmp, storage, ids)
    }

    #[test]
    fn dep_add_rejects_a_second_parent() {
        let (_tmp, storage, ids) = storage_with(3);
        storage
            .add_dependency(&ids[0], &ids[1], DependencyType::ChildOf)
            .unwrap();
        // Re-adding the same parent is fine.
        storage
            .add_dependency(&ids[0], &ids[1], DependencyType::ChildOf)
            .unwrap();
        let err = storage
            .add_dependency(&ids[0], &ids[2], DependencyType::ChildOf)
            .unwrap_err();
        assert!(err.to_string().contains("already has a parent"), "{err}");
        // Other dependency types are unaffected.
        storage
            .add_dependency(&ids[0], &ids[2], DependencyType::BlockedBy)
            .unwrap();
        let issue = storage.get_issue(&ids[0]).unwrap().unwrap();
        assert_eq!(issue.depends_on[&ids[1]], DependencyType::ChildOf);
        assert_eq!(issue.depends_on[&ids[2]], DependencyType::BlockedBy);
    }

    #[test]
    fn retyping_the_parent_then_adding_another_is_allowed() {
        let (_tmp, storage, ids) = storage_with(3);
        storage
            .add_dependency(&ids[0], &ids[1], DependencyType::ChildOf)
            .unwrap();
        storage
            .add_dependency(&ids[0], &ids[1], DependencyType::BlockedBy)
            .unwrap();
        storage
            .add_dependency(&ids[0], &ids[2], DependencyType::ChildOf)
            .unwrap();
    }

    #[test]
    fn create_rejects_two_parents_and_writes_nothing() {
        let (_tmp, storage, ids) = storage_with(2);
        let before = storage.list_all_issues_tolerant().unwrap().items.len();
        let result = storage.create_issue(
            "child".into(),
            String::new(),
            None,
            None,
            2,
            IssueType::Task,
            None,
            Vec::new(),
            None,
            vec![
                (ids[0].clone(), DependencyType::ChildOf),
                (ids[1].clone(), DependencyType::ChildOf),
            ],
            Some(Role::Agent),
            None,
        );
        assert!(result.is_err());
        assert_eq!(
            storage.list_all_issues_tolerant().unwrap().items.len(),
            before
        );
    }
}

#[cfg(test)]
mod inherited_blocker_tests {
    use super::*;
    use crate::types::{DependencyType, IssueType};

    fn setup() -> (tempfile::TempDir, Storage) {
        let tmp = tempfile::tempdir().unwrap();
        let storage =
            Storage::init(tmp.path().join(".tazk"), false, IssueStorageLayout::Flat).unwrap();
        (tmp, storage)
    }

    /// Write `(id, status, [(target, type)])` issues straight to disk, so
    /// cycles, several parents and missing targets can be set up.
    fn fixture(
        specs: &[(&str, Status, &[(&str, DependencyType)])],
    ) -> (tempfile::TempDir, Storage) {
        let (tmp, storage) = setup();
        for (id, status, deps) in specs {
            let mut issue = Issue::new((*id).into(), format!("issue {id}"), 2, IssueType::Task);
            issue.status = *status;
            for (target, ty) in *deps {
                issue.depends_on.insert((*target).into(), *ty);
            }
            let path = storage.get_tazk_dir().join(format!("issues/{id}.md"));
            fs::write(path, issue_to_markdown(&issue).unwrap()).unwrap();
        }
        (tmp, storage)
    }

    fn ready_ids(storage: &Storage) -> Vec<String> {
        let mut ids: Vec<String> = storage
            .get_ready(None, None, None, "oldest")
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        ids.sort();
        ids
    }

    fn blocked_item(storage: &Storage, id: &str) -> Option<BlockedIssue> {
        storage
            .get_blocked()
            .unwrap()
            .into_iter()
            .find(|b| b.issue.id == id)
    }

    use DependencyType::{BlockedBy, ChildOf};
    use Status::{Blocked, Closed, Open};

    #[test]
    fn blocked_direct_parent_blocks_child() {
        let (_t, s) = fixture(&[
            ("tz-1", Open, &[("tz-2", BlockedBy)]),
            ("tz-2", Open, &[]),
            ("tz-3", Open, &[("tz-1", ChildOf)]),
        ]);
        assert_eq!(ready_ids(&s), vec!["tz-2"]);
        let item = blocked_item(&s, "tz-3").unwrap();
        assert!(item.blocked_by.is_empty());
        assert_eq!(
            item.inherited_blocked_by,
            vec![InheritedBlocker {
                ancestor: "tz-1".into(),
                blocked_by: vec!["tz-2".into()],
                blocked_status: false,
            }]
        );
        assert_eq!(item.blocked_by_count, 1);
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["inherited_blocked_by"][0]["ancestor"], "tz-1");
        assert_eq!(json["inherited_blocked_by"][0]["blocked_by"][0], "tz-2");
        let stats = s.get_stats().unwrap();
        assert_eq!(stats.blocked_issues, 2);
        assert_eq!(stats.ready_issues, 1);
    }

    #[test]
    fn blocked_grandparent_blocks_grandchild() {
        let (_t, s) = fixture(&[
            ("tz-1", Open, &[("tz-9", BlockedBy)]),
            ("tz-2", Open, &[("tz-1", ChildOf)]),
            ("tz-3", Open, &[("tz-2", ChildOf)]),
            ("tz-9", Open, &[]),
        ]);
        assert_eq!(ready_ids(&s), vec!["tz-9"]);
        let item = blocked_item(&s, "tz-3").unwrap();
        assert_eq!(item.inherited_blocked_by.len(), 1);
        assert_eq!(item.inherited_blocked_by[0].ancestor, "tz-1");
    }

    #[test]
    fn closing_the_ancestor_blocker_unblocks_descendants() {
        let (_t, s) = fixture(&[
            ("tz-1", Open, &[("tz-2", BlockedBy)]),
            ("tz-2", Closed, &[]),
            ("tz-3", Open, &[("tz-1", ChildOf)]),
        ]);
        assert_eq!(ready_ids(&s), vec!["tz-1", "tz-3"]);
        assert!(blocked_item(&s, "tz-3").is_none());
    }

    #[test]
    fn missing_ancestor_blocker_target_blocks() {
        let (_t, s) = fixture(&[
            ("tz-1", Open, &[("tz-404", BlockedBy)]),
            ("tz-3", Open, &[("tz-1", ChildOf)]),
        ]);
        assert!(ready_ids(&s).is_empty());
        let item = blocked_item(&s, "tz-3").unwrap();
        assert_eq!(item.inherited_blocked_by[0].blocked_by, vec!["tz-404"]);
    }

    #[test]
    fn ancestor_in_blocked_status_blocks() {
        let (_t, s) = fixture(&[("tz-1", Blocked, &[]), ("tz-3", Open, &[("tz-1", ChildOf)])]);
        assert!(ready_ids(&s).is_empty());
        let item = blocked_item(&s, "tz-3").unwrap();
        assert!(item.blocked_by.is_empty());
        assert_eq!(item.inherited_blocked_by[0].ancestor, "tz-1");
        assert!(item.inherited_blocked_by[0].blocked_status);
        assert_eq!(item.blocked_by_count, 1);
    }

    #[test]
    fn closed_ancestor_does_not_block() {
        let (_t, s) = fixture(&[
            ("tz-1", Closed, &[("tz-404", BlockedBy)]),
            ("tz-3", Open, &[("tz-1", ChildOf)]),
        ]);
        assert_eq!(ready_ids(&s), vec!["tz-3"]);
    }

    #[test]
    fn child_of_cycle_terminates() {
        let (_t, s) = fixture(&[
            ("tz-1", Open, &[("tz-2", ChildOf)]),
            ("tz-2", Open, &[("tz-1", ChildOf), ("tz-9", BlockedBy)]),
            ("tz-3", Open, &[("tz-3", ChildOf)]),
            ("tz-9", Open, &[]),
        ]);
        // tz-1 inherits tz-2's blocker; tz-2 is blocked directly (and via
        // tz-1, which is not blocked itself); the self-cycle is ready.
        assert_eq!(ready_ids(&s), vec!["tz-3", "tz-9"]);
        assert!(blocked_item(&s, "tz-1").is_some());
        assert!(blocked_item(&s, "tz-3").is_none());
        let _ = s.get_stats().unwrap();
    }

    #[test]
    fn any_blocked_parent_of_several_blocks() {
        let (_t, s) = fixture(&[
            ("tz-1", Open, &[]),
            ("tz-2", Open, &[("tz-9", BlockedBy)]),
            ("tz-3", Open, &[("tz-1", ChildOf), ("tz-2", ChildOf)]),
            ("tz-4", Open, &[("tz-1", ChildOf)]),
            ("tz-9", Open, &[]),
        ]);
        assert_eq!(ready_ids(&s), vec!["tz-1", "tz-4", "tz-9"]);
        let item = blocked_item(&s, "tz-3").unwrap();
        assert_eq!(item.inherited_blocked_by[0].ancestor, "tz-2");
    }

    #[test]
    fn direct_and_inherited_blockers_are_reported_separately() {
        let (_t, s) = fixture(&[
            ("tz-1", Open, &[("tz-9", BlockedBy)]),
            ("tz-3", Open, &[("tz-1", ChildOf), ("tz-8", BlockedBy)]),
            ("tz-8", Open, &[]),
            ("tz-9", Open, &[]),
        ]);
        let item = blocked_item(&s, "tz-3").unwrap();
        assert_eq!(item.blocked_by, vec!["tz-8"]);
        assert_eq!(item.inherited_blocked_by.len(), 1);
        assert_eq!(item.blocked_by_count, 2);
    }
}

#[cfg(test)]
mod body_section_tests {
    //! Per-type body section rules on `create` and `triage promote` (tz-51).
    use super::*;

    fn storage() -> (tempfile::TempDir, Storage) {
        let tmp = tempfile::tempdir().unwrap();
        let storage =
            Storage::init(tmp.path().join(".tazk"), false, IssueStorageLayout::Flat).unwrap();
        (tmp, storage)
    }

    fn fields(
        issue_type: IssueType,
        description: &str,
        design: Option<&str>,
        acceptance: Option<&str>,
    ) -> NewIssue {
        NewIssue {
            title: "T".to_string(),
            description: description.to_string(),
            design: design.map(str::to_string),
            acceptance: acceptance.map(str::to_string),
            priority: 2,
            issue_type,
            assignee: None,
            labels: Vec::new(),
            external_ref: None,
            deps: Vec::new(),
            role: issue_type.takes_role().then_some(Role::Agent),
            effort: None,
        }
    }

    /// How an issue is made: `create`, or `triage promote` of one item.
    #[derive(Clone, Copy, Debug)]
    enum Path {
        Create,
        Promote,
    }

    const PATHS: [Path; 2] = [Path::Create, Path::Promote];

    fn make(storage: &Storage, path: Path, f: NewIssue) -> Result<Issue> {
        match path {
            Path::Create => storage.create_issue(
                f.title,
                f.description,
                f.design,
                f.acceptance,
                f.priority,
                f.issue_type,
                f.assignee,
                f.labels,
                f.external_ref,
                f.deps,
                f.role,
                f.effort,
            ),
            Path::Promote => {
                let item = storage.add_triage_item("item text").unwrap();
                storage.promote_triage_items(&[item.name], f)
            }
        }
    }

    /// Assert `make` fails with `needle` in the message and writes nothing.
    fn assert_rejected(path: Path, f: NewIssue, needle: &str) {
        let (_tmp, storage) = storage();
        let items_before = if let Path::Promote = path {
            storage.add_triage_item("kept").unwrap();
            1
        } else {
            0
        };
        let err = match path {
            Path::Create => make(&storage, path, f),
            Path::Promote => {
                let names = storage.list_triage_items().unwrap();
                storage.promote_triage_items(&[names[0].name.clone()], f)
            }
        }
        .unwrap_err();
        assert!(err.to_string().contains(needle), "{path:?}: {err}");
        assert!(
            storage
                .list_issues(None, None, None, None, None)
                .unwrap()
                .is_empty(),
            "{path:?}: nothing should be written"
        );
        assert_eq!(storage.list_triage_items().unwrap().len(), items_before);
        // The rejected command did not use up an ID.
        let ok = storage
            .create_issue(
                "ok".to_string(),
                String::new(),
                None,
                None,
                2,
                IssueType::Task,
                None,
                Vec::new(),
                None,
                Vec::new(),
                Some(Role::Agent),
                None,
            )
            .unwrap();
        assert_eq!(ok.id, "tz-1");
    }

    #[test]
    fn design_is_forbidden_on_epic_spec_and_gate() {
        for path in PATHS {
            for ty in [IssueType::Epic, IssueType::Spec, IssueType::Gate] {
                assert_rejected(
                    path,
                    fields(ty, "Body", Some("approach"), Some("Check it")).tap_acceptance(ty),
                    "Design",
                );
            }
        }
    }

    #[test]
    fn acceptance_is_forbidden_on_spec() {
        for path in PATHS {
            assert_rejected(
                path,
                fields(IssueType::Spec, "Body", None, Some("criteria")),
                "specs have no Design or Acceptance Criteria",
            );
        }
    }

    #[test]
    fn spec_and_gate_need_a_description() {
        // A promoted issue defaults a blank description to the item text, so
        // only `create` can be rejected for this.
        for ty in [IssueType::Spec, IssueType::Gate] {
            for desc in ["", "  \n\t"] {
                assert_rejected(
                    Path::Create,
                    fields(ty, desc, None, Some("Check it")).tap_acceptance(ty),
                    &format!("A {ty} needs a Description"),
                );
            }
        }
        let (_tmp, storage) = storage();
        let item = storage.add_triage_item("item text").unwrap();
        let spec = storage
            .promote_triage_items(&[item.name], fields(IssueType::Spec, " ", None, None))
            .unwrap();
        assert_eq!(spec.description, "item text");
    }

    #[test]
    fn gate_needs_acceptance() {
        for path in PATHS {
            for acceptance in [None, Some(""), Some(" \n")] {
                assert_rejected(
                    path,
                    fields(IssueType::Gate, "Condition", None, acceptance),
                    "A gate needs Acceptance Criteria",
                );
            }
        }
    }

    #[test]
    fn valid_spec_and_gate_are_created() {
        for path in PATHS {
            let (_tmp, storage) = storage();
            let spec = make(
                &storage,
                path,
                fields(IssueType::Spec, "The spec", Some("  "), Some("")),
            )
            .unwrap();
            assert_eq!(spec.issue_type, IssueType::Spec);
            assert_eq!(spec.acceptance_criteria, "");
            let gate = make(
                &storage,
                path,
                fields(IssueType::Gate, "Condition", None, Some("Check it")),
            )
            .unwrap();
            assert_eq!(gate.acceptance_criteria, "Check it");
        }
    }

    #[test]
    fn epic_gets_default_acceptance() {
        for path in PATHS {
            for acceptance in [None, Some(""), Some("  \n")] {
                let (_tmp, storage) = storage();
                let epic = make(
                    &storage,
                    path,
                    fields(IssueType::Epic, "", None, acceptance),
                )
                .unwrap();
                assert_eq!(epic.acceptance_criteria, "- All child issues closed.");
                let stored = storage.get_issue(&epic.id).unwrap().unwrap();
                assert_eq!(stored.acceptance_criteria, "- All child issues closed.");
            }
        }
    }

    #[test]
    fn explicit_epic_acceptance_replaces_the_default() {
        for path in PATHS {
            let (_tmp, storage) = storage();
            let epic = make(
                &storage,
                path,
                fields(IssueType::Epic, "", None, Some("- Ships in prod.")),
            )
            .unwrap();
            assert_eq!(epic.acceptance_criteria, "- Ships in prod.");
        }
    }

    #[test]
    fn work_types_are_unchanged() {
        for path in PATHS {
            for ty in [
                IssueType::Task,
                IssueType::Bug,
                IssueType::Feature,
                IssueType::Chore,
            ] {
                let (_tmp, storage) = storage();
                let bare = make(&storage, path, fields(ty, "", None, None)).unwrap();
                assert_eq!(bare.design, "");
                assert_eq!(bare.acceptance_criteria, "");
                let full =
                    make(&storage, path, fields(ty, "d", Some("design"), Some("acc"))).unwrap();
                assert_eq!(full.design, "design");
                assert_eq!(full.acceptance_criteria, "acc");
            }
        }
    }

    trait TapAcceptance {
        /// Give a gate the acceptance criteria it needs, so a test isolates
        /// the section it is about.
        fn tap_acceptance(self, ty: IssueType) -> Self;
    }

    impl TapAcceptance for NewIssue {
        fn tap_acceptance(mut self, ty: IssueType) -> Self {
            self.acceptance = match ty {
                IssueType::Gate => Some("Check it".to_string()),
                _ => None,
            };
            self
        }
    }
}
