//! Parsing iCalendar (`.ics`, RFC 5545) events for reader event preview cards.
//!
//! Provides pure-Rust extraction of the first `VEVENT` without external
//! calendar dependencies. Handles line unfolding, quoted parameters, nested
//! components, text unescaping, and date-range formatting for UI display.
//! There is no time-zone database: a `TZID` time is converted only when the
//! file's `VTIMEZONE` has a single fixed offset; otherwise the wall time is
//! shown with the zone name instead of pretending it is local.

mod format;
mod time;

use std::collections::HashMap;

use chrono::{FixedOffset, TimeDelta};
use serde::{Deserialize, Serialize};

use crate::content_line::{parse_content_line, unescape_text, unfold, ContentLine};
use format::format_date_range;
use time::{parse_duration_seconds, parse_utc_offset, read_time, ParsedTime, RawTime, ZoneBuilder};

/// Parsed calendar event: exactly what the reader's event card shows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarEvent {
    pub summary: String,
    pub location: Option<String>,
    pub organizer: Option<String>,
    pub formatted_time: String,
    pub is_cancelled: bool,
    /// The `.ics` attachment the event came from, if any (none for an event
    /// found in the body text).
    pub attachment_id: Option<i64>,
    /// Filesystem-safe name to open or save that attachment under
    /// (`crate::paths::safe_attachment_name_for_mime`); never derived from
    /// the summary, which may contain path separators.
    pub save_name: Option<String>,
}

impl CalendarEvent {
    /// Link the event to the attachment it was parsed from.
    pub fn set_attachment(&mut self, att: &crate::models::Attachment) {
        self.attachment_id = Some(att.id);
        self.save_name = Some(crate::paths::safe_attachment_name_for_mime(
            att.filename.as_deref(),
            att.mime_type.as_deref(),
            att.id,
        ));
    }
}

/// An event with the times it was formatted from, kept for tests.
struct Parsed {
    event: CalendarEvent,
    #[cfg_attr(not(test), allow(dead_code))]
    start: ParsedTime,
    #[cfg_attr(not(test), allow(dead_code))]
    end: Option<ParsedTime>,
}

fn parse_organizer(line: &ContentLine<'_>) -> Option<String> {
    let val = line.value.trim();
    let email = match val.get(..7) {
        Some(p) if p.eq_ignore_ascii_case("mailto:") => &val[7..],
        _ => val,
    }
    .trim();
    let cn = line
        .param("CN")
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string);

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
    parse(ics_data).map(|p| p.event)
}

fn parse(ics_data: &str) -> Option<Parsed> {
    let unfolded = unfold(ics_data);
    // Open components, innermost last. Properties are only read where the
    // innermost component is the one they belong to, so a VALARM's
    // SUMMARY never overwrites the event's.
    let mut stack: Vec<String> = Vec::new();
    let mut event_seen = false;
    // The first event is complete; keep scanning only for VTIMEZONEs.
    let mut event_done = false;
    let mut zone: Option<ZoneBuilder> = None;
    let mut zones: HashMap<String, FixedOffset> = HashMap::new();

    let mut calendar_method: Option<String> = None;
    let mut event_method: Option<String> = None;
    let mut summary: Option<String> = None;
    let mut location: Option<String> = None;
    let mut organizer: Option<String> = None;
    let mut status: Option<String> = None;
    let mut dtstart: Option<RawTime> = None;
    let mut dtend: Option<RawTime> = None;
    let mut duration_secs: Option<i64> = None;

    for line in unfolded.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(cl) = parse_content_line(line) else {
            continue;
        };
        let value_upper = || cl.value.trim().to_ascii_uppercase();

        match cl.name.as_str() {
            "BEGIN" => {
                let comp = value_upper();
                if comp == "VTIMEZONE" {
                    zone = Some(ZoneBuilder::default());
                }
                stack.push(comp);
                continue;
            }
            "END" => {
                let comp = value_upper();
                if let Some(pos) = stack.iter().rposition(|c| *c == comp) {
                    stack.truncate(pos);
                }
                match comp.as_str() {
                    "VTIMEZONE" => {
                        if let Some(z) = zone.take() {
                            if let (Some(id), Some(off)) = (&z.tzid, z.fixed_offset()) {
                                zones.insert(id.clone(), off);
                            }
                        }
                    }
                    "VEVENT" if event_seen => event_done = true,
                    "VCALENDAR" => break,
                    _ => {}
                }
                continue;
            }
            _ => {}
        }

        let top = stack.last().map(String::as_str);
        let parent = stack
            .len()
            .checked_sub(2)
            .and_then(|i| stack.get(i))
            .map(String::as_str);
        match (top, cl.name.as_str()) {
            (Some("VCALENDAR"), "METHOD") => calendar_method = Some(cl.value.trim().to_string()),
            (Some("VTIMEZONE"), "TZID") => {
                if let Some(z) = zone.as_mut() {
                    z.tzid = Some(cl.value.trim().to_string());
                }
            }
            (Some("STANDARD" | "DAYLIGHT"), "TZOFFSETTO") if parent == Some("VTIMEZONE") => {
                if let Some(z) = zone.as_mut() {
                    z.offsets.push(parse_utc_offset(cl.value));
                }
            }
            (Some("VEVENT"), name) if !event_done => {
                event_seen = true;
                match name {
                    "SUMMARY" => summary = Some(unescape_text(cl.value)),
                    // Blank means absent, so frontends only test presence.
                    "LOCATION" => {
                        location = Some(unescape_text(cl.value).trim().to_string())
                            .filter(|l| !l.is_empty());
                    }
                    "STATUS" => status = Some(cl.value.trim().to_string()),
                    "METHOD" => event_method = Some(cl.value.trim().to_string()),
                    "ORGANIZER" => organizer = parse_organizer(&cl),
                    "DTSTART" => dtstart = read_time(&cl),
                    "DTEND" => dtend = read_time(&cl),
                    "DURATION" => duration_secs = parse_duration_seconds(cl.value),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let resolve = |raw: RawTime| raw.time.with_tzid(raw.tzid.as_deref(), &zones);
    let start = resolve(dtstart?);
    let is_all_day = matches!(start, ParsedTime::Date(_));

    // Without DTEND, derive the end from DURATION; an overflowing one gives
    // no end rather than a panic.
    let end = match (dtend, duration_secs) {
        (Some(e), _) => Some(resolve(e)),
        (None, Some(secs)) => {
            let delta = if is_all_day {
                TimeDelta::try_days((secs / 86_400).max(1))
            } else {
                TimeDelta::try_seconds(secs)
            };
            delta.and_then(|d| start.checked_add(d))
        }
        (None, None) => None,
    };

    let formatted_time = format_date_range(&start, end.as_ref(), is_all_day);

    let method = event_method.or(calendar_method);
    let is_cancelled = status
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case("CANCELLED"))
        || method
            .as_deref()
            .is_some_and(|m| m.eq_ignore_ascii_case("CANCEL"));

    let summary = summary
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(Event)".to_string());

    Some(Parsed {
        event: CalendarEvent {
            summary,
            location,
            organizer,
            formatted_time,
            is_cancelled,
            attachment_id: None,
            save_name: None,
        },
        start,
        end,
    })
}

/// Parse an iCalendar byte slice into a `CalendarEvent`. Invalid UTF-8 is
/// replaced rather than rejecting the whole invitation.
pub fn parse_ics_bytes(bytes: &[u8]) -> Option<CalendarEvent> {
    parse_ics(&String::from_utf8_lossy(bytes))
}

#[cfg(test)]
mod tests;
