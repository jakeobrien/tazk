use std::process::Command;

#[test]
fn cli_help_and_version_start_successfully() {
    for argument in ["--help", "--version"] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tz"));
        command.arg(argument);
        let output = command.output().expect("run the actual CLI binary");
        assert!(
            output.status.success(),
            "{argument}: {:?}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.stdout.is_empty(), "{argument} produced no output");
    }
}

use std::fs;
use tazk::storage::{IssueStorageLayout, Storage};
use tazk::types::{Issue, IssueType};

fn fixture(layout: IssueStorageLayout) -> (tempfile::TempDir, Storage) {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::init(temp.path().join("database"), false, layout).unwrap();
    (temp, storage)
}

/// Create an issue with a chosen ID. `tz create` can no longer set IDs, so go
/// through JSONL import, which accepts canonical IDs only.
fn create(storage: &Storage, id: &str, title: &str) -> anyhow::Result<Issue> {
    let issue = Issue::new(id.into(), title.into(), 2, IssueType::Task);
    let input = storage
        .get_tazk_dir()
        .parent()
        .expect("database has a parent")
        .join("create.jsonl");
    fs::write(&input, serde_json::to_string(&issue)?)?;
    let (imported, skipped, errors) = storage.import_from_jsonl(&input, false)?;
    anyhow::ensure!(
        imported == 1 && skipped == 0 && errors.is_empty(),
        "import of {id:?} failed: skipped {skipped}, errors {errors:?}"
    );
    Ok(storage.get_issue(id)?.expect("imported issue exists"))
}

#[test]
fn unsafe_ids_are_rejected_by_the_format_check() {
    for layout in [IssueStorageLayout::Flat, IssueStorageLayout::Sharded] {
        let (_temp, storage) = fixture(layout);
        create(&storage, "tz-1", "Keep me").unwrap();
        for id in [
            "",
            ".",
            "..",
            "../../outside",
            r"..\..\outside",
            "/absolute",
            r"C:\outside",
            r"C:outside",
            r"\\server\share",
            "tz-1:stream",
            "CON",
            "NUL.txt",
            "tz-2.",
            "tz-2 ",
            "tz-0",
            "tz-01",
            "TZ-3",
            "tz-A",
            "tz-a-b",
            "tazk-3",
        ] {
            assert!(
                create(&storage, id, "Bad").is_err(),
                "import accepted {id:?}"
            );
            assert!(
                storage.rename_issue("tz-1", id, false).is_err(),
                "rename accepted {id:?}"
            );
            assert!(storage.get_issue(id).is_err(), "read accepted {id:?}");
            assert!(
                storage.list_comments(id).is_err(),
                "comments accepted {id:?}"
            );
        }
        assert_eq!(storage.get_issue("tz-1").unwrap().unwrap().title, "Keep me");
    }
}

#[test]
fn hostile_jsonl_does_not_overwrite_a_file_outside_the_database() {
    let (temp, storage) = fixture(IssueStorageLayout::Flat);
    let sentinel = temp.path().join("outside.md");
    fs::write(&sentinel, "keep this").unwrap();
    let issue = Issue::new(
        "../../outside".into(),
        "overwrite".into(),
        2,
        IssueType::Task,
    );
    let input = temp.path().join("input.jsonl");
    fs::write(&input, serde_json::to_string(&issue).unwrap()).unwrap();
    let (imported, _, errors) = storage.import_from_jsonl(&input, true).unwrap();
    assert_eq!(imported, 0);
    assert!(
        errors.iter().any(|e| e.contains("../../outside")),
        "{errors:?}"
    );
    assert!(tazk::sync::load_jsonl_issues(&input).is_err());
    let output = Command::new(env!("CARGO_BIN_EXE_tz"))
        .args(["--tz-no-cmd-logging", "--json", "--tz-dir"])
        .arg(storage.get_tazk_dir())
        .args(["sync", "--jsonl"])
        .arg(&input)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read_to_string(sentinel).unwrap(), "keep this");
}

#[cfg(unix)]
#[test]
fn a_symlink_cannot_redirect_issue_writes_outside_the_database() {
    let (temp, storage) = fixture(IssueStorageLayout::Flat);
    let sentinel = temp.path().join("outside.md");
    fs::write(&sentinel, "keep this").unwrap();
    std::os::unix::fs::symlink(&sentinel, storage.get_tazk_dir().join("issues/tz-1.md")).unwrap();
    assert!(create(&storage, "tz-1", "overwrite").is_err());
    assert!(storage.get_issue("tz-1").is_err());
    assert_eq!(fs::read_to_string(sentinel).unwrap(), "keep this");
}

fn import_fixture_issues(storage: &Storage, ids: &[&str]) -> Vec<Issue> {
    let base = chrono::Utc::now() - chrono::Duration::days(1);
    let issues: Vec<Issue> = ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let mut issue = Issue::new(
                (*id).into(),
                format!("original {index}"),
                2,
                IssueType::Task,
            );
            issue.created_at = base + chrono::Duration::seconds(index as i64);
            issue.updated_at = issue.created_at;
            issue
        })
        .collect();
    let input = tempfile::NamedTempFile::new().unwrap();
    let content = issues
        .iter()
        .map(|issue| serde_json::to_string(issue).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(input.path(), content).unwrap();
    let (imported, skipped, errors) = storage.import_from_jsonl(input.path(), true).unwrap();
    assert_eq!(imported, issues.len());
    assert_eq!(skipped, 0);
    assert!(errors.is_empty());
    issues
}

#[test]
fn repacking_swaps_and_cycles_preserves_all_issues_dependencies_and_comments() {
    use tazk::types::DependencyType;
    for layout in [IssueStorageLayout::Flat, IssueStorageLayout::Sharded] {
        for ids in [vec!["tz-2", "tz-1"], vec!["tz-3", "tz-1", "tz-2"]] {
            let (_temp, storage) = fixture(layout);
            let issues = import_fixture_issues(&storage, &ids);
            storage
                .add_dependency(ids[0], ids[1], DependencyType::BlockedBy)
                .unwrap();
            let comment = storage
                .add_comment(ids[0], "review", "keep this comment")
                .unwrap();
            let (_, mapping) = storage.repack_numeric_ids(false, None).unwrap();
            for original in &issues {
                let id = mapping.get(&original.id).unwrap_or(&original.id);
                assert_eq!(
                    storage.get_issue(id).unwrap().unwrap().title,
                    original.title
                );
            }
            let new_first = &mapping[ids[0]];
            let new_second = mapping.get(ids[1]).map(String::as_str).unwrap_or(ids[1]);
            assert!(storage
                .get_issue(new_first)
                .unwrap()
                .unwrap()
                .depends_on
                .contains_key(new_second));
            let comments = storage.list_comments(new_first).unwrap();
            assert_eq!(comments.len(), 1);
            assert_eq!(comments[0].id, comment.id);
            assert_eq!(&comments[0].issue_id, new_first);
            let all = tazk::sync::load_markdown_issues(&storage.get_tazk_dir()).unwrap();
            assert_eq!(all.len(), ids.len());
        }
    }
}

#[test]
fn every_id_migration_preserves_comment_ownership_and_jsonl_ids() {
    for mode in ["rename", "hash", "numeric", "repack"] {
        let (_temp, storage) = fixture(IssueStorageLayout::Flat);
        let old_id = if mode == "numeric" { "tz-abcd" } else { "tz-2" };
        let originals = import_fixture_issues(&storage, &[old_id]);
        let comment = storage
            .add_comment(old_id, "review", "keep my identity")
            .unwrap();
        fs::write(
            storage.get_tazk_dir().join("issues.jsonl"),
            serde_json::to_vec(&originals[0]).unwrap(),
        )
        .unwrap();
        let new_id = match mode {
            "rename" => {
                storage.rename_issue(old_id, "tz-9", false).unwrap();
                "tz-9".to_owned()
            }
            "hash" => storage.migrate_to_hash_ids(false, true).unwrap().1[old_id].clone(),
            "numeric" => storage.migrate_to_numeric_ids(false, true).unwrap().1[old_id].clone(),
            _ => storage.repack_numeric_ids(false, None).unwrap().1[old_id].clone(),
        };
        let comments = storage.list_comments(&new_id).unwrap();
        assert_eq!(comments.len(), 1, "{mode}");
        assert_eq!(comments[0].id, comment.id, "{mode}");
        assert_eq!(comments[0].issue_id, new_id, "{mode}");
        assert!(storage.list_comments(old_id).unwrap().is_empty(), "{mode}");
        let jsonl =
            tazk::sync::load_jsonl_issues(&storage.get_tazk_dir().join("issues.jsonl")).unwrap();
        assert!(jsonl.contains_key(&new_id), "{mode}");
        assert!(!jsonl.contains_key(old_id), "{mode}");
        assert!(!storage
            .get_tazk_dir()
            .join("tazk-transaction.json")
            .exists());
    }
}

#[test]
fn migration_preflight_failure_preserves_sources_and_config() {
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    create(&storage, "tz-1", "keep issue").unwrap();
    create(&storage, "tz-3", "move me").unwrap();
    fs::create_dir_all(storage.get_tazk_dir().join("comments")).unwrap();
    fs::write(
        storage.get_tazk_dir().join("comments/tz-2.json"),
        b"orphan comments",
    )
    .unwrap();
    assert!(storage.repack_numeric_ids(false, None).is_err());
    assert_eq!(
        storage.get_issue("tz-1").unwrap().unwrap().title,
        "keep issue"
    );
    assert_eq!(storage.get_issue("tz-3").unwrap().unwrap().title, "move me");
    assert_eq!(
        fs::read(storage.get_tazk_dir().join("comments/tz-2.json")).unwrap(),
        b"orphan comments"
    );
}

#[test]
fn repacking_closed_issues_can_swap_their_ids_with_open_issues() {
    for layout in [IssueStorageLayout::Flat, IssueStorageLayout::Sharded] {
        let (_temp, storage) = fixture(layout);
        import_fixture_issues(&storage, &["tz-1", "tz-2"]);
        storage.close_issue("tz-1", Some("done")).unwrap();
        storage.repack_numeric_ids(false, Some(2)).unwrap();
        let open = storage.get_issue("tz-1").unwrap().unwrap();
        let closed = storage.get_issue("tz-2").unwrap().unwrap();
        assert_eq!(open.title, "original 1");
        assert_eq!(open.status, tazk::types::Status::Open);
        assert_eq!(closed.title, "original 0");
        assert_eq!(closed.status, tazk::types::Status::Closed);
    }
}

#[test]
fn layout_migration_preserves_mtime_and_newer_jsonl_edits() {
    use std::time::{Duration, SystemTime};
    use tazk::sync::{load_jsonl_issues, load_markdown_issues, SyncEngine};

    for (source, target) in [
        (IssueStorageLayout::Flat, IssueStorageLayout::Sharded),
        (IssueStorageLayout::Sharded, IssueStorageLayout::Flat),
    ] {
        let (_temp, storage) = fixture(source);
        let mut issue = create(&storage, "tz-1", "original title").unwrap();
        let root = storage.get_tazk_dir();
        let original_mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let markdown = load_markdown_issues(&root).unwrap();
        filetime::set_file_mtime(
            &markdown[&issue.id].path,
            filetime::FileTime::from_system_time(original_mtime),
        )
        .unwrap();
        issue.title = "newer independently edited title".into();
        issue.updated_at = (original_mtime + Duration::from_secs(60)).into();
        let jsonl_path = root.join("issues.jsonl");
        fs::write(&jsonl_path, serde_json::to_vec(&issue).unwrap()).unwrap();
        let engine = SyncEngine::new();
        let before = engine
            .analyze(
                load_markdown_issues(&root).unwrap(),
                load_jsonl_issues(&jsonl_path).unwrap(),
            )
            .unwrap();
        assert_eq!(before.jsonl_newer, ["tz-1"]);

        storage
            .migrate_issue_storage_layout(target, false, true)
            .unwrap();

        let markdown = load_markdown_issues(&root).unwrap();
        assert_eq!(markdown[&issue.id].mtime, original_mtime);
        let after = engine
            .analyze(markdown, load_jsonl_issues(&jsonl_path).unwrap())
            .unwrap();
        assert_eq!(after.jsonl_newer, ["tz-1"]);
        assert!(after.markdown_newer.is_empty());
    }
}

#[test]
fn id_migrations_preserve_newer_independent_jsonl_edits() {
    use tazk::sync::{load_jsonl_issues, load_markdown_issues, SyncEngine};

    enum Migration {
        Rename,
        Hash,
        Numeric,
        Repack,
    }

    for layout in [IssueStorageLayout::Flat, IssueStorageLayout::Sharded] {
        for migration in [
            Migration::Rename,
            Migration::Hash,
            Migration::Numeric,
            Migration::Repack,
        ] {
            let (_temp, storage) = fixture(layout);
            let old_id = if matches!(migration, Migration::Numeric) {
                "tz-abcd"
            } else {
                "tz-2"
            };
            let mut independent = import_fixture_issues(&storage, &[old_id])
                .into_iter()
                .next()
                .unwrap();
            let original_updated_at = independent.updated_at;
            let original_mtime = original_updated_at.into();
            let root = storage.get_tazk_dir();
            let markdown = load_markdown_issues(&root).unwrap();
            filetime::set_file_mtime(
                &markdown[old_id].path,
                filetime::FileTime::from_system_time(original_mtime),
            )
            .unwrap();
            let original_mtime = fs::metadata(&markdown[old_id].path)
                .unwrap()
                .modified()
                .unwrap();
            independent.title = "newer independently edited title".into();
            independent.updated_at += chrono::Duration::seconds(60);
            let jsonl_path = root.join("issues.jsonl");
            fs::write(&jsonl_path, serde_json::to_vec(&independent).unwrap()).unwrap();

            match migration {
                Migration::Rename => {
                    storage.rename_issue(old_id, "tz-9", false).unwrap();
                }
                Migration::Hash => {
                    storage.migrate_to_hash_ids(false, true).unwrap();
                }
                Migration::Numeric => {
                    storage.migrate_to_numeric_ids(false, true).unwrap();
                }
                Migration::Repack => {
                    storage.repack_numeric_ids(false, None).unwrap();
                }
            }

            let markdown = load_markdown_issues(&root).unwrap();
            let migrated = markdown.values().next().unwrap();
            assert_eq!(migrated.mtime, original_mtime);
            assert_eq!(migrated.issue.updated_at, original_updated_at);
            let jsonl = load_jsonl_issues(&jsonl_path).unwrap();
            let record = &jsonl[&migrated.issue.id];
            assert_eq!(record.issue.title, independent.title);
            assert_eq!(record.updated_at, independent.updated_at);
            let plan = SyncEngine::new().analyze(markdown, jsonl).unwrap();
            assert_eq!(plan.jsonl_newer.len(), 1);
            assert!(plan.markdown_newer.is_empty());
        }
    }
}

#[test]
fn closing_and_reopening_prerequisites_updates_ready_blocked_and_stats() {
    use tazk::types::DependencyType;
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    for id in ["tz-1", "tz-2", "tz-3", "tz-4"] {
        create(&storage, id, id).unwrap();
    }
    storage
        .add_dependency("tz-2", "tz-1", DependencyType::BlockedBy)
        .unwrap();
    storage
        .add_dependency("tz-3", "tz-missing", DependencyType::BlockedBy)
        .unwrap();
    storage
        .add_dependency("tz-4", "tz-1", DependencyType::RelatedTo)
        .unwrap();
    assert_eq!(storage.get_stats().unwrap().blocked_issues, 2);
    storage.close_issue("tz-1", Some("done")).unwrap();
    let ready = storage.get_ready(None, None, None, "priority").unwrap();
    assert!(ready.iter().any(|issue| issue.id == "tz-2"));
    assert!(ready.iter().any(|issue| issue.id == "tz-4"));
    assert!(!ready.iter().any(|issue| issue.id == "tz-3"));
    let blocked = storage.get_blocked().unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].issue.id, "tz-3");
    assert_eq!(blocked[0].blocked_by, ["tz-missing"]);
    let stats = storage.get_stats().unwrap();
    assert_eq!(stats.ready_issues, ready.len());
    assert_eq!(stats.blocked_issues, blocked.len());
    storage
        .reopen_issue("tz-1", Some(tazk::types::Role::Agent), None)
        .unwrap();
    let ready = storage.get_ready(None, None, None, "priority").unwrap();
    assert!(!ready.iter().any(|issue| issue.id == "tz-2"));
    assert_eq!(storage.get_stats().unwrap().blocked_issues, 2);
}

#[test]
fn ready_filters_do_not_hide_a_prerequisite_in_another_assignee_or_priority() {
    use tazk::types::DependencyType;
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    create(&storage, "tz-1", "prerequisite").unwrap();
    create(&storage, "tz-2", "dependent").unwrap();
    storage
        .update_issue(
            "tz-1",
            std::collections::HashMap::from([
                ("priority".to_owned(), "0".to_owned()),
                ("assignee".to_owned(), "other".to_owned()),
            ]),
        )
        .unwrap();
    storage
        .update_issue(
            "tz-2",
            std::collections::HashMap::from([("assignee".to_owned(), "worker".to_owned())]),
        )
        .unwrap();
    storage
        .add_dependency("tz-2", "tz-1", DependencyType::BlockedBy)
        .unwrap();
    assert!(storage
        .get_ready(
            Some("worker"),
            Some(vec![2]),
            Some(IssueType::Task),
            "priority"
        )
        .unwrap()
        .is_empty());
    storage.close_issue("tz-1", Some("done")).unwrap();
    let ready = storage
        .get_ready(
            Some("worker"),
            Some(vec![2]),
            Some(IssueType::Task),
            "priority",
        )
        .unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, "tz-2");
}

#[test]
fn updates_refuse_unrecognized_or_ambiguous_sections_without_losing_text() {
    for extra in [
        "\n# User Report\n\nirreplaceable investigation\n",
        "\n# Notes\n\nfirst notes\n\n# Notes\n\nsecond notes\n",
    ] {
        let (_temp, storage) = fixture(IssueStorageLayout::Flat);
        create(&storage, "tz-1", "keep").unwrap();
        let path = storage.get_tazk_dir().join("issues/tz-1.md");
        let original = fs::read_to_string(&path).unwrap() + extra;
        fs::write(&path, &original).unwrap();
        let error = storage
            .update_issue(
                "tz-1",
                std::collections::HashMap::from([("priority".to_owned(), "1".to_owned())]),
            )
            .expect_err("a partial parse must not be rewritten");
        assert!(format!("{error:#}").contains("Markdown section"));
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }
}

#[test]
fn crlf_and_bom_frontmatter_survive_an_ordinary_update() {
    for bom in ["", "\u{feff}"] {
        let (_temp, storage) = fixture(IssueStorageLayout::Flat);
        create(&storage, "tz-1", "Title ending in ---").unwrap();
        let path = storage.get_tazk_dir().join("issues/tz-1.md");
        let original = fs::read_to_string(&path).unwrap() + "\n# Description\n\n  indented text\n";
        fs::write(&path, format!("{bom}{}", original.replace('\n', "\r\n"))).unwrap();
        let updated = storage
            .update_issue(
                "tz-1",
                std::collections::HashMap::from([("priority".to_owned(), "1".to_owned())]),
            )
            .unwrap();
        assert_eq!(updated.title, "Title ending in ---");
        assert_eq!(updated.description, "  indented text");
        assert_eq!(storage.get_issue("tz-1").unwrap().unwrap().priority, 1);
    }
}

#[test]
fn unstructured_preamble_is_rejected_instead_of_discarded() {
    let issue = Issue::new("tz-1".into(), "title".into(), 2, IssueType::Task);
    let markdown = tazk::format::issue_to_markdown(&issue).unwrap();
    let malformed = markdown + "\nimportant prose before any section\n";
    let error = tazk::format::markdown_to_issue("tz-1", &malformed).unwrap_err();
    assert!(format!("{error:#}").contains("before the first Markdown section"));
}

fn run_cli(storage: &Storage, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tz"))
        .args(["--tz-no-cmd-logging", "--json", "--tz-dir"])
        .arg(storage.get_tazk_dir())
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn jsonl_sync_waits_for_the_database_lock_instead_of_writing_through_it() {
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    create(&storage, "tz-1", "keep").unwrap();
    let _lock = tazk::lock::Lock::acquire(&storage.get_tazk_dir()).unwrap();
    let output = run_cli(&storage, &["sync"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Failed to acquire lock"));
    assert!(!storage.get_tazk_dir().join("issues.jsonl").exists());
}

#[test]
fn a_stale_jsonl_sync_plan_cannot_overwrite_a_new_local_edit() {
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    create(&storage, "tz-1", "original").unwrap();
    let engine = tazk::sync::SyncEngine::new();
    let markdown = tazk::sync::load_markdown_issues(&storage.get_tazk_dir()).unwrap();
    let jsonl = std::collections::HashMap::new();
    let plan = engine.analyze_ref(&markdown, &jsonl).unwrap();
    storage
        .update_issue(
            "tz-1",
            std::collections::HashMap::from([("title".to_owned(), "concurrent edit".to_owned())]),
        )
        .unwrap();
    let error = engine
        .apply(&plan, &markdown, &jsonl, &storage.get_tazk_dir(), false)
        .unwrap_err();
    assert!(error.to_string().contains("changed after analysis"));
    assert_eq!(
        storage.get_issue("tz-1").unwrap().unwrap().title,
        "concurrent edit"
    );
    assert!(!storage.get_tazk_dir().join("issues.jsonl").exists());
}

#[test]
fn custom_jsonl_paths_are_honored_and_dry_runs_return_json_with_planned_counts() {
    let (temp, storage) = fixture(IssueStorageLayout::Flat);
    create(&storage, "tz-1", "local").unwrap();
    let custom = temp.path().join("custom.jsonl");
    fs::write(&custom, "").unwrap();
    let dry = run_cli(
        &storage,
        &["sync", "--jsonl", custom.to_str().unwrap(), "--dry-run"],
    );
    assert!(
        dry.status.success(),
        "{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&dry.stdout).unwrap();
    assert_eq!(report["created_in_jsonl"], 1);
    assert_eq!(report["dry_run"], true);
    assert!(fs::read(&custom).unwrap().is_empty());
    assert!(!storage.get_tazk_dir().join("issues.jsonl").exists());
    let applied = run_cli(&storage, &["sync", "--jsonl", custom.to_str().unwrap()]);
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(report["created_in_jsonl"], 1);
    assert_eq!(report["dry_run"], false);
    assert!(tazk::sync::load_jsonl_issues(&custom)
        .unwrap()
        .contains_key("tz-1"));
    assert!(!storage.get_tazk_dir().join("issues.jsonl").exists());
}

#[test]
fn equal_time_content_conflicts_are_reported_without_overwriting_either_copy() {
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    let issue = create(&storage, "tz-1", "markdown title").unwrap();
    let mut remote = issue.clone();
    remote.title = "JSONL title".into();
    let markdown_path = storage.get_tazk_dir().join("issues/tz-1.md");
    let jsonl_path = storage.get_tazk_dir().join("issues.jsonl");
    let jsonl_bytes = serde_json::to_vec(&remote).unwrap();
    fs::write(&jsonl_path, &jsonl_bytes).unwrap();
    filetime::set_file_mtime(
        &markdown_path,
        filetime::FileTime::from_system_time(issue.updated_at.into()),
    )
    .unwrap();
    let original_markdown = fs::read(&markdown_path).unwrap();
    let output = run_cli(&storage, &["sync"]);
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["skipped_conflicts"], 1);
    assert!(report["errors"][0].as_str().unwrap().contains("Conflict"));
    assert_eq!(fs::read(markdown_path).unwrap(), original_markdown);
    assert_eq!(fs::read(jsonl_path).unwrap(), jsonl_bytes);
}

#[test]
fn jsonl_sync_ignores_timestamp_and_label_order_when_content_is_unchanged() {
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    create(&storage, "tz-1", "same content").unwrap();
    let issue = storage
        .set_labels("tz-1", vec!["alpha".into(), "beta".into()])
        .unwrap();
    let mut jsonl_issue = issue.clone();
    jsonl_issue.labels.reverse();
    jsonl_issue.updated_at += chrono::Duration::hours(1);
    let jsonl_path = storage.get_tazk_dir().join("issues.jsonl");
    fs::write(&jsonl_path, serde_json::to_vec(&jsonl_issue).unwrap()).unwrap();
    let output = run_cli(&storage, &["sync", "--dry-run"]);
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["updated_jsonl"], 0);
    assert_eq!(report["updated_markdown"], 0);
    assert_eq!(report["skipped_conflicts"], 0);
}

#[test]
fn duplicate_records_are_rejected_before_sync() {
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    let issue = create(&storage, "tz-1", "keep").unwrap();
    let record = serde_json::to_string(&issue).unwrap();
    let jsonl_path = storage.get_tazk_dir().join("issues.jsonl");
    fs::write(&jsonl_path, format!("{record}\n{record}\n")).unwrap();
    assert!(tazk::sync::load_jsonl_issues(&jsonl_path).is_err());
    let duplicate_dir = storage.get_tazk_dir().join("issues/duplicate");
    fs::create_dir_all(&duplicate_dir).unwrap();
    fs::copy(
        storage.get_tazk_dir().join("issues/tz-1.md"),
        duplicate_dir.join("tz-1.md"),
    )
    .unwrap();
    assert!(tazk::sync::load_markdown_issues(&storage.get_tazk_dir()).is_err());
}

#[cfg(windows)]
#[test]
fn an_unsuccessful_jsonl_write_returns_failure() {
    let (temp, storage) = fixture(IssueStorageLayout::Flat);
    create(&storage, "tz-1", "keep").unwrap();
    let path = temp.path().join("readonly.jsonl");
    fs::write(&path, "").unwrap();
    let original_permissions = fs::metadata(&path).unwrap().permissions();
    let mut readonly = original_permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&path, readonly).unwrap();
    let output = run_cli(&storage, &["sync", "--jsonl", path.to_str().unwrap()]);
    fs::set_permissions(&path, original_permissions).unwrap();
    assert!(!output.status.success());
    assert!(fs::read(path).unwrap().is_empty());
}

#[test]
fn readonly_rejects_modifications_before_opening_storage() {
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    create(&storage, "tz-1", "keep").unwrap();
    let path = storage.get_tazk_dir().join("issues/tz-1.md");
    let before = fs::read(&path).unwrap();
    for args in [
        vec!["update", "tz-1", "--title", "changed"],
        vec!["create", "new", "-r", "agent"],
        vec!["triage", "add", "found"],
        vec!["triage", "rm", "x"],
        vec!["triage", "update", "x", "--append", "more"],
        vec!["triage", "promote", "x", "--title", "t", "-r", "agent"],
        vec!["triage", "attach", "x", "--to", "tz-1", "--author", "a"],
        vec!["close", "tz-1"],
        vec!["rename", "tz-1", "tz-2"],
        vec!["config", "set", "tz-hash-ids", "true"],
        vec!["comments", "add", "tz-1", "--body", "new", "--author", "a"],
        vec!["label", "add", "tz-1", "new"],
        vec!["dep", "add", "tz-1", "--target", "tz-2"],
        vec!["sync"],
        vec!["export", "--tz-output-default"],
    ] {
        let output = run_cli(&storage, &[vec!["--readonly"], args].concat());
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("--readonly"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    let fresh = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tz"))
        .current_dir(fresh.path())
        .args(["--readonly", "init"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!fresh.path().join(".tazk").exists());
}

#[test]
fn command_history_keeps_multiline_arguments_on_one_line() {
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    let output = Command::new(env!("CARGO_BIN_EXE_tz"))
        .arg("--tz-dir")
        .arg(storage.get_tazk_dir())
        .args([
            "create",
            "Multiline description",
            "--role",
            "agent",
            "--description",
            "first line\nsecond \"line\"\r\nthird line",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let history = fs::read_to_string(storage.get_tazk_dir().join("command_history.log")).unwrap();
    assert_eq!(history.lines().count(), 1);
    assert!(!history.contains("BD_INVOKE"));
    assert!(history.contains(r#"first line\nsecond \"line\"\r\nthird line"#));
}

#[test]
fn readonly_reads_do_not_bootstrap_or_log_and_refuse_recovery() {
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    create(&storage, "tz-1", "keep").unwrap();
    for name in ["config.yaml", ".gitignore"] {
        fs::remove_file(storage.get_tazk_dir().join(name)).unwrap();
    }
    for args in [
        vec!["show", "tz-1"],
        vec!["list"],
        vec!["ready"],
        vec!["stats"],
        vec!["blocked"],
        vec!["export"],
        vec!["comments", "list", "tz-1"],
        vec!["config", "list"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_tz"))
            .args(["--readonly", "--tz-dir"])
            .arg(storage.get_tazk_dir())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for name in ["config.yaml", ".gitignore", "command_history.log"] {
        assert!(
            !storage.get_tazk_dir().join(name).exists(),
            "created {name}"
        );
    }
    let readonly = Storage::open_readonly(storage.get_tazk_dir()).unwrap();
    assert!(create(&readonly, "tz-2", "must fail").is_err());
    assert!(readonly.set_config_value("tz-hash-ids", "true").is_err());
    let journal = storage.get_tazk_dir().join("tazk-transaction.json");
    fs::write(&journal, "pending recovery sentinel").unwrap();
    let output = run_cli(&storage, &["--readonly", "show", "tz-1"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires recovery"));
    assert_eq!(
        fs::read_to_string(&journal).unwrap(),
        "pending recovery sentinel"
    );
}

#[test]
fn issue_file_outside_the_id_format_is_invalid_not_a_crash() {
    for layout in [IssueStorageLayout::Flat, IssueStorageLayout::Sharded] {
        let (_temp, storage) = fixture(layout);
        create(&storage, "tz-1", "good").unwrap();
        let good = fs::read_to_string(storage.issue_file_path("tz-1").unwrap().unwrap()).unwrap();
        fs::write(storage.get_tazk_dir().join("issues/tz-01.md"), &good).unwrap();

        // Default validation: an error that names the file; no panic.
        let error = storage
            .list_issues(None, None, None, None, None)
            .unwrap_err();
        assert!(format!("{error:#}").contains("tz-01"), "{error:#}");

        // Tolerant loaders skip it and keep the valid issue.
        let loaded = storage.list_all_issues_tolerant().unwrap();
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.skipped.len(), 1);
        assert!(loaded.skipped[0].path.ends_with("tz-01.md"));

        // Numbering ignores the invalid name.
        let next = storage
            .create_issue(
                "next".into(),
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
        assert_eq!(next.id, "tz-2");
    }
}

#[test]
fn migrate_to_numeric_renumbers_exactly_the_hash_ids() {
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    // A hash ID that looks numeric-ish, and numeric IDs with a big gap.
    import_fixture_issues(&storage, &["tz-1", "tz-5000", "tz-1e5", "tz-a3f9"]);
    let (_, mapping) = storage.migrate_to_numeric_ids(false, true).unwrap();
    let mut old: Vec<_> = mapping.keys().cloned().collect();
    old.sort();
    assert_eq!(old, ["tz-1e5", "tz-a3f9"]);
    let mut new: Vec<_> = mapping.values().cloned().collect();
    new.sort();
    assert_eq!(new, ["tz-5001", "tz-5002"]);
    assert!(storage.get_issue("tz-5000").unwrap().is_some());
}

#[test]
fn jsonl_import_rejects_non_canonical_ids_naming_the_record() {
    let (temp, storage) = fixture(IssueStorageLayout::Flat);
    let lines = ["tz-0", "tz-ok-1", "TZ-7", "tz-3"]
        .iter()
        .map(|id| {
            let issue = Issue::new((*id).into(), "t".into(), 2, IssueType::Task);
            serde_json::to_string(&issue).unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let input = temp.path().join("in.jsonl");
    fs::write(&input, lines).unwrap();
    let (imported, _, errors) = storage.import_from_jsonl(&input, true).unwrap();
    assert_eq!(imported, 1);
    assert_eq!(errors.len(), 3, "{errors:?}");
    for (error, id) in errors.iter().zip(["tz-0", "tz-ok-1", "TZ-7"]) {
        assert!(error.contains(id), "{error}");
    }
    assert!(storage.get_issue("tz-3").unwrap().is_some());
    let error = tazk::sync::load_jsonl_issues(&input).unwrap_err();
    assert!(format!("{error:#}").contains("tz-0"), "{error:#}");
}

/// Add a triage item with exact text (so tests can compare bytes).
fn triage_item(storage: &Storage, text: &str) -> tazk::storage::triage::TriageItem {
    storage.add_triage_item(text).unwrap()
}

#[test]
fn rename_updates_triage_mentions_and_leaves_other_items_untouched() {
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    create(&storage, "tz-1", "one").unwrap();
    create(&storage, "tz-12", "twelve").unwrap();
    let mentions = triage_item(&storage, "Found while on tz-1 (not tz-12).");
    let plain = triage_item(&storage, "Nothing to see here.");
    let triage_dir = storage.get_tazk_dir().join("triage");
    let plain_path = triage_dir.join(&plain.name);
    let plain_before = fs::read(&plain_path).unwrap();
    let plain_mtime = fs::metadata(&plain_path).unwrap().modified().unwrap();

    let changes = storage.rename_issue("tz-1", "tz-7", true).unwrap();
    assert!(
        changes.iter().any(|c| c.contains(&mentions.name)),
        "{changes:?}"
    );
    assert!(
        !changes.iter().any(|c| c.contains(&plain.name)),
        "{changes:?}"
    );
    assert_eq!(
        storage.get_triage_item(&mentions.name).unwrap().body,
        mentions.body,
        "dry run must not write"
    );

    storage.rename_issue("tz-1", "tz-7", false).unwrap();
    let updated = storage.get_triage_item(&mentions.name).unwrap();
    assert_eq!(updated.body, "Found while on tz-7 (not tz-12).\n");
    assert_eq!(fs::read(&plain_path).unwrap(), plain_before);
    assert_eq!(
        fs::metadata(&plain_path).unwrap().modified().unwrap(),
        plain_mtime
    );
    // Names (creation timestamps) are kept.
    let names: Vec<_> = storage
        .list_triage_items()
        .unwrap()
        .into_iter()
        .map(|i| i.name)
        .collect();
    assert!(names.contains(&mentions.name) && names.contains(&plain.name));
}

#[test]
fn migrate_to_hash_updates_triage_mentions() {
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    import_fixture_issues(&storage, &["tz-2", "tz-5"]);
    let item = triage_item(&storage, "See tz-5, and tz-2.");
    let plain = triage_item(&storage, "No mentions");

    let (changes, _) = storage.migrate_to_hash_ids(true, true).unwrap();
    assert!(
        changes.iter().any(|c| c.contains(&item.name)),
        "{changes:?}"
    );
    assert!(
        !changes.iter().any(|c| c.contains(&plain.name)),
        "{changes:?}"
    );
    assert_eq!(storage.get_triage_item(&item.name).unwrap().body, item.body);

    let (_, mapping) = storage.migrate_to_hash_ids(false, true).unwrap();
    assert_eq!(
        storage.get_triage_item(&item.name).unwrap().body,
        format!("See {}, and {}.\n", mapping["tz-5"], mapping["tz-2"])
    );
    assert_eq!(
        storage.get_triage_item(&plain.name).unwrap().body,
        plain.body
    );

    // And back to numeric: the hash mentions follow the renumbering.
    let (_, mapping) = storage.migrate_to_numeric_ids(false, true).unwrap();
    let body = storage.get_triage_item(&item.name).unwrap().body;
    assert!(
        mapping.values().all(|new_id| body.contains(new_id)),
        "{body}"
    );
}

#[test]
fn repack_rewrites_triage_mentions_in_one_pass() {
    let (_temp, storage) = fixture(IssueStorageLayout::Flat);
    import_fixture_issues(&storage, &["tz-2", "tz-5"]);
    let item = triage_item(&storage, "See tz-5, and tz-2.");
    // tz-2 -> tz-1 and tz-5 -> tz-2: a sequential rewrite would collapse both.
    storage.repack_numeric_ids(false, None).unwrap();
    assert_eq!(
        storage.get_triage_item(&item.name).unwrap().body,
        "See tz-2, and tz-1.\n"
    );
}
