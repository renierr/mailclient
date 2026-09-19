use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::store::accounts;

use crate::bridge::qobject;
use crate::bridge::session::{checkout_session, current_account};
use crate::bridge::worker::spawn_job;

impl qobject::Bridge {
    pub fn refresh_server_capabilities(self: Pin<&mut Self>, account_id: i64) -> QString {
        spawn_job(self, "Capabilities", move |db, _progress| async move {
            let acc = if account_id >= 0 {
                match accounts::get(&db, account_id) {
                    Ok(a) => a,
                    Err(e) => {
                        let payload = serde_json::json!({
                            "account_id": account_id,
                            "email": "",
                            "imap_host": "",
                            "imap_port": 0,
                            "capabilities": Vec::<String>::new(),
                            "error": e.to_string(),
                        });
                        return Ok((payload.to_string(), None));
                    }
                }
            } else {
                match current_account(&db, account_id) {
                    Ok(a) => a,
                    Err(e) => {
                        let payload = serde_json::json!({
                            "account_id": account_id,
                            "email": "",
                            "imap_host": "",
                            "imap_port": 0,
                            "capabilities": Vec::<String>::new(),
                            "error": e,
                        });
                        return Ok((payload.to_string(), None));
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
                Ok(caps) => serde_json::json!({
                    "account_id": acc.id,
                    "email": acc.email_address,
                    "imap_host": acc.imap_host,
                    "imap_port": acc.imap_port,
                    "capabilities": caps,
                    "error": "",
                }),
                Err(e) => serde_json::json!({
                    "account_id": acc.id,
                    "email": acc.email_address,
                    "imap_host": acc.imap_host,
                    "imap_port": acc.imap_port,
                    "capabilities": Vec::<String>::new(),
                    "error": e,
                }),
            };
            // Read-only: capabilities change nothing the feeds show, so the
            // list keeps its scroll position and selection.
            Ok((payload.to_string(), None))
        })
    }
}
