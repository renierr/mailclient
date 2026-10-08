//! The message list's client-side filters: the quick filters (unread,
//! starred, attachments, date range) AND-ed with the short typed filter,
//! over rows the list already holds. They never fetch. Both frontends hand
//! the whole row set over once and paint the rows this keeps.

use serde::Deserialize;

use super::{date_bounds_pass, normalise_date};

/// The active list filters. Empty strings and `false` are unset.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ListFilter {
    pub unread: bool,
    pub starred: bool,
    pub attachments: bool,
    /// Inclusive `YYYY-MM-DD` lower day bound.
    pub after: String,
    /// Exclusive `YYYY-MM-DD` upper day bound.
    pub before: String,
    /// The short typed filter ([`super::filter_matches`]); search hits pass
    /// `""`, since their query already matched them.
    pub text: String,
}

/// The row fields the filters read, as the list feeds carry them.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ListRow {
    pub date_raw: Option<String>,
    pub unread: bool,
    pub starred: bool,
    pub has_attachments: bool,
    pub subject: String,
    pub from: String,
    pub from_name: String,
    pub snippet: String,
}

impl ListFilter {
    /// Indexes of the `rows` that pass every active filter, in order.
    #[must_use]
    pub fn keep(&self, rows: &[ListRow]) -> Vec<usize> {
        let after = normalise_date(self.after.trim());
        let before = normalise_date(self.before.trim());
        let text = self.text.trim().to_lowercase();
        rows.iter()
            .enumerate()
            .filter(|(_, r)| {
                (!self.unread || r.unread)
                    && (!self.starred || r.starred)
                    && (!self.attachments || r.has_attachments)
                    && date_bounds_pass(r.date_raw.as_deref(), after.as_deref(), before.as_deref())
                    && (text.is_empty()
                        || [&r.subject, &r.from, &r.from_name, &r.snippet]
                            .iter()
                            .any(|f| f.to_lowercase().contains(&text)))
            })
            .map(|(i, _)| i)
            .collect()
    }
}

/// [`ListFilter::keep`] over JSON: a filter object and an array of feed
/// rows (extra fields are ignored) in, a JSON array of kept indexes out.
pub fn keep_json(filter_json: &str, rows_json: &str) -> serde_json::Result<String> {
    let filter: ListFilter = serde_json::from_str(filter_json)?;
    let rows: Vec<ListRow> = serde_json::from_str(rows_json)?;
    serde_json::to_string(&filter.keep(&rows))
}

/// A custom date range as typed: both bounds normalised (`2026-9-1` reads
/// as `2026-09-01`), or why it cannot be applied. Both empty is valid and
/// clears the date filter.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DateRange {
    pub after: String,
    pub before: String,
    /// `""` when the range can be applied.
    pub error: String,
}

/// Check a typed After (inclusive) / Before (exclusive) pair.
#[must_use]
pub fn date_range_check(after: &str, before: &str) -> DateRange {
    let read = |s: &str| -> Result<String, ()> {
        let s = s.trim();
        if s.is_empty() {
            Ok(String::new())
        } else {
            normalise_date(s).ok_or(())
        }
    };
    let fail = |error: &str| DateRange {
        after: after.trim().to_string(),
        before: before.trim().to_string(),
        error: error.to_string(),
    };
    let Ok(a) = read(after) else {
        return fail("After is not a date (YYYY-MM-DD)");
    };
    let Ok(b) = read(before) else {
        return fail("Before is not a date (YYYY-MM-DD)");
    };
    if !a.is_empty() && !b.is_empty() && a >= b {
        return fail("Before must be later than After");
    }
    DateRange {
        after: a,
        before: b,
        error: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(subject: &str, date: &str, unread: bool, starred: bool, files: bool) -> ListRow {
        ListRow {
            date_raw: (!date.is_empty()).then(|| date.to_string()),
            unread,
            starred,
            has_attachments: files,
            subject: subject.into(),
            from: "a@example.com".into(),
            ..ListRow::default()
        }
    }

    fn rows() -> Vec<ListRow> {
        vec![
            row("Invoice", "2026-09-02T10:00:00Z", true, false, true),
            row("Hello", "2026-09-10T10:00:00Z", false, true, false),
            row("No date", "", true, true, true),
        ]
    }

    #[test]
    fn no_filter_keeps_everything() {
        assert_eq!(ListFilter::default().keep(&rows()), vec![0, 1, 2]);
    }

    #[test]
    fn quick_filters_and_together() {
        let f = ListFilter {
            unread: true,
            attachments: true,
            ..ListFilter::default()
        };
        assert_eq!(f.keep(&rows()), vec![0, 2]);
        let f = ListFilter {
            unread: true,
            starred: true,
            ..ListFilter::default()
        };
        assert_eq!(f.keep(&rows()), vec![2]);
    }

    #[test]
    fn date_bounds_drop_undated_rows_and_read_lenient_days() {
        let f = ListFilter {
            after: "2026-9-1".into(),
            before: "2026-09-05".into(),
            ..ListFilter::default()
        };
        assert_eq!(f.keep(&rows()), vec![0]);
    }

    #[test]
    fn text_matches_any_field_case_insensitively() {
        let f = ListFilter {
            text: " INV ".into(),
            ..ListFilter::default()
        };
        assert_eq!(f.keep(&rows()), vec![0]);
        let f = ListFilter {
            text: "example".into(),
            ..ListFilter::default()
        };
        assert_eq!(f.keep(&rows()), vec![0, 1, 2]);
    }

    #[test]
    fn keep_json_ignores_extra_fields() {
        let rows = r#"[{"uid":1,"subject":"a","unread":true},{"uid":2,"subject":"b"}]"#;
        assert_eq!(keep_json(r#"{"unread":true}"#, rows).unwrap(), "[0]");
        assert!(keep_json("{", rows).is_err());
    }

    #[test]
    fn date_range_normalises_and_explains() {
        let ok = date_range_check(" 2026-1-5 ", "2026-02-01");
        assert_eq!(
            (ok.after.as_str(), ok.before.as_str(), ok.error.as_str()),
            ("2026-01-05", "2026-02-01", "")
        );
        assert_eq!(date_range_check("", "").error, "");
        assert_eq!(date_range_check("", "2026-02-01").after, "");
        assert_eq!(
            date_range_check("soon", "").error,
            "After is not a date (YYYY-MM-DD)"
        );
        assert_eq!(
            date_range_check("", "2026-13-01").error,
            "Before is not a date (YYYY-MM-DD)"
        );
        assert_eq!(
            date_range_check("2026-02-01", "2026-02-01").error,
            "Before must be later than After"
        );
    }
}
