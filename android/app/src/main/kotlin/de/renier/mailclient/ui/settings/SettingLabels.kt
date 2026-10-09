package de.renier.mailclient.ui.settings

/**
 * How a preference's value reads in the form. The values themselves come
 * from the core (`settingChoicesJson`); the wording is the Qt and Flutter
 * forms' (SHARED-CORE.md: frontends label choices in the same words).
 * Values arrive in their raw string form (`"1"`/`"0"`, minutes, `"1.25"`).
 */
object SettingLabels {
    fun of(key: String, value: String): String = when (key) {
        "ui_scale" -> "${((value.toDoubleOrNull() ?: 1.0) * 100).toInt()}%"
        "reader_font_size" -> when (value) {
            "small" -> "Small"
            "large" -> "Large"
            else -> "Normal"
        }
        "message_sort_field" -> when (value) {
            "from" -> "Sender"
            "subject" -> "Subject"
            else -> "Date"
        }
        "list_density" -> if (value == "compact") "Compact" else "Comfortable"
        "start_view" -> if (value == "inbox") "Inbox of the last used account" else "Folder list"
        "mark_read_delay_secs" -> if (value == "0") "Immediately" else "After $value seconds"
        "link_click_action" ->
            if (value == "browser") "Open directly in browser" else "Show safety dialog first (recommended)"
        "compose_send_format" -> when (value) {
            "plain" -> "Plain text (safest)"
            "multipart" -> "Multipart plain + HTML"
            "html" -> "HTML only"
            else -> "Automatic (recommended)"
        }
        "sync_interval_minutes" -> when (value) {
            "0" -> "Manually"
            "60" -> "Every hour"
            else -> "Every $value minutes"
        }
        "notification_action" -> if (value == "trash") "Delete" else "Archive"
        "background_scheduler" -> when (value) {
            "alarm" -> "On-time alarm"
            "push" -> "Push (IMAP IDLE)"
            else -> "Battery-saving (recommended)"
        }
        else -> value
    }
}
