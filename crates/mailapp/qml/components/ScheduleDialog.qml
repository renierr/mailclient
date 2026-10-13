import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// Pick the date and time a mail goes out: a month grid and hour/minute
// fields, opening on the current date and time. The result is a UTC instant
// (`scheduleAccepted` carries `date.toISOString()`), stored in the outbox as
// `send_at` and delivered by the next sync past it. Device-local by design:
// the dialog says the composing device must stay on and online.
AppDialog {
    id: root
    title: qsTr("Send later")
    preferredWidth: 400
    preferredHeight: 600
    minWidth: 340
    minHeight: 460
    padding: Theme.lg

    signal scheduleAccepted(string sendAtUtc, bool notifySelf)

    // Mail a delivery note to the own address once this send goes out.
    // Opens on the setting's default (the core's `SCHEDULED_RECEIPT`);
    // the user's decision here overrides it for this one send.
    property bool notifySelf: false

    // Only the composer offers the choice: rescheduling a held row moves
    // its time and nothing else, so the row's own receipt flag stands and
    // the toggle would decide something it cannot carry out.
    property bool showReceipt: true

    // Selection (local wall-clock parts).
    property int viewYear: 0
    property int viewMonth: 0
    property int selDay: 0
    property bool hasSelection: false

    // At least a minute out, like the core's `SCHEDULE_LEAD_SECS`.
    readonly property int leadSecs: 60

    function midnight(d) {
        return new Date(d.getFullYear(), d.getMonth(), d.getDate());
    }

    function daysInMonth(y, m) {
        return new Date(y, m + 1, 0).getDate();
    }

    // Monday-first offset of the 1st (getDay: 0 = Sunday).
    function monthOffset(y, m) {
        return (new Date(y, m, 1).getDay() + 6) % 7;
    }

    function openFor(sendAtUtc) {
        var prechecked = arguments.length > 1 && arguments[1] === true;
        // Opens on the held instant when rescheduling, else on right now
        // (still invalid until moved into the future — the button enforces
        // the lead time).
        var d = sendAtUtc ? new Date(sendAtUtc) : new Date();
        if (sendAtUtc && isNaN(d.getTime()))
            d = new Date();
        root.viewYear = d.getFullYear();
        root.viewMonth = d.getMonth();
        root.selDay = d.getDate();
        root.hasSelection = true;
        hourSpin.value = d.getHours();
        minuteSpin.value = d.getMinutes();
        // Rescheduling a row that already asks for a receipt keeps it.
        root.notifySelf = prechecked;
        open();
    }

    function selectedDate() {
        if (!root.hasSelection)
            return null;
        return new Date(root.viewYear, root.viewMonth, root.selDay, hourSpin.value, minuteSpin.value, 0, 0);
    }

    function selectedUtc() {
        return root.selectedDate().toISOString();
    }

    readonly property bool valid: {
        var d = root.selectedDate();
        if (!d)
            return false;
        return d.getTime() - Date.now() > root.leadSecs * 1000;
    }

    readonly property string summary: {
        var d = root.selectedDate();
        if (!d)
            return "";
        return qsTr("Sends %1 at %2").arg(Qt.formatDate(d, "dddd, d MMMM")).arg(Qt.formatTime(d, "HH:mm"));
    }

    footer: RowLayout {
        spacing: Theme.sm
        Item {
            Layout.fillWidth: true
        }
        AppButton {
            Layout.rightMargin: Theme.sm
            Layout.bottomMargin: Theme.md
            text: qsTr("Cancel")
            onClicked: root.close()
        }
        AppButton {
            Layout.rightMargin: Theme.lg + 8
            Layout.bottomMargin: Theme.md
            text: qsTr("Schedule send")
            intent: "primary"
            enabled: root.valid
            onClicked: {
                root.scheduleAccepted(root.selectedUtc(), root.notifySelf);
                root.close();
            }
        }
    }

    contentItem: ScrollView {
        id: bodyScroll
        clip: true

        ColumnLayout {
            width: bodyScroll.availableWidth
            spacing: Theme.sm

            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                text: qsTr(
                          "The mail waits in this device's outbox and goes out with the first sync past its time — the device must be on and online then.")
            }

            // --- month navigation -----------------------------------------
            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.sm
                IconButton {
                    text: Icons.arrowBack
                    iconFont: true
                    tooltip: qsTr("Previous month")
                    onClicked: {
                        if (root.viewMonth === 0) {
                            root.viewMonth = 11;
                            root.viewYear -= 1;
                        } else {
                            root.viewMonth -= 1;
                        }
                    }
                }
                Label {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    color: Theme.text
                    font.pixelSize: Theme.fontMedium
                    font.bold: true
                    text: Qt.formatDate(new Date(root.viewYear, root.viewMonth, 1), "MMMM yyyy")
                }
                IconButton {
                    text: Icons.chevronRight
                    iconFont: true
                    tooltip: qsTr("Next month")
                    onClicked: {
                        if (root.viewMonth === 11) {
                            root.viewMonth = 0;
                            root.viewYear += 1;
                        } else {
                            root.viewMonth += 1;
                        }
                    }
                }
            }

            // --- weekday header + day grid ------------------------------------
            GridLayout {
                Layout.fillWidth: true
                columns: 7
                columnSpacing: 2
                rowSpacing: 2

                Repeater {
                    model: [qsTr("Mo"), qsTr("Tu"), qsTr("We"), qsTr("Th"), qsTr("Fr"), qsTr("Sa"), qsTr("Su")]
                    Label {
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontTiny
                        font.bold: true
                        text: modelData
                    }
                }

                Repeater {
                    model: 42
                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: Math.round(30 * Theme.uiScale)
                        radius: Theme.radius
                        color: isSel ? Theme.accent : dayHover.hovered && !isPast ? Theme.hover : "transparent"
                        border.width: isToday ? 1 : 0
                        border.color: Theme.accent

                        readonly property date cellDate: new Date(root.viewYear, root.viewMonth, 1 - root.monthOffset(
                                                                      root.viewYear, root.viewMonth) + index)
                        readonly property bool inMonth: cellDate.getMonth() === root.viewMonth
                        readonly property bool isPast: cellDate < root.midnight(new Date())
                        readonly property bool isToday: cellDate.getTime() === root.midnight(new Date()).getTime()
                        readonly property bool isSel: root.hasSelection && cellDate.getFullYear() === root.viewYear
                                                      && cellDate.getMonth() === root.viewMonth && cellDate.getDate()
                                                      === root.selDay

                        Label {
                            anchors.centerIn: parent
                            color: isSel ? Theme.accentText : !inMonth || isPast ? Theme.textMuted : Theme.text
                            font.pixelSize: Theme.fontSmall
                            text: cellDate.getDate()
                        }

                        HoverHandler {
                            id: dayHover
                        }
                        MouseArea {
                            anchors.fill: parent
                            enabled: !isPast
                            cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
                            onClicked: {
                                root.selDay = cellDate.getDate();
                                if (!inMonth) {
                                    root.viewYear = cellDate.getFullYear();
                                    root.viewMonth = cellDate.getMonth();
                                }
                                root.hasSelection = true;
                            }
                        }
                    }
                }
            }

            // --- time ---------------------------------------------------------
            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.sm
                Label {
                    text: qsTr("Time")
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                }
                SpinBox {
                    id: hourSpin
                    from: 0
                    to: 23
                    editable: true
                    font.pixelSize: Theme.fontBase
                }
                Label {
                    text: ":"
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontBase
                }
                SpinBox {
                    id: minuteSpin
                    from: 0
                    to: 59
                    stepSize: 5
                    editable: true
                    font.pixelSize: Theme.fontBase
                }
                Item {
                    Layout.fillWidth: true
                }
            }

            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: root.valid ? Theme.accent : Theme.danger
                font.pixelSize: Theme.fontSmall
                text: root.valid ? root.summary : qsTr("Pick a time at least a minute in the future")
            }

            CheckBox {
                id: receiptCheck
                Layout.fillWidth: true
                visible: root.showReceipt
                text: qsTr("Mail me a delivery receipt")
                checked: root.notifySelf
                onToggled: root.notifySelf = checked
            }

            Label {
                Layout.fillWidth: true
                visible: root.showReceipt
                wrapMode: Text.Wrap
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                text: qsTr(
                          "When it goes out, this device mails itself a note naming the subject, the time and how many recipients it had.")
            }
        }
    }
}
