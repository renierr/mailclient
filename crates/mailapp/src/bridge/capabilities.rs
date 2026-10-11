use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::store::accounts;

use crate::bridge::qobject;
use crate::bridge::worker::spawn_job;
use mailcore::sync::capabilities;
use mailcore::sync::pool::{checkout_session, resolve_account};

impl qobject::Bridge {
    pub fn refresh_server_capabilities(self: Pin<&mut Self>, account_id: i64) -> QString {
        spawn_job(self, "Capabilities", move |db, _progress| async move {
            let acc = if account_id >= 0 {
                match accounts::get(db, account_id) {
                    Ok(a) => a,
                    Err(e) => {
                        return Ok((
                            capabilities::error_payload(account_id, &e.to_string()),
                            None,
                        ));
                    }
                }
            } else {
                match resolve_account(db, account_id) {
                    Ok(a) => a,
                    Err(e) => {
                        return Ok((capabilities::error_payload(account_id, &e), None));
                    }
                }
            };
            let caps_res = async {
                let mut imap = checkout_session(&acc).await?;
                let caps = imap.capabilities_list().await.map_err(|e| e.to_string())?;
                imap.checkin();
                Ok::<_, String>(caps)
            }
            .await;
            let payload = match caps_res {
                Ok(caps) => capabilities::payload(&acc, &caps),
                Err(e) => capabilities::failed_payload(&acc, &e),
            };
            // Read-only: capabilities change nothing the feeds show, so the
            // list keeps its scroll position and selection.
            Ok((payload, None))
        })
    }
}
