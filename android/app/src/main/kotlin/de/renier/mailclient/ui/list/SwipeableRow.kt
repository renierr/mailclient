package de.renier.mailclient.ui.list

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R
import kotlin.math.abs
import kotlin.math.roundToInt

// A list row that can be dragged left or right: the action that side of the
// gesture was configured for runs when the drag ends past the threshold, and
// the row always snaps back. No drag, no action; a side set to "none" stays
// dead. The gesture lives here rather than on the row content, so the content
// never needs to move for the pointer and the cue renders behind it.

private const val SWIPE_THRESHOLD_DP = 72
private const val SWIPE_LIMIT_DP = 140

@Composable
fun SwipeableRow(
    leftAction: String,
    rightAction: String,
    enabled: Boolean,
    onAction: (String) -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val density = LocalDensity.current
    val threshold = with(density) { SWIPE_THRESHOLD_DP.dp.toPx() }
    val limit = with(density) { SWIPE_LIMIT_DP.dp.toPx() }
    val active = enabled && (leftAction != "none" || rightAction != "none")
    var dragging by remember { mutableStateOf(false) }
    var dragOffset by remember { mutableStateOf(0f) }
    var cueAction by remember { mutableStateOf("") }
    val shown by animateFloatAsState(
        targetValue = dragOffset,
        animationSpec = if (dragging) snap() else tween(160),
        label = "swipe offset",
    )

    Box(
        modifier = modifier
            .fillMaxWidth()
            .clipToBounds()
            .then(
                if (active) {
                    Modifier.pointerInput(Unit) {
                        detectHorizontalDragGestures(
                            onDragStart = { cueAction = "" },
                            onDragEnd = {
                                val value = dragOffset
                                val action = if (value < 0f) leftAction else rightAction
                                val passed = abs(value) >= threshold && action != "none"
                                dragging = false
                                dragOffset = 0f
                                cueAction = ""
                                if (passed) onAction(action)
                            },
                            onDragCancel = {
                                dragging = false
                                dragOffset = 0f
                                cueAction = ""
                            },
                            onHorizontalDrag = { change, dragDelta ->
                                change.consume()
                                val next = (dragOffset + dragDelta).coerceIn(-limit, limit)
                                dragOffset = next
                                dragging = true
                                cueAction = when {
                                    next < 0f -> leftAction
                                    next > 0f -> rightAction
                                    else -> ""
                                }
                            },
                        )
                    }
                } else {
                    Modifier
                },
            ),
    ) {
        if (cueAction.isNotEmpty() && cueAction != "none") {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(
                        MaterialTheme.colorScheme.surfaceContainerHighest,
                        RoundedCornerShape(8.dp),
                    )
                    .padding(horizontal = 16.dp, vertical = 6.dp),
                contentAlignment = if (dragOffset > 0f) Alignment.CenterStart else Alignment.CenterEnd,
            ) {
                Icon(
                    painter = painterResource(cueIcon(cueAction)),
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.size(20.dp),
                )
                Text(
                    cueText(cueAction),
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .offset { IntOffset(shown.roundToInt(), 0) },
        ) {
            content()
        }
    }
}

private fun cueIcon(action: String): Int = when (action) {
    "archive" -> R.drawable.ic_archive
    "delete" -> R.drawable.ic_delete
    "mark_read" -> R.drawable.ic_mark_email_read
    "mark_unread" -> R.drawable.ic_mail
    "star" -> R.drawable.ic_star
    "unstar" -> R.drawable.ic_star_border
    else -> R.drawable.ic_archive
}

private fun cueText(action: String): String = when (action) {
    "archive" -> "Archive"
    "delete" -> "Delete"
    "mark_read" -> "Mark as read"
    "mark_unread" -> "Mark as unread"
    "star" -> "Star"
    "unstar" -> "Remove star"
    else -> "None"
}
