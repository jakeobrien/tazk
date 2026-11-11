//! Regression test for the migrate description-truncation bug.
//!
//! Renumbering hash-based IDs to numeric IDs (`tz migrate --to numeric`)
//! parses every issue and rewrites the renamed ones. The parser used to treat
//! any line whose *trimmed* form started with "# " as a top-level section
//! header, so a comment inside an indented code block (e.g. "    # x") was
//! taken as an unknown section header and everything from that line onward
//! was silently discarded — the migration then persisted the truncated parse
//! and deleted the intact original file.

use std::fs;
use std::path::PathBuf;
use tazk::storage::{IssueStorageLayout, Storage};
use tazk::types::IssueType;

#[test]
fn migration_preserves_description_with_indented_code_block() {
    let scratch: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scratch")
        .join(format!("migration_desc_trunc_{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);

    let storage = Storage::init(
        scratch.join(".tazk"),
        true, // hash IDs, as in a parallel-safe agent workspace
        IssueStorageLayout::Flat,
    )
    .expect("init storage");

    // Shape taken from the real victim (tz-hkj8lo -> tz-5976): an indented
    // code block whose first line is a "# ..." comment, followed by more
    // content that must survive.
    let description = "Repro steps:\n\n    \
        # Description  ->  ## User Report\n    \
        sed -i 's/^# Description$/## User Report/' issue.md\n\n\
        Everything after the indented comment must survive renumbering.";

    storage
        .create_issue(
            "Victim issue".to_string(),
            description.to_string(),
            None,
            None,
            2,
            IssueType::Task,
            None,
            Vec::new(),
            None,
            Some("tz-hkj8lo".to_string()),
            Vec::new(),
            None,
            None,
        )
        .expect("create issue");

    let (_changes, id_mapping) = storage
        .migrate_to_numeric_ids(false, true)
        .expect("migrate to numeric IDs");
    let new_id = id_mapping.get("tz-hkj8lo").expect("hash ID was renumbered");

    let migrated = storage
        .get_issue(new_id)
        .expect("read migrated issue")
        .expect("migrated issue exists");
    assert_eq!(
        migrated.description, description,
        "renumbering must be content-preserving: the description changed"
    );

    let _ = fs::remove_dir_all(&scratch);
}
