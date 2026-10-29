// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

RowBase {
    id: r
    property real value: 0
    property real from: 0
    property real to: 100
    property real step: 1
    property int decimals: 0
    signal moved(real v)

    function nudge(direction) {
        var factor = Math.pow(10, decimals)
        var next = Math.round((value + direction * step) * factor) / factor
        next = Math.max(from, Math.min(to, next))
        if (next !== value)
            moved(next)
    }

    Row {
        spacing: 6
        Rectangle {
            id: minus
            width: 32; height: 32; radius: 8
            color: minusArea.containsMouse ? Theme.chipHover : Theme.chip
            border.width: 1; border.color: Theme.line2
            Rectangle { anchors.centerIn: parent; width: 12; height: 2; radius: 1; color: Theme.tx }
            MouseArea {
                id: minusArea
                anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                onPressed: { r.nudge(-1); hold.direction = -1; hold.interval = 380; hold.start() }
                onReleased: hold.stop()
                onCanceled: hold.stop()
            }
            activeFocusOnTab: true
            Accessible.role: Accessible.Button
            Accessible.name: backend.tr("Reducir") + " " + r.title
            Accessible.onPressAction: r.nudge(-1)
            Keys.onReturnPressed: r.nudge(-1)
            Keys.onSpacePressed: r.nudge(-1)
            Keys.onLeftPressed: r.nudge(-1)
            Keys.onRightPressed: r.nudge(1)
            Rectangle { anchors.fill: parent; anchors.margins: -3; radius: 11; color: "transparent"; border.width: 2; border.color: Theme.ac; visible: minus.activeFocus }
        }
        Text {
            width: 64
            height: 32
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            text: r.value.toFixed(r.decimals)
            color: Theme.tx
            font.pixelSize: 14
            font.weight: Font.DemiBold
            MouseArea {
                anchors.fill: parent
                onWheel: (w) => r.nudge(w.angleDelta.y > 0 ? 1 : -1)
            }
        }
        Rectangle {
            id: plus
            width: 32; height: 32; radius: 8
            color: plusArea.containsMouse ? Theme.chipHover : Theme.chip
            border.width: 1; border.color: Theme.line2
            Rectangle { anchors.centerIn: parent; width: 12; height: 2; radius: 1; color: Theme.tx }
            Rectangle { anchors.centerIn: parent; width: 2; height: 12; radius: 1; color: Theme.tx }
            MouseArea {
                id: plusArea
                anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                onPressed: { r.nudge(1); hold.direction = 1; hold.interval = 380; hold.start() }
                onReleased: hold.stop()
                onCanceled: hold.stop()
            }
            activeFocusOnTab: true
            Accessible.role: Accessible.Button
            Accessible.name: backend.tr("Aumentar") + " " + r.title
            Accessible.onPressAction: r.nudge(1)
            Keys.onReturnPressed: r.nudge(1)
            Keys.onSpacePressed: r.nudge(1)
            Keys.onLeftPressed: r.nudge(-1)
            Keys.onRightPressed: r.nudge(1)
            Rectangle { anchors.fill: parent; anchors.margins: -3; radius: 11; color: "transparent"; border.width: 2; border.color: Theme.ac; visible: plus.activeFocus }
        }
    }
    Timer {
        id: hold
        property int direction: 1
        repeat: true
        onTriggered: { interval = 55; r.nudge(direction) }
    }
}
