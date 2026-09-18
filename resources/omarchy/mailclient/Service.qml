import QtQuick
import Quickshell
import Quickshell.Io
import "Model.js" as Model

// Headless state for the mailclient.unread bar widget.
//
// Two poll loops share one binary:
// - statusTimer runs `mailapp --status --json` (SQLite only, no network)
//   every refreshIntervalSec for a cheap badge.
// - syncTimer runs `mailapp --sync-once --json` (real IMAP sync) every
//   syncIntervalMin; 0 disables network sync (badge follows the open app).
// A rise in the unread count after the first baseline poll sends one
// desktop notification; clicking it opens the mail app via --exec.
Item {
  id: root

  property var settings: ({})
  property var bar: null

  property int unread: 0
  property var recent: []
  property bool syncing: false
  property string lastError: ""
  // Highest unread count already notified for. -1 = no baseline yet
  // (first poll only sets it). Named to avoid QQuickItem's FINAL
  // `baseline` anchor-line property.
  property int knownUnread: -1

  readonly property string mailBin: String(setting("mailBin", "mailapp") || "mailapp")
  readonly property string account: String(setting("account", "") || "")
  readonly property int refreshIntervalSec: intSetting("refreshIntervalSec", 30, 10, 3600)
  readonly property int syncIntervalMin: intSetting("syncIntervalMin", 15, 0, 1440)
  readonly property bool notify: boolSetting("notify", true)
  readonly property bool busy: statusProcess.running || syncProcess.running

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
    // A running sync already refreshes the cache on completion; skip the
    // cheap poll while it flies so ticks never pile up.
    if (statusProcess.running || syncProcess.running) return
    statusProcess.command = [root.mailBin, "--status", "--json"].concat(accountArgs())
    statusProcess.running = true
  }

  function syncNow() {
    if (syncProcess.running) return
    root.syncing = true
    syncProcess.command = [root.mailBin, "--sync-once", "--json"].concat(accountArgs())
    syncProcess.running = true
  }

  function applyReport(raw, fromSync) {
    var parsed = Model.parseReport(raw)
    if (!parsed.ok) {
      lastError = "Could not read mail status"
      return
    }
    lastError = parsed.errors.length > 0 ? String(parsed.errors[0]) : ""
    unread = parsed.unread
    recent = parsed.recent
    if (knownUnread < 0) {
      knownUnread = unread
    } else if (fromSync && root.notify && unread > knownUnread) {
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
    id: statusTimer
    interval: root.refreshIntervalSec * 1000
    repeat: true
    running: true
    triggeredOnStart: true
    onTriggered: root.refresh()
  }

  Timer {
    id: syncTimer
    interval: Math.max(1, root.syncIntervalMin) * 60000
    repeat: true
    running: root.syncIntervalMin > 0
    triggeredOnStart: true
    onTriggered: root.syncNow()
  }

  Process {
    id: statusProcess
    running: false
    command: []
    stdout: StdioCollector { id: statusStdout; waitForEnd: true }
    stderr: StdioCollector { id: statusStderr; waitForEnd: true }
    onExited: function(exitCode) {
      if (exitCode === 0) root.applyReport(statusStdout.text, false)
      else root.lastError = String(statusStderr.text || statusStdout.text || "mail status failed").trim().substring(0, 140)
    }
  }

  Process {
    id: syncProcess
    running: false
    command: []
    stdout: StdioCollector { id: syncStdout; waitForEnd: true }
    stderr: StdioCollector { id: syncStderr; waitForEnd: true }
    onExited: function(exitCode) {
      root.syncing = false
      if (exitCode === 0) root.applyReport(syncStdout.text, true)
      else root.lastError = String(syncStderr.text || syncStdout.text || "mail sync failed").trim().substring(0, 140)
      // The sync rewrote the cache; re-read the cheap state right away so
      // the badge never lags one poll behind.
      Qt.callLater(root.refresh)
    }
  }
}
