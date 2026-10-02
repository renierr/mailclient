//! A daily window of local time in which an account is left alone in the
//! background: no push connection, no scheduled check.

use chrono::{DateTime, Duration, LocalResult, NaiveDateTime, NaiveTime, TimeZone, Utc};

/// Local wall-clock window `[start, end)`. Wraps past midnight when `start`
/// is later than `end` (`22:00`–`06:00`); `start == end` is an empty window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuietHours {
    pub start: NaiveTime,
    pub end: NaiveTime,
}

impl QuietHours {
    /// Whether the local time `t` falls inside the window.
    #[must_use]
    pub fn contains(&self, t: NaiveTime) -> bool {
        if self.start <= self.end {
            t >= self.start && t < self.end
        } else {
            t >= self.start || t < self.end
        }
    }

    /// Whether `now`, read in its own time zone, falls inside the window.
    #[must_use]
    pub fn contains_at<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> bool {
        self.contains(now.naive_local().time())
    }

    /// The next instant after `now` at which the window starts or ends, in
    /// `now`'s time zone. `None` for an empty window.
    #[must_use]
    pub fn next_change<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> Option<DateTime<Utc>> {
        if self.start == self.end {
            return None;
        }
        let local = now.naive_local();
        let today = local.date();
        (0..=1)
            .flat_map(|days| {
                let date = today + Duration::days(days);
                [date.and_time(self.start), date.and_time(self.end)]
            })
            .filter(|at| *at > local)
            .min()
            .map(|at| resolve(&now.timezone(), at))
    }
}

/// `at` as an instant. A time skipped by a DST jump resolves to the hour
/// after it; a repeated one to its first occurrence.
fn resolve<Tz: TimeZone>(tz: &Tz, at: NaiveDateTime) -> DateTime<Utc> {
    match tz.from_local_datetime(&at) {
        LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => t.with_timezone(&Utc),
        LocalResult::None => match tz.from_local_datetime(&(at + Duration::hours(1))) {
            LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => t.with_timezone(&Utc),
            LocalResult::None => at.and_utc(),
        },
    }
}

/// `"7:05"` / `"07:05"` as a time; `None` for anything else.
#[must_use]
pub fn parse_time(value: &str) -> Option<NaiveTime> {
    let (h, m) = value.trim().split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    NaiveTime::from_hms_opt(h.parse().ok()?, m.parse().ok()?, 0)
}

/// The stored form of a time: `"HH:MM"`.
#[must_use]
pub fn format_time(t: NaiveTime) -> String {
    t.format("%H:%M").to_string()
}

/// A typed time in its stored form (`"7:05"` → `"07:05"`); `None` when it
/// does not read as one. What both settings forms check typed times with.
#[must_use]
pub fn normalize_time(value: &str) -> Option<String> {
    parse_time(value).map(format_time)
}

/// A time as `(hour, minute)` for a time picker; `None` when it does not
/// read as one.
#[must_use]
pub fn time_parts(value: &str) -> Option<(u32, u32)> {
    use chrono::Timelike;
    parse_time(value).map(|t| (t.hour(), t.minute()))
}

/// The stored form of a picked time; `None` when out of range.
#[must_use]
pub fn time_at(hour: u32, minute: u32) -> Option<String> {
    NaiveTime::from_hms_opt(hour, minute, 0).map(format_time)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn t(s: &str) -> NaiveTime {
        parse_time(s).unwrap()
    }

    fn window(start: &str, end: &str) -> QuietHours {
        QuietHours {
            start: t(start),
            end: t(end),
        }
    }

    fn at(tz: &FixedOffset, s: &str) -> DateTime<FixedOffset> {
        tz.from_local_datetime(&NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap())
            .unwrap()
    }

    #[test]
    fn times_parse_loosely_and_store_padded() {
        assert_eq!(format_time(t("7:05")), "07:05");
        assert_eq!(format_time(t(" 23:59 ")), "23:59");
        for bad in ["", "7", "24:00", "07:60", "07:5", "a:00", "007:00"] {
            assert!(parse_time(bad).is_none(), "{bad:?}");
            assert_eq!(normalize_time(bad), None, "{bad:?}");
        }
        assert_eq!(normalize_time("6:30").as_deref(), Some("06:30"));
        assert_eq!(time_parts("6:30"), Some((6, 30)));
        assert_eq!(time_at(6, 5).as_deref(), Some("06:05"));
        assert_eq!(time_at(24, 0), None);
    }

    #[test]
    fn a_window_may_wrap_past_midnight() {
        let night = window("00:00", "07:00");
        assert!(night.contains(t("00:00")));
        assert!(night.contains(t("06:59")));
        assert!(!night.contains(t("07:00")), "the end is exclusive");
        assert!(!night.contains(t("23:59")));

        let late = window("22:30", "06:00");
        assert!(late.contains(t("23:00")));
        assert!(late.contains(t("05:00")));
        assert!(!late.contains(t("12:00")));

        assert!(!window("07:00", "07:00").contains(t("07:00")));
    }

    #[test]
    fn the_next_change_is_the_nearest_boundary_ahead() {
        let tz = FixedOffset::east_opt(2 * 3600).unwrap();
        let night = window("00:00", "07:00");
        let change = |s| night.next_change(&at(&tz, s)).unwrap();
        assert_eq!(change("2026-03-10 03:00"), at(&tz, "2026-03-10 07:00"));
        assert_eq!(change("2026-03-10 07:00"), at(&tz, "2026-03-11 00:00"));
        assert_eq!(change("2026-03-10 18:00"), at(&tz, "2026-03-11 00:00"));

        let late = window("22:30", "06:00");
        let change = |s| late.next_change(&at(&tz, s)).unwrap();
        assert_eq!(change("2026-03-10 23:00"), at(&tz, "2026-03-11 06:00"));
        assert_eq!(change("2026-03-10 12:00"), at(&tz, "2026-03-10 22:30"));

        assert!(window("01:00", "01:00")
            .next_change(&at(&tz, "2026-03-10 12:00"))
            .is_none());
    }
}
