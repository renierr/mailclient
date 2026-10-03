//! The search query language, read once here so the local FTS5 index and
//! the server-side IMAP SEARCH understand a query the same way.
//!
//! Words are AND-ed and match word starts (`inv` finds "invoice").
//! `"two words"` is an exact phrase, `-word` excludes, and `from:`, `to:`
//! (To/Cc/Bcc) and `subject:` limit a term to one field. Everything else is
//! literal text: typed FTS operators, `*` and unknown prefixes cannot change
//! what a query means or make it fail.
//!
//! Filter tokens narrow the result instead of adding text: `is:unread`,
//! `is:read`, `is:starred`/`is:flagged`, `is:unstarred`/`is:unflagged`,
//! `has:attachment(s)` (each negatable with `-`), and `after:`/`since:`
//! (inclusive) and `before:` (exclusive) with a `YYYY-MM-DD` date. Dates
//! are whole UTC days, compared against the stored UTC message date. A
//! filter token is written without a space after the colon (`is: read` is
//! two words of text); only the date keys also accept `after: 2026-01-01`,
//! since a full date can never be a word the user meant as text.

/// The search syntax as both frontends show it (search field tooltip).
pub const SYNTAX_HELP: &str = "All words must match, by word start (inv finds invoice)\n\
\"exact phrase\"   -exclude\n\
from:name   to:address   subject:word\n\
is:unread   is:read   is:starred   has:attachment   (-is:read excludes)\n\
after:2026-01-31   before:2026-02-28";

/// The fields a term must match in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchField {
    /// Subject, sender, recipients and body.
    Any,
    /// Sender address and display name.
    From,
    /// To, Cc and Bcc addresses.
    To,
    Subject,
}

/// One term of a parsed query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchTerm {
    pub field: SearchField,
    /// One word, or a phrase's words joined by single spaces. Never holds
    /// `"` or `*`, always holds a letter or digit.
    pub text: String,
    /// Quoted: these words in this order, the last one whole.
    pub phrase: bool,
    /// `-` prefix: the message must not match.
    pub negated: bool,
}

/// Structured filters extracted from query tokens (`is:unread`, `has:attachment`, `after:...`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchFilters {
    /// `Some(true)` for unread, `Some(false)` for read.
    pub unread: Option<bool>,
    /// `Some(true)` for starred/flagged, `Some(false)` for unstarred.
    pub starred: Option<bool>,
    /// `Some(true)` for messages with attachments, `Some(false)` for messages without.
    pub has_attachments: Option<bool>,
    /// `after:` / `since:` UTC day, normalised to `YYYY-MM-DD` (inclusive).
    pub after: Option<String>,
    /// `before:` UTC day, normalised to `YYYY-MM-DD` (exclusive).
    pub before: Option<String>,
}

impl SearchFilters {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.unread.is_none()
            && self.starred.is_none()
            && self.has_attachments.is_none()
            && self.after.is_none()
            && self.before.is_none()
    }
}

/// The result of parsing a raw query into text search terms and structured filters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedQuery {
    pub terms: Vec<SearchTerm>,
    pub filters: SearchFilters,
}

const FIELDS: [(&str, SearchField); 3] = [
    ("from:", SearchField::From),
    ("to:", SearchField::To),
    ("subject:", SearchField::Subject),
];

/// `s` as a normalised `YYYY-MM-DD` day, or `None` when it is not a date
/// with a four-digit year. chrono alone is lenient (`2026-9-1`, `+2026-09-01`
/// and `26-09-01` all parse), and the result is compared as text, so it is
/// re-printed in the one canonical form.
fn normalise_date(s: &str) -> Option<String> {
    let year = s.split('-').next()?;
    if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let d = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
    Some(d.format("%Y-%m-%d").to_string())
}

fn is_date_key(key: &str) -> bool {
    ["after", "since", "before"]
        .iter()
        .any(|k| key.eq_ignore_ascii_case(k))
}

fn try_parse_filter_pair(key: &str, val: &str, negated: bool, filters: &mut SearchFilters) -> bool {
    if key.eq_ignore_ascii_case("is") {
        if val.eq_ignore_ascii_case("unread") {
            filters.unread = Some(!negated);
            return true;
        } else if val.eq_ignore_ascii_case("read") {
            filters.unread = Some(negated);
            return true;
        } else if val.eq_ignore_ascii_case("starred") || val.eq_ignore_ascii_case("flagged") {
            filters.starred = Some(!negated);
            return true;
        } else if val.eq_ignore_ascii_case("unstarred") || val.eq_ignore_ascii_case("unflagged") {
            filters.starred = Some(negated);
            return true;
        }
    } else if key.eq_ignore_ascii_case("has") {
        if val.eq_ignore_ascii_case("attachment") || val.eq_ignore_ascii_case("attachments") {
            filters.has_attachments = Some(!negated);
            return true;
        }
    } else if key.eq_ignore_ascii_case("after") || key.eq_ignore_ascii_case("since") {
        if let Some(d) = normalise_date(val) {
            filters.after = Some(d);
            return true;
        }
    } else if key.eq_ignore_ascii_case("before") {
        if let Some(d) = normalise_date(val) {
            filters.before = Some(d);
            return true;
        }
    }
    false
}

/// Split free-text input into terms and structured filters.
#[must_use]
pub fn parse_query_full(raw: &str) -> ParsedQuery {
    let mut terms = Vec::new();
    let mut filters = SearchFilters::default();
    let mut rest = raw.trim_start();
    while !rest.is_empty() {
        let negated = rest.starts_with('-');
        let candidate = if negated { &rest[1..] } else { rest };

        // Quoted phrase is always text.
        if !candidate.starts_with('"') {
            let end = candidate
                .find(char::is_whitespace)
                .unwrap_or(candidate.len());
            let word = &candidate[..end];
            if let Some((k, v)) = word.split_once(':') {
                if v.is_empty() && is_date_key(k) {
                    // `after: 2026-01-01`: only a valid date is taken, so
                    // the next word is never swallowed as a filter value.
                    let after_colon = candidate[end..].trim_start();
                    let val_end = after_colon
                        .find(char::is_whitespace)
                        .unwrap_or(after_colon.len());
                    let next_word = &after_colon[..val_end];
                    if !next_word.is_empty()
                        && try_parse_filter_pair(k, next_word, negated, &mut filters)
                    {
                        rest = after_colon[val_end..].trim_start();
                        continue;
                    }
                } else if try_parse_filter_pair(k, v, negated, &mut filters) {
                    rest = candidate[end..].trim_start();
                    continue;
                }
            }
        }

        if negated {
            rest = &rest[1..];
        }
        let mut field = SearchField::Any;
        if let Some((f, r)) = strip_field(rest) {
            field = f;
            // `from: anna` reads like `from:anna`.
            rest = r.trim_start();
        }
        let (text, phrase, after) = if let Some(r) = rest.strip_prefix('"') {
            // An unclosed quote runs to the end of the input.
            let end = r.find('"').unwrap_or(r.len());
            let words: Vec<String> = r[..end].split_whitespace().map(clean).collect();
            (words.join(" "), true, r.get(end + 1..).unwrap_or(""))
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            (clean(&rest[..end]), false, &rest[end..])
        };
        rest = after.trim_start();
        if text.chars().any(char::is_alphanumeric) {
            terms.push(SearchTerm {
                field,
                text,
                phrase,
                negated,
            });
        }
    }
    ParsedQuery { terms, filters }
}

/// Split free-text input into terms. Never fails: whatever cannot be a
/// term (a lone `-`, `***`, an empty phrase) is dropped.
#[must_use]
pub fn parse_query(raw: &str) -> Vec<SearchTerm> {
    parse_query_full(raw).terms
}

fn strip_field(s: &str) -> Option<(SearchField, &str)> {
    FIELDS.iter().find_map(|&(name, field)| {
        let head = s.get(..name.len())?;
        head.eq_ignore_ascii_case(name)
            .then(|| (field, &s[name.len()..]))
    })
}

/// Quotes delimit phrases and `*` is implied: neither is ever searched for.
fn clean(word: &str) -> String {
    word.chars().filter(|c| !matches!(c, '"' | '*')).collect()
}

/// True when the query has something to look for: exclusions alone match
/// nothing (there is no "everything except" search).
#[must_use]
pub fn is_searchable(raw: &str) -> bool {
    let q = parse_query_full(raw);
    q.terms.iter().any(|t| !t.negated) || !q.filters.is_empty()
}

/// The FTS5 MATCH expression for `raw`, or `None` when nothing is
/// searchable. Every term is a quoted FTS string, so the tokenizer sees
/// only text; words are prefix matches, phrases are exact.
#[must_use]
pub fn fts_query(raw: &str) -> Option<String> {
    let terms = parse_query(raw);
    let mut query = terms
        .iter()
        .filter(|t| !t.negated)
        .map(fts_term)
        .collect::<Vec<_>>()
        .join(" ");
    if query.is_empty() {
        return None;
    }
    for t in terms.iter().filter(|t| t.negated) {
        query = format!("({query}) NOT {}", fts_term(t));
    }
    Some(query)
}

/// The excluded (`-`) terms of `raw` as one FTS5 MATCH expression that hits
/// any of them, quoted like [`fts_query`]. Used when no positive term is
/// left to carry the `NOT`s: filter-only searches drop these rows instead.
#[must_use]
pub fn fts_exclusions(raw: &str) -> Option<String> {
    let query = parse_query(raw)
        .iter()
        .filter(|t| t.negated)
        .map(fts_term)
        .collect::<Vec<_>>()
        .join(" OR ");
    (!query.is_empty()).then_some(query)
}

fn fts_term(t: &SearchTerm) -> String {
    let columns = match t.field {
        SearchField::Any => "",
        SearchField::From => "{from_addr from_name} : ",
        SearchField::To => "{to_addrs cc_addrs bcc_addrs} : ",
        SearchField::Subject => "{subject} : ",
    };
    let prefix = if t.phrase { "" } else { "*" };
    format!("{columns}\"{}\"{prefix}", t.text.replace('"', "\"\""))
}

/// How the search field runs what is typed into it. Both frontends follow
/// this, so the same text searches the same way everywhere.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SearchPlan {
    pub mode: SearchMode,
    /// The query as searched: trimmed.
    pub query: String,
    /// Most hits the index returns for one query ([`HIT_LIMIT`]); fewer
    /// local hits than this ask the server too.
    pub hit_limit: u64,
    /// How long typing must pause before a thin result asks the server.
    pub debounce_ms: u64,
}

/// What the search field does with its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// Nothing typed: the folder as it is.
    Off,
    /// One or two letters: an instant filter over the shown folder
    /// ([`filter_matches`]); too short to be worth the index.
    Filter,
    /// Three letters and more, or any filter token (`is:`, `has:`,
    /// `after:`, `before:`) at any length: the FTS index, topped up from
    /// the server when the index runs thin (fewer than [`HIT_LIMIT`] hits).
    Index,
}

/// Fewest characters that search the index instead of filtering.
pub const INDEX_MIN_CHARS: usize = 3;
/// Most hits one index query returns. Fewer than this and the index ran
/// thin: the server is asked too.
pub const HIT_LIMIT: u64 = 50;
/// Typing pause before a server search, in milliseconds.
pub const SERVER_DEBOUNCE_MS: u64 = 800;

/// The plan for `raw` as typed.
#[must_use]
pub fn plan(raw: &str) -> SearchPlan {
    let query = raw.trim();
    let parsed = parse_query_full(query);
    let mode = if query.is_empty() {
        SearchMode::Off
    } else if !parsed.filters.is_empty() {
        SearchMode::Index
    } else {
        if query.chars().count() < INDEX_MIN_CHARS {
            SearchMode::Filter
        } else {
            SearchMode::Index
        }
    };
    SearchPlan {
        mode,
        query: query.to_string(),
        hit_limit: HIT_LIMIT,
        debounce_ms: SERVER_DEBOUNCE_MS,
    }
}

/// The short-input filter: `query` (trimmed, any case) appears in the
/// subject, the sender address or name, or the preview. An empty query
/// matches everything.
#[must_use]
pub fn filter_matches(
    query: &str,
    subject: &str,
    from: &str,
    from_name: &str,
    snippet: &str,
) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty()
        || [subject, from, from_name, snippet]
            .iter()
            .any(|field| field.to_lowercase().contains(&q))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(field: SearchField, text: &str, phrase: bool, negated: bool) -> SearchTerm {
        SearchTerm {
            field,
            text: text.to_string(),
            phrase,
            negated,
        }
    }

    #[test]
    fn words_phrases_exclusions_and_fields() {
        use SearchField::*;
        assert_eq!(
            parse_query(
                r#"invoice "project  plan" -reminder FROM:anna to: bob subject:"q3 report""#
            ),
            vec![
                term(Any, "invoice", false, false),
                term(Any, "project plan", true, false),
                term(Any, "reminder", false, true),
                term(From, "anna", false, false),
                term(To, "bob", false, false),
                term(Subject, "q3 report", true, false),
            ]
        );
        assert_eq!(
            parse_query("-from:spam@example.com"),
            vec![term(From, "spam@example.com", false, true)]
        );
    }

    #[test]
    fn short_input_filters_and_longer_input_searches() {
        assert_eq!(plan("  ").mode, SearchMode::Off);
        assert_eq!(plan(" ab ").mode, SearchMode::Filter);
        assert_eq!(plan(" ab ").query, "ab");
        assert_eq!(plan("abc").mode, SearchMode::Index);
        assert_eq!(plan("äöü").mode, SearchMode::Index, "letters, not bytes");
        assert_eq!(
            plan("ab ").mode,
            SearchMode::Filter,
            "trailing space is not a letter"
        );
    }

    #[test]
    fn the_filter_looks_at_subject_sender_and_preview() {
        let m = |q| filter_matches(q, "Invoice", "anna@example.com", "Anna", "see attached");
        assert!(m(""));
        assert!(m(" IN "));
        assert!(m("an"));
        assert!(m("ttach"));
        assert!(!m("zz"));
    }

    #[test]
    fn junk_never_becomes_a_term() {
        assert!(parse_query("").is_empty());
        assert!(parse_query(r#"   "" *** - from: "  ""#).is_empty());
        // An unknown prefix is just text; an unclosed quote runs to the end.
        assert_eq!(parse_query("form:x")[0].text, "form:x");
        assert_eq!(parse_query(r#""open end"#)[0].text, "open end");
        assert_eq!(parse_query("foo*")[0].text, "foo");
    }

    #[test]
    fn exclusions_alone_are_not_searchable() {
        assert!(!is_searchable("-reminder"));
        assert!(is_searchable("invoice -reminder"));
        assert_eq!(fts_query("-reminder"), None);
        assert_eq!(fts_query("***"), None);
    }

    #[test]
    fn fts_quotes_every_term() {
        assert_eq!(fts_query("quick"), Some(r#""quick"*"#.to_string()));
        assert_eq!(
            fts_query(r#"AND OR NEAR "quoted" foo*"#),
            Some(r#""AND"* "OR"* "NEAR"* "quoted" "foo"*"#.to_string())
        );
        assert_eq!(
            fts_query(r#"invoice "project plan" from:anna -to:bob -subject:draft"#),
            Some(
                r#"(("invoice"* "project plan" {from_addr from_name} : "anna"*) NOT {to_addrs cc_addrs bcc_addrs} : "bob"*) NOT {subject} : "draft"*"#
                    .to_string()
            )
        );
    }

    #[test]
    fn filter_tokens_extracted_and_negations_handled() {
        let q = parse_query_full("is:unread has:attachment after:2026-01-01 before:2026-02-01");
        assert!(q.terms.is_empty());
        assert_eq!(q.filters.unread, Some(true));
        assert_eq!(q.filters.has_attachments, Some(true));
        assert_eq!(q.filters.after.as_deref(), Some("2026-01-01"));
        assert_eq!(q.filters.before.as_deref(), Some("2026-02-01"));

        let q2 = parse_query_full("is:read -is:starred -has:attachments is:flagged");
        assert_eq!(q2.filters.unread, Some(false));
        // Later tokens overwrite earlier ones
        assert_eq!(q2.filters.starred, Some(true));
        assert_eq!(q2.filters.has_attachments, Some(false));

        let q3 = parse_query_full("-is:unread is:unstarred since:2026-05-10");
        assert_eq!(q3.filters.unread, Some(false));
        assert_eq!(q3.filters.starred, Some(false));
        assert_eq!(q3.filters.after.as_deref(), Some("2026-05-10"));

        let q4 = parse_query_full(r#"invoice is:unread "is:read" from:bob"#);
        assert_eq!(q4.terms.len(), 3);
        assert_eq!(q4.terms[0].text, "invoice");
        assert_eq!(q4.terms[1].text, "is:read"); // quoted stays literal text
        assert!(q4.terms[1].phrase);
        assert_eq!(q4.terms[2].text, "bob");
        assert_eq!(q4.terms[2].field, SearchField::From);
        assert_eq!(q4.filters.unread, Some(true));

        // Only date keys take a spaced value; `is: read` is text.
        let q5 = parse_query_full("this is: read after: 2026-01-01");
        assert_eq!(
            q5.filters,
            SearchFilters {
                after: Some("2026-01-01".to_string()),
                ..SearchFilters::default()
            }
        );
        let texts: Vec<_> = q5.terms.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["this", "is:", "read"]);
        let q6 = parse_query_full("has: attachment before: soon");
        assert!(q6.filters.is_empty());
        let texts: Vec<_> = q6.terms.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["has:", "attachment", "before:", "soon"]);
    }

    #[test]
    fn dates_are_normalised_with_four_digit_years() {
        let q = parse_query_full("after:2026-9-1 before:2026-10-01");
        assert_eq!(q.filters.after.as_deref(), Some("2026-09-01"));
        assert_eq!(q.filters.before.as_deref(), Some("2026-10-01"));
        // Not a four-digit-year date: stays text, like an invalid date.
        for raw in ["after:+2026-09-01", "after:26-09-01", "before:2026-02-30"] {
            let q = parse_query_full(raw);
            assert!(q.filters.is_empty(), "{raw}");
            assert_eq!(q.terms.len(), 1, "{raw}");
        }
    }

    #[test]
    fn exclusions_become_one_or_expression() {
        assert_eq!(fts_exclusions("invoice is:unread"), None);
        assert_eq!(
            fts_exclusions("-newsletter is:unread -from:bob"),
            Some(r#""newsletter"* OR {from_addr from_name} : "bob"*"#.to_string())
        );
    }

    #[test]
    fn filter_tokens_trigger_searchable_and_index_mode() {
        assert!(is_searchable("is:unread"));
        assert!(is_searchable("-is:unread"));
        assert!(is_searchable("has:attachment"));
        assert!(is_searchable("after:2026-01-01"));

        // Mode is Index even when query text alone would be short
        assert_eq!(plan("is:read").mode, SearchMode::Index);
        assert_eq!(plan("has:attachment").mode, SearchMode::Index);
        assert_eq!(plan("a is:unread").mode, SearchMode::Index);
    }
}
