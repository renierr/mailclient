//! Single-instance relay for `mailapp --open` clicks.
//!
//! The running window listens on a loopback TCP port and records it in
//! `gui.port` next to the database. A `--open` process queues its jump in
//! SQLite (`settings::set_pending_open`), then nudges that port; the window
//! takes the request when nudged instead of polling for it. Only a fixed
//! handshake crosses the socket and the request itself stays in the
//! database, so a stray connection can at most make the window look for a
//! request that is not there. An answered nudge is also the liveness check:
//! no answer (no file, a stale port after a crash, a stranger on it) means
//! no window, and the caller boots one.
//!
//! Loopback TCP rather than a Unix socket or a named pipe: std has it on
//! every platform, so Linux and Windows share one path and no dependency.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

const HELLO: &[u8] = b"mailclient-open\n";
const ACK: &[u8] = b"ok\n";
/// A live window answers at once; a hung or foreign peer must not stall a
/// click (or the listener) for longer than this.
const TIMEOUT: Duration = Duration::from_millis(500);

/// `gui.port` beside the database, so a dev database and the real one each
/// have their own window.
pub fn port_path(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("gui.port")
}

/// Start listening for nudges: bind a loopback port, record it, and run
/// `on_nudge` on a background thread for every valid handshake.
pub fn listen(db_path: &Path, on_nudge: impl Fn() + Send + 'static) -> std::io::Result<()> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let path = port_path(db_path);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, port.to_string())?;
    std::thread::Builder::new()
        .name("mailclient-relay".to_string())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                if answer(&stream) {
                    on_nudge();
                }
            }
        })?;
    Ok(())
}

/// Tell a running window to take its queued request. True when a window
/// answered the handshake.
pub fn nudge(db_path: &Path) -> bool {
    let Some(port) = std::fs::read_to_string(port_path(db_path))
        .ok()
        .and_then(|text| parse_port(&text))
    else {
        return false;
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, TIMEOUT) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(TIMEOUT));
    let _ = stream.set_write_timeout(Some(TIMEOUT));
    stream.write_all(HELLO).is_ok() && read_line(&stream).as_deref() == Some(ACK)
}

/// The window is closing: later clicks must boot a new one, not nudge a
/// port nobody listens on (they would notice, but only after a timeout).
pub fn forget(db_path: &Path) {
    let _ = std::fs::remove_file(port_path(db_path));
}

/// The listener's side of the handshake: the hello in, the ack out.
fn answer(mut stream: &TcpStream) -> bool {
    let _ = stream.set_read_timeout(Some(TIMEOUT));
    let _ = stream.set_write_timeout(Some(TIMEOUT));
    read_line(stream).as_deref() == Some(HELLO) && stream.write_all(ACK).is_ok()
}

/// One short line, newline included; `None` on error or timeout.
fn read_line(stream: &TcpStream) -> Option<Vec<u8>> {
    let mut line = Vec::new();
    BufReader::new(stream)
        .take(64)
        .read_until(b'\n', &mut line)
        .ok()?;
    Some(line)
}

fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|p| *p > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_port_file_sits_beside_the_database() {
        assert_eq!(
            port_path(Path::new("/data/mail/mailclient.sqlite")),
            Path::new("/data/mail/gui.port")
        );
        assert_eq!(
            port_path(Path::new("mailclient.sqlite")),
            Path::new("./gui.port")
        );
    }

    #[test]
    fn only_a_real_port_parses() {
        assert_eq!(parse_port("41234\n"), Some(41234));
        assert_eq!(parse_port("0"), None);
        assert_eq!(parse_port("70000"), None);
        assert_eq!(parse_port("garbage"), None);
    }

    // Loopback only: the listener and the nudge are this process.
    #[test]
    fn a_nudge_reaches_the_listener_and_a_missing_one_says_so() {
        let dir = std::env::temp_dir().join(format!("mailclient-relay-{}", std::process::id()));
        let db = dir.join("mailclient.sqlite");
        assert!(!nudge(&db), "no port file: no window");

        let (tx, rx) = std::sync::mpsc::channel();
        listen(&db, move || {
            let _ = tx.send(());
        })
        .expect("listen on loopback");
        assert!(nudge(&db));
        rx.recv_timeout(Duration::from_secs(2))
            .expect("the listener ran its callback");

        forget(&db);
        assert!(!nudge(&db), "forgotten: no window");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
