use super::*;

fn field(value: &str, label: Option<&str>) -> ContactField {
    ContactField {
        value: value.to_string(),
        label: label.map(str::to_string),
    }
}

#[test]
fn parses_a_vcard_4_card() {
    let vcf = "BEGIN:VCARD\r\n\
VERSION:4.0\r\n\
FN:Dr. Jane Q. Doe\r\n\
N:Doe;Jane;Q.;Dr.;\r\n\
ORG:Example Corp;Research\r\n\
TITLE:Lead Engineer\r\n\
EMAIL;TYPE=work:jane@example.com\r\n\
EMAIL;TYPE=home:jane.doe@example.org\r\n\
TEL;VALUE=uri;TYPE=\"cell,voice\":tel:+1-555-0100\r\n\
TEL;TYPE=work,fax:+1 555 0101\r\n\
ADR;TYPE=work:;Suite 4;1 Main St;Springfield;IL;62701;USA\r\n\
URL:https://example.com/jane\r\n\
PHOTO:data:image/png;base64,iVBORw0KGgo=\r\n\
END:VCARD\r\n";
    let c = parse_vcard(vcf).expect("card");
    assert_eq!(c.name, "Dr. Jane Q. Doe");
    assert_eq!(
        c.affiliation.as_deref(),
        Some("Lead Engineer · Example Corp, Research")
    );
    assert_eq!(
        c.emails,
        vec![
            field("jane@example.com", Some("Work")),
            field("jane.doe@example.org", Some("Home")),
        ]
    );
    assert_eq!(
        c.phones,
        vec![
            field("+1-555-0100", Some("Mobile")),
            field("+1 555 0101", Some("Work fax")),
        ]
    );
    assert_eq!(
        c.address.as_deref(),
        Some("1 Main St, Suite 4, 62701 Springfield, IL, USA")
    );
    assert_eq!(c.url.as_deref(), Some("https://example.com/jane"));
    assert_eq!(c.more_cards, 0);
    assert!(c.loaded);
    assert_eq!(c.attachment_id, None);
}

#[test]
fn parses_a_vcard_2_1_card_with_quoted_printable_and_bare_types() {
    // Latin-1 bytes, quoted-printable with a soft line break, bare 2.1
    // type parameters and a property group.
    let vcf = "BEGIN:VCARD\r\n\
VERSION:2.1\r\n\
N;CHARSET=ISO-8859-1;ENCODING=QUOTED-PRINTABLE:M=FCller;J=FC=\r\n\
rgen\r\n\
TEL;CELL;PREF:+49 170 0000000\r\n\
TEL;HOME;VOICE:+49 30 0000000\r\n\
item1.EMAIL;INTERNET:juergen@example.com\r\n\
END:VCARD\r\n";
    let c = parse_vcard(vcf).expect("card");
    assert_eq!(c.name, "Jürgen Müller");
    assert_eq!(
        c.phones,
        vec![
            field("+49 170 0000000", Some("Mobile")),
            field("+49 30 0000000", Some("Home")),
        ]
    );
    assert_eq!(c.emails, vec![field("juergen@example.com", None)]);
}

#[test]
fn folded_lines_and_escapes() {
    let vcf = "BEGIN:VCARD\nVERSION:3.0\nFN:Smith\\, John\nNOTE:x\nADR:;;Line one\\nLine two;Town;;;\n  \nORG:Very long organisation\n  name\nEND:VCARD\n";
    let c = parse_vcard(vcf).expect("card");
    assert_eq!(c.name, "Smith, John");
    assert_eq!(c.address.as_deref(), Some("Line one, Line two, Town"));
    assert_eq!(
        c.affiliation.as_deref(),
        Some("Very long organisation name")
    );
}

#[test]
fn name_falls_back_to_n_then_org_then_email() {
    let n_only = "BEGIN:VCARD\r\nN:Doe;John;;;\r\nEND:VCARD\r\n";
    assert_eq!(parse_vcard(n_only).unwrap().name, "John Doe");

    let org_only = "BEGIN:VCARD\r\nORG:Example Corp\r\nEMAIL:info@example.com\r\nEND:VCARD\r\n";
    let c = parse_vcard(org_only).unwrap();
    assert_eq!(c.name, "Example Corp");
    // Not repeated as the line under the name.
    assert_eq!(c.affiliation, None);

    let email_only = "BEGIN:VCARD\r\nEMAIL:info@example.com\r\nEND:VCARD\r\n";
    assert_eq!(parse_vcard(email_only).unwrap().name, "info@example.com");

    let empty = "BEGIN:VCARD\r\nVERSION:4.0\r\nEND:VCARD\r\n";
    assert_eq!(parse_vcard(empty).unwrap().name, "(Contact)");
}

#[test]
fn only_the_first_card_is_shown_and_the_rest_counted() {
    let vcf = "BEGIN:VCARD\r\nFN:First\r\nEMAIL:a@example.com\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nFN:Second\r\nEMAIL:b@example.com\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nFN:Third\r\nEND:VCARD\r\n";
    let c = parse_vcard(vcf).unwrap();
    assert_eq!(c.name, "First");
    assert_eq!(c.emails, vec![field("a@example.com", None)]);
    assert_eq!(c.more_cards, 2);
}

#[test]
fn preferred_address_wins_and_duplicates_are_dropped() {
    let vcf = "BEGIN:VCARD\r\nFN:X\r\n\
ADR;TYPE=home:;;Home St 1;Town;;;\r\n\
ADR;TYPE=work,pref:;;Work St 2;City;;;\r\n\
EMAIL:x@example.com\r\nEMAIL;TYPE=pref:mailto:X@example.com\r\n\
END:VCARD\r\n";
    let c = parse_vcard(vcf).unwrap();
    assert_eq!(c.address.as_deref(), Some("Work St 2, City"));
    assert_eq!(c.emails, vec![field("x@example.com", None)]);
}

#[test]
fn lists_are_capped() {
    let mut vcf = String::from("BEGIN:VCARD\r\nFN:Many\r\n");
    for i in 0..20 {
        vcf.push_str(&format!("TEL:+1 555 {i:04}\r\n"));
    }
    vcf.push_str("END:VCARD\r\n");
    assert_eq!(parse_vcard(&vcf).unwrap().phones.len(), MAX_ENTRIES);
}

#[test]
fn not_a_vcard() {
    assert_eq!(parse_vcard(""), None);
    assert_eq!(parse_vcard("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n"), None);
    assert_eq!(parse_vcard_bytes(&[0xff, 0xfe, 0x00]), None);
}

#[test]
fn latin1_bytes_are_read() {
    let mut bytes = b"BEGIN:VCARD\r\nFN:Ren".to_vec();
    bytes.push(0xe9);
    bytes.extend_from_slice(b"e\r\nEND:VCARD\r\n");
    assert_eq!(parse_vcard_bytes(&bytes).unwrap().name, "Renée");
}

#[test]
fn recognises_vcard_attachments() {
    assert!(is_vcard_attachment(Some("Contact.VCF"), None));
    assert!(is_vcard_attachment(Some("x.vcard"), None));
    assert!(is_vcard_attachment(None, Some("text/x-vcard")));
    assert!(is_vcard_attachment(Some("card"), Some("text/vcard")));
    assert!(!is_vcard_attachment(
        Some("invite.ics"),
        Some("text/calendar")
    ));
    assert!(!is_vcard_attachment(None, None));
}

#[test]
fn a_phone_kind_is_capitalised_without_slicing_bytes() {
    // `s[..1]` was safe only because the kind is always one of the ASCII
    // labels `label` maps to, so no input can reach a multi-byte first
    // character today. This keeps the capitalising path itself covered.
    let card = "BEGIN:VCARD\nVERSION:3.0\nTEL;CELL:+1 555 0100\nEND:VCARD";
    let parsed = parse_vcard(card).expect("a minimal card parses");
    let f = &parsed.phones[0];
    assert_eq!(f.label.as_deref(), Some("Mobile"));
    assert_eq!(f.value, "+1 555 0100");
}
