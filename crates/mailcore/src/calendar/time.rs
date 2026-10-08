//! Event times: `DTSTART`/`DTEND` values, `TZID` resolution against fixed
//! `VTIMEZONE` offsets, and `DURATION`.

use std::collections::HashMap;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, TimeDelta, Utc};

use crate::content_line::ContentLine;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ParsedTime {
    Date(NaiveDate),
    DateTimeUtc(DateTime<Utc>),
    /// Floating time: no zone, so local wherever it is read.
    DateTimeLocal(NaiveDateTime),
    /// Wall time in a named zone we cannot resolve.
    DateTimeZoned(NaiveDateTime, String),
}

impl ParsedTime {
    pub(super) fn parse(val: &str, value_type: Option<&str>) -> Option<Self> {
        let val = val.trim();
        if value_type.is_some_and(|t| t.eq_ignore_ascii_case("DATE")) {
            return NaiveDate::parse_from_str(val, "%Y%m%d")
                .ok()
                .map(Self::Date);
        }
        if let Some(s) = val.strip_suffix(['Z', 'z']) {
            return NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S")
                .ok()
                .map(|ndt| Self::DateTimeUtc(ndt.and_utc()));
        }
        if val.contains(['T', 't']) {
            return NaiveDateTime::parse_from_str(val, "%Y%m%dT%H%M%S")
                .ok()
                .map(Self::DateTimeLocal);
        }
        if val.len() == 8 {
            return NaiveDate::parse_from_str(val, "%Y%m%d")
                .ok()
                .map(Self::Date);
        }
        None
    }

    /// Attach a `TZID` to a floating time: converted when the zone has a
    /// known fixed offset, labelled otherwise.
    pub(super) fn with_tzid(
        self,
        tzid: Option<&str>,
        zones: &HashMap<String, FixedOffset>,
    ) -> Self {
        let (Self::DateTimeLocal(ndt), Some(tzid)) = (&self, tzid) else {
            return self;
        };
        let tzid = tzid.trim();
        if tzid.is_empty() {
            return self;
        }
        if let Some(offset) = zones.get(tzid) {
            if let Some(utc) = ndt.checked_sub_offset(*offset) {
                return Self::DateTimeUtc(utc.and_utc());
            }
        }
        Self::DateTimeZoned(*ndt, tzid.to_string())
    }

    pub(super) fn checked_add(&self, delta: TimeDelta) -> Option<Self> {
        Some(match self {
            Self::Date(d) => Self::Date(d.checked_add_signed(delta)?),
            Self::DateTimeUtc(dt) => Self::DateTimeUtc(dt.checked_add_signed(delta)?),
            Self::DateTimeLocal(ndt) => Self::DateTimeLocal(ndt.checked_add_signed(delta)?),
            Self::DateTimeZoned(ndt, tz) => {
                Self::DateTimeZoned(ndt.checked_add_signed(delta)?, tz.clone())
            }
        })
    }

    #[cfg(test)]
    pub(super) fn to_iso(&self) -> String {
        match self {
            Self::Date(d) => d.format("%Y-%m-%d").to_string(),
            Self::DateTimeUtc(dt) => dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            Self::DateTimeLocal(ndt) | Self::DateTimeZoned(ndt, _) => {
                ndt.format("%Y-%m-%dT%H:%M:%S").to_string()
            }
        }
    }

    pub(super) fn date(&self) -> NaiveDate {
        match self {
            Self::Date(d) => *d,
            Self::DateTimeUtc(dt) => dt.with_timezone(&chrono::Local).date_naive(),
            Self::DateTimeLocal(ndt) | Self::DateTimeZoned(ndt, _) => ndt.date(),
        }
    }

    pub(super) fn time_string(&self) -> String {
        match self {
            Self::Date(_) => String::new(),
            Self::DateTimeUtc(dt) => dt.with_timezone(&chrono::Local).format("%H:%M").to_string(),
            Self::DateTimeLocal(ndt) | Self::DateTimeZoned(ndt, _) => {
                ndt.format("%H:%M").to_string()
            }
        }
    }

    pub(super) fn zone(&self) -> Option<&str> {
        match self {
            Self::DateTimeZoned(_, tz) => Some(tz),
            _ => None,
        }
    }
}

/// Parse an RFC 5545 duration (`PT1H30M`, `P1D`, `P2W`, optional `+`).
/// `None` for malformed, negative or overflowing values.
pub(super) fn parse_duration_seconds(d: &str) -> Option<i64> {
    let d = d.trim();
    let d = d.strip_prefix('+').unwrap_or(d);
    let rest = d.strip_prefix(['P', 'p'])?;
    let mut total: i64 = 0;
    let mut in_time = false;
    let mut num_buf = String::new();

    for c in rest.chars() {
        let c = c.to_ascii_uppercase();
        if c == 'T' {
            in_time = true;
            continue;
        }
        if c.is_ascii_digit() {
            num_buf.push(c);
            continue;
        }
        let val = num_buf.parse::<i64>().ok()?;
        num_buf.clear();
        let unit: i64 = match c {
            'W' if !in_time => 7 * 86_400,
            'D' if !in_time => 86_400,
            'H' if in_time => 3_600,
            'M' if in_time => 60,
            'S' if in_time => 1,
            _ => return None,
        };
        total = total.checked_add(val.checked_mul(unit)?)?;
    }
    num_buf.is_empty().then_some(total)
}

/// Parse a UTC offset like `+0200`, `-0500` or `+053000`.
pub(super) fn parse_utc_offset(s: &str) -> Option<FixedOffset> {
    let s = s.trim();
    let (sign, digits) = match s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => return None,
    };
    if !(digits.len() == 4 || digits.len() == 6) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let h: i32 = digits[0..2].parse().ok()?;
    let m: i32 = digits[2..4].parse().ok()?;
    let sec: i32 = if digits.len() == 6 {
        digits[4..6].parse().ok()?
    } else {
        0
    };
    FixedOffset::east_opt(sign * (h * 3600 + m * 60 + sec))
}

/// A `DTSTART`/`DTEND` as read, resolved against `VTIMEZONE`s at the end
/// (a `VTIMEZONE` may follow the event).
pub(super) struct RawTime {
    pub(super) time: ParsedTime,
    pub(super) tzid: Option<String>,
}

pub(super) fn read_time(line: &ContentLine<'_>) -> Option<RawTime> {
    Some(RawTime {
        time: ParsedTime::parse(line.value, line.param("VALUE"))?,
        tzid: line.param("TZID").map(str::to_string),
    })
}

/// A `VTIMEZONE` being read: its id and every `TZOFFSETTO` of its
/// `STANDARD`/`DAYLIGHT` parts.
#[derive(Default)]
pub(super) struct ZoneBuilder {
    pub(super) tzid: Option<String>,
    pub(super) offsets: Vec<Option<FixedOffset>>,
}

impl ZoneBuilder {
    /// A zone with exactly one offset is fixed and safe to convert with.
    pub(super) fn fixed_offset(&self) -> Option<FixedOffset> {
        let first = (*self.offsets.first()?)?;
        self.offsets
            .iter()
            .all(|o| *o == Some(first))
            .then_some(first)
    }
}
