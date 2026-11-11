//! Validate identifiers before turning them into paths in the issue store.

use crate::types::ID_PREFIX;
use anyhow::{Context, Result};
use std::path::Path;

/// Which of the two canonical suffix formats an issue ID uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IdKind {
    /// `[1-9][0-9]*`
    Numeric,
    /// `[0-9a-z]*[a-z][0-9a-z]*` (at least one letter)
    Hash,
}

/// A canonical issue ID: `tz-` plus a numeric or hash suffix.
///
/// The two suffix formats cannot overlap, so the kind is exact. The strict
/// pattern cannot produce path separators, Windows device names or other
/// unportable filenames, so a parsed ID is always safe as a filename stem.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IssueId<'a>(&'a str);

impl<'a> IssueId<'a> {
    pub(crate) fn parse(value: &'a str) -> Result<Self> {
        let valid = value
            .strip_prefix(ID_PREFIX)
            .is_some_and(|suffix| Self::classify(suffix).is_some());
        anyhow::ensure!(
            valid,
            "Invalid issue ID {value:?}: expected {ID_PREFIX}<n> or {ID_PREFIX}<hash> \
             (numeric: [1-9][0-9]*; hash: lowercase base36 with at least one letter)"
        );
        Ok(Self(value))
    }

    fn classify(suffix: &str) -> Option<IdKind> {
        if !suffix
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
            || suffix.is_empty()
        {
            return None;
        }
        if suffix.bytes().all(|b| b.is_ascii_digit()) {
            (!suffix.starts_with('0')).then_some(IdKind::Numeric)
        } else {
            Some(IdKind::Hash)
        }
    }

    pub(crate) fn as_str(self) -> &'a str {
        self.0
    }

    /// The part after `tz-`.
    pub(crate) fn suffix(self) -> &'a str {
        &self.0[ID_PREFIX.len()..]
    }

    pub(crate) fn kind(self) -> IdKind {
        Self::classify(self.suffix()).expect("validated in parse")
    }

    /// The number of a numeric ID (`None` for hash IDs, or a number too large
    /// for `u32`).
    pub(crate) fn numeric_suffix(self) -> Option<u32> {
        match self.kind() {
            IdKind::Numeric => self.suffix().parse().ok(),
            IdKind::Hash => None,
        }
    }
}

/// The number of a numeric ID string; `None` for hash IDs and non-canonical
/// strings.
pub(crate) fn numeric_id_suffix(id: &str) -> Option<u32> {
    IssueId::parse(id).ok()?.numeric_suffix()
}

/// A single portable filename component (used for triage item names).
#[derive(Clone, Copy, Debug)]
pub(crate) struct PortableName<'a>(&'a str);

impl<'a> PortableName<'a> {
    pub(crate) fn parse(value: &'a str) -> Result<Self> {
        let stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
        let device = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
        if value.is_empty()
            || matches!(value, "." | "..")
            || value.trim() != value
            || value.ends_with('.')
            || device
            || value
                .chars()
                .any(|c| c.is_control() || r#"<>:"/\|?*"#.contains(c))
        {
            anyhow::bail!("Invalid name {value:?}: expected one portable filename component");
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(self) -> &'a str {
        self.0
    }
}

/// Check existing ancestors too, so a symlink or Windows junction cannot send
/// an otherwise valid ID outside the explicitly selected database directory.
pub(crate) fn ensure_contained(root: &Path, path: &Path) -> Result<()> {
    anyhow::ensure!(
        path.starts_with(root),
        "Path is outside the database: {}",
        path.display()
    );
    let canonical_root = root
        .canonicalize()
        .context("Failed to resolve database directory")?;
    let mut existing = path;
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                existing = existing.parent().context("Path has no existing ancestor")?;
            }
            Err(error) => return Err(error).context("Failed to inspect database path"),
        }
    }
    let resolved = existing
        .canonicalize()
        .with_context(|| format!("Failed to resolve {}", existing.display()))?;
    anyhow::ensure!(
        resolved.starts_with(canonical_root),
        "Path escapes the database: {}",
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_canonical_ids() {
        for id in ["tz-1", "tz-42", "tz-a1b", "tz-0k3x", "tz-a", "tz-10"] {
            assert!(IssueId::parse(id).is_ok(), "{id}");
        }
    }

    #[test]
    fn parse_rejects_non_canonical_ids() {
        for id in [
            "tz-0", "tz-01", "TZ-1", "tz-A1b", "tz-", "tz-a-b", "x-1", "1", "../x", "con",
            "tz-1.md", "tz-1 ", "tz-é", "tazk-1", "",
        ] {
            assert!(IssueId::parse(id).is_err(), "{id}");
        }
    }

    #[test]
    fn kind_is_exact() {
        let kind = |id| IssueId::parse(id).unwrap().kind();
        assert_eq!(kind("tz-1"), IdKind::Numeric);
        assert_eq!(kind("tz-120"), IdKind::Numeric);
        assert_eq!(kind("tz-a1b"), IdKind::Hash);
        assert_eq!(kind("tz-0k3x"), IdKind::Hash);
        assert_eq!(kind("tz-1e5"), IdKind::Hash);
    }

    #[test]
    fn numeric_suffix_only_for_numeric_ids() {
        assert_eq!(numeric_id_suffix("tz-42"), Some(42));
        assert_eq!(numeric_id_suffix("tz-a3f9"), None);
        assert_eq!(numeric_id_suffix("tz-99999999999"), None);
        assert_eq!(numeric_id_suffix("foo-7"), None);
    }

    #[test]
    fn portable_name_rejects_unportable_names() {
        for name in [
            "", ".", "..", "con", "NUL.md", "a/b", "a:b", "x.", " x", "COM1",
        ] {
            assert!(PortableName::parse(name).is_err(), "{name:?}");
        }
        assert!(PortableName::parse("20251104T001800-k3f9.md").is_ok());
    }
}
