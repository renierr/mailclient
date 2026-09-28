//! Parsing the composer's JSON form.
//!
//! Send and Save-draft take the same shape from the UI and differ only in
//! what they do with it, so it is parsed once here.

use crate::models::Account;
use crate::sync::sender::{SendFormat, SendPolicy, SendRequest};

/// A composer form, owned, so it can be moved onto the network thread.
#[derive(Default, Debug, Clone)]
pub struct ComposeForm {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    /// Sender address; empty means the account's own.
    pub from: String,
    /// Display name for `From:`; empty means the account default.
    pub from_name: String,
    /// Where replies should go instead of `From:`; empty means no header.
    pub reply_to: String,
    pub subject: String,
    /// Composer body. May hold HTML source — `resolve_bodies` sorts that out.
    pub body: String,
    /// Explicit HTML override; empty means derive from `body`.
    pub body_html: String,
    /// Local file paths (or `file://` URLs) to attach.
    pub attachments: Vec<String>,
    /// UID of the draft this was opened from, `-1` for a fresh message.
    /// A successful send or save removes it.
    pub draft_uid: i32,
}

impl ComposeForm {
    /// Parse `{to, cc?, bcc?, from?, from_name?, reply_to?, subject, body,
    /// body_html?, attachments?, draft_uid?}`.
    pub fn parse(json: &str) -> Result<Self, String> {
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|_| "invalid message form".to_string())?;
        let text = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        // Recipient fields arrive as one string from a text input; `,` and `;`
        // both separate, because both are what people type.
        let addresses = |k: &str| {
            text(k)
                .split([',', ';'])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        };
        // An array of paths; a legacy newline/`;`-separated string is accepted too.
        let attachments = match v.get("attachments") {
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            Some(serde_json::Value::String(s)) => s
                .split(['\n', ';'])
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect(),
            _ => Vec::new(),
        };
        Ok(Self {
            to: addresses("to"),
            cc: addresses("cc"),
            bcc: addresses("bcc"),
            from: text("from"),
            from_name: text("from_name"),
            reply_to: text("reply_to"),
            subject: text("subject"),
            body: text("body"),
            body_html: text("body_html"),
            attachments,
            draft_uid: v
                .get("draft_uid")
                .and_then(|x| x.as_i64())
                .unwrap_or(-1)
                .clamp(-1, i32::MAX as i64) as i32,
        })
    }

    /// Borrow the form as a send request.
    ///
    /// `from_name` falls back to the account's default, and the password
    /// fields stay empty — nothing before the SMTP submit needs a secret, so
    /// none is read out of the keyring until then.
    pub fn as_request<'a>(
        &'a self,
        account: &'a Account,
        format: SendFormat,
        include_plain: bool,
        request_mdn: bool,
        policy: &'a SendPolicy,
    ) -> SendRequest<'a> {
        let some = |s: &'a str| (!s.is_empty()).then_some(s);
        SendRequest {
            to: &self.to,
            cc: &self.cc,
            bcc: &self.bcc,
            from: some(&self.from),
            from_name: some(&self.from_name).or_else(|| some(account.from_name.trim())),
            reply_to: some(&self.reply_to),
            subject: &self.subject,
            body_text: &self.body,
            body_html: some(&self.body_html),
            attachments: &self.attachments,
            format,
            include_plain,
            policy,
            password: "",
            imap_password: None,
            request_mdn,
        }
    }

    /// Reject a message with nowhere to go.
    ///
    /// Only all three empty is refused: a To that holds placeholder text, or
    /// a Bcc-only send, are both legitimate. Unparseable entries are filtered
    /// further down, where "no real recipient remains" is the error.
    pub fn require_recipient(&self) -> Result<(), String> {
        if self.to.is_empty() && self.cc.is_empty() && self.bcc.is_empty() {
            return Err("add at least one recipient (To, Cc or Bcc)".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ComposeForm;
    use crate::sync::sender::support::test_account;
    use crate::sync::sender::{SendFormat, SendPolicy};

    #[test]
    fn recipients_split_on_either_separator_and_drop_blanks() {
        let f = ComposeForm::parse(r#"{"to":"a@example.com, b@example.com ;; c@example.com"}"#)
            .unwrap();
        assert_eq!(f.to, ["a@example.com", "b@example.com", "c@example.com"]);
        assert_eq!(f.draft_uid, -1);
    }

    #[test]
    fn a_message_with_only_bcc_is_allowed_but_an_empty_one_is_not() {
        ComposeForm::parse(r#"{"bcc":"a@example.com"}"#)
            .unwrap()
            .require_recipient()
            .unwrap();
        assert!(ComposeForm::parse("{}")
            .unwrap()
            .require_recipient()
            .is_err());
        assert!(ComposeForm::parse("not json").is_err());
    }

    #[test]
    fn attachments_accept_an_array_or_a_legacy_string() {
        let a = ComposeForm::parse(r#"{"attachments":[" /a.txt ", ""]}"#).unwrap();
        assert_eq!(a.attachments, ["/a.txt"]);
        let b = ComposeForm::parse(r#"{"attachments":"/a.txt\n/b.txt;"}"#).unwrap();
        assert_eq!(b.attachments, ["/a.txt", "/b.txt"]);
    }

    #[test]
    fn draft_uid_out_of_range_reads_as_fresh() {
        let f = ComposeForm::parse(r#"{"draft_uid":-7}"#).unwrap();
        assert_eq!(f.draft_uid, -1);
        let f = ComposeForm::parse(r#"{"draft_uid":42}"#).unwrap();
        assert_eq!(f.draft_uid, 42);
    }

    #[test]
    fn empty_fields_become_none_and_the_sender_name_falls_back_to_the_account() {
        let mut acc = test_account();
        acc.from_name = " Account Name ".to_string();
        let policy = SendPolicy::Unrestricted;
        let f = ComposeForm::parse(r#"{"to":"a@example.com"}"#).unwrap();
        let req = f.as_request(&acc, SendFormat::Auto, true, false, &policy);
        assert_eq!(req.from, None);
        assert_eq!(req.reply_to, None);
        assert_eq!(req.body_html, None);
        assert_eq!(req.from_name, Some("Account Name"));

        let f = ComposeForm::parse(r#"{"to":"a@example.com","from_name":"Typed"}"#).unwrap();
        let req = f.as_request(&acc, SendFormat::Auto, true, false, &policy);
        assert_eq!(req.from_name, Some("Typed"));
    }
}
