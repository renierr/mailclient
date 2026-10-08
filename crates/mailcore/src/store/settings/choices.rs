//! What each preference may hold and what it starts as, for both settings
//! forms. The frontends label the values; they never list them or copy a
//! default themselves.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Value};

use super::*;

/// One preference: its built-in default and, for a pick-one setting, the
/// values offered, in display order. Typed like the settings feeds send
/// them: flags as booleans, minutes/seconds as integers, the scale as a
/// number, everything else as text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Choice {
    pub default: Value,
    /// Empty for a switch or free text.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Value>,
}

#[derive(Clone, Copy)]
enum Kind {
    Flag,
    Int,
    Real,
    Text,
}

/// Every user preference: key, kind and the offered values (raw form).
const TABLE: &[(&str, Kind, &[&str])] = &[
    (SENT_COPY_ENABLED, Kind::Flag, &[]),
    (LOAD_REMOTE_IMAGES, Kind::Flag, &[]),
    (
        COMPOSE_SEND_FORMAT,
        Kind::Text,
        &["auto", "plain", "multipart", "html"],
    ),
    (COMPOSE_INCLUDE_PLAIN, Kind::Flag, &[]),
    (AUTO_MARK_READ, Kind::Flag, &[]),
    (
        MARK_READ_DELAY_SECS,
        Kind::Int,
        &["0", "3", "5", "10", "30"],
    ),
    (COLLECT_SENT_CONTACTS, Kind::Flag, &[]),
    (MESSAGE_SORT_FIELD, Kind::Text, &["date", "from", "subject"]),
    (MESSAGE_SORT_DESC, Kind::Flag, &[]),
    (CONFIRM_DELETE, Kind::Flag, &[]),
    (LIST_DENSITY, Kind::Text, &["comfortable", "compact"]),
    (READER_FONT_SIZE, Kind::Text, &["small", "normal", "large"]),
    (LINK_CLICK_ACTION, Kind::Text, &["examine", "browser"]),
    (START_VIEW, Kind::Text, &["folders", "inbox"]),
    (
        SYNC_INTERVAL_MINUTES,
        Kind::Int,
        &["0", "5", "10", "15", "30", "60"],
    ),
    (SIGNATURE_ENABLED, Kind::Flag, &[]),
    (SIGNATURE_TEXT, Kind::Text, &[]),
    (REPLY_BELOW_QUOTE, Kind::Flag, &[]),
    (REQUEST_MDN, Kind::Flag, &[]),
    (REQUEST_DSN, Kind::Flag, &[]),
    (UI_SCALE, Kind::Real, &["1", "1.1", "1.25", "1.5"]),
    (NOTIFICATIONS_ENABLED, Kind::Flag, &[]),
    (
        BACKGROUND_SCHEDULER,
        Kind::Text,
        &["workmanager", "alarm", "push"],
    ),
    (QUIET_HOURS_ENABLED, Kind::Flag, &[]),
    (QUIET_HOURS_START, Kind::Text, &[]),
    (QUIET_HOURS_END, Kind::Text, &[]),
];

fn typed(kind: Kind, raw: &str) -> Value {
    match kind {
        Kind::Flag => json!(raw == "1"),
        Kind::Int => json!(raw.parse::<i64>().unwrap_or_default()),
        Kind::Real => json!(raw.parse::<f64>().unwrap_or(1.0)),
        Kind::Text => json!(raw),
    }
}

/// Every user preference by key.
pub fn choices() -> BTreeMap<&'static str, Choice> {
    TABLE
        .iter()
        .map(|&(key, kind, values)| {
            let choice = Choice {
                default: typed(kind, defaults(key).unwrap_or_default()),
                values: values.iter().map(|v| typed(kind, v)).collect(),
            };
            (key, choice)
        })
        .collect()
}

/// [`choices`] as a JSON object.
pub fn choices_json() -> String {
    serde_json::to_string(&choices()).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preference_with_a_default_is_listed() {
        let listed = choices();
        for (key, _, _) in TABLE {
            assert!(defaults(key).is_some(), "{key} has no default");
        }
        assert_eq!(listed.len(), TABLE.len());
        assert_eq!(listed[UI_SCALE].default, json!(1.0));
        assert_eq!(listed[SYNC_INTERVAL_MINUTES].default, json!(0));
        assert_eq!(listed[QUIET_HOURS_END].default, json!("07:00"));
        assert_eq!(listed[CONFIRM_DELETE].default, json!(true));
    }

    #[test]
    fn offered_values_are_what_the_normalizers_keep() {
        let kept = |key: &str, raw: &str| -> String {
            match key {
                COMPOSE_SEND_FORMAT => normalize_send_format(raw).into(),
                MESSAGE_SORT_FIELD => normalize_sort_field(raw).into(),
                LIST_DENSITY => normalize_density(raw).into(),
                READER_FONT_SIZE => normalize_reader_font(raw).into(),
                LINK_CLICK_ACTION => normalize_link_click(raw).into(),
                START_VIEW => normalize_start_view(raw).into(),
                BACKGROUND_SCHEDULER => normalize_background_scheduler(raw).into(),
                MARK_READ_DELAY_SECS => normalize_delay_secs(raw.parse().unwrap()).to_string(),
                SYNC_INTERVAL_MINUTES => normalize_sync_interval(raw.parse().unwrap()).to_string(),
                UI_SCALE => {
                    let v: f32 = raw.parse().unwrap();
                    assert_eq!(normalize_ui_scale(v), v, "{key}={raw}");
                    raw.into()
                }
                _ => unreachable!("{key}"),
            }
        };
        for &(key, _, values) in TABLE {
            if values.is_empty() {
                continue;
            }
            for raw in values {
                assert_eq!(kept(key, raw), *raw, "{key}={raw}");
            }
            assert!(
                values.contains(&defaults(key).unwrap()),
                "{key}: default not offered"
            );
        }
    }
}
