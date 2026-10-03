//! The human-readable date range shown on the event card.

use chrono::{Datelike, NaiveDate};

use super::time::ParsedTime;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

fn month_name(d: NaiveDate) -> &'static str {
    MONTHS[d.month0() as usize]
}

fn weekday_name(d: NaiveDate) -> &'static str {
    WEEKDAYS[d.weekday().num_days_from_monday() as usize]
}

fn format_day(d: NaiveDate) -> String {
    format!("{} {}", month_name(d), d.day())
}

fn format_full_day(d: NaiveDate) -> String {
    format!(
        "{}, {} {}, {}",
        weekday_name(d),
        month_name(d),
        d.day(),
        d.year()
    )
}

fn with_zone(time: String, zone: Option<&str>) -> String {
    match zone {
        Some(z) => format!("{time} ({z})"),
        None => time,
    }
}

pub(super) fn format_date_range(
    start: &ParsedTime,
    end: Option<&ParsedTime>,
    is_all_day: bool,
) -> String {
    let start_date = start.date();

    if is_all_day {
        // DTEND of an all-day event is exclusive: the last day is the one before.
        let last = end
            .map(|e| e.date())
            .filter(|e| *e > start_date)
            .and_then(|e| e.pred_opt())
            .unwrap_or(start_date);
        if last == start_date {
            return format!("{} · All day", format_full_day(start_date));
        }
        return if last.year() == start_date.year() {
            format!(
                "{} – {}, {} · All day",
                format_day(start_date),
                format_day(last),
                last.year()
            )
        } else {
            format!(
                "{}, {} – {}, {} · All day",
                format_day(start_date),
                start_date.year(),
                format_day(last),
                last.year()
            )
        };
    }

    let Some(e) = end else {
        return format!(
            "{} · {}",
            format_full_day(start_date),
            with_zone(start.time_string(), start.zone())
        );
    };
    // Label the start only when its zone differs from the end's.
    let start_zone = start.zone().filter(|z| Some(*z) != e.zone());
    let start_time = with_zone(start.time_string(), start_zone);
    let end_time = with_zone(e.time_string(), e.zone());
    let e_date = e.date();
    if e_date == start_date {
        format!(
            "{} · {start_time} – {end_time}",
            format_full_day(start_date)
        )
    } else {
        format!(
            "{}, {}, {start_time} – {}, {}, {end_time}",
            format_day(start_date),
            start_date.year(),
            format_day(e_date),
            e_date.year()
        )
    }
}
