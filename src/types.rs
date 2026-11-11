use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

/// The one prefix every issue ID starts with (`tz-1`, `tz-a1b2c`).
pub const ID_PREFIX: &str = "tz-";

/// Issue status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    InProgress,
    Closed,
    Blocked,
    Deferred,
}

impl Status {
    /// Get the string representation of this status
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::InProgress => "in_progress",
            Status::Closed => "closed",
            Status::Blocked => "blocked",
            Status::Deferred => "deferred",
        }
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Status {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(Status::Open),
            "in_progress" => Ok(Status::InProgress),
            "closed" => Ok(Status::Closed),
            "blocked" => Ok(Status::Blocked),
            "deferred" => Ok(Status::Deferred),
            _ => Err(anyhow::anyhow!(
                "Invalid status: '{}'. Valid values are: open, in_progress, closed, blocked, deferred",
                s
            )),
        }
    }
}

/// Issue type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueType {
    Bug,
    Feature,
    Task,
    Epic,
    Chore,
    Gate,
    Spec,
}

impl IssueType {
    /// Get the string representation of this issue type
    pub fn as_str(&self) -> &'static str {
        match self {
            IssueType::Bug => "bug",
            IssueType::Feature => "feature",
            IssueType::Task => "task",
            IssueType::Epic => "epic",
            IssueType::Chore => "chore",
            IssueType::Gate => "gate",
            IssueType::Spec => "spec",
        }
    }
}

impl std::fmt::Display for IssueType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for IssueType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "bug" => Ok(IssueType::Bug),
            "feature" => Ok(IssueType::Feature),
            "task" => Ok(IssueType::Task),
            "epic" => Ok(IssueType::Epic),
            "chore" => Ok(IssueType::Chore),
            "gate" => Ok(IssueType::Gate),
            "spec" => Ok(IssueType::Spec),
            _ => Err(anyhow::anyhow!(
                "Invalid issue type: '{}'. Valid values are: bug, feature, task, epic, gate, chore, spec",
                s
            )),
        }
    }
}

/// Who an issue is intended for: a person or an automated agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Human,
    Agent,
}

impl Role {
    /// Get the string representation of this role
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Human => "human",
            Role::Agent => "agent",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Role {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "human" => Ok(Role::Human),
            "agent" => Ok(Role::Agent),
            _ => Err(anyhow::anyhow!(
                "Invalid role: '{}'. Valid values are: human, agent",
                s
            )),
        }
    }
}

/// Lowest valid [`Issue::effort`] value.
pub const MIN_EFFORT: u8 = 1;
/// Highest valid [`Issue::effort`] value.
pub const MAX_EFFORT: u8 = 5;

/// Parse and validate an effort estimate (an integer from [`MIN_EFFORT`] to
/// [`MAX_EFFORT`], where the maximum is the most effort).
pub fn parse_effort(s: &str) -> anyhow::Result<u8> {
    let value: u8 = s.trim().parse().map_err(|_| {
        anyhow::anyhow!(
            "Invalid effort: '{}'. Must be an integer from {} to {}",
            s,
            MIN_EFFORT,
            MAX_EFFORT
        )
    })?;
    validate_effort(value)?;
    Ok(value)
}

/// Check that an already-numeric effort estimate is in range.
pub fn validate_effort(value: u8) -> anyhow::Result<()> {
    if !(MIN_EFFORT..=MAX_EFFORT).contains(&value) {
        anyhow::bail!(
            "Invalid effort: '{}'. Must be an integer from {} to {}",
            value,
            MIN_EFFORT,
            MAX_EFFORT
        );
    }
    Ok(())
}

/// A free-text field of an issue that a targeted search/replace edit can rewrite.
///
/// `tz update --search/--replace` swaps a substring of one of these fields rather
/// than overwriting the whole field, which is the safer interface for agents
/// editing long descriptions (the "aider" search/replace pattern). We use an
/// explicit enum rather than a bare string key so the set of editable fields is
/// closed and checked at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditField {
    Title,
    Description,
    Design,
    Notes,
    Acceptance,
}

impl EditField {
    /// Canonical, user-facing name of this field.
    pub fn as_str(&self) -> &'static str {
        match self {
            EditField::Title => "title",
            EditField::Description => "description",
            EditField::Design => "design",
            EditField::Notes => "notes",
            EditField::Acceptance => "acceptance",
        }
    }
}

impl std::fmt::Display for EditField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for EditField {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "title" => Ok(EditField::Title),
            "description" | "desc" => Ok(EditField::Description),
            "design" => Ok(EditField::Design),
            "notes" => Ok(EditField::Notes),
            "acceptance" | "acceptance_criteria" => Ok(EditField::Acceptance),
            _ => Err(anyhow::anyhow!(
                "Invalid field: '{}'. Valid values are: title, description, design, notes, acceptance",
                s
            )),
        }
    }
}

/// Dependency type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DependencyType {
    #[serde(rename = "blocked_by")]
    BlockedBy,
    #[serde(rename = "related_to")]
    RelatedTo,
    #[serde(rename = "child_of")]
    ChildOf,
    #[serde(rename = "discovered_from", alias = "discovered-from")]
    DiscoveredFrom,
    #[serde(rename = "tracks")]
    Tracks,
}

impl DependencyType {
    /// Get the string representation of this dependency type
    pub fn as_str(&self) -> &'static str {
        match self {
            DependencyType::BlockedBy => "blocked_by",
            DependencyType::RelatedTo => "related_to",
            DependencyType::ChildOf => "child_of",
            DependencyType::DiscoveredFrom => "discovered_from",
            DependencyType::Tracks => "tracks",
        }
    }
}

impl std::fmt::Display for DependencyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for DependencyType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "blocked_by" => Ok(DependencyType::BlockedBy),
            "related_to" => Ok(DependencyType::RelatedTo),
            "child_of" => Ok(DependencyType::ChildOf),
            "discovered_from" | "discovered-from" => Ok(DependencyType::DiscoveredFrom),
            "tracks" => Ok(DependencyType::Tracks),
            _ => Err(anyhow::anyhow!(
                "Invalid dependency type: '{}'. Valid values are: blocked_by, related_to, child_of, discovered_from, tracks",
                s
            )),
        }
    }
}

/// Dependency representation for JSON output
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    /// The issue this one depends on.
    pub id: String,
    #[serde(rename = "type")]
    pub dep_type: String,
}

/// File-backed issue comment.
///
/// Comments are stored outside the markdown issue body so they do not churn
/// the main issue description.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub issue_id: String,
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Custom serialization for depends_on HashMap -> dependencies array, sorted by
/// ID so exports are deterministic.
fn serialize_dependencies<S>(
    map: &HashMap<String, DependencyType>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut sorted: Vec<(&String, &DependencyType)> = map.iter().collect();
    sorted.sort_unstable_by_key(|(id, _)| *id);
    let deps: Vec<Dependency> = sorted
        .into_iter()
        .map(|(id, dep_type)| Dependency {
            id: id.clone(),
            dep_type: dep_type.to_string(),
        })
        .collect();
    deps.serialize(serializer)
}

/// Helper enum for deserializing either old HashMap or new array format
#[derive(Deserialize)]
#[serde(untagged)]
enum DependenciesFormat {
    Array(Vec<Dependency>),
    Map(HashMap<String, DependencyType>),
}

/// Custom deserialization for dependencies array -> depends_on HashMap
fn deserialize_dependencies<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, DependencyType>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    match DependenciesFormat::deserialize(deserializer)? {
        DependenciesFormat::Array(deps) => {
            let mut map = HashMap::new();
            for dep in deps {
                let dep_type = dep.dep_type.parse::<DependencyType>().map_err(|_| {
                    Error::custom(format!("Invalid dependency type: {}", dep.dep_type))
                })?;
                map.insert(dep.id, dep_type);
            }
            Ok(map)
        }
        DependenciesFormat::Map(map) => Ok(map),
    }
}

/// Issue structure
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub design: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub acceptance_criteria: String,
    pub status: Status,
    pub priority: i32,
    pub issue_type: IssueType,
    #[serde(default)]
    pub assignee: String,
    /// Whether the issue is meant for a human or an agent. `None` if unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    /// Effort estimate from 1 to 5 (5 is the most effort). `None` if unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<u8>,
    pub external_ref: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(
        default,
        rename = "dependencies",
        serialize_with = "serialize_dependencies",
        deserialize_with = "deserialize_dependencies"
    )]
    pub depends_on: HashMap<String, DependencyType>,
    #[serde(default, skip_deserializing)]
    pub dependents: Vec<Dependency>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

impl Issue {
    pub fn new(id: String, title: String, priority: i32, issue_type: IssueType) -> Self {
        let now = Utc::now();
        Self {
            id,
            title,
            description: String::new(),
            design: String::new(),
            notes: String::new(),
            acceptance_criteria: String::new(),
            status: Status::Open,
            priority,
            issue_type,
            assignee: String::new(),
            role: None,
            effort: None,
            external_ref: None,
            labels: Vec::new(),
            depends_on: HashMap::new(),
            dependents: Vec::new(),
            created_at: now,
            updated_at: now,
            closed_at: None,
        }
    }

    /// Mutable access to one of the issue's free-text fields, selected by
    /// [`EditField`]. Used by targeted search/replace edits so the field name
    /// validated at the CLI maps to exactly one storage location.
    pub fn text_field_mut(&mut self, field: EditField) -> &mut String {
        match field {
            EditField::Title => &mut self.title,
            EditField::Description => &mut self.description,
            EditField::Design => &mut self.design,
            EditField::Notes => &mut self.notes,
            EditField::Acceptance => &mut self.acceptance_criteria,
        }
    }

    /// Get dependencies of a specific type
    /// Returns an iterator to avoid unnecessary allocations
    pub fn get_blocking_dependencies(&self) -> impl Iterator<Item = &String> + '_ {
        self.depends_on
            .iter()
            .filter(|(_, dep_type)| **dep_type == DependencyType::BlockedBy)
            .map(|(id, _)| id)
    }

    /// Check if there are any blocking dependencies (zero-cost check)
    pub fn has_blocking_dependencies(&self) -> bool {
        self.depends_on
            .values()
            .any(|dep_type| *dep_type == DependencyType::BlockedBy)
    }
}

/// Statistics structure
#[derive(Debug, Serialize, Deserialize)]
pub struct Stats {
    pub total_issues: usize,
    pub open_issues: usize,
    pub in_progress_issues: usize,
    pub blocked_issues: usize,
    pub closed_issues: usize,
    pub ready_issues: usize,
    pub average_lead_time_hours: f64,
}

/// Blocked issue structure (for blocked command)
#[derive(Debug, Serialize, Deserialize)]
pub struct BlockedIssue {
    #[serde(flatten)]
    pub issue: Issue,
    pub blocked_by: Vec<String>,
    pub blocked_by_count: usize,
}

/// Tree node for dependency tree visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub id: String,
    pub title: String,
    pub status: Status,
    pub priority: i32,
    pub dep_type: Option<String>,
    pub children: Vec<TreeNode>,
    pub is_cycle: bool,
    pub depth_exceeded: bool,
}
