package de.renier.mailclient.ui.delegate

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.json.JSONObject

// Placeholder for a shell flow the native reader delegated back
// (composer, find-similar): shows what was asked until that screen exists,
// then this routes there instead.
@Composable
fun DelegateScreen(payload: String, onClose: () -> Unit) {
    val req = remember(payload) {
        try {
            JSONObject(payload)
        } catch (_: Exception) {
            JSONObject()
        }
    }
    Column(
        modifier = Modifier
            .fillMaxSize()
            .safeDrawingPadding()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text("Not built yet", style = MaterialTheme.typography.headlineSmall)
        Text(
            "The reader asked for “${req.optString("kind")}” " +
                "(account ${req.optLong("accountId")}, " +
                "folder ${req.optLong("folderId")}, " +
                "uid ${req.optInt("uid")}), which has no native screen yet.",
            style = MaterialTheme.typography.bodyMedium,
        )
        Spacer(modifier = Modifier.height(8.dp))
        Button(onClick = onClose) { Text("Back to Home") }
    }
}
