//! The single event stream Dart listens on.
//!
//! Replaces `mailapp`'s `job_finished` / `job_progress` Qt signals. Dart opens
//! it once at startup and keeps it for the life of the app; a finished event
//! tells the state layer which account/folder to re-read.

use std::sync::{Mutex, OnceLock};

use flutter_rust_bridge::frb;

// `StreamSink` is emitted by the codegen into `frb_generated`, not exported
// from the runtime crate -- it is generic over the codec the generated glue
// picked, so it cannot exist before that file does.
use crate::frb_generated::StreamSink;

/// Where a job is in its life.
#[frb]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobPhase {
    /// A milestone worth acting on before the job is over (SMTP accepted the
    /// message, say). Nothing is invalidated yet — `Finished` still follows.
    Progress,
    /// The job is over, successfully or not. `ok` says which.
    Finished,
}

/// One thing that happened on the network thread.
#[frb]
#[derive(Clone, Debug)]
pub struct JobEvent {
    /// `"Sync"`, `"Send"`, `"Search"`, `"Folders"`, … — what ran.
    pub kind: String,
    pub phase: JobPhase,
    /// Human-readable, meant for a status bar. On failure this is the error.
    pub status: String,
    /// Whether the job succeeded. Always `true` for [`JobPhase::Progress`].
    pub ok: bool,
    /// Account whose cached data changed, or `-1` for "nothing changed".
    pub account_id: i64,
    /// Folder whose messages changed; `-1` with a real `account_id` means
    /// every folder of that account (a full sync).
    pub folder_id: i64,
}

fn sink() -> &'static Mutex<Option<StreamSink<JobEvent>>> {
    static SINK: OnceLock<Mutex<Option<StreamSink<JobEvent>>>> = OnceLock::new();
    SINK.get_or_init(|| Mutex::new(None))
}

/// Subscribe to job events. Call once; a second call replaces the first
/// listener, which is what a Flutter hot restart does.
#[frb]
pub fn job_events(sink: StreamSink<JobEvent>) {
    *crate::api::events::sink()
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(sink);
}

/// Push an event to Dart, if anyone is listening.
///
/// A dropped event is not an error: before Dart subscribes, and after a hot
/// restart tears the old sink down, there is genuinely no one to tell.
pub(crate) fn emit_event(event: JobEvent) {
    let guard = sink().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = guard.as_ref() {
        if s.add(event).is_err() {
            log::debug!("events: no listener, dropping event");
        }
    }
}
