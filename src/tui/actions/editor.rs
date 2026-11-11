//! Open the selected / displayed file in `$EDITOR` (`o`, spec §11, §13).
//!
//! The TUI never writes files itself: it hands the file to the user's editor,
//! waits, then asks for a reload so any change shows up.
//!
//! `$EDITOR` may carry arguments (`code -w`, `emacsclient -t`); it is split
//! into words with simple quoting rules ([`split_command`]), not run through a
//! shell. Unset or blank falls back to `vi`.

use crate::storage::Storage;
use crate::tui::app::{self, Action, App, EditorTarget};
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Editor used when `$EDITOR` is unset or blank.
pub const FALLBACK_EDITOR: &str = "vi";

/// The file behind `target`, looking in whichever layout (flat or sharded)
/// holds it. Fails if the file does not exist (e.g. deleted since the last
/// load).
pub fn resolve_path(storage: &Storage, target: &EditorTarget) -> Result<PathBuf> {
    match target {
        EditorTarget::Issue(id) => storage
            .issue_file_path(id)?
            .ok_or_else(|| anyhow!("issue file not found: {id}")),
        EditorTarget::Triage(name) => storage.triage_item_path(name).map(|(_, path)| path),
    }
}

/// Split an `$EDITOR` value into words. Whitespace separates words; single
/// quotes keep everything literal; double quotes allow `\"` and `\\`; outside
/// quotes a backslash escapes the next character (except on Windows, where it
/// is a path separator).
pub fn split_command(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    word.push(c);
                }
            }
            '"' => {
                in_word = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' if !cfg!(windows) => match chars.next() {
                            Some(n @ ('"' | '\\')) => word.push(n),
                            Some(n) => {
                                word.push('\\');
                                word.push(n);
                            }
                            None => word.push('\\'),
                        },
                        c => word.push(c),
                    }
                }
            }
            '\\' if !cfg!(windows) => {
                in_word = true;
                word.push(chars.next().unwrap_or('\\'));
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    words
}

/// The command that opens `path`: `editor` (the value of `$EDITOR`, if any)
/// split into program and arguments, with the path appended; `vi` when unset
/// or blank.
pub fn command(editor: Option<&str>, path: &Path) -> Command {
    let mut words = editor.map(split_command).unwrap_or_default();
    if words.is_empty() {
        words.push(FALLBACK_EDITOR.to_string());
    }
    let mut command = Command::new(words.remove(0));
    command.args(words).arg(path);
    command
}

/// Run the editor on `path` with the terminal's stdio and wait for it. Err is
/// a short message for the hint bar.
pub fn run_editor(editor: Option<&str>, path: &Path) -> Result<(), String> {
    let mut command = command(editor, path);
    let program = command.get_program().to_string_lossy().into_owned();
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("Editor {program} exited with {status}")),
        Err(e) => Err(format!("Cannot launch editor {program}: {e}")),
    }
}

/// Gives the terminal to a child process and takes it back.
pub trait TerminalHost {
    /// Leave raw mode and the alternate screen.
    fn suspend(&mut self);
    /// Re-enter them and force a full redraw.
    fn resume(&mut self) -> Result<()>;
}

/// Perform the editor launch requested by `o`, if any.
///
/// The terminal is suspended around the editor and always resumed, whatever
/// the outcome. A reload is requested afterwards (the file may have changed).
/// Problems (path not found, launch failure, non-zero exit) become hint-bar
/// messages; the only error returned is a failure to resume the terminal.
pub fn perform_pending(
    app: &mut App,
    resolve: &dyn Fn(&EditorTarget) -> Result<PathBuf>,
    host: &mut dyn TerminalHost,
    editor: Option<&str>,
) -> Result<()> {
    let Some(target) = app.take_editor_request() else {
        return Ok(());
    };
    let path = match resolve(&target) {
        Ok(path) => path,
        Err(e) => {
            app.set_status(format!("Cannot open file: {e:#}"));
            return Ok(());
        }
    };
    host.suspend();
    let outcome = run_editor(editor, &path);
    host.resume()?;
    app::update(app, Action::Reload);
    if let Err(message) = outcome {
        app.set_status(message);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::IssueStorageLayout;
    use crate::tui::testing as fixtures;
    use crate::tui::ModeArg;
    use crate::types::IssueType;
    use std::ffi::OsStr;

    fn words(c: &Command) -> Vec<String> {
        std::iter::once(c.get_program())
            .chain(c.get_args())
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn unset_or_blank_editor_falls_back_to_vi() {
        let c = command(None, Path::new("/x/a.md"));
        assert_eq!(c.get_program(), OsStr::new("vi"));
        assert_eq!(words(&c), ["vi", "/x/a.md"]);
        assert_eq!(
            words(&command(Some("  "), Path::new("a.md"))),
            ["vi", "a.md"]
        );
        assert_eq!(words(&command(Some(""), Path::new("a.md"))), ["vi", "a.md"]);
    }

    #[test]
    fn editor_with_arguments_is_split() {
        let c = command(Some("code -w"), Path::new("a.md"));
        assert_eq!(words(&c), ["code", "-w", "a.md"]);
        let c = command(Some("  emacsclient   -t  -a '' "), Path::new("a b.md"));
        assert_eq!(words(&c), ["emacsclient", "-t", "-a", "", "a b.md"]);
    }

    #[cfg(not(windows))]
    #[test]
    fn quotes_group_words() {
        assert_eq!(
            split_command(r#""/opt/my editor/ed" --flag 'a b' c\ d"#),
            ["/opt/my editor/ed", "--flag", "a b", "c d"]
        );
        assert_eq!(split_command(r#""a \"q\" \\ \n""#), [r#"a "q" \ \n"#]);
    }

    fn storage(layout: IssueStorageLayout) -> (tempfile::TempDir, Storage) {
        let tmp = tempfile::tempdir().unwrap();
        let storage = Storage::init(tmp.path().join(".tazk"), false, layout).unwrap();
        (tmp, storage)
    }

    fn create(storage: &Storage) -> String {
        storage
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
            .unwrap()
            .id
    }

    #[test]
    fn resolves_flat_and_sharded_issue_paths() {
        for (layout, rel) in [
            (IssueStorageLayout::Flat, "issues/tz-1.md"),
            (IssueStorageLayout::Sharded, "issues/n/0/1/tz-1.md"),
        ] {
            let (_tmp, storage) = storage(layout);
            let id = create(&storage);
            let path = resolve_path(&storage, &EditorTarget::Issue(id)).unwrap();
            assert_eq!(path, storage.get_tazk_dir().join(rel));
            assert!(path.is_file());
        }
    }

    #[test]
    fn resolves_issues_after_a_layout_migration_and_triage_items() {
        let (_tmp, storage) = storage(IssueStorageLayout::Flat);
        let id = create(&storage);
        // The file is found in the other layout when the config disagrees.
        storage
            .set_config_value("issue-storage-layout", "sharded")
            .unwrap();
        let path = resolve_path(&storage, &EditorTarget::Issue(id)).unwrap();
        assert_eq!(path, storage.get_tazk_dir().join("issues/tz-1.md"));

        let name = storage.add_triage_item("a report").unwrap().name;
        let path = resolve_path(&storage, &EditorTarget::Triage(name.clone())).unwrap();
        assert_eq!(path, storage.get_tazk_dir().join("triage").join(&name));
    }

    #[test]
    fn missing_files_are_errors() {
        let (_tmp, storage) = storage(IssueStorageLayout::Flat);
        assert!(resolve_path(&storage, &EditorTarget::Issue("tz-9".into())).is_err());
        assert!(resolve_path(&storage, &EditorTarget::Triage("nope.md".into())).is_err());
    }

    #[derive(Default)]
    struct FakeHost {
        suspended: usize,
        resumed: usize,
        fail_resume: bool,
    }

    impl TerminalHost for FakeHost {
        fn suspend(&mut self) {
            self.suspended += 1;
        }
        fn resume(&mut self) -> Result<()> {
            self.resumed += 1;
            if self.fail_resume {
                Err(anyhow!("no terminal"))
            } else {
                Ok(())
            }
        }
    }

    fn app_with_request() -> App {
        let mut app = App::with_mode(ModeArg::Issues);
        app.editor_request = Some(EditorTarget::Issue("a".into()));
        app
    }

    fn found(_: &EditorTarget) -> Result<PathBuf> {
        Ok(PathBuf::from("/dev/null"))
    }

    #[cfg(unix)]
    #[test]
    fn editor_true_resumes_and_requests_a_reload() {
        let mut app = app_with_request();
        let mut host = FakeHost::default();
        perform_pending(&mut app, &found, &mut host, Some("true")).unwrap();
        assert_eq!((host.suspended, host.resumed), (1, 1));
        assert!(app.take_reload());
        assert_eq!(app.status, None);
        assert_eq!(app.editor_request, None);
    }

    #[test]
    fn missing_editor_shows_a_hint_and_the_ui_keeps_working() {
        let mut app = app_with_request();
        let mut host = FakeHost::default();
        perform_pending(&mut app, &found, &mut host, Some("/nonexistent")).unwrap();
        assert_eq!((host.suspended, host.resumed), (1, 1), "always resumed");
        let status = app.status.clone().unwrap();
        assert!(
            status.contains("Cannot launch editor /nonexistent"),
            "{status}"
        );
        // Still responsive: the app handles further actions.
        app::update(&mut app, Action::OpenHelp);
        assert_eq!(app.overlay, crate::tui::app::Overlay::Help);
    }

    #[cfg(unix)]
    #[test]
    fn failing_editor_reports_its_exit_status() {
        let mut app = app_with_request();
        let mut host = FakeHost::default();
        perform_pending(&mut app, &found, &mut host, Some("false")).unwrap();
        assert!(app.status.clone().unwrap().contains("exited with"));
        assert!(app.take_reload());
    }

    #[test]
    fn unresolvable_path_never_suspends_the_terminal() {
        let mut app = app_with_request();
        let mut host = FakeHost::default();
        let missing = |_: &EditorTarget| -> Result<PathBuf> { Err(anyhow!("gone")) };
        perform_pending(&mut app, &missing, &mut host, Some("true")).unwrap();
        assert_eq!(host.suspended, 0);
        assert!(app.status.clone().unwrap().contains("gone"));
    }

    #[test]
    fn resume_failure_is_returned() {
        let mut app = app_with_request();
        let mut host = FakeHost {
            fail_resume: true,
            ..Default::default()
        };
        assert!(perform_pending(&mut app, &found, &mut host, Some("/nonexistent")).is_err());
    }

    #[test]
    fn no_request_does_nothing() {
        let mut app = App::with_mode(ModeArg::Issues);
        let mut host = FakeHost::default();
        perform_pending(&mut app, &found, &mut host, None).unwrap();
        assert_eq!(host.suspended, 0);
    }

    #[test]
    fn o_requests_the_selected_or_displayed_file() {
        let mut app = App::from_snapshot(
            ModeArg::Issues,
            fixtures::snapshot_of([fixtures::issue("a", "A").priority(0)]),
        );
        app::update(&mut app, Action::OpenEditor);
        assert_eq!(
            app.take_editor_request(),
            Some(EditorTarget::Issue("a".into()))
        );

        let mut app = App::from_snapshot(
            ModeArg::Triage,
            fixtures::snapshot_with(
                Vec::<fixtures::IssueBuilder>::new(),
                vec![fixtures::triage_item("t.md", "x")],
            ),
        );
        app::update(&mut app, Action::OpenEditor);
        assert_eq!(
            app.take_editor_request(),
            Some(EditorTarget::Triage("t.md".into()))
        );

        let mut app = App::with_mode(ModeArg::Issues);
        app::update(&mut app, Action::OpenEditor);
        assert_eq!(app.take_editor_request(), None);
        assert_eq!(app.status.as_deref(), Some("Nothing to open"));
    }
}
