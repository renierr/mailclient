import QtQuick
import Quickshell
import Quickshell.Io
import "Model.js" as Model

// Headless state for the mailclient.unread bar widget.
//
// Single poll loop: `mailapp --sync-once --json` (real IMAP sync) every
// syncIntervalMin. A cache-only `--status` poll would just re-read the same
// SQLite rows while the app is closed, so it was dropped as noise — every
// tick here does real network and rewrites the cache itself.
// A rise in the unread count after the first baseline sync sends one
// desktop notification; clicking it opens the mail app via --exec.
Item {
  id: root

  property var settings: ({})
  property var bar: null

  property int unread: 0
  property var recent: []
  property bool syncing: false
  property string lastError: ""
  property double lastSyncAt: 0
  // Highest unread count already notified for. -1 = no baseline yet
  // (first sync only sets it). Named to avoid QQuickItem's FINAL
  // `baseline` anchor-line property.
  property int knownUnread: -1

  readonly property string mailBin: String(setting("mailBin", "mailapp") || "mailapp")
  readonly property string account: String(setting("account", "") || "")
  readonly property int syncIntervalMin: intSetting("syncIntervalMin", 15, 1, 1440)
  readonly property bool notify: boolSetting("notify", true)
  readonly property bool busy: syncProcess.running

  function setting(name, fallback) {
    var value = settings ? settings[name] : undefined
    return value === undefined || value === null || value === "" ? fallback : value
  }

  function intSetting(name, fallback, min, max) {
    var n = parseInt(String(setting(name, fallback)), 10)
    if (!isFinite(n)) n = fallback
    return Math.max(min, Math.min(max, n))
  }

  function boolSetting(name, fallback) {
    var value = settings ? settings[name] : undefined
    if (value === undefined || value === null || value === "") return fallback
    if (typeof value === "boolean") return value
    return String(value).toLowerCase() === "true" || String(value) === "1"
  }

  function accountArgs() {
    return root.account !== "" ? ["--account", root.account] : []
  }

  function refresh() {
    root.syncNow()
  }

  function syncNow() {
    if (syncProcess.running) return
    root.syncing = true
    syncProcess.command = [root.mailBin, "--sync-once", "--json"].concat(accountArgs())
    syncProcess.running = true
  }

  function applyReport(raw) {
    var parsed = Model.parseReport(raw)
    if (!parsed.ok) {
      lastError = "Could not read mail sync result"
      return
    }
    lastError = parsed.errors.length > 0 ? String(parsed.errors[0]) : ""
    unread = parsed.unread
    recent = parsed.recent
    lastSyncAt = Date.now()
    if (knownUnread < 0) {
      knownUnread = unread
    } else if (root.notify && unread > knownUnread) {
      notifyNew(unread - knownUnread)
      knownUnread = unread
    } else if (unread < knownUnread) {
      knownUnread = unread
    }
  }

  function notifyNew(count) {
    if (!root.bar || typeof root.bar.run !== "function") return
    var q = root.bar.shellQuote ? root.bar.shellQuote : function(s) { return "'" + String(s).replace(/'/g, "'\\''") + "'" }
    var headline = count === 1 ? "New mail" : "New mail (" + count + ")"
    var body = String(root.unread) + " unread"
    root.bar.run("omarchy notification send -g " + q("󰇮") + " " + q(headline) + " " + q(body) + " --exec " + q(root.mailBin))
  }

  function openApp() {
    if (root.bar && typeof root.bar.run === "function") {
      var q = root.bar.shellQuote ? root.bar.shellQuote : function(s) { return "'" + String(s).replace(/'/g, "'\\''") + "'" }
      root.bar.run("uwsm-app -- " + q(root.mailBin))
    } else {
      Quickshell.execDetached([root.mailBin])
    }
  }

  Timer {
    id: syncTimer
    interval: root.syncIntervalMin * 60000
    repeat: true
    running: true
    triggeredOnStart: true
    onTriggered: root.syncNow()
  }

  Process {
    id: syncProcess
    running: false
    command: []
    stdout: StdioCollector { id: syncStdout; waitForEnd: true }
    stderr: StdioCollector { id: syncStderr; waitForEnd: true }
    onExited: function(exitCode) {
      root.syncing = false
      if (exitCode === 0) root.applyReport(syncStdout.text)
      else root.lastError = String(syncStderr.text || syncStdout.text || "mail sync failed").trim().substring(0, 140)
    }
  }
}
