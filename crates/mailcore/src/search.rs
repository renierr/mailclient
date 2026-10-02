//! The search query language, read once here so the local FTS5 index and
//! the server-side IMAP SEARCH understand a query the same way.
//!
//! Words are AND-ed and match word starts (`inv` finds "invoice").
//! `"two words"` is an exact phrase, `-word` excludes, and `from:`, `to:`
//! (To/Cc/Bcc) and `subject:` limit a term to one field. Everything else is
//! literal text: typed FTS operators, `*` and unknown prefixes cannot change
//! what a query means or make it fail.

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

const FIELDS: [(&str, SearchField); 3] = [
    ("from:", SearchField::From),
    ("to:", SearchField::To),
    ("subject:", SearchField::Subject),
];

/// Split free-text input into terms. Never fails: whatever cannot be a
/// term (a lone `-`, `***`, an empty phrase) is dropped.
#[must_use]
pub fn parse_query(raw: &str) -> Vec<SearchTerm> {
    let mut terms = Vec::new();
    let mut rest = raw.trim_start();
    while !rest.is_empty() {
        let negated = rest.starts_with('-');
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
    terms
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
    parse_query(raw).iter().any(|t| !t.negated)
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
}
