# Mail sync: flow, triggers and timing

How the client talks to the IMAP (and SMTP) servers: what starts each
exchange, what it does on the wire, and how each setting changes it. The
logic lives in `mailcore`; the frontends and the Android host only decide
*when* to call it.

There are two worlds:

- **Foreground** — the app is open (Qt desktop, or Flutter on any
  platform). It syncs the account you are looking at.
- **Background** — Android only, app closed. Native Kotlin plus the Rust
  core, no Flutter engine. It checks inboxes and posts notifications, using
  polling, push (IMAP IDLE), or both.

The Qt desktop client has no background mode: when it is closed, nothing
syncs.

## 1. Who triggers what

| Trigger | Where | Accounts | Scope | Connection |
|---|---|---|---|---|
| App start | Qt, Flutter | the open account | all subscribed folders | pooled session |
| Switching account | Qt, Flutter | the new account | all subscribed folders | pooled session |
| Sync button, `Ctrl+R` / `F5` (Qt) | Qt, Flutter | the open account | all subscribed folders | pooled session |
| Auto-sync timer (check interval) | Qt, Flutter | the open account | all subscribed folders | pooled session |
| App back in foreground | Flutter on Android | the open account | all subscribed folders | pooled session |
| Notification tap, mail not cached yet | Flutter | the open account | all subscribed folders | pooled session |
| WorkManager tick | Android background | polled accounts that are due | inbox only | fresh connection each |
| Exact-alarm tick | Android background | polled accounts that are due | inbox only | fresh connection each |
| Server says "mailbox changed" during IDLE | Android push service | that one account | inbox only | the IDLE connection itself |
| Push keep-alive alarm | Android push service | every push account | no sync, IDLE re-issued | the IDLE connection itself |
| Network change | Android push service | every push account | reconnect, then inbox | new connection |

Foreground syncs never touch accounts you are not looking at. Background
checks never sync anything but inboxes.

## 2. Foreground sync

### When it runs

- **Start-up.** The cached mailbox is painted first, then a sync of the
  open account starts deferred, so a slow server never holds the first paint.
- **Account switch.** Same: cache first, then sync the new account.
- **Manual.** The Sync button (both frontends), `Ctrl+R` / `F5` (Qt).
- **Auto-sync timer.** A repeating timer at the open account's **check
  interval** (its own value or the app-wide default). It fires only if no
  other job is running ("never mid-action"); a busy tick is simply skipped,
  not queued. *Manually* (0) stops the timer. The timer is re-read when you
  switch accounts or save Settings.
- **Resume (Flutter on Android).** Android freezes a backgrounded app, so
  the timer did not tick meanwhile. On resume the app shows the cache
  (which background checks may have filled), then syncs — unless auto-sync
  is off or a sync was requested less than a minute ago.
- **Notification tap.** Opens the mail; if it is not in the cache yet, a
  sync is started and the app waits briefly for it to land.
- **Push while the app is open (Android).** A push check that found new
  mail does not notify; it tells the open app to reload from the cache
  (no extra network).

### What one sync does (`headless::sync_account`, scope `All`)

All on the single network thread (`mailclient-net`), over a pooled IMAP
session that stays logged in between syncs:

1. **SMTP outbox flush** — queued sends go out (fresh SMTP connection),
   plus the copy into Sent if *Save a copy in Sent* is on for the account.
2. **Flag push** — local read/star/... changes go up to the server.
3. **Queued moves** — after the flags, since a move changes the UID.
4. **Folder list** — one quick `LIST` every sync; the full multi-pass
   discovery only when the tree changed or `FULL_DISCOVERY_INTERVAL_SECS`
   has passed (`sync/imap/folders.rs`).
5. **Folders** — every *subscribed* folder: the inbox over the
   `FULL_SYNC_WINDOW` newest messages, every other folder over the smaller
   `QUICK_SYNC_WINDOW` (`sync/imap/types.rs`). Hidden folders are skipped.
   Per-folder errors are collected; one broken folder does not stop the rest.

Opening a folder or a message reads the cache; body fetches and "load
older" are separate on-demand jobs, not part of the sync.

## 3. Android background (app closed)

### Planning: settings → mechanisms

Whenever an interval, the background method or an account changes, the app
asks the core for a plan (`sync::background::schedule::plan`) and
`MailSchedule.kt` starts exactly what it names and stops the rest:

- Every account with an interval above 0 checks in the background.
- Accounts with **Push** on (per account; default = app-wide method is
  *push*) go to the **push service**.
- All other accounts are **polled** by **one** poller ticking at the
  **shortest interval among them**. The poller is the exact alarm when the
  app-wide method is *alarm*, otherwise WorkManager.
- So push and polling can run side by side: e.g. one account on push, two
  polled every 15 and 60 minutes.

### Poller tick (`background::background_check`)

Both pollers end up in the same tick, run natively in the Rust core:

1. Take the **sync lock** (a lock file next to the database). If another
   tick or a push check holds it, the tick is skipped quietly.
2. Pick the **due accounts** (`schedule::due_accounts`): polled accounts
   whose own interval has run out since their last check
   (`bg_checked_at_{id}`). An account counts as due half a tick early,
   because schedulers fire late or anywhere inside their period — otherwise
   a 30-minute account on a 15-minute tick would slip to 45 minutes.
3. Sync each due account, **inbox only**, over a **fresh connection**
   (never the GUI's pooled session), using the cached folder tree (no
   `LIST`) once it knows the inbox. The outbox and flag push still run.
4. Stamp each successfully checked account.
5. Compare against per-folder high-water marks and build a report of new
   unread mail; the host turns it into a notification (see section 5).
6. Record the run in the history shown under Settings → background checks.

**WorkManager poller** (`MailCheckWorker.kt`) — the battery-friendly one.
Periodic work, first run one interval after scheduling, only with a network.
Android's floor for periodic work is 15 minutes, and Doze may push a run
into its next maintenance window, so ticks can come noticeably late.
Re-scheduling keeps the running period, so opening the app does not push
the next check back.

**Exact-alarm poller** (`MailAlarm.kt`) — the on-time one. A one-shot
`setExactAndAllowWhileIdle` alarm that fires in Doze and re-arms itself
for the next interval. Its receiver hands the check to expedited one-time
work (a receiver only gets seconds), which Doze does not defer while the
quota lasts. Without the exact-alarm permission the alarm is inexact but
still fires in Doze. Re-armed after reboot and app update.

### Push service (IMAP IDLE)

`MailPushService.kt` is a foreground service (Android requires the
low-importance "Push mail is on" notification for it) hosting the Rust
`PushMonitor` (`sync/push.rs`) on its own thread. Per push account:

1. **Connect** and log in, then **catch up**: one inbox check, since mail
   may have arrived while not connected.
2. **IDLE** on the inbox. The connection is now silent; the CPU sleeps.
   The service holds a wake lock only while the monitor is busy (capped so
   a stuck monitor cannot keep the phone awake).
3. **The server reports a change** (`EXISTS`, `EXPUNGE`, `FETCH`, ...):
   end the IDLE (`DONE`), run the inbox check over the **same session**
   (no reconnect, no TLS handshake), notify, IDLE again.
4. **Keep-alive alarm** every 15 minutes (`MailPush.KEEPALIVE_MINUTES`,
   exact and Doze-proof when allowed): ends and re-issues every IDLE. This
   keeps the connection and the carrier's NAT mapping alive, retries
   accounts that are backing off, re-reads which accounts use push, and
   restarts the service if Android killed it. A keep-alive does not sync.
5. **Awake refresh**: while the phone is awake anyway, IDLE is also
   re-issued after `IDLE_REFRESH` (25 minutes), inside the 30-minute limit
   RFC 2177 lets servers enforce. Tokio timers stop while the phone sleeps,
   which is why the alarm, not this timer, carries the cadence then.
6. **Network change** (Wi-Fi ↔ mobile, lost/regained): every account
   reconnects at once.
7. **Errors**: reconnect with backoff 30 s, doubling up to 15 minutes (the
   keep-alive cadence, so a dead server costs one attempt per alarm). Any
   keep-alive or network signal retries immediately.
8. A server **without IDLE** gets a check on every keep-alive instead.

The push check takes the same sync lock as a poller tick; if a tick holds
it, the push check retries a few times before giving up for that change.

**The check interval and push:** for a push account, the interval only
decides *whether* it checks in the background (above 0 = yes,
*Manually* = no push either). It does not set how often anything happens
— the server pushes, and the keep-alive is fixed at 15 minutes.

## 4. Server heartbeats ("Still here")

Some servers send an untagged `* OK Still here` during IDLE at a fixed
interval (Dovecot's `imap_idle_notify_interval`, for example, defaults to a
few minutes). The client cannot stop them. Each one wakes the radio and the
CPU just to be ignored, so a server sending one every 2 minutes costs far
more battery than the client's own 15-minute keep-alive.

The IDLE loop counts them and measures the average gap on the wall clock
(the monotonic clock stops while the phone sleeps and would make gaps look
shorter). The push monitor stores it per account
(`account_settings::record_idle_heartbeats`); a 15-minute IDLE without any
heartbeat clears it again. When the gap is shorter than the keep-alive,
the account's **Push** setting (Flutter, Android) shows a hint suggesting
*Check at the interval* for that account — polling every 15 minutes wakes
the phone 4 times an hour instead of 30.

## 5. Notifications (Android)

Every background report goes through `notify::plan_for`, which decides per
report: alert, update the shown notification, clear it, or stay silent.

- Only accounts with **New-mail notifications** on (per account, default =
  app-wide) contribute mail to an alert.
- Mail is reported once: per-folder high-water marks move when a
  notification has been handled, and mail the foreground showed while the
  app was open is marked seen when the app goes to the background.
- While the app is open, new mail reloads the list instead of alerting.
- Opening the app clears the notification.

## 6. Settings reference

All of these sit in Settings → Accounts & sync. "Settings for" switches
between the app-wide defaults and one account; an account value of
*Default (…)* inherits the app-wide one (`store::account_settings`).

| Setting | Per account | Foreground effect | Background effect (Android) |
|---|---|---|---|
| Check for new mail (interval) | yes | Auto-sync timer of the open account | Whether the account checks in the background; for polled accounts, how often (poller ticks at the shortest polled interval) |
| Push (IMAP IDLE) | yes, Flutter on Android | none | Push service instead of polling; default follows the app-wide method |
| Background method (WorkManager / alarm / push) | no, app-wide | none | Which poller runs, and the default for Push |
| Show notifications for new mail | yes, Flutter | none | Whether the account's new mail alerts |
| Save a copy of sent mail in Sent | yes | Sent copy after an SMTP send or outbox flush | Same, when a background tick flushes the outbox |
| Suggest recipients from sent mail | yes | Collects addresses you sent to | Same, during background syncs |

## 7. Timing at a glance

| What | How often | Defined in |
|---|---|---|
| Foreground auto-sync | open account's interval, only while idle | Qt `Main.qml`, Flutter `mail_state.dart` |
| Resume sync (Flutter, Android) | on resume, at most once a minute | `shouldSyncOnResume` |
| Full folder discovery | when the tree changed, else after `FULL_DISCOVERY_INTERVAL_SECS` | `sync/imap/folders.rs` |
| WorkManager tick | shortest polled interval, at least 15 min, may run late in Doze | `MailCheckWorker.kt` |
| Exact-alarm tick | shortest polled interval, on time in Doze | `MailAlarm.kt` |
| Push keep-alive (re-IDLE) | every 15 min | `MailPush.kt` |
| Push awake refresh | after 25 min of awake time | `push::IDLE_REFRESH` |
| Push reconnect backoff | 30 s doubling to 15 min | `push::backoff` |
| Due tolerance per account | half a poller tick early | `schedule::due_accounts` |

## 8. Coordination and safety

- **One network thread per process** in the foreground; jobs queue on it,
  so two foreground syncs never overlap.
- **Sync lock** (lock file beside the database) serialises the background
  tick, the push check and the desktop `--sync-once` CLI. The GUI never
  takes it: outbox rows are claimed atomically, so an overlapping GUI sync
  cannot send twice.
- Background checks use their own connections (fresh, or the IDLE one), so
  they never steal or stall the session the GUI is reading through.
