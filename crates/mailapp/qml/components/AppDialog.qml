import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// Generic resizable, draggable dialog.
// Mirrors Compose's window mechanics (header drag, corner resize grip,
// boundary clamping) in a reusable component for all modal managers.
Dialog {
    id: root
    modal: true
    parent: Overlay.overlay
    padding: Theme.lg

    // Size constraints & defaults
    property int minWidth: 380
    property int minHeight: 280
    property int preferredWidth: 560
    property int preferredHeight: 480

    property bool resizable: true
    property bool draggable: true
    property bool rememberGeometry: true
    property string subtitle: ""
    property bool showHeaderCloseButton: false
    property int headerHeight: subtitle !== "" ? 52 : 48
    property alias headerExtra: headerExtraItem.data

    property real wantW: preferredWidth
    property real wantH: preferredHeight
    property real wantX: 0
    property real wantY: 0
    property bool positioned: false

    readonly property real hostW: (parent && parent.width > 0) ? parent.width : preferredWidth
    readonly property real hostH: (parent && parent.height > 0) ? parent.height : preferredHeight
    readonly property int gapX: hostW < 900 ? 16 : 48
    readonly property int gapY: hostH < 700 ? 16 : 32
    readonly property real maxW: Math.max(240, hostW - gapX)
    readonly property real maxH: Math.max(200, hostH - gapY)

    width: Math.min(Math.max(wantW, Math.min(minWidth, maxW)), maxW)
    height: Math.min(Math.max(wantH, Math.min(minHeight, maxH)), maxH)
    x: positioned ? Math.round(Math.max(0, Math.min(wantX, hostW - width)))
                  : Math.round((hostW - width) / 2)
    y: positioned ? Math.round(Math.max(0, Math.min(wantY, hostH - height)))
                  : Math.round((hostH - height) / 2)

    function applyDefaultGeometry() {
        wantW = preferredWidth
        wantH = preferredHeight
        positioned = false
    }

    function applyResize(edges, sX, sY, sW, sH, dx, dy) {
        if (!resizable)
            return
        var nx = sX
        var ny = sY
        var nw = sW
        var nh = sH
        var minW = Math.min(minWidth, maxW)
        var minH = Math.min(minHeight, maxH)
        if (edges & Qt.RightEdge)
            nw = Math.min(Math.max(minW, sW + dx), hostW - nx)
        if (edges & Qt.LeftEdge) {
            nw = Math.min(Math.max(minW, sW - dx), sX + sW)
            nx = sX + sW - nw
        }
        if (edges & Qt.BottomEdge)
            nh = Math.min(Math.max(minH, sH + dy), hostH - ny)
        if (edges & Qt.TopEdge) {
            nh = Math.min(Math.max(minH, sH - dy), sY + sH)
            ny = sY + sH - nh
        }
        positioned = true
        wantX = nx
        wantY = ny
        wantW = nw
        wantH = nh
    }

    onOpened: {
        if (!rememberGeometry || !positioned) {
            applyDefaultGeometry()
        }
    }

    background: Rectangle {
        color: Theme.bg
        radius: Theme.radiusLg
        border.width: 1
        border.color: Theme.border
    }

    header: Rectangle {
        id: headerRect
        implicitHeight: root.headerHeight
        color: "transparent"

        MouseArea {
            id: dragArea
            anchors.fill: parent
            enabled: root.draggable
            cursorShape: pressed ? Qt.ClosedHandCursor : (enabled ? Qt.OpenHandCursor : Qt.ArrowCursor)
            property real sMX
            property real sMY
            property real sX
            property real sY
            onPressed: function (mouse) {
                var p = mapToItem(root.parent, mouse.x, mouse.y)
                sMX = p.x
                sMY = p.y
                sX = root.x
                sY = root.y
                root.positioned = true
            }
            onPositionChanged: function (mouse) {
                if (!pressed)
                    return
                var p = mapToItem(root.parent, mouse.x, mouse.y)
                root.wantX = sX + p.x - sMX
                root.wantY = sY + p.y - sMY
            }
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: Theme.lg
            anchors.rightMargin: Theme.lg
            spacing: Theme.md
            z: 1

            ColumnLayout {
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignVCenter
                spacing: 1

                Label {
                    id: headerTitleLabel
                    Layout.fillWidth: true
                    text: root.title
                    color: Theme.text
                    font.pixelSize: Theme.fontMedium
                    font.bold: true
                    elide: Text.ElideRight
                }

                Label {
                    id: headerSubtitleLabel
                    Layout.fillWidth: true
                    text: root.subtitle
                    visible: root.subtitle !== ""
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontTiny
                    elide: Text.ElideRight
                }
            }

            Item {
                id: headerExtraItem
                Layout.alignment: Qt.AlignVCenter
                implicitWidth: childrenRect.width
                implicitHeight: childrenRect.height
                visible: children.length > 0
            }

            IconButton {
                id: headerCloseBtn
                Layout.alignment: Qt.AlignVCenter
                visible: root.showHeaderCloseButton
                text: "✕"
                tooltip: qsTr("Close")
                Accessible.name: qsTr("Close")
                onClicked: root.close()
            }
        }

        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: Theme.border
        }
    }

    // Overlay resize handles on top of content & footer
    Item {
        id: resizeContainer
        parent: (root.contentItem && root.contentItem.parent)
                ? root.contentItem.parent
                : (root.background ? root.background.parent : null)
        z: 9999
        anchors.fill: parent
        visible: root.resizable

        function startResize(mouseArea, edges, mouse) {
            var p = mouseArea.mapToItem(root.parent, mouse.x, mouse.y)
            mouseArea.sMX = p.x
            mouseArea.sMY = p.y
            mouseArea.sX = root.x
            mouseArea.sY = root.y
            mouseArea.sW = root.width
            mouseArea.sH = root.height
            if (edges & (Qt.RightEdge | Qt.BottomEdge)) {
                root.wantX = mouseArea.sX
                root.wantY = mouseArea.sY
            }
        }

        function updateResize(mouseArea, edges, mouse) {
            if (!mouseArea.pressed)
                return
            var p = mouseArea.mapToItem(root.parent, mouse.x, mouse.y)
            root.applyResize(edges, mouseArea.sX, mouseArea.sY, mouseArea.sW, mouseArea.sH,
                             p.x - mouseArea.sMX, p.y - mouseArea.sMY)
        }

        // Bottom-right corner resize grip
        Item {
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            width: 20
            height: 20

            Canvas {
                anchors.centerIn: parent
                width: 12
                height: 12
                onPaint: {
                    var ctx = getContext("2d")
                    ctx.clearRect(0, 0, width, height)
                    ctx.strokeStyle = Theme.border
                    ctx.lineWidth = 1.5
                    for (var i = 0; i < 3; i++) {
                        ctx.beginPath()
                        ctx.moveTo(2 + i * 3, height - 1)
                        ctx.lineTo(width - 1, 2 + i * 3)
                        ctx.stroke()
                    }
                }
            }

            MouseArea {
                id: brGripArea
                anchors.fill: parent
                hoverEnabled: true
                preventStealing: true
                cursorShape: Qt.SizeFDiagCursor
                property real sMX; property real sMY; property real sX; property real sY; property real sW; property real sH
                onPressed: mouse => resizeContainer.startResize(brGripArea, Qt.RightEdge | Qt.BottomEdge, mouse)
                onPositionChanged: mouse => resizeContainer.updateResize(brGripArea, Qt.RightEdge | Qt.BottomEdge, mouse)
            }
        }

        // Right edge
        MouseArea {
            id: rEdgeArea
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            anchors.topMargin: root.headerHeight
            anchors.bottomMargin: 20
            width: 6
            cursorShape: Qt.SizeHorCursor
            property real sMX; property real sMY; property real sX; property real sY; property real sW; property real sH
            onPressed: mouse => resizeContainer.startResize(rEdgeArea, Qt.RightEdge, mouse)
            onPositionChanged: mouse => resizeContainer.updateResize(rEdgeArea, Qt.RightEdge, mouse)
        }

        // Bottom edge
        MouseArea {
            id: bEdgeArea
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.leftMargin: 20
            anchors.rightMargin: 20
            height: 6
            cursorShape: Qt.SizeVerCursor
            property real sMX; property real sMY; property real sX; property real sY; property real sW; property real sH
            onPressed: mouse => resizeContainer.startResize(bEdgeArea, Qt.BottomEdge, mouse)
            onPositionChanged: mouse => resizeContainer.updateResize(bEdgeArea, Qt.BottomEdge, mouse)
        }

        // Left edge
        MouseArea {
            id: lEdgeArea
            anchors.left: parent.left
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            anchors.topMargin: root.headerHeight
            anchors.bottomMargin: 20
            width: 6
            cursorShape: Qt.SizeHorCursor
            property real sMX; property real sMY; property real sX; property real sY; property real sW; property real sH
            onPressed: mouse => resizeContainer.startResize(lEdgeArea, Qt.LeftEdge, mouse)
            onPositionChanged: mouse => resizeContainer.updateResize(lEdgeArea, Qt.LeftEdge, mouse)
        }

        // Bottom-left corner
        MouseArea {
            id: blCornerArea
            anchors.left: parent.left
            anchors.bottom: parent.bottom
            width: 14
            height: 14
            cursorShape: Qt.SizeBDiagCursor
            property real sMX; property real sMY; property real sX; property real sY; property real sW; property real sH
            onPressed: mouse => resizeContainer.startResize(blCornerArea, Qt.LeftEdge | Qt.BottomEdge, mouse)
            onPositionChanged: mouse => resizeContainer.updateResize(blCornerArea, Qt.LeftEdge | Qt.BottomEdge, mouse)
        }
    }
}
