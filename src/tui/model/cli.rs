//! Command-line filter flags -> initial [`Filters`] (spec §2.1).
//!
//! Each `VALUES` flag is repeatable and/or comma-separated (clap splits on
//! commas; empty parts are ignored here). The literal `none` selects issues
//! with the property unset (role, effort). A flag that was not given leaves
//! its filter at the default, which for status is "everything except closed".
//! Pure: no rendering, no terminal access.

use super::filter::{FilterSet, FilterValue, Filters};
use crate::types::{parse_effort, IssueType, Role, Status};
use anyhow::{Context, Result};

/// The raw flag values, as given on the command line.
#[derive(Debug, Clone, Copy, Default)]
pub struct FilterFlags<'a> {
    pub role: &'a [String],
    pub status: &'a [String],
    pub issue_type: &'a [String],
    pub priority: &'a [String],
    pub effort: &'a [String],
}

impl Filters {
    /// The initial filters for the given flags. Errors name the flag and the
    /// bad value.
    pub fn from_flags(flags: FilterFlags) -> Result<Self> {
        let mut filters = Filters::default();
        if let Some(set) = parse_set("--role", flags.role, |s| {
            parse_optional(s, |s| s.parse::<Role>())
        })? {
            filters.role = set;
        }
        if let Some(set) = parse_set("--status", flags.status, |s| s.parse::<Status>())? {
            filters.status = set;
        }
        if let Some(set) = parse_set("--type", flags.issue_type, |s| s.parse::<IssueType>())? {
            filters.issue_type = set;
        }
        if let Some(set) = parse_set("--priority", flags.priority, parse_priority)? {
            filters.priority = set;
        }
        if let Some(set) = parse_set("--effort", flags.effort, |s| {
            parse_optional(s, parse_effort)
        })? {
            filters.effort = set;
        }
        Ok(filters)
    }
}

/// `none` -> `None`, anything else through `parse`.
fn parse_optional<T>(s: &str, parse: impl Fn(&str) -> Result<T>) -> Result<Option<T>> {
    if s == "none" {
        Ok(None)
    } else {
        parse(s).map(Some)
    }
}

/// `1` or `P1` (priorities may be negative or above 4).
fn parse_priority(s: &str) -> Result<i32> {
    let digits = s.strip_prefix('P').unwrap_or(s);
    digits
        .parse::<i32>()
        .map_err(|_| anyhow::anyhow!("Invalid priority: '{s}'. Must be an integer such as 0 or P1"))
}

/// Parse every value of a flag into an "only these" set; `None` if the flag
/// was not given (or had only empty values).
fn parse_set<T: FilterValue>(
    flag: &str,
    values: &[String],
    parse: impl Fn(&str) -> Result<T>,
) -> Result<Option<FilterSet<T>>> {
    let mut parsed = Vec::new();
    for part in values.iter().flat_map(|v| v.split(',')) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        parsed.push(parse(part).with_context(|| format!("invalid value for {flag}"))?);
    }
    Ok((!parsed.is_empty()).then(|| FilterSet::only(parsed)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn only_role(values: &[&str]) -> Result<Filters> {
        Filters::from_flags(FilterFlags {
            role: &v(values),
            ..Default::default()
        })
    }

    #[test]
    fn no_flags_gives_the_default_filters() {
        assert_eq!(
            Filters::from_flags(FilterFlags::default()).unwrap(),
            Filters::default()
        );
    }

    #[test]
    fn priority_repeats_and_commas_combine() {
        let f = Filters::from_flags(FilterFlags {
            priority: &v(&["0,1", "3"]),
            ..Default::default()
        })
        .unwrap();
        for p in [0, 1, 3] {
            assert!(f.priority.allows(&p), "P{p}");
        }
        for p in [2, 4, 7, -1] {
            assert!(!f.priority.allows(&p), "P{p}");
        }
    }

    #[test]
    fn priority_accepts_p_prefix_and_spaces() {
        let f = Filters::from_flags(FilterFlags {
            priority: &v(&["P1, 2"]),
            ..Default::default()
        })
        .unwrap();
        assert!(f.priority.allows(&1) && f.priority.allows(&2) && !f.priority.allows(&0));
    }

    #[test]
    fn role_none_and_agent() {
        let f = only_role(&["none,agent"]).unwrap();
        assert!(f.role.allows(&None));
        assert!(f.role.allows(&Some(Role::Agent)));
        assert!(!f.role.allows(&Some(Role::Human)));
    }

    #[test]
    fn effort_none_selects_only_unset() {
        let f = Filters::from_flags(FilterFlags {
            effort: &v(&["none"]),
            ..Default::default()
        })
        .unwrap();
        assert!(f.effort.allows(&None));
        for e in 1..=5 {
            assert!(!f.effort.allows(&Some(e)));
        }
    }

    #[test]
    fn effort_values_and_type() {
        let f = Filters::from_flags(FilterFlags {
            effort: &v(&["1", "3"]),
            issue_type: &v(&["bug,spec"]),
            ..Default::default()
        })
        .unwrap();
        assert!(f.effort.allows(&Some(1)) && f.effort.allows(&Some(3)));
        assert!(!f.effort.allows(&Some(2)) && !f.effort.allows(&None));
        assert!(f.issue_type.allows(&IssueType::Bug) && f.issue_type.allows(&IssueType::Spec));
        assert!(!f.issue_type.allows(&IssueType::Task));
    }

    #[test]
    fn status_defaults_to_all_but_closed_and_can_be_overridden() {
        let default = Filters::from_flags(FilterFlags::default()).unwrap();
        assert!(!default.status.allows(&Status::Closed));
        assert!(default.status.allows(&Status::Open));
        let f = Filters::from_flags(FilterFlags {
            status: &v(&["closed"]),
            ..Default::default()
        })
        .unwrap();
        assert!(f.status.allows(&Status::Closed));
        assert!(!f.status.allows(&Status::Open));
    }

    #[test]
    fn empty_values_are_ignored() {
        let f = only_role(&["", ","]).unwrap();
        assert_eq!(f, Filters::default());
    }

    fn error_of(flags: FilterFlags) -> String {
        format!("{:#}", Filters::from_flags(flags).unwrap_err())
    }

    #[test]
    fn invalid_values_name_the_flag_and_the_value() {
        let bad = v(&["foo"]);
        let e = error_of(FilterFlags {
            status: &bad,
            ..Default::default()
        });
        assert!(e.contains("--status") && e.contains("'foo'"), "{e}");
        let e = error_of(FilterFlags {
            role: &bad,
            ..Default::default()
        });
        assert!(e.contains("--role") && e.contains("'foo'"), "{e}");
        let e = error_of(FilterFlags {
            issue_type: &bad,
            ..Default::default()
        });
        assert!(e.contains("--type") && e.contains("'foo'"), "{e}");
        let e = error_of(FilterFlags {
            priority: &bad,
            ..Default::default()
        });
        assert!(e.contains("--priority") && e.contains("'foo'"), "{e}");
        let e = error_of(FilterFlags {
            effort: &v(&["9"]),
            ..Default::default()
        });
        assert!(e.contains("--effort") && e.contains("'9'"), "{e}");
    }

    #[test]
    fn one_bad_value_among_good_ones_is_an_error() {
        let e = error_of(FilterFlags {
            status: &v(&["open,bogus"]),
            ..Default::default()
        });
        assert!(e.contains("'bogus'"), "{e}");
    }
}
