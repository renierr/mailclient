use std::pin::Pin;

use cxx_qt_lib::QString;

use crate::bridge::qobject;
use crate::bridge::qstring;

impl qobject::SettingsBridge {
    fn shared_db() -> Option<&'static mailcore::Db> {
        crate::bridge::shared_db()
            .map_err(|e| {
                log::warn!("settings: cannot open db: {e}");
            })
            .ok()
    }

    /// Reload properties from the settings store.
    pub fn load(mut self: Pin<&mut Self>) {
        if let Some(db) = Self::shared_db() {
            self.as_mut().set_sent_copy_enabled(
                mailcore::store::settings::get_bool(
                    db,
                    mailcore::store::settings::SENT_COPY_ENABLED,
                )
                .unwrap_or(true),
            );
            self.as_mut().set_load_remote_images(
                mailcore::store::settings::get_bool(
                    db,
                    mailcore::store::settings::LOAD_REMOTE_IMAGES,
                )
                .unwrap_or(false),
            );
            self.as_mut()
                .set_compose_send_format(qstring(&mailcore::store::settings::get_send_format(db)));
            self.as_mut().set_compose_include_plain(
                mailcore::store::settings::get_bool(
                    db,
                    mailcore::store::settings::COMPOSE_INCLUDE_PLAIN,
                )
                .unwrap_or(true),
            );
            self.as_mut().set_auto_mark_read(
                mailcore::store::settings::get_bool(db, mailcore::store::settings::AUTO_MARK_READ)
                    .unwrap_or(true),
            );
            self.as_mut()
                .set_mark_read_delay_secs(mailcore::store::settings::get_delay_secs(
                    db,
                    mailcore::store::settings::MARK_READ_DELAY_SECS,
                ) as i32);
            self.as_mut().set_collect_sent_contacts(
                mailcore::store::settings::get_bool(
                    db,
                    mailcore::store::settings::COLLECT_SENT_CONTACTS,
                )
                .unwrap_or(true),
            );
            self.as_mut().set_confirm_delete(
                mailcore::store::settings::get_bool(db, mailcore::store::settings::CONFIRM_DELETE)
                    .unwrap_or(true),
            );
            self.as_mut()
                .set_list_density(qstring(&mailcore::store::settings::get_density(db)));
            self.as_mut()
                .set_reader_font_size(qstring(&mailcore::store::settings::get_reader_font(db)));
            self.as_mut()
                .set_link_click_action(qstring(&mailcore::store::settings::get_link_click(db)));
            self.as_mut()
                .set_sync_interval_minutes(mailcore::store::settings::get_sync_interval(db) as i32);
            self.as_mut().set_signature_enabled(
                mailcore::store::settings::get_bool(
                    db,
                    mailcore::store::settings::SIGNATURE_ENABLED,
                )
                .unwrap_or(false),
            );
            self.as_mut()
                .set_signature_text(qstring(&mailcore::store::settings::get_signature_text(db)));
            self.as_mut().set_reply_below_quote(
                mailcore::store::settings::get_bool(
                    db,
                    mailcore::store::settings::REPLY_BELOW_QUOTE,
                )
                .unwrap_or(false),
            );
            self.as_mut().set_request_mdn(
                mailcore::store::settings::get_bool(db, mailcore::store::settings::REQUEST_MDN)
                    .unwrap_or(false),
            );
            self.as_mut()
                .set_ui_scale(mailcore::store::settings::get_ui_scale(db));
        }
    }

    /// Persist current properties to the settings store. Returns `""` when
    /// everything was written, else which settings failed -- the dialog must
    /// not report "Settings saved" for values that are lost on restart.
    pub fn save(self: Pin<&mut Self>) -> QString {
        let Some(db) = Self::shared_db() else {
            return qstring("settings not saved: the database is unavailable");
        };
        let mut failed: Vec<&str> = Vec::new();
        let mut note = |label: &'static str, result: mailcore::error::Result<()>| {
            if let Err(e) = result {
                log::warn!("settings: cannot save {label}: {e}");
                failed.push(label);
            }
        };
        let sent = *self.sent_copy_enabled();
        let remote = *self.load_remote_images();
        let format = mailcore::store::settings::normalize_send_format(
            &self.compose_send_format().to_string(),
        )
        .to_string();
        let include_plain = *self.compose_include_plain();
        let auto_read = *self.auto_mark_read();
        let delay = *self.mark_read_delay_secs() as i64;
        let collect_contacts = *self.collect_sent_contacts();
        let confirm_delete = *self.confirm_delete();
        let density =
            mailcore::store::settings::normalize_density(&self.list_density().to_string())
                .to_string();
        let reader_font =
            mailcore::store::settings::normalize_reader_font(&self.reader_font_size().to_string())
                .to_string();
        let link_click =
            mailcore::store::settings::normalize_link_click(&self.link_click_action().to_string())
                .to_string();
        let sync_interval = *self.sync_interval_minutes() as i64;
        let signature_enabled = *self.signature_enabled();
        let signature_text = self.signature_text().to_string();
        let reply_below_quote = *self.reply_below_quote();
        let request_mdn = *self.request_mdn();
        let ui_scale = *self.ui_scale();
        note(
            "sent-copy",
            mailcore::store::settings::set_bool(
                db,
                mailcore::store::settings::SENT_COPY_ENABLED,
                sent,
            ),
        );
        note(
            "remote-images",
            mailcore::store::settings::set_bool(
                db,
                mailcore::store::settings::LOAD_REMOTE_IMAGES,
                remote,
            ),
        );
        note(
            "send-format",
            mailcore::store::settings::set(
                db,
                mailcore::store::settings::COMPOSE_SEND_FORMAT,
                &format,
            ),
        );
        note(
            "include-plain",
            mailcore::store::settings::set_bool(
                db,
                mailcore::store::settings::COMPOSE_INCLUDE_PLAIN,
                include_plain,
            ),
        );
        note(
            "auto-mark-read",
            mailcore::store::settings::set_bool(
                db,
                mailcore::store::settings::AUTO_MARK_READ,
                auto_read,
            ),
        );
        note(
            "mark-read delay",
            mailcore::store::settings::set_delay_secs(
                db,
                mailcore::store::settings::MARK_READ_DELAY_SECS,
                delay,
            ),
        );
        note(
            "sent-contact collection",
            mailcore::store::settings::set_bool(
                db,
                mailcore::store::settings::COLLECT_SENT_CONTACTS,
                collect_contacts,
            ),
        );
        note(
            "confirm-delete",
            mailcore::store::settings::set_bool(
                db,
                mailcore::store::settings::CONFIRM_DELETE,
                confirm_delete,
            ),
        );
        note(
            "list density",
            mailcore::store::settings::set(db, mailcore::store::settings::LIST_DENSITY, &density),
        );
        note(
            "reader font size",
            mailcore::store::settings::set(
                db,
                mailcore::store::settings::READER_FONT_SIZE,
                &reader_font,
            ),
        );
        note(
            "link click action",
            mailcore::store::settings::set(
                db,
                mailcore::store::settings::LINK_CLICK_ACTION,
                &link_click,
            ),
        );
        note(
            "sync interval",
            mailcore::store::settings::set_sync_interval(db, sync_interval),
        );
        note(
            "signature toggle",
            mailcore::store::settings::set_bool(
                db,
                mailcore::store::settings::SIGNATURE_ENABLED,
                signature_enabled,
            ),
        );
        note(
            "signature text",
            mailcore::store::settings::set(
                db,
                mailcore::store::settings::SIGNATURE_TEXT,
                &signature_text,
            ),
        );
        note(
            "reply position",
            mailcore::store::settings::set_bool(
                db,
                mailcore::store::settings::REPLY_BELOW_QUOTE,
                reply_below_quote,
            ),
        );
        note(
            "receipt request",
            mailcore::store::settings::set_bool(
                db,
                mailcore::store::settings::REQUEST_MDN,
                request_mdn,
            ),
        );
        note(
            "interface scale",
            mailcore::store::settings::set_ui_scale(db, ui_scale),
        );
        if failed.is_empty() {
            qstring("")
        } else {
            qstring(&format!("settings not saved: {}", failed.join(", ")))
        }
    }
}
