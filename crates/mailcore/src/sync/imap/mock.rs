//! Mock IMAP server harness for regression tests (compiled with `cfg(test)`
//! only). Line-based, single-connection: answers `LOGIN` / `CAPABILITY` /
//! `ENABLE` itself and delegates everything else to a per-test closure.

use std::sync::Arc;

pub(crate) struct MockImapServer {
    pub(crate) port: u16,
    pub(crate) received: Arc<tokio::sync::Mutex<Vec<String>>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl MockImapServer {
    pub(crate) async fn start<F>(caps: &'static str, custom_handler: F) -> Self
    where
        F: Fn(&str, &str) -> Vec<String> + Send + Sync + 'static,
    {
        Self::start_with_greeting(
            caps,
            format!("* OK [CAPABILITY {caps}] Mock IMAP Server ready\r\n"),
            custom_handler,
        )
        .await
    }

    pub(crate) async fn start_with_greeting<F>(
        caps: &'static str,
        greeting: String,
        custom_handler: F,
    ) -> Self
    where
        F: Fn(&str, &str) -> Vec<String> + Send + Sync + 'static,
    {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let received = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let rec_clone = Arc::clone(&received);

        let handle = tokio::spawn(async move {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);

            let greeting = greeting;
            if writer.write_all(greeting.as_bytes()).await.is_err() {
                return;
            }

            let mut line = String::new();
            while let Ok(n) = reader.read_line(&mut line).await {
                if n == 0 {
                    break;
                }
                let raw_cmd = line.trim_end_matches(['\r', '\n']).to_string();
                line.clear();
                if raw_cmd.is_empty() {
                    continue;
                }

                rec_clone.lock().await.push(raw_cmd.clone());

                let mut parts = raw_cmd.splitn(2, ' ');
                let tag = parts.next().unwrap_or("*");
                let rest = parts.next().unwrap_or("");
                let upper_rest = rest.to_ascii_uppercase();

                let responses = if upper_rest.starts_with("LOGIN") {
                    vec![format!("{tag} OK LOGIN completed\r\n")]
                } else if upper_rest.starts_with("CAPABILITY") {
                    vec![
                        format!("* CAPABILITY {caps}\r\n"),
                        format!("{tag} OK CAPABILITY completed\r\n"),
                    ]
                } else if upper_rest.starts_with("ENABLE") {
                    if caps.contains("ENABLE") {
                        let enabled = rest.strip_prefix("ENABLE ").unwrap_or("").trim();
                        vec![
                            format!("* ENABLED {enabled}\r\n"),
                            format!("{tag} OK ENABLE completed\r\n"),
                        ]
                    } else {
                        vec![format!("{tag} BAD ENABLE unknown command\r\n")]
                    }
                } else {
                    custom_handler(tag, rest)
                };

                for resp in responses {
                    if writer.write_all(resp.as_bytes()).await.is_err() {
                        return;
                    }
                }
            }
        });

        Self {
            port,
            received,
            _handle: handle,
        }
    }
}

pub(crate) fn test_mock_account(port: u16) -> crate::models::Account {
    crate::models::Account {
        id: 1,
        name: "Mock Account".to_string(),
        email_address: "alice@example.com".to_string(),
        from_name: "Alice".to_string(),
        imap_host: "127.0.0.1".to_string(),
        imap_port: port,
        imap_security: "plain".to_string(),
        imap_username: "alice@example.com".to_string(),
        smtp_host: "127.0.0.1".to_string(),
        smtp_port: 25,
        smtp_security: "plain".to_string(),
        smtp_username: "alice@example.com".to_string(),
        auth_vault_key: "vault_key".to_string(),
        check_interval_secs: 300,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    }
}
