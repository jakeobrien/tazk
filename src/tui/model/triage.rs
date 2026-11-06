//! Triage item helpers for the triage grid and detail (spec §9.5, §10).
//! Pure logic (no rendering, no terminal access).

use crate::tui::ui::cell::{wrap_title, CONTENT_WIDTH};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};

/// Lines of text previewed in a triage cell.
pub const PREVIEW_LINES: usize = 2;

/// Readable creation time format (`2025-11-04 00:18`).
const DISPLAY_FORMAT: &str = "%Y-%m-%d %H:%M";

/// The creation time encoded in a standard item name
/// `<YYYYMMDDTHHMMSS>-<4 base36 chars>.md` (UTC), or `None` for any other
/// name (hand-dropped files, impossible dates).
pub fn parse_name_time(name: &str) -> Option<DateTime<Utc>> {
    let stem = name.strip_suffix(".md")?;
    let (stamp, suffix) = stem.split_once('-')?;
    let valid_stamp = stamp.len() == 15
        && stamp.bytes().enumerate().all(|(i, b)| match i {
            8 => b == b'T',
            _ => b.is_ascii_digit(),
        });
    let valid_suffix = suffix.len() == 4
        && suffix
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase());
    if !valid_stamp || !valid_suffix {
        return None;
    }
    // chrono accepts second 60 (leap second); a file name never means that.
    if &stamp[13..] >= "60" {
        return None;
    }
    NaiveDateTime::parse_from_str(stamp, "%Y%m%dT%H%M%S")
        .ok()
        .map(|t| t.and_utc())
}

/// [`parse_name_time`] formatted as `YYYY-MM-DD HH:MM` in `zone`.
pub fn created_label_in<Tz: TimeZone>(name: &str, zone: &Tz) -> Option<String>
where
    Tz::Offset: std::fmt::Display,
{
    parse_name_time(name).map(|t| t.with_timezone(zone).format(DISPLAY_FORMAT).to_string())
}

/// [`parse_name_time`] formatted as `YYYY-MM-DD HH:MM` in local time.
pub fn created_label(name: &str) -> Option<String> {
    created_label_in(name, &Local)
}

/// Cell preview of `body`: raw text (markdown is not rendered), whitespace
/// collapsed to single spaces, wrapped to at most `lines` lines of `width`
/// columns, the last ending in `…` only if text was cut.
pub fn preview(body: &str, width: usize, lines: usize) -> Vec<String> {
    wrap_title(body, width, lines)
}

/// The preview a triage cell shows: [`PREVIEW_LINES`] lines of cell width.
pub fn cell_preview(body: &str) -> Vec<String> {
    preview(body, CONTENT_WIDTH, PREVIEW_LINES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    #[test]
    fn standard_name_parses_as_utc() {
        let t = parse_name_time("20251104T001800-k3f9.md").unwrap();
        assert_eq!(t.to_rfc3339(), "2025-11-04T00:18:00+00:00");
    }

    #[test]
    fn label_uses_the_given_offset() {
        let utc = FixedOffset::east_opt(0).unwrap();
        assert_eq!(
            created_label_in("20251104T001800-k3f9.md", &utc).as_deref(),
            Some("2025-11-04 00:18")
        );
        let minus5 = FixedOffset::west_opt(5 * 3600).unwrap();
        assert_eq!(
            created_label_in("20251104T001800-k3f9.md", &minus5).as_deref(),
            Some("2025-11-03 19:18")
        );
        let plus9_30 = FixedOffset::east_opt(9 * 3600 + 1800).unwrap();
        assert_eq!(
            created_label_in("20251104T235900-0000.md", &plus9_30).as_deref(),
            Some("2025-11-05 09:29")
        );
    }

    #[test]
    fn local_label_matches_local_conversion() {
        crate::tui::testing::init();
        assert_eq!(
            created_label("20251104T001800-k3f9.md").as_deref(),
            Some("2025-11-04 00:18")
        );
    }

    #[test]
    fn other_names_have_no_time() {
        for name in [
            "notes.md",
            "idea-about-login.md",
            "",
            ".md",
            "20251104T001800.md",
            "20251104T001800-k3f9",
            "20251104T001800-k3f9.txt",
            "20251104T001800-k3f.md",
            "20251104T001800-k3f9x.md",
            "20251104T001800-K3F9.md",
            "20251104T001800-k3-9.md",
        ] {
            assert_eq!(parse_name_time(name), None, "{name:?}");
            assert_eq!(created_label(name), None, "{name:?}");
        }
    }

    #[test]
    fn malformed_timestamps_have_no_time() {
        for name in [
            "20251104X001800-k3f9.md", // wrong separator
            "2025110T0018000-k3f9.md", // misplaced T
            "20251304T001800-k3f9.md", // month 13
            "20251132T001800-k3f9.md", // day 32
            "20250230T001800-k3f9.md", // Feb 30
            "20251104T241800-k3f9.md", // hour 24
            "20251104T006100-k3f9.md", // minute 61
            "20251104T001860-k3f9.md", // second 60
            "2025110-T001800-k3f9.md", // sign/garbage in digits
            "+0251104T001800-k3f9.md",
            "20251104T0018-k3f9.md", // too short
        ] {
            assert_eq!(parse_name_time(name), None, "{name:?}");
        }
    }

    #[test]
    fn preview_collapses_whitespace() {
        assert_eq!(
            preview("  Login\n\nfails \t on   redirect \n", 30, 2),
            ["Login fails on redirect"]
        );
        assert!(preview("", 30, 2).is_empty());
        assert!(preview(" \n\t ", 30, 2).is_empty());
    }

    #[test]
    fn preview_wraps_to_two_lines_and_cuts_with_an_ellipsis_only_when_needed() {
        assert_eq!(
            preview("aaaa bbbb cccc dddd", 10, 2),
            ["aaaa bbbb", "cccc dddd"]
        );
        assert_eq!(
            preview("aaaa bbbb cccc dddd eeee", 10, 2),
            ["aaaa bbbb", "cccc dddd…"]
        );
        // Raw text: markdown is not interpreted.
        assert_eq!(preview("# Title **bold**", 30, 2), ["# Title **bold**"]);
    }

    #[test]
    fn cell_preview_uses_the_cell_content_width() {
        let long = "word ".repeat(40);
        let lines = cell_preview(&long);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with('…'));
        assert!(lines.iter().all(|l| l.chars().count() <= CONTENT_WIDTH));
        assert_eq!(cell_preview("short note"), ["short note"]);
    }
}
