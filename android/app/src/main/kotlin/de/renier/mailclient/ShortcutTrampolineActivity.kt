package de.renier.mailclient

import android.app.Activity
import android.content.Intent
import android.os.Bundle

// Target of the static launcher shortcut (xml/shortcuts.xml). The system
// launches static shortcuts with FLAG_ACTIVITY_CLEAR_TASK, which would
// rebuild a running shell and drop an open composer. This activity lives in
// its own task, so only that task is cleared; it hands the intent on to
// MainActivity single top, the way the dynamic shortcuts do, and finishes.
class ShortcutTrampolineActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        startActivity(
            Intent(this, MainActivity::class.java)
                .setAction(intent.action)
                .putExtras(intent)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
        )
        finish()
    }
}
