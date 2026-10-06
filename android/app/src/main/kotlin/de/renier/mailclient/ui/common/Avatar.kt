package de.renier.mailclient.ui.common

import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// Sender avatar: core-decided initials and per-theme colour
// (mailcore::badge, carried on every account/row feed). White text, like the
// other frontends. The frontend only draws the circle.
@Composable
fun Avatar(
    initials: String,
    avatarLight: String,
    avatarDark: String,
    modifier: Modifier = Modifier,
    size: Dp = 40.dp,
) {
    val dark = isSystemInDarkTheme()
    val fallback = MaterialTheme.colorScheme.primaryContainer
    val bg = try {
        Color(android.graphics.Color.parseColor(if (dark) avatarDark else avatarLight))
    } catch (_: Exception) {
        fallback
    }
    Box(
        modifier = modifier
            .size(size)
            .clip(CircleShape)
            .background(bg),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            initials.take(2),
            color = Color.White,
            style = MaterialTheme.typography.titleSmall.copy(fontSize = (size.value * 0.36f).sp),
        )
    }
}
