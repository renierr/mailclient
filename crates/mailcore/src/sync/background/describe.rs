//! Background checks in words, for the settings status block: the last run,
//! the recent-runs list, the standby bucket that limits checks, and how
//! often a server sends IDLE heartbeats. Time is shown in the device's local
//! zone; the caller passes "now" and the offset so the wording is testable.

use chrono::{DateTime, Duration, FixedOffset, Local, Utc};

use super::LastRun;

/// A run started this long ago without finishing was stopped by Android
/// rather than still running.
pub const STALE_RUN_AFTER_MINUTES: i64 = 10;

/// One line about the newest run (`None`: nothing ran yet).
#[must_use]
pub fn last_run_line(run: Option<&LastRun>, now: DateTime<Utc>, offset: FixedOffset) -> String {
    let Some((run, started)) = run.and_then(|r| started(r).map(|s| (r, s))) else {
        return "No background check has run yet.".to_string();
    };
    let when = when(started, now, offset);
    if finished(run).is_none() {
        return if now - started < stale() {
            format!("Background check running since {when}.")
        } else {
            format!("Last background check started {when} but did not finish — Android stopped it.")
        };
    }
    if run.skipped {
        return format!("Last background check {when}: skipped, another sync was running.");
    }
    if let Some(error) = run.errors.first() {
        return format!("Last background check {when} failed: {error}");
    }
    let tail = match run.outcome.as_deref() {
        Some(o) if !o.is_empty() => format!(", {o}"),
        _ => String::new(),
    };
    format!("Last background check {when}: {}{tail}.", found(run.new))
}

/// One compact history line: when, which scheduler, and what happened
/// ("09:15 (5 min ago) · alarm · 2 new messages · notified (2)").
#[must_use]
pub fn run_line(run: &LastRun, now: DateTime<Utc>, offset: FixedOffset) -> String {
    let start = started(run);
    let mut parts = vec![start.map_or_else(|| "?".to_string(), |s| when(s, now, offset))];
    if !run.trigger.is_empty() {
        parts.push(run.trigger.clone());
    }
    if finished(run).is_none() {
        let running = start.is_some_and(|s| now - s < stale());
        parts.push(
            if running {
                "running"
            } else {
                "stopped by Android"
            }
            .to_string(),
        );
    } else if run.skipped {
        parts.push("skipped, another sync was running".to_string());
    } else {
        parts.push(found(run.new));
        if let Some(error) = run.errors.first() {
            parts.push(format!("failed: {error}"));
        }
    }
    if let Some(o) = run.outcome.as_deref().filter(|o| !o.is_empty()) {
        parts.push(o.to_string());
    }
    parts.join(" · ")
}

/// What the status block shows: the newest run and every recent run in
/// words.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RunLines {
    pub last: String,
    pub history: Vec<String>,
}

/// [`last_run_line`] and [`run_line`] for `runs` (newest first), as of now
/// in the device's zone.
#[must_use]
pub fn run_lines(runs: &[LastRun]) -> RunLines {
    let now = Utc::now();
    let offset = *Local::now().offset();
    RunLines {
        last: last_run_line(runs.first(), now, offset),
        history: runs.iter().map(|r| run_line(r, now, offset)).collect(),
    }
}

/// The name of an Android standby bucket that limits background work, or
/// `None` for buckets that do not (exempt, active, working set) and unknown
/// values.
#[must_use]
pub fn limiting_bucket(bucket: i32) -> Option<&'static str> {
    match bucket {
        30 => Some("frequent"),
        40 => Some("rare"),
        45 => Some("restricted"),
        50 => Some("never used"),
        _ => None,
    }
}

/// A heartbeat gap in words: "every 45 seconds", "every 4 minutes".
#[must_use]
pub fn heartbeat_gap(secs: i64) -> String {
    if secs < 90 {
        format!("every {secs} seconds")
    } else {
        format!("every {} minutes", (secs as f64 / 60.0).round() as i64)
    }
}

fn stale() -> Duration {
    Duration::minutes(STALE_RUN_AFTER_MINUTES)
}

fn started(run: &LastRun) -> Option<DateTime<Utc>> {
    parse(&run.started_at)
}

fn finished(run: &LastRun) -> Option<DateTime<Utc>> {
    run.finished_at.as_deref().and_then(parse)
}

fn parse(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

fn found(n: usize) -> String {
    match n {
        0 => "no new mail".to_string(),
        1 => "1 new message".to_string(),
        n => format!("{n} new messages"),
    }
}

/// "09:15 (5 min ago)" today, "3 Oct, 09:15 (2 days ago)" before.
fn when(at: DateTime<Utc>, now: DateTime<Utc>, offset: FixedOffset) -> String {
    let local = at.with_timezone(&offset);
    let today = now.with_timezone(&offset).date_naive() == local.date_naive();
    let clock = if today {
        local.format("%H:%M").to_string()
    } else {
        local.format("%-d %b, %H:%M").to_string()
    };
    format!("{clock} ({})", ago(now - at))
}

fn ago(d: Duration) -> String {
    if d.num_minutes() < 1 {
        "just now".to_string()
    } else if d.num_minutes() < 60 {
        format!("{} min ago", d.num_minutes())
    } else if d.num_hours() < 48 {
        format!("{} h ago", d.num_hours())
    } else {
        format!("{} days ago", d.num_days())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        parse(s).unwrap()
    }

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    fn run(started: &str, finished: Option<&str>) -> LastRun {
        LastRun {
            started_at: started.to_string(),
            finished_at: finished.map(str::to_string),
            trigger: "alarm".to_string(),
            ..Default::default()
        }
    }

    const NOW: &str = "2026-03-10T12:00:00Z";

    #[test]
    fn nothing_ran_yet() {
        assert_eq!(
            last_run_line(None, at(NOW), utc()),
            "No background check has run yet."
        );
    }

    #[test]
    fn finished_runs_say_what_they_found() {
        let mut r = run("2026-03-10T11:55:00Z", Some("2026-03-10T11:55:20Z"));
        r.new = 2;
        r.outcome = Some("notified (2)".to_string());
        assert_eq!(
            last_run_line(Some(&r), at(NOW), utc()),
            "Last background check 11:55 (5 min ago): 2 new messages, notified (2)."
        );
        assert_eq!(
            run_line(&r, at(NOW), utc()),
            "11:55 (5 min ago) · alarm · 2 new messages · notified (2)"
        );
    }

    #[test]
    fn unfinished_runs_turn_stale() {
        let fresh = run("2026-03-10T11:58:00Z", None);
        assert_eq!(
            last_run_line(Some(&fresh), at(NOW), utc()),
            "Background check running since 11:58 (2 min ago)."
        );
        let stale = run("2026-03-10T11:00:00Z", None);
        assert!(last_run_line(Some(&stale), at(NOW), utc()).contains("Android stopped it"));
        assert!(run_line(&stale, at(NOW), utc()).ends_with("stopped by Android"));
    }

    #[test]
    fn skipped_and_failed_runs() {
        let mut r = run("2026-03-10T11:59:30Z", Some("2026-03-10T11:59:31Z"));
        r.skipped = true;
        assert_eq!(
            last_run_line(Some(&r), at(NOW), utc()),
            "Last background check 11:59 (just now): skipped, another sync was running."
        );
        r.skipped = false;
        r.errors = vec!["timeout".to_string()];
        assert_eq!(
            last_run_line(Some(&r), at(NOW), utc()),
            "Last background check 11:59 (just now) failed: timeout"
        );
        assert!(run_line(&r, at(NOW), utc()).contains("no new mail · failed: timeout"));
    }

    #[test]
    fn older_runs_carry_the_date_in_local_time() {
        let r = run("2026-03-08T23:30:00Z", Some("2026-03-08T23:31:00Z"));
        let cet = FixedOffset::east_opt(3600).unwrap();
        assert_eq!(
            run_line(&r, at(NOW), cet),
            "9 Mar, 00:30 (36 h ago) · alarm · no new mail"
        );
    }

    #[test]
    fn buckets_and_gaps() {
        assert_eq!(limiting_bucket(10), None);
        assert_eq!(limiting_bucket(40), Some("rare"));
        assert_eq!(heartbeat_gap(45), "every 45 seconds");
        assert_eq!(heartbeat_gap(240), "every 4 minutes");
    }
}
