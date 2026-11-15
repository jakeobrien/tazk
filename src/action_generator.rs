//! Random action generator for property-based testing
//!
//! This module provides a reusable library for generating and executing
//! random sequences of tz commands against the tazk binary.
//!
//! ## Key Assumption: Sequential Issue Numbering
//!
//! **IMPORTANT**: This generator assumes that tazk always numbers
//! new issues sequentially as `N+1` where `N` is the maximum previous issue ID.
//!
//! This assumption allows us to:
//! - Generate valid action sequences without executing commands
//! - Predict which issue IDs will exist at any point in the sequence
//! - Generate contextually valid actions (e.g., only update issues that exist)
//! - Verify correctness by checking that created issues match predictions
//!
//! ### Verification Strategy
//!
//! When executing action sequences, we VERIFY this assumption by:
//! 1. Each `Create` action includes an `expected_id` field
//! 2. After executing `tz create`, we parse the output to extract the actual issue ID
//! 3. We assert that `actual_id == expected_id`
//! 4. If the assertion fails, the test fails with a clear error message
//!
//! This verification ensures that:
//! - Our model of issue state matches reality
//! - The implementation follows sequential numbering
//! - All subsequent actions operate on the correct issues
//!
//! ### Example
//!
//! ```text
//! Action sequence:
//!   1. Init { tz_hash_ids: Some(false) }
//!   2. Create { expected_id: "tz-1", ...}  →  Verify actual ID is "tz-1"
//!   3. Create { expected_id: "tz-2", ... } →  Verify actual ID is "tz-2"
//!   4. Update { issue_id: "tz-1", ... }     →  Valid because tz-1 exists
//!   5. Close { issue_id: "tz-2", ... }      →  Valid because tz-2 exists
//! ```

use anyhow::{Context, Result};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashSet;
use std::process::Command;

// Re-export production types instead of defining duplicates
use crate::types::ID_PREFIX;
pub use crate::types::{DependencyType, IssueType, Status};

/// Represents a tazk command/action
#[derive(Debug, Clone, PartialEq)]
pub enum TazkAction {
    /// Initialize tazk in directory
    Init { tz_hash_ids: Option<bool> },

    /// Create a new issue with expected ID for verification
    Create {
        expected_id: String,
        title: String,
        priority: i32,
        issue_type: IssueType,
        description: Option<String>,
    },

    /// List issues with optional filters
    List {
        status: Option<Status>,
        priority: Option<i32>,
    },

    /// Show a specific issue
    Show { issue_id: String },

    /// Update an issue
    Update {
        issue_id: String,
        priority: Option<i32>,
    },

    /// Close an issue
    Close { issue_id: String, reason: String },

    /// Reopen an issue
    Reopen { issue_id: String },

    /// Add a dependency
    AddDependency {
        issue_id: String,
        depends_on: String,
        dep_type: DependencyType,
    },

    /// Export to JSONL
    Export { output: String },
}

impl std::fmt::Display for TazkAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TazkAction::Init { tz_hash_ids } => {
                let mut parts = vec!["init".to_string()];
                if let Some(true) = tz_hash_ids {
                    parts.push("--tz-hash-ids".to_string());
                }
                write!(f, "{}", parts.join(" "))
            }
            TazkAction::Create {
                expected_id,
                priority,
                issue_type,
                ..
            } => write!(
                f,
                "create {} (p:{}, type:{})",
                expected_id,
                priority,
                issue_type.as_str()
            ),
            TazkAction::List { status, priority } => {
                let mut parts = vec!["list".to_string()];
                if let Some(s) = status {
                    parts.push(format!("status:{}", s.as_str()));
                }
                if let Some(p) = priority {
                    parts.push(format!("priority:{}", p));
                }
                write!(f, "{}", parts.join(" "))
            }
            TazkAction::Show { issue_id } => write!(f, "show {}", issue_id),
            TazkAction::Update { issue_id, priority } => {
                let mut parts = vec![format!("update {}", issue_id)];
                if let Some(p) = priority {
                    parts.push(format!("priority:{}", p));
                }
                write!(f, "{}", parts.join(" "))
            }
            TazkAction::Close { issue_id, .. } => write!(f, "close {}", issue_id),
            TazkAction::Reopen { issue_id } => write!(f, "reopen {}", issue_id),
            TazkAction::AddDependency {
                issue_id,
                depends_on,
                dep_type,
            } => write!(
                f,
                "dep add {} → {} ({})",
                issue_id,
                depends_on,
                dep_type.as_str()
            ),
            TazkAction::Export { output } => write!(f, "export to {}", output),
        }
    }
}

/// Generates random action sequences
///
/// Maintains state to ensure generated actions are contextually valid:
/// - Tracks which issues exist (for Update, Show, Close, etc.)
/// - Tracks which issues are closed (for Reopen)
/// - Predicts issue IDs using sequential numbering assumption
pub struct ActionGenerator {
    rng: StdRng,
    next_issue_num: usize,
    existing_issues: Vec<String>, // Maintains creation order
    closed_issues: HashSet<String>,
    use_hash_ids: bool,
}

impl ActionGenerator {
    /// Create a new generator with the given seed
    pub fn new(seed: u64) -> Self {
        Self::new_with_mode(seed, false)
    }

    /// Create a new generator with explicit hash ID mode
    pub fn new_with_mode(seed: u64, use_hash_ids: bool) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
            next_issue_num: 1,
            existing_issues: Vec::new(),
            closed_issues: HashSet::new(),
            use_hash_ids,
        }
    }

    /// Generate a sequence of random actions
    ///
    /// Always starts with Init, then generates num_actions random valid actions
    pub fn generate_sequence(&mut self, num_actions: usize) -> Vec<TazkAction> {
        let mut actions = Vec::new();

        // Always start with init
        actions.push(TazkAction::Init {
            tz_hash_ids: Some(self.use_hash_ids),
        });

        // Generate random actions
        for _ in 0..num_actions {
            actions.push(self.generate_action());
        }

        actions
    }

    /// Generate a single random action based on current state
    fn generate_action(&mut self) -> TazkAction {
        // Weight actions based on what makes sense
        let action_type = if self.existing_issues.is_empty() {
            // If no issues exist, must create one
            0
        } else {
            // Otherwise, pick randomly
            // Bias towards creating issues (30% chance) to build up state
            let rand_val = self.rng.gen_range(0..100);
            if rand_val < 30 {
                0 // Create
            } else if rand_val < 45 {
                1 // List
            } else if rand_val < 55 {
                2 // Show
            } else if rand_val < 70 {
                3 // Update
            } else if rand_val < 80 {
                4 // Close
            } else if rand_val < 85 {
                5 // Reopen
            } else if rand_val < 95 {
                6 // AddDependency
            } else {
                7 // Export
            }
        };

        match action_type {
            0 => self.generate_create(),
            1 => self.generate_list(),
            2 => self.generate_show(),
            3 => self.generate_update(),
            4 => self.generate_close(),
            5 => self.generate_reopen(),
            6 => self.generate_add_dependency(),
            7 => self.generate_export(),
            _ => unreachable!(),
        }
    }

    fn generate_create(&mut self) -> TazkAction {
        let title = format!("Issue {}", self.rng.gen_range(1000..9999));
        let priority = self.rng.gen_range(0..5);
        let issue_type = match self.rng.gen_range(0..7) {
            0 => IssueType::Bug,
            1 => IssueType::Feature,
            2 => IssueType::Task,
            3 => IssueType::Epic,
            4 => IssueType::Gate,
            5 => IssueType::Spec,
            _ => IssueType::Chore,
        };

        // Specs and gates must have a description.
        let description =
            if matches!(issue_type, IssueType::Spec | IssueType::Gate) || self.rng.gen_bool(0.5) {
                Some(format!("Description for {}", title))
            } else {
                None
            };

        // Compute expected ID based on mode
        let expected_id = if self.use_hash_ids {
            // For hash mode, use a placeholder since hash IDs are timestamp-based
            // and cannot be predicted in advance (they depend on execution-time timestamp)
            format!("{ID_PREFIX}HASH")
        } else {
            // Use sequential numbering
            format!("{ID_PREFIX}{}", self.next_issue_num)
        };

        // Track the issue (will be updated with actual ID for hash mode)
        self.next_issue_num += 1;
        if !self.use_hash_ids {
            // Only pre-add for numeric mode; hash mode adds after execution
            self.existing_issues.push(expected_id.clone());
        }

        TazkAction::Create {
            expected_id,
            title,
            priority,
            issue_type,
            description,
        }
    }

    fn generate_list(&mut self) -> TazkAction {
        let status = if self.rng.gen_bool(0.3) {
            Some(match self.rng.gen_range(0..4) {
                0 => Status::Open,
                1 => Status::InProgress,
                2 => Status::Blocked,
                _ => Status::Closed,
            })
        } else {
            None
        };

        let priority = if self.rng.gen_bool(0.3) {
            Some(self.rng.gen_range(0..5))
        } else {
            None
        };

        TazkAction::List { status, priority }
    }

    fn generate_show(&mut self) -> TazkAction {
        let issue_id = self.pick_random_issue();
        TazkAction::Show { issue_id }
    }

    fn generate_update(&mut self) -> TazkAction {
        let issue_id = self.pick_random_issue();

        let priority = if self.rng.gen_bool(0.5) {
            Some(self.rng.gen_range(0..5))
        } else {
            None
        };

        TazkAction::Update { issue_id, priority }
    }

    fn generate_close(&mut self) -> TazkAction {
        let issue_id = self.pick_random_issue();
        self.closed_issues.insert(issue_id.clone());

        TazkAction::Close {
            issue_id,
            reason: "Completed".to_string(),
        }
    }

    fn generate_reopen(&mut self) -> TazkAction {
        // Try to reopen a closed issue, or pick any issue
        let issue_id = if !self.closed_issues.is_empty() && self.rng.gen_bool(0.7) {
            let idx = self.rng.gen_range(0..self.closed_issues.len());
            let issue = self.closed_issues.iter().nth(idx).unwrap().clone();
            self.closed_issues.remove(&issue);
            issue
        } else {
            self.pick_random_issue()
        };

        TazkAction::Reopen { issue_id }
    }

    fn generate_add_dependency(&mut self) -> TazkAction {
        // Need at least 2 issues for a dependency
        if self.existing_issues.len() < 2 {
            return self.generate_create();
        }

        let issue_id =
            self.existing_issues[self.rng.gen_range(0..self.existing_issues.len())].clone();

        // Pick a different issue to depend on
        let mut depends_on =
            self.existing_issues[self.rng.gen_range(0..self.existing_issues.len())].clone();
        while depends_on == issue_id && self.existing_issues.len() > 1 {
            depends_on =
                self.existing_issues[self.rng.gen_range(0..self.existing_issues.len())].clone();
        }

        let dep_type = match self.rng.gen_range(0..3) {
            0 => DependencyType::BlockedBy,
            1 => DependencyType::RelatedTo,
            _ => DependencyType::ChildOf,
        };

        TazkAction::AddDependency {
            issue_id,
            depends_on,
            dep_type,
        }
    }

    fn generate_export(&mut self) -> TazkAction {
        TazkAction::Export {
            output: "issues.jsonl".to_string(),
        }
    }

    fn pick_random_issue(&mut self) -> String {
        if self.existing_issues.is_empty() {
            // Shouldn't happen, but handle it
            format!("{ID_PREFIX}1")
        } else {
            let idx = self.rng.gen_range(0..self.existing_issues.len());
            self.existing_issues[idx].clone()
        }
    }
}

/// Executes actions against the tazk binary
pub struct ActionExecutor {
    binary_path: String,
    work_dir: String,
}

impl ActionExecutor {
    /// Create a new executor for the given binary
    pub fn new(binary_path: &str, work_dir: &str) -> Self {
        Self {
            binary_path: binary_path.to_string(),
            work_dir: work_dir.to_string(),
        }
    }

    /// Build a command with the binary path, and working directory
    fn build_command(&self) -> Command {
        let mut cmd = Command::new(&self.binary_path);
        cmd.current_dir(&self.work_dir);

        // Set TAZK_DIR to force using storage in the working directory.
        // This prevents tz from walking up and finding ancestor databases.
        let tazk_dir = std::path::PathBuf::from(&self.work_dir).join(".tazk");
        cmd.env("TAZK_DIR", tazk_dir);
        cmd
    }

    /// Execute a single action
    ///
    /// For Create actions, verifies that the created issue ID matches expected_id
    pub fn execute(&self, action: &TazkAction) -> Result<ExecutionResult> {
        // Track actual_issue_id for Create actions
        let mut actual_issue_id: Option<String> = None;

        let output = match action {
            TazkAction::Init { tz_hash_ids } => {
                let mut cmd = self.build_command();
                cmd.arg("init");
                if let Some(true) = tz_hash_ids {
                    cmd.arg("--tz-hash-ids");
                }
                cmd.output().context("Failed to execute init command")?
            }

            TazkAction::Create {
                expected_id,
                title,
                priority,
                issue_type,
                description,
            } => {
                let mut cmd = self.build_command();
                cmd.arg("create").arg(title);
                // Epics and specs take no role; every other type requires one.
                if issue_type.takes_role() {
                    cmd.arg("-r").arg("agent");
                }
                cmd.arg("-p")
                    .arg(priority.to_string())
                    .arg("-t")
                    .arg(issue_type.as_str());

                if let Some(desc) = description {
                    cmd.arg("-d").arg(desc);
                }
                // A gate needs acceptance criteria.
                if matches!(issue_type, IssueType::Gate) {
                    cmd.arg("--acceptance")
                        .arg("Check that the condition holds");
                }

                let output = cmd.output().context("Failed to execute create command")?;

                // Extract the actual issue ID from output
                if output.status.success() {
                    actual_issue_id = extract_issue_id(&String::from_utf8_lossy(&output.stdout));
                }

                // Verify the created issue ID matches our expectation
                // Skip verification for hash mode (expected_id contains "HASH")
                if output.status.success() && !expected_id.contains("HASH") {
                    if let Some(ref actual) = actual_issue_id {
                        if actual != expected_id {
                            let mut error_msg = String::from("ISSUE ID MISMATCH!\n");
                            error_msg.push_str(&format!("Expected: {}\n", expected_id));
                            error_msg.push_str(&format!("Actual:   {}\n\n", actual));
                            error_msg.push_str("Possible causes:\n");
                            error_msg.push_str("  - Existing issues in database\n");
                            error_msg.push_str("  - Sequential numbering assumption violated\n");
                            error_msg.push_str("  - Test directory not isolated\n");

                            anyhow::bail!(error_msg);
                        }
                    }
                }

                output
            }

            TazkAction::List { status, priority } => {
                let mut cmd = self.build_command();
                cmd.arg("list");

                if let Some(s) = status {
                    cmd.arg("--status").arg(s.as_str());
                }
                if let Some(p) = priority {
                    cmd.arg("--priority").arg(p.to_string());
                }

                cmd.output().context("Failed to execute list command")?
            }

            TazkAction::Show { issue_id } => {
                let mut cmd = self.build_command();
                cmd.arg("show").arg(issue_id);
                cmd.output().context("Failed to execute show command")?
            }

            TazkAction::Update { issue_id, priority } => {
                let mut cmd = self.build_command();
                cmd.arg("update").arg(issue_id);

                if let Some(p) = priority {
                    cmd.arg("--priority").arg(p.to_string());
                }

                cmd.output().context("Failed to execute update command")?
            }

            TazkAction::Close { issue_id, reason } => {
                let mut cmd = self.build_command();
                cmd.arg("close").arg(issue_id).arg("--reason").arg(reason);
                cmd.output().context("Failed to execute close command")?
            }

            TazkAction::Reopen { issue_id } => {
                let mut cmd = self.build_command();
                cmd.arg("reopen").arg(issue_id);
                cmd.output().context("Failed to execute reopen command")?
            }

            TazkAction::AddDependency {
                issue_id,
                depends_on,
                dep_type,
            } => {
                let mut cmd = self.build_command();
                cmd.arg("dep")
                    .arg("add")
                    .arg(issue_id)
                    .arg("--target")
                    .arg(depends_on)
                    .arg("-t")
                    .arg(dep_type.as_str());
                cmd.output().context("Failed to execute dep add command")?
            }

            TazkAction::Export { output } => {
                let mut cmd = self.build_command();
                cmd.arg("export").arg("--output").arg(output);
                cmd.output().context("Failed to execute export command")?
            }
        };

        Ok(ExecutionResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
            actual_issue_id,
        })
    }

    /// Execute a sequence of actions
    #[allow(dead_code)]
    pub fn execute_sequence(&self, actions: &[TazkAction]) -> Result<Vec<ExecutionResult>> {
        let mut results = Vec::new();
        for action in actions {
            let result = self.execute(action)?;
            results.push(result);
        }
        Ok(results)
    }
}

/// Extract issue ID from create command output
///
/// Looks for patterns like:
/// - "Created issue: test-1" (sequential)
/// - "Created: test-1" (sequential)
/// - "Created issue: test-4f10" (hash)
/// - "Created: test-4f10" (hash)
fn extract_issue_id(output: &str) -> Option<String> {
    // Try to find issue ID in various formats
    for line in output.lines() {
        // Look for "Created issue: <id>"
        if let Some(pos) = line.find("Created issue:") {
            let id = line[pos + 14..].trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }

        // Look for "Created: <id>"
        if let Some(pos) = line.find("Created:") {
            let id = line[pos + 8..].trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }

        // Look for issue ID pattern (prefix-number or prefix-hash)
        let words: Vec<&str> = line.split_whitespace().collect();
        for word in words {
            if word.contains('-') {
                let suffix = word.split('-').next_back().unwrap_or("");

                // Check if it's a sequential number
                if suffix.parse::<usize>().is_ok() {
                    return Some(word.to_string());
                }

                // Check if it's a base36 hash (3-8 characters, at least one letter)
                if (3..=8).contains(&suffix.len())
                    && suffix
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                    && suffix.chars().any(|c| c.is_ascii_lowercase())
                {
                    return Some(word.to_string());
                }
            }
        }
    }

    None
}

/// Result of executing an action
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    /// For Create actions, the actual issue ID that was created
    pub actual_issue_id: Option<String>,
}

/// Reference interpreter for tazk actions
///
/// Maintains an in-memory representation of the tazk state by interpreting
/// TazkAction sequences. This provides a "golden state" for verification.
pub struct ReferenceInterpreter {
    pub issues: std::collections::HashMap<String, ReferenceIssue>,
    pub next_id: usize,
    pub use_hash_ids: bool,
}

/// Simplified issue representation for reference interpreter
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceIssue {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: Status,
    pub priority: i32,
    pub issue_type: IssueType,
    pub depends_on: std::collections::HashMap<String, DependencyType>,
}

impl Default for ReferenceInterpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl ReferenceInterpreter {
    /// Create a new reference interpreter
    pub fn new() -> Self {
        Self {
            issues: std::collections::HashMap::new(),
            next_id: 1,
            use_hash_ids: false,
        }
    }

    /// Create a new reference interpreter with hash IDs enabled
    pub fn new_with_hash_ids() -> Self {
        Self {
            issues: std::collections::HashMap::new(),
            next_id: 1,
            use_hash_ids: true,
        }
    }

    /// Execute an action, updating the internal state
    pub fn execute(&mut self, action: &TazkAction) -> Result<()> {
        match action {
            TazkAction::Init { tz_hash_ids } => {
                if let Some(use_hash) = tz_hash_ids {
                    self.use_hash_ids = *use_hash;
                }
                Ok(())
            }

            TazkAction::Create {
                expected_id,
                title,
                priority,
                issue_type,
                description,
            } => {
                // For hash mode, the expected_id has already been replaced with the actual ID
                // by the test harness, so we just use it directly
                // For numeric mode, verify it matches the expected sequential ID
                if !self.use_hash_ids {
                    let computed_id = format!("{ID_PREFIX}{}", self.next_id);
                    if *expected_id != computed_id {
                        anyhow::bail!(
                            "Reference interpreter ID mismatch: expected {}, got {}",
                            computed_id,
                            expected_id
                        );
                    }
                }

                let issue = ReferenceIssue {
                    id: expected_id.clone(),
                    title: title.clone(),
                    description: description.clone().unwrap_or_default(),
                    status: Status::Open,
                    priority: *priority,
                    issue_type: *issue_type,
                    depends_on: std::collections::HashMap::new(),
                };

                self.issues.insert(expected_id.clone(), issue);
                self.next_id += 1;
                Ok(())
            }

            TazkAction::List { .. } => {
                // List doesn't modify state
                Ok(())
            }

            TazkAction::Show { .. } => {
                // Show doesn't modify state
                Ok(())
            }

            TazkAction::Update { issue_id, priority } => {
                if let Some(issue) = self.issues.get_mut(issue_id) {
                    if let Some(p) = priority {
                        issue.priority = *p;
                    }
                }
                // Silently ignore updates to non-existent issues
                Ok(())
            }

            TazkAction::Close { issue_id, .. } => {
                if let Some(issue) = self.issues.get_mut(issue_id) {
                    issue.status = Status::Closed;
                }
                Ok(())
            }

            TazkAction::Reopen { issue_id } => {
                if let Some(issue) = self.issues.get_mut(issue_id) {
                    issue.status = Status::Open;
                }
                Ok(())
            }

            TazkAction::AddDependency {
                issue_id,
                depends_on,
                dep_type,
            } => {
                if let Some(issue) = self.issues.get_mut(issue_id) {
                    // Mirror the single-parent rule: a second parent is rejected.
                    let second_parent = *dep_type == DependencyType::ChildOf
                        && issue
                            .depends_on
                            .iter()
                            .any(|(id, t)| *t == DependencyType::ChildOf && id != depends_on);
                    if !second_parent {
                        issue.depends_on.insert(depends_on.clone(), *dep_type);
                    }
                }
                Ok(())
            }

            TazkAction::Export { .. } => {
                // Export doesn't modify state
                Ok(())
            }
        }
    }

    /// Get the final state as a reference to the issues HashMap
    pub fn get_final_state(&self) -> &std::collections::HashMap<String, ReferenceIssue> {
        &self.issues
    }

    /// Get the next expected issue ID
    pub fn get_next_id(&self) -> usize {
        self.next_id
    }
}
