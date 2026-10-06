package de.renier.mailclient.ui.shell

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import de.renier.mailclient.ui.common.PaneDivider

/** How many mail panes fit side by side. */
enum class PaneLayout { One, Two, Three }

// Flutter's Breakpoints (and Qt's), in dp: below 700 one pane at a time,
// below 1100 folders + list with the reader taking the list's place, else
// all three. Divided by the font scale like Flutter's uiScale, so large
// text on a small tablet falls back to fewer, wider panes instead of
// clipping rows. Keyed on width, never on "is this a tablet".
private const val COMPACT_DP = 700f
private const val MEDIUM_DP = 1100f

@Composable
fun paneLayout(uiScale: Float = 1f): PaneLayout {
    val config = LocalConfiguration.current
    // Scale-aware like Flutter and Qt: a larger interface scale or text
    // size behaves like a narrower window.
    val effective = config.screenWidthDp / config.fontScale.coerceAtLeast(1f) / uiScale.coerceAtLeast(1f)
    return when {
        effective >= MEDIUM_DP -> PaneLayout.Three
        effective >= COMPACT_DP -> PaneLayout.Two
        else -> PaneLayout.One
    }
}

/**
 * Dragged pane widths. Plain state like Flutter's and the Qt SplitView:
 * not remembered across launches.
 */
class PaneWidths {
    var sidebar by mutableStateOf(260.dp)
    var list by mutableStateOf(380.dp)
    var twoPaneSidebar by mutableStateOf(240.dp)
    var sidebarVisible by mutableStateOf(true)
}

@Composable
fun rememberPaneWidths(): PaneWidths = remember { PaneWidths() }

private fun Dp.clampTo(min: Dp, max: Dp): Dp = coerceIn(min, maxOf(min, max))

/**
 * The tablet and wide-window layouts. [reader] is null while no message is
 * open: three panes show a placeholder, two panes show the list. In
 * [fullscreen] the reader takes the whole width; its pane stays where it
 * is in the composition, so the open message keeps its state and scroll.
 */
@Composable
fun MailPanes(
    layout: PaneLayout,
    widths: PaneWidths,
    folders: @Composable () -> Unit,
    list: @Composable () -> Unit,
    reader: (@Composable () -> Unit)?,
    fullscreen: Boolean = false,
) {
    BoxWithConstraints(modifier = Modifier.fillMaxSize()) {
        val total = maxWidth
        if (layout == PaneLayout.Two) {
            // Keep ~300dp for the main pane, so shrinking the window can
            // never push the list off-screen.
            val cap = (total - 300.dp).clampTo(200.dp, 480.dp)
            val side = widths.twoPaneSidebar.clampTo(160.dp, cap)
            Row(modifier = Modifier.fillMaxSize()) {
                if (!fullscreen) {
                    Box(modifier = Modifier.width(side).fillMaxHeight()) { folders() }
                    PaneDivider(onDelta = { widths.twoPaneSidebar = (side + it).clampTo(160.dp, cap) })
                }
                Box(modifier = Modifier.weight(1f).fillMaxHeight()) {
                    if (reader != null) reader() else list()
                }
            }
            return@BoxWithConstraints
        }
        // Three panes: the reader keeps at least ~320dp whatever the drags.
        val sideShown = widths.sidebarVisible
        val side = if (sideShown) widths.sidebar.clampTo(160.dp, 480.dp) else 0.dp
        val listCap = (total - side - 320.dp - 48.dp).clampTo(240.dp, 700.dp)
        val listWidth = widths.list.clampTo(240.dp, listCap)
        Row(modifier = Modifier.fillMaxSize()) {
            if (!fullscreen) {
                if (sideShown) {
                    Box(modifier = Modifier.width(side).fillMaxHeight()) { folders() }
                    PaneDivider(onDelta = { widths.sidebar = (side + it).clampTo(160.dp, 480.dp) })
                }
                Box(modifier = Modifier.width(listWidth).fillMaxHeight()) { list() }
                PaneDivider(onDelta = { widths.list = (listWidth + it).clampTo(240.dp, listCap) })
            }
            Box(modifier = Modifier.weight(1f).fillMaxHeight()) {
                if (reader != null) {
                    reader()
                } else {
                    Text(
                        "Select a message to read it here",
                        color = MaterialTheme.colorScheme.outline,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.align(Alignment.Center).padding(24.dp),
                    )
                }
            }
        }
    }
}
