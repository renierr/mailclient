package de.renier.mailclient.ui.common

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsDraggedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

// The drag handle between two panes, Flutter's PaneDivider: a 1dp line in a
// 24dp touch strip with a grip in the middle, so a finger can catch it.
// Reports the drag in dp; the caller clamps and stores the width.
@Composable
fun PaneDivider(onDelta: (Dp) -> Unit, modifier: Modifier = Modifier) {
    val density = LocalDensity.current
    val interaction = remember { MutableInteractionSource() }
    val active by interaction.collectIsDraggedAsState()
    val scheme = MaterialTheme.colorScheme
    Box(
        modifier = modifier
            .width(24.dp)
            .fillMaxHeight()
            .semantics { contentDescription = "Resize panes" }
            .draggable(
                orientation = Orientation.Horizontal,
                interactionSource = interaction,
                state = rememberDraggableState { px -> onDelta(with(density) { px.toDp() }) },
            ),
        contentAlignment = Alignment.Center,
    ) {
        Box(
            modifier = Modifier
                .width(if (active) 3.dp else 1.dp)
                .fillMaxHeight()
                .background(if (active) scheme.primary else scheme.outlineVariant),
        )
        Box(
            modifier = Modifier
                .size(width = 8.dp, height = 40.dp)
                .background(
                    if (active) scheme.primaryContainer else scheme.surfaceContainerHighest,
                    RoundedCornerShape(4.dp),
                )
                .border(1.dp, scheme.outlineVariant, RoundedCornerShape(4.dp)),
        )
    }
}
