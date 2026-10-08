package de.renier.mailclient.ui.common

import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity

// Below this window height a dialog or two bars plus the keyboard do not fit
// (a phone in landscape): Flutter's MailDialog.prefersPage.
private const val SHORT_SCREEN_DP = 480

/**
 * Whether the app's window is short (not the whole display, so split screen
 * counts). Measured in the interface scale's dp: a larger interface scale
 * leaves less room, like a smaller window.
 */
@Composable
fun isShortScreen(): Boolean {
    val config = LocalConfiguration.current
    val windowPx = config.screenHeightDp * config.densityDpi / 160f
    return windowPx / LocalDensity.current.density < SHORT_SCREEN_DP
}
