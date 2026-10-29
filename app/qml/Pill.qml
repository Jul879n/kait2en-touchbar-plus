// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

// A palette entry: click adds it, drag drops it on the bar.
Rectangle {
    id: p
    property string icon: ""
    property string text: ""
    property color tint: Theme.glow
    signal activated
    signal dragStarted(real x, real y)
    signal dragMoved(real x, real y)
    signal dragEnded(real x, real y)

    implicitHeight: 38
    implicitWidth: row.implicitWidth + 28
    radius: height / 2
    color: area.containsMouse ? "#2a2f3a" : Theme.s2
    border.width: 1
    border.color: Theme.mu
    scale: area.pressed ? 0.97 : 1
    activeFocusOnTab: true
    Accessible.role: Accessible.Button
    Accessible.name: text
    Accessible.onPressAction: p.activated()
    Behavior on color { ColorAnimation { duration: 110 } }
    Behavior on scale { NumberAnimation { duration: 90 } }

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 8
        Icon {
            visible: p.icon !== ""
            anchors.verticalCenter: parent.verticalCenter
            name: p.icon
            size: 18
            tint: p.tint
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: p.text
            color: Theme.tx
            font.pixelSize: 14
        }
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        property real px: 0
        property real py: 0
        property bool dragging: false
        onPressed: (m) => { px = m.x; py = m.y; dragging = false }
        onPositionChanged: (m) => {
            if (!pressed)
                return
            if (!dragging && Math.hypot(m.x - px, m.y - py) > 6) {
                dragging = true
                var s = mapToItem(null, m.x, m.y)
                p.dragStarted(s.x, s.y)
            }
            if (dragging) {
                var q = mapToItem(null, m.x, m.y)
                p.dragMoved(q.x, q.y)
            }
        }
        onReleased: (m) => {
            if (dragging) {
                dragging = false
                var q = mapToItem(null, m.x, m.y)
                p.dragEnded(q.x, q.y)
            } else if (containsMouse) {
                p.activated()
            }
        }
        onCanceled: { if (dragging) { dragging = false; Dnd.cancel() } }
    }
    Keys.onReturnPressed: p.activated()
    Keys.onSpacePressed: p.activated()
    Rectangle {
        anchors.fill: parent
        anchors.margins: -3
        radius: p.radius + 3
        color: "transparent"
        border.width: 2
        border.color: Theme.ac
        visible: p.activeFocus
    }
}
