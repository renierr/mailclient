//! Parsing iCalendar (`.ics`, RFC 5545) events for reader event preview cards.
//!
//! Provides pure-Rust extraction of `VEVENT` components without external
//! calendar dependencies. Handles line unfolding, parameter parsing, text
//! unescaping, and date-range formatting for UI display.

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

/// Parsed calendar event metadata for UI presentation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarEvent {
    pub summary: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub organizer: Option<String>,
    pub start_iso: Option<String>,
    pub end_iso: Option<String>,
    pub formatted_time: String,
    pub is_all_day: bool,
    pub status: Option<String>,
    pub is_cancelled: bool,
    pub method: Option<String>,
    pub attachment_id: Option<i64>,
}

/// Unfold RFC 5545 lines: CRLF or LF followed by a space or tab is deleted.
pub fn unfold(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if let Some(&'\n') = chars.peek() {
                chars.next();
                if let Some(&next_c) = chars.peek() {
                    if next_c == ' ' || next_c == '\t' {
                        chars.next();
                        continue;
                    }
                }
                out.push('\r');
                out.push('\n');
            } else {
                out.push('\r');
            }
        } else if c == '\n' {
            if let Some(&next_c) = chars.peek() {
                if next_c == ' ' || next_c == '\t' {
                    chars.next();
                    continue;
                }
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

/// Unescape RFC 5545 text value: `\,` -> `,`, `\;` -> `;`, `\n`/`\N` -> newline, `\\` -> `\`.
pub fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(';') => out.push(';'),
                Some(',') => out.push(','),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedTime {
    Date(NaiveDate),
    DateTimeUtc(DateTime<Utc>),
    DateTimeLocal(NaiveDateTime),
}

impl ParsedTime {
    fn parse(val: &str) -> Option<Self> {
        let val = val.trim();
        if val.ends_with('Z') || val.ends_with('z') {
            let s = &val[..val.len() - 1];
            if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S") {
                return Some(Self::DateTimeUtc(DateTime::from_naive_utc_and_offset(
                    ndt, Utc,
                )));
            }
        }
        if val.contains('T') {
            if let Ok(ndt) = NaiveDateTime::parse_from_str(val, "%Y%m%dT%H%M%S") {
                return Some(Self::DateTimeLocal(ndt));
            }
        }
        if val.len() == 8 {
            if let Ok(nd) = NaiveDate::parse_from_str(val, "%Y%m%d") {
                return Some(Self::Date(nd));
            }
        }
        None
    }

    fn to_iso(&self) -> String {
        match self {
            Self::Date(d) => d.format("%Y-%m-%d").to_string(),
            Self::DateTimeUtc(dt) => dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            Self::DateTimeLocal(ndt) => ndt.format("%Y-%m-%dT%H:%M:%S").to_string(),
        }
    }

    fn date(&self) -> NaiveDate {
        match self {
            Self::Date(d) => *d,
            Self::DateTimeUtc(dt) => dt.with_timezone(&chrono::Local).date_naive(),
            Self::DateTimeLocal(ndt) => ndt.date(),
        }
    }

    fn time_string(&self) -> Option<String> {
        match self {
            Self::Date(_) => None,
            Self::DateTimeUtc(dt) => {
                let local = dt.with_timezone(&chrono::Local);
                Some(local.format("%H:%M").to_string())
            }
            Self::DateTimeLocal(ndt) => Some(ndt.format("%H:%M").to_string()),
        }
    }
}

/// Parse simple ISO 8601 duration: `PT1H`, `PT30M`, `PT1H30M`, `P1D`.
fn parse_duration_seconds(d: &str) -> Option<i64> {
    let d = d.trim();
    if !d.starts_with('P') {
        return None;
    }
    let mut total_secs: i64 = 0;
    let mut in_time = false;
    let mut num_buf = String::new();

    for c in d[1..].chars() {
        if c == 'T' {
            in_time = true;
            continue;
        }
        if c.is_ascii_digit() {
            num_buf.push(c);
        } else {
            let val = num_buf.parse::<i64>().ok()?;
            num_buf.clear();
            match c {
                'D' => total_secs += val * 86400,
                'W' => total_secs += val * 86400 * 7,
                'H' if in_time => total_secs += val * 3600,
                'M' if in_time => total_secs += val * 60,
                'S' if in_time => total_secs += val,
                _ => {}
            }
        }
    }
    Some(total_secs)
}

fn format_date_range(start: &ParsedTime, end: Option<&ParsedTime>, is_all_day: bool) -> String {
    let start_date = start.date();
    let month_name = match start_date.month() {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "",
    };
    let weekday = match start_date.weekday() {
        chrono::Weekday::Mon => "Mon",
        chrono::Weekday::Tue => "Tue",
        chrono::Weekday::Wed => "Wed",
        chrono::Weekday::Thu => "Thu",
        chrono::Weekday::Fri => "Fri",
        chrono::Weekday::Sat => "Sat",
        chrono::Weekday::Sun => "Sun",
    };

    if is_all_day {
        let single_day = match end {
            None => true,
            Some(e) => {
                let e_date = e.date();
                e_date == start_date || e_date == start_date.succ_opt().unwrap_or(start_date)
            }
        };
        if single_day {
            return format!(
                "{weekday}, {month_name} {}, {} · All day",
                start_date.day(),
                start_date.year()
            );
        }
        if let Some(e) = end {
            let e_date = e.date();
            let end_month = match e_date.month() {
                1 => "Jan",
                2 => "Feb",
                3 => "Mar",
                4 => "Apr",
                5 => "May",
                6 => "Jun",
                7 => "Jul",
                8 => "Aug",
                9 => "Sep",
                10 => "Oct",
                11 => "Nov",
                12 => "Dec",
                _ => "",
            };
            return format!(
                "{month_name} {}, {} – {end_month} {}, {}",
                start_date.day(),
                start_date.year(),
                e_date.day(),
                e_date.year()
            );
        }
        return format!(
            "{weekday}, {month_name} {}, {} · All day",
            start_date.day(),
            start_date.year()
        );
    }

    let start_time = start.time_string().unwrap_or_default();
    match end {
        Some(e) => {
            let e_date = e.date();
            let end_time = e.time_string().unwrap_or_default();
            if e_date == start_date {
                format!(
                    "{weekday}, {month_name} {}, {} · {start_time} – {end_time}",
                    start_date.day(),
                    start_date.year()
                )
            } else {
                let end_month = match e_date.month() {
                    1 => "Jan",
                    2 => "Feb",
                    3 => "Mar",
                    4 => "Apr",
                    5 => "May",
                    6 => "Jun",
                    7 => "Jul",
                    8 => "Aug",
                    9 => "Sep",
                    10 => "Oct",
                    11 => "Nov",
                    12 => "Dec",
                    _ => "",
                };
                format!(
                    "{month_name} {}, {}, {start_time} – {end_month} {}, {}, {end_time}",
                    start_date.day(),
                    start_date.year(),
                    e_date.day(),
                    e_date.year()
                )
            }
        }
        None => format!(
            "{weekday}, {month_name} {}, {} · {start_time}",
            start_date.day(),
            start_date.year()
        ),
    }
}

fn parse_organizer(params_and_val: &str) -> Option<String> {
    let (params_part, val) = match params_and_val.split_once(':') {
        Some((p, v)) => (p, v),
        None => ("", params_and_val),
    };
    let email = val
        .trim()
        .strip_prefix("mailto:")
        .or_else(|| val.trim().strip_prefix("MAILTO:"))
        .unwrap_or(val.trim());

    let mut cn = None;
    for param in params_part.split(';') {
        let param = param.trim();
        if let Some(rest) = param.strip_prefix("CN=") {
            let clean = rest.trim().trim_matches('"');
            if !clean.is_empty() {
                cn = Some(clean.to_string());
            }
        }
    }

    match (cn, email) {
        (Some(name), addr) if !addr.is_empty() && name != addr => Some(format!("{name} <{addr}>")),
        (Some(name), _) => Some(name),
        (None, addr) if !addr.is_empty() => Some(addr.to_string()),
        _ => None,
    }
}

/// Parse an iCalendar string into a `CalendarEvent`. Returns `None` if no
/// valid `VEVENT` is found or parsing fails.
pub fn parse_ics(ics_data: &str) -> Option<CalendarEvent> {
    let unfolded = unfold(ics_data);
    let mut in_calendar = false;
    let mut in_event = false;

    let mut calendar_method: Option<String> = None;
    let mut event_method: Option<String> = None;
    let mut summary: Option<String> = None;
    let mut description: Option<String> = None;
    let mut location: Option<String> = None;
    let mut organizer: Option<String> = None;
    let mut status: Option<String> = None;
    let mut dtstart: Option<ParsedTime> = None;
    let mut dtend: Option<ParsedTime> = None;
    let mut duration_secs: Option<i64> = None;
    let mut is_all_day = false;

    for line in unfolded.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.eq_ignore_ascii_case("BEGIN:VCALENDAR") {
            in_calendar = true;
            continue;
        }
        if line.eq_ignore_ascii_case("END:VCALENDAR") {
            break;
        }

        if !in_calendar && !in_event {
            // Also tolerate ICS files missing BEGIN:VCALENDAR wrapper
            if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
                in_event = true;
                continue;
            }
        }

        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            in_event = true;
            continue;
        }
        if line.eq_ignore_ascii_case("END:VEVENT") {
            if in_event {
                break;
            }
            continue;
        }

        let (prop_with_params, val) = match line.split_once(':') {
            Some((p, v)) => (p, v),
            None => continue,
        };

        let prop_upper = prop_with_params.to_ascii_uppercase();
        let prop_name = prop_upper.split(';').next().unwrap_or("").trim();

        if !in_event {
            if prop_name == "METHOD" {
                calendar_method = Some(val.trim().to_string());
            }
            continue;
        }

        match prop_name {
            "SUMMARY" => summary = Some(unescape_text(val)),
            "DESCRIPTION" => description = Some(unescape_text(val)),
            "LOCATION" => location = Some(unescape_text(val)),
            "STATUS" => status = Some(val.trim().to_string()),
            "METHOD" => event_method = Some(val.trim().to_string()),
            "ORGANIZER" => organizer = parse_organizer(line),
            "DTSTART" => {
                if prop_upper.contains("VALUE=DATE") {
                    is_all_day = true;
                }
                if let Some(pt) = ParsedTime::parse(val) {
                    if matches!(pt, ParsedTime::Date(_)) {
                        is_all_day = true;
                    }
                    dtstart = Some(pt);
                }
            }
            "DTEND" => {
                if let Some(pt) = ParsedTime::parse(val) {
                    dtend = Some(pt);
                }
            }
            "DURATION" => {
                duration_secs = parse_duration_seconds(val);
            }
            _ => {}
        }
    }

    let start = dtstart?;

    // If DTEND was not present but DURATION was, calculate DTEND
    let end = match (dtend, duration_secs) {
        (Some(e), _) => Some(e),
        (None, Some(secs)) => match &start {
            ParsedTime::DateTimeUtc(dt) => Some(ParsedTime::DateTimeUtc(
                *dt + chrono::Duration::seconds(secs),
            )),
            ParsedTime::DateTimeLocal(ndt) => Some(ParsedTime::DateTimeLocal(
                *ndt + chrono::Duration::seconds(secs),
            )),
            ParsedTime::Date(d) => {
                let days = (secs / 86400).max(1);
                Some(ParsedTime::Date(*d + chrono::Duration::days(days)))
            }
        },
        (None, None) => None,
    };

    let formatted_time = format_date_range(&start, end.as_ref(), is_all_day);
    let start_iso = Some(start.to_iso());
    let end_iso = end.as_ref().map(|e| e.to_iso());

    let method = event_method.or(calendar_method);
    let is_cancelled = status
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("CANCELLED"))
        .unwrap_or(false)
        || method
            .as_deref()
            .map(|m| m.eq_ignore_ascii_case("CANCEL"))
            .unwrap_or(false);

    let summary = summary
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(Event)".to_string());

    Some(CalendarEvent {
        summary,
        description,
        location,
        organizer,
        start_iso,
        end_iso,
        formatted_time,
        is_all_day,
        status,
        is_cancelled,
        method,
        attachment_id: None,
    })
}

/// Parse an iCalendar byte slice into a `CalendarEvent`.
pub fn parse_ics_bytes(bytes: &[u8]) -> Option<CalendarEvent> {
    let s = std::str::from_utf8(bytes).ok()?;
    parse_ics(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unfold_and_unescape() {
        let folded = "SUMMARY:This is a long line that has been folded into \r\n multiple lines \r\n with spaces.\r\n";
        assert_eq!(
            unfold(folded),
            "SUMMARY:This is a long line that has been folded into multiple lines with spaces.\r\n"
        );

        let unescaped = unescape_text(r"Line 1\nLine 2\, with commas\; and semicolons\\done");
        assert_eq!(
            unescaped,
            "Line 1\nLine 2, with commas; and semicolons\\done"
        );
    }

    #[test]
    fn test_parse_timed_event() {
        let ics = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Example Corp.//EN\r\n\
METHOD:REQUEST\r\n\
BEGIN:VEVENT\r\n\
UID:meet-123@example.org\r\n\
DTSTART:20261006T140000Z\r\n\
DTEND:20261006T150000Z\r\n\
SUMMARY:Sprint Planning\r\n\
DESCRIPTION:Review backlog and sprint goals.\r\n\
LOCATION:Meeting Room 3B\r\n\
ORGANIZER;CN=\"Alice Smith\":mailto:alice@example.org\r\n\
STATUS:CONFIRMED\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

        let event = parse_ics(ics).expect("should parse event");
        assert_eq!(event.summary, "Sprint Planning");
        assert_eq!(
            event.description.as_deref(),
            Some("Review backlog and sprint goals.")
        );
        assert_eq!(event.location.as_deref(), Some("Meeting Room 3B"));
        assert_eq!(
            event.organizer.as_deref(),
            Some("Alice Smith <alice@example.org>")
        );
        assert_eq!(event.status.as_deref(), Some("CONFIRMED"));
        assert_eq!(event.method.as_deref(), Some("REQUEST"));
        assert!(!event.is_cancelled);
        assert!(!event.is_all_day);
        assert_eq!(event.start_iso.as_deref(), Some("2026-10-06T14:00:00Z"));
        assert_eq!(event.end_iso.as_deref(), Some("2026-10-06T15:00:00Z"));
        assert!(event.formatted_time.contains("Oct 6, 2026"));
    }

    #[test]
    fn test_parse_all_day_event() {
        let ics = "BEGIN:VCALENDAR\r\n\
BEGIN:VEVENT\r\n\
DTSTART;VALUE=DATE:20261006\r\n\
DTEND;VALUE=DATE:20261007\r\n\
SUMMARY:Team Offsite\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

        let event = parse_ics(ics).expect("should parse all-day event");
        assert_eq!(event.summary, "Team Offsite");
        assert!(event.is_all_day);
        assert_eq!(event.formatted_time, "Tue, Oct 6, 2026 · All day");
    }

    #[test]
    fn test_cancelled_event() {
        let ics = "BEGIN:VCALENDAR\r\n\
METHOD:CANCEL\r\n\
BEGIN:VEVENT\r\n\
DTSTART:20261006T100000Z\r\n\
SUMMARY:Cancelled Sync\r\n\
STATUS:CANCELLED\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

        let event = parse_ics(ics).expect("should parse cancelled event");
        assert_eq!(event.summary, "Cancelled Sync");
        assert!(event.is_cancelled);
    }

    #[test]
    fn test_duration_fallback() {
        let ics = "BEGIN:VCALENDAR\r\n\
BEGIN:VEVENT\r\n\
DTSTART:20261006T090000Z\r\n\
DURATION:PT1H30M\r\n\
SUMMARY:90-min Workshop\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

        let event = parse_ics(ics).expect("should parse duration");
        assert_eq!(event.summary, "90-min Workshop");
        assert_eq!(event.end_iso.as_deref(), Some("2026-10-06T10:30:00Z"));
    }
}
