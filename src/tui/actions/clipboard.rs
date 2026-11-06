//! Copy the selected / displayed ID or filename to the clipboard (`y`,
//! spec §11, §13).
//!
//! Backend choice: **no clipboard crate**. A crate such as `arboard` brings in
//! X11/Wayland/image dependencies for a one-line feature, so instead
//! [`SystemClipboard`] uses what the platform already provides, in this order:
//!
//! 1. Over SSH (`SSH_CONNECTION` / `SSH_TTY` set) a local tool would fill the
//!    *remote* machine's clipboard, so OSC 52 goes first.
//! 2. The platform's clipboard command: `pbcopy` (macOS), `wl-copy` (Wayland),
//!    `xclip` / `xsel` (X11), `clip.exe` (Windows / WSL).
//! 3. OSC 52 (`ESC ] 52 ; c ; <base64> BEL`) written to the terminal, which
//!    most modern terminals honour and which travels through SSH. tmux needs
//!    `set -g set-clipboard on`; terminals that ignore OSC 52 silently drop it,
//!    which we cannot detect, so the confirmation then claims too much.
//!
//! The backend sits behind [`Clipboard`] so tests use a fake. Nothing here
//! panics; failures become a short hint-bar message.

use crate::tui::app::App;
use std::io::{self, Write};
use std::process::{Command, Stdio};

/// Something text can be copied to.
pub trait Clipboard {
    /// Copy `text`; the error is a short message for the hint bar.
    fn copy(&mut self, text: &str) -> Result<(), String>;
}

/// Perform the copy requested by `y`, if any, and report the result in the
/// hint bar: `Copied <text>` or `Clipboard unavailable: <why>`.
pub fn perform_pending(app: &mut App, clipboard: &mut dyn Clipboard) {
    let Some(text) = app.take_copy() else {
        return;
    };
    match clipboard.copy(&text) {
        Ok(()) => app.set_status(format!("Copied {text}")),
        Err(why) => app.set_status(format!("Clipboard unavailable: {why}")),
    }
}

/// The real clipboard: system tools with an OSC 52 fallback (see module docs).
#[derive(Debug, Default)]
pub struct SystemClipboard;

impl Clipboard for SystemClipboard {
    fn copy(&mut self, text: &str) -> Result<(), String> {
        let over_ssh =
            std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some();
        if !over_ssh && copy_with_tool(text) {
            return Ok(());
        }
        write_osc52(&mut io::stdout(), text).map_err(|e| e.to_string())?;
        // Over SSH there is no other way; elsewhere it is the last resort.
        Ok(())
    }
}

/// Clipboard commands to try on this machine, with their arguments.
fn tool_candidates() -> Vec<(&'static str, &'static [&'static str])> {
    let mut tools: Vec<(&'static str, &'static [&'static str])> = Vec::new();
    if cfg!(target_os = "macos") {
        tools.push(("pbcopy", &[]));
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        tools.push(("wl-copy", &[]));
    }
    if std::env::var_os("DISPLAY").is_some() {
        tools.push(("xclip", &["-selection", "clipboard"]));
        tools.push(("xsel", &["--clipboard", "--input"]));
    }
    if cfg!(any(target_os = "windows", target_os = "linux")) {
        tools.push(("clip.exe", &[]));
    }
    tools
}

/// Pipe `text` into the first clipboard tool that works.
fn copy_with_tool(text: &str) -> bool {
    tool_candidates()
        .into_iter()
        .any(|(program, args)| pipe_to(program, args, text))
}

fn pipe_to(program: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let wrote = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    // Dropping stdin above closes the pipe so the tool can finish.
    child.wait().is_ok_and(|s| s.success()) && wrote
}

/// The OSC 52 "set clipboard" sequence for `text`.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

fn write_osc52(out: &mut impl Write, text: &str) -> io::Result<()> {
    out.write_all(osc52(text).as_bytes())?;
    out.flush()
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::{update, Action, Focus};
    use crate::tui::testing as fixtures;
    use crate::tui::ModeArg;

    #[derive(Default)]
    struct Fake {
        copied: Vec<String>,
        fail: Option<&'static str>,
    }

    impl Clipboard for Fake {
        fn copy(&mut self, text: &str) -> Result<(), String> {
            match self.fail {
                Some(why) => Err(why.to_string()),
                None => {
                    self.copied.push(text.to_string());
                    Ok(())
                }
            }
        }
    }

    /// `me` is selected; it has the parent `p`. Wide enough for the split view.
    fn app(width: u16) -> App {
        let snap = fixtures::snapshot_with(
            vec![
                fixtures::issue("me", "Me").priority(0).child_of("p"),
                fixtures::issue("p", "Parent").priority(1),
            ],
            vec![fixtures::triage_item("20251104T001800-k3f9.md", "idea")],
        );
        let mut a = App::from_snapshot(ModeArg::Issues, snap);
        update(&mut a, Action::Resize(width, 40));
        a
    }

    fn copy(a: &mut App) -> Option<String> {
        update(a, Action::CopyId);
        let mut fake = Fake::default();
        perform_pending(a, &mut fake);
        fake.copied.pop()
    }

    #[test]
    fn grid_copies_the_selected_issue() {
        let mut a = app(100);
        assert_eq!(copy(&mut a).as_deref(), Some("me"));
        assert_eq!(a.status.as_deref(), Some("Copied me"));
    }

    #[test]
    fn split_with_detail_focused_copies_the_displayed_issue() {
        let mut a = app(170);
        a.focus = Focus::Detail;
        assert_eq!(copy(&mut a).as_deref(), Some("me"));
        // Following a link: the displayed issue is the top of the stack,
        // while the grid selection stays put.
        a.detail.push("p".into());
        assert_eq!(copy(&mut a).as_deref(), Some("p"));
        a.focus = Focus::Grid;
        assert_eq!(copy(&mut a).as_deref(), Some("me"));
    }

    #[test]
    fn full_screen_with_a_link_stack_copies_the_top_of_the_stack() {
        let mut a = app(100);
        a.full_screen_detail = true;
        a.detail.push("p".into());
        assert_eq!(copy(&mut a).as_deref(), Some("p"));
        a.detail.push("me".into());
        assert_eq!(copy(&mut a).as_deref(), Some("me"));
    }

    #[test]
    fn triage_copies_the_filename() {
        let mut a = app(100);
        update(&mut a, Action::CycleMode);
        a.selection.triage = Some("20251104T001800-k3f9.md".into());
        assert_eq!(copy(&mut a).as_deref(), Some("20251104T001800-k3f9.md"));
    }

    #[test]
    fn nothing_selected_copies_nothing_and_says_so() {
        let mut a = App::from_snapshot(
            ModeArg::Issues,
            fixtures::snapshot_of(Vec::<fixtures::IssueBuilder>::new()),
        );
        assert_eq!(copy(&mut a), None);
        assert_eq!(a.status.as_deref(), Some("Nothing to copy"));
    }

    #[test]
    fn failure_shows_a_hint_bar_message() {
        let mut a = app(100);
        update(&mut a, Action::CopyId);
        let mut fake = Fake {
            fail: Some("no tool"),
            ..Fake::default()
        };
        perform_pending(&mut a, &mut fake);
        assert_eq!(a.status.as_deref(), Some("Clipboard unavailable: no tool"));
        assert!(fake.copied.is_empty());
        // The request was consumed.
        perform_pending(&mut a, &mut fake);
    }

    #[test]
    fn other_actions_never_request_a_copy() {
        let mut a = app(100);
        update(&mut a, Action::Tick);
        assert_eq!(a.take_copy(), None);
    }

    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (input, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), want, "{input:?}");
        }
    }

    #[test]
    fn osc52_sequence_format() {
        assert_eq!(osc52("tazk-1"), "\x1b]52;c;dGF6ay0x\x07");
        let mut out = Vec::new();
        write_osc52(&mut out, "foo").unwrap();
        assert_eq!(out, b"\x1b]52;c;Zm9v\x07");
    }
}
