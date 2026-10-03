use super::*;

fn wrap(event_lines: &str) -> String {
    format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n{event_lines}END:VEVENT\r\nEND:VCALENDAR\r\n")
}

/// Start and end as ISO strings, independent of the machine's time zone.
fn isos(ics: &str) -> (String, Option<String>) {
    let p = parse(ics).expect("should parse");
    (p.start.to_iso(), p.end.as_ref().map(ParsedTime::to_iso))
}

#[test]
fn test_unfold_and_unescape() {
    let folded = "SUMMARY:This is a long line that has been folded into \r\n multiple lines \r\n with spaces.\r\n";
    assert_eq!(
        unfold(folded),
        "SUMMARY:This is a long line that has been folded into multiple lines with spaces.\n"
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
    assert_eq!(event.location.as_deref(), Some("Meeting Room 3B"));
    assert_eq!(
        event.organizer.as_deref(),
        Some("Alice Smith <alice@example.org>")
    );
    assert!(!event.is_cancelled);
    assert!(!event.formatted_time.contains("All day"));
    assert_eq!(
        isos(ics),
        (
            "2026-10-06T14:00:00Z".to_string(),
            Some("2026-10-06T15:00:00Z".to_string())
        )
    );
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
    let ics = wrap("DTSTART:20261006T090000Z\r\nDURATION:PT1H30M\r\nSUMMARY:90-min Workshop\r\n");
    let event = parse_ics(&ics).expect("should parse duration");
    assert_eq!(event.summary, "90-min Workshop");
    assert_eq!(isos(&ics).1.as_deref(), Some("2026-10-06T10:30:00Z"));
}

#[test]
fn duration_overflow_does_not_panic() {
    for dur in [
        "P99999999D",
        "P9999999999999999W",
        "PT9223372036854775807S",
        "P99999999999999999999D",
        "P1D2",
        "P1Y",
        "-P1D",
    ] {
        let ics = wrap(&format!("DTSTART:20261006T090000Z\r\nDURATION:{dur}\r\n"));
        assert_eq!(isos(&ics).1, None, "{dur}");
        let ics = wrap(&format!(
            "DTSTART;VALUE=DATE:20261006\r\nDURATION:{dur}\r\n"
        ));
        assert_eq!(isos(&ics).1, None, "{dur}");
    }
    assert_eq!(parse_duration_seconds("P1W"), Some(7 * 86_400));
    assert_eq!(parse_duration_seconds("+P1DT2H"), Some(86_400 + 7_200));
    assert_eq!(
        parse_duration_seconds("PT9223372036854775807S"),
        Some(i64::MAX)
    );
}

#[test]
fn hostile_input_does_not_panic() {
    let inputs = [
        "",
        ":",
        "BEGIN:VEVENT",
        "BEGIN:VEVENT\nDTSTART:",
        "BEGIN:VEVENT\nDTSTART;TZID=\"unterminated:20261006T090000\n",
        "BEGIN:VEVENT\nDTSTART:99991231T235959Z\nDURATION:PT59S\n",
        "BEGIN:VEVENT\nDTSTART:99991231T235959Z\nDTEND:99991231T235959Z\n",
        "BEGIN:VEVENT\nDTSTART;VALUE=DATE:99991231\nDURATION:P1D\n",
        "BEGIN:VEVENT\nDTSTART;VALUE=DATE:2026\u{e9}\u{e9}\n",
        "BEGIN:VEVENT\nDTSTART:2026\u{e9}T\u{e9}Z\n",
        "BEGIN:VEVENT\nORGANIZER;CN=\u{e9}:mail\u{e9}\n",
        "BEGIN:VEVENT\nORGANIZER:ma\u{e9}lto:x\n",
        "BEGIN:VTIMEZONE\nTZID:X\nBEGIN:STANDARD\nTZOFFSETTO:+\u{e9}\u{e9}\nEND:STANDARD\nEND:VTIMEZONE\nBEGIN:VEVENT\nDTSTART;TZID=X:20261006T090000\n",
        "END:VEVENT\nEND:VEVENT\nBEGIN:VEVENT\nEND:VALARM\nDTSTART:20261006T090000\n",
        "BEGIN:VEVENT\nSUMMARY:\\",
    ];
    for input in inputs {
        let _ = parse_ics(input);
    }
    let _ = parse_ics_bytes(&[0xff, 0xfe, b'B']);
}

#[test]
fn nested_alarm_does_not_overwrite_event() {
    let ics = wrap(
        "DTSTART:20261006T090000Z\r\n\
SUMMARY:Design Review\r\n\
BEGIN:VALARM\r\n\
ACTION:DISPLAY\r\n\
SUMMARY:Reminder\r\n\
DESCRIPTION:Alarm text\r\n\
LOCATION:Alarm place\r\n\
DURATION:PT15M\r\n\
TRIGGER:-PT15M\r\n\
END:VALARM\r\n\
LOCATION:Room 1\r\n",
    );
    let event = parse_ics(&ics).unwrap();
    assert_eq!(event.summary, "Design Review");
    assert_eq!(event.location.as_deref(), Some("Room 1"));
    assert_eq!(isos(&ics).1, None);
}

#[test]
fn value_date_time_is_not_all_day() {
    let ics = wrap(
        "DTSTART;VALUE=DATE-TIME:20261006T090000\r\nDTEND;VALUE=DATE-TIME:20261006T100000\r\n",
    );
    let event = parse_ics(&ics).unwrap();
    assert!(!event.formatted_time.contains("All day"));
    assert_eq!(event.formatted_time, "Tue, Oct 6, 2026 · 09:00 – 10:00");
}

#[test]
fn tzid_without_fixed_zone_is_labelled() {
    let ics = wrap(
        "DTSTART;TZID=America/New_York:20261006T090000\r\n\
DTEND;TZID=America/New_York:20261006T100000\r\n",
    );
    let event = parse_ics(&ics).unwrap();
    assert_eq!(
        event.formatted_time,
        "Tue, Oct 6, 2026 · 09:00 – 10:00 (America/New_York)"
    );
    assert_eq!(isos(&ics).0, "2026-10-06T09:00:00");

    let ics = wrap("DTSTART;TZID=\"Europe/Berlin\":20261006T090000\r\n");
    let event = parse_ics(&ics).unwrap();
    assert_eq!(
        event.formatted_time,
        "Tue, Oct 6, 2026 · 09:00 (Europe/Berlin)"
    );
}

#[test]
fn tzid_with_dst_vtimezone_stays_labelled() {
    let ics = "BEGIN:VCALENDAR\r\n\
BEGIN:VTIMEZONE\r\nTZID:Example/Zone\r\n\
BEGIN:STANDARD\r\nTZOFFSETTO:-0500\r\nEND:STANDARD\r\n\
BEGIN:DAYLIGHT\r\nTZOFFSETTO:-0400\r\nEND:DAYLIGHT\r\n\
END:VTIMEZONE\r\n\
BEGIN:VEVENT\r\nDTSTART;TZID=Example/Zone:20261006T090000\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n";
    let event = parse_ics(ics).unwrap();
    assert!(event.formatted_time.ends_with("09:00 (Example/Zone)"));
}

#[test]
fn tzid_with_fixed_vtimezone_converts_to_utc() {
    let ics = "BEGIN:VCALENDAR\r\n\
BEGIN:VEVENT\r\n\
DTSTART;TZID=Example/Fixed:20261006T090000\r\n\
DTEND;TZID=Example/Fixed:20261006T100000\r\n\
END:VEVENT\r\n\
BEGIN:VTIMEZONE\r\nTZID:Example/Fixed\r\n\
BEGIN:STANDARD\r\nTZOFFSETFROM:+0530\r\nTZOFFSETTO:+0530\r\nEND:STANDARD\r\n\
END:VTIMEZONE\r\n\
END:VCALENDAR\r\n";
    let event = parse_ics(ics).unwrap();
    assert_eq!(
        isos(ics),
        (
            "2026-10-06T03:30:00Z".to_string(),
            Some("2026-10-06T04:30:00Z".to_string())
        )
    );
    assert!(!event.formatted_time.contains("Example/Fixed"));
}

#[test]
fn multi_day_all_day_end_is_exclusive() {
    let ics = wrap("DTSTART;VALUE=DATE:20261006\r\nDTEND;VALUE=DATE:20261009\r\n");
    let event = parse_ics(&ics).unwrap();
    assert_eq!(event.formatted_time, "Oct 6 – Oct 8, 2026 · All day");

    let ics = wrap("DTSTART;VALUE=DATE:20261230\r\nDTEND;VALUE=DATE:20270102\r\n");
    let event = parse_ics(&ics).unwrap();
    assert_eq!(event.formatted_time, "Dec 30, 2026 – Jan 1, 2027 · All day");

    let ics = wrap("DTSTART;VALUE=DATE:20261006\r\nDURATION:P2D\r\n");
    let event = parse_ics(&ics).unwrap();
    assert_eq!(event.formatted_time, "Oct 6 – Oct 7, 2026 · All day");

    let ics = wrap("DTSTART;VALUE=DATE:20261006\r\n");
    let event = parse_ics(&ics).unwrap();
    assert_eq!(event.formatted_time, "Tue, Oct 6, 2026 · All day");
}

#[test]
fn organizer_params_quoted_and_case_insensitive() {
    let ics = wrap(
        "DTSTART:20261006T090000\r\n\
ORGANIZER;cn=\"Doe: J; ^'Jr^'\";SENT-BY=\"mailto:a@example.com\":MAILTO:doe@example.com\r\n\
LOCATION;ALTREP=\"https://example.com/room\":Room 1\r\n",
    );
    let event = parse_ics(&ics).unwrap();
    assert_eq!(
        event.organizer.as_deref(),
        Some("Doe: J; \"Jr\" <doe@example.com>")
    );
    assert_eq!(event.location.as_deref(), Some("Room 1"));
}

#[test]
fn save_name_comes_from_the_attachment_not_the_summary() {
    let mut event = parse_ics(&wrap("DTSTART:20261006T090000\r\nSUMMARY:Q3/Q4\r\n")).unwrap();
    assert_eq!(event.save_name, None);
    let att = crate::models::Attachment {
        id: 7,
        filename: Some("../in:vite.ics".to_string()),
        mime_type: Some("text/calendar".to_string()),
        message_id: 1,
        size: 0,
        content_id: None,
        storage_path: None,
        data: None,
        is_inline: true,
    };
    event.set_attachment(&att);
    assert_eq!(event.attachment_id, Some(7));
    let name = event.save_name.unwrap();
    assert!(!name.contains(['/', '\\', ':']), "{name}");
    assert!(name.ends_with(".ics"), "{name}");
}

#[test]
fn blank_location_is_absent() {
    let ics = wrap("SUMMARY:Sync\r\nDTSTART:20261006T140000Z\r\nLOCATION:   \r\n");
    let event = parse_ics(&ics).expect("should parse");
    assert_eq!(event.location, None);
}

#[test]
fn outlook_style_invite_parses() {
    let ics = "BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nPRODID:Microsoft Exchange Server 2010\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:W. Europe Standard Time\r\nBEGIN:STANDARD\r\nDTSTART:16010101T030000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nRRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\nRRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=3\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nORGANIZER;CN=Jane Doe:mailto:jane@example.com\r\nATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;CN=Joe:mailto:joe@example.com\r\nDESCRIPTION;LANGUAGE=de-DE:Hallo\\n\r\nSUMMARY;LANGUAGE=de-DE:Planung\r\nDTSTART;TZID=W. Europe Standard Time:20261006T100000\r\nDTEND;TZID=W. Europe Standard Time:20261006T110000\r\nUID:040000008200E00074C5B7101A82E00800000000\r\nCLASS:PUBLIC\r\nPRIORITY:5\r\nDTSTAMP:20261001T080000Z\r\nTRANSP:OPAQUE\r\nSTATUS:CONFIRMED\r\nSEQUENCE:0\r\nLOCATION;LANGUAGE=de-DE:Raum 1\r\nBEGIN:VALARM\r\nDESCRIPTION:REMINDER\r\nTRIGGER;RELATED=START:-PT15M\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let event = parse_ics(ics).expect("outlook invite should parse");
    assert_eq!(event.summary, "Planung");
    assert_eq!(event.location.as_deref(), Some("Raum 1"));
}
