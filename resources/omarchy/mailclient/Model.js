// Pure helpers for the mailclient.unread bar widget: CLI JSON parsing,
// badge text, and tooltip text. No Qt imports so this stays testable.

var MAIL_GLYPH = "󰇮"

function parseReport(raw) {
  var text = String(raw || "").trim()
  var empty = { ok: false, unread: 0, recent: [], errors: [] }
  if (text === "") return empty
  try {
    var parsed = JSON.parse(text)
    if (!parsed || typeof parsed !== "object" || parsed.ok !== true) return empty
    return {
      ok: true,
      unread: Math.max(0, parseInt(parsed.unread, 10) || 0),
      recent: Array.isArray(parsed.recent) ? parsed.recent : [],
      errors: Array.isArray(parsed.errors) ? parsed.errors : []
    }
  } catch (e) {
    return empty
  }
}

function badge(unread) {
  var n = Math.max(0, parseInt(unread, 10) || 0)
  return n > 0 ? MAIL_GLYPH + " " + n : MAIL_GLYPH
}

function elide(text, max) {
  var value = String(text || "").replace(/\s+/g, " ").trim()
  if (value === "") return ""
  return value.length > max ? value.substring(0, max - 1) + "…" : value
}

function tooltip(unread, recent, lastError, syncing) {
  var n = Math.max(0, parseInt(unread, 10) || 0)
  var lines = []
  if (syncing) lines.push("Syncing…")
  lines.push(n === 0 ? "No unread mail" : (n === 1 ? "1 unread message" : n + " unread messages"))
  var items = Array.isArray(recent) ? recent.slice(0, 5) : []
  for (var i = 0; i < items.length; i++) {
    var from = elide(items[i].from, 32)
    var subject = elide(items[i].subject, 48)
    if (from !== "" || subject !== "") lines.push((from !== "" ? from : "(unknown)") + " — " + (subject !== "" ? subject : "(no subject)"))
  }
  if (lastError) lines.push("Error: " + elide(lastError, 100))
  lines.push("Click: open mail · Right-click: sync now")
  return lines.join("\n")
}

if (typeof module !== "undefined") {
  module.exports = {
    parseReport: parseReport,
    badge: badge,
    tooltip: tooltip,
    elide: elide
  }
}
