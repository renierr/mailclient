package de.renier.mailclient.ui.lock

import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import de.renier.mailclient.AppLock
import de.renier.mailclient.R

/**
 * What covers the shell while [AppLock] is locked (or has not read its
 * setting yet: then only the background shows, no prompt). An opaque
 * Surface, so no touch reaches the shell beneath; back sends the app to
 * the background instead of walking the hidden panes.
 */
@Composable
fun LockCover() {
    val locked = AppLock.locked
    if (locked == false) return
    val activity = LocalActivity.current
    val focus = LocalFocusManager.current
    // A field in the shell keeps neither focus nor the keyboard under the cover.
    LaunchedEffect(Unit) { focus.clearFocus(force = true) }
    BackHandler { activity?.moveTaskToBack(true) }
    Surface(modifier = Modifier.fillMaxSize()) {
        if (locked == null) return@Surface
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
            modifier = Modifier
                .fillMaxSize()
                .safeDrawingPadding()
                .verticalScroll(rememberScrollState())
                .padding(24.dp),
        ) {
            Icon(
                painterResource(R.drawable.ic_lock),
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
                modifier = Modifier.size(48.dp),
            )
            Text(
                "Mailclient is locked",
                style = MaterialTheme.typography.titleLarge,
                textAlign = TextAlign.Center,
            )
            AppLock.error?.let {
                Text(
                    it,
                    color = MaterialTheme.colorScheme.error,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.widthIn(max = 360.dp),
                )
            }
            Button(onClick = { activity?.let { AppLock.unlock(it) } }, enabled = activity != null) {
                Text("Unlock")
            }
        }
    }
}
