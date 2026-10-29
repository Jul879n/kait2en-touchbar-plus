// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

// One item of the bar, drawn like the key it makes on the Touch Bar.
Rectangle {
    id: c
    property var item: ({})
    property var app: null
    property bool selected: false
    property bool strip: false
    property bool fullColor: false
    property string label: ""
    property bool ghost: false
    signal pressedItem
    signal dragStarted(real x, real y)
    signal dragMoved(real x, real y)
    signal dragEnded(real x, real y)
    signal resizeMoved(real x)
    property alias grabX: area.px

    readonly property string iconName: Theme.iconOf(item)
    readonly property bool isSpacer: item.type === "spacer"
    implicitHeight: 40
    radius: 9
    color: isSpacer ? "transparent" : (strip && !selected ? "transparent" : (area.containsMouse ? Theme.chipHover : (selected ? "#16203a" : Theme.chip)))
    border.width: selected ? 2 : 1
    border.color: selected ? Theme.ac : (isSpacer ? Theme.line2 : (strip ? "transparent" : Theme.line2))
    opacity: ghost ? 0.35 : 1
    activeFocusOnTab: true
    Accessible.role: Accessible.Button
    Accessible.name: label
    Accessible.onPressAction: c.pressedItem()
    Keys.onReturnPressed: c.pressedItem()
    Keys.onSpacePressed: c.pressedItem()
    Behavior on color { ColorAnimation { duration: 110 } }

    Icon {
        visible: !c.isSpacer && item.type !== "now_playing" && c.iconName !== ""
        anchors.centerIn: parent
        name: c.iconName
        size: 22
        tint: Theme.tint(c.iconName, c.fullColor)
    }
    Text {
        visible: !c.isSpacer && item.type !== "now_playing" && c.iconName === ""
        anchors.centerIn: parent
        width: parent.width - 8
        horizontalAlignment: Text.AlignHCenter
        elide: Text.ElideRight
        text: item.label || c.label.substring(0, 6)
        color: Theme.glow
        font.pixelSize: 12
    }
    Row {
        visible: c.isSpacer
        anchors.centerIn: parent
        spacing: 4
        Icon { name: "move-horizontal"; size: 16; tint: Theme.mu; anchors.verticalCenter: parent.verticalCenter }
        Text {
            text: Theme.colsOf(item)
            color: Theme.mu
            font.pixelSize: 12
            anchors.verticalCenter: parent.verticalCenter
        }
    }
    Row {
        visible: item.type === "now_playing"
        anchors.centerIn: parent
        spacing: 8
        Rectangle {
            width: 30; height: 30; radius: 5
            gradient: Gradient {
                orientation: Gradient.Horizontal
                GradientStop { position: 0; color: "#7aa2ff" }
                GradientStop { position: 1; color: "#b88cff" }
            }
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: app ? app.tt("Artista - Título") : "Artista - Título"
            color: Theme.glow
            font.pixelSize: 12
        }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: dragging ? Qt.ClosedHandCursor : Qt.PointingHandCursor
        property real px: 0
        property real py: 0
        property bool dragging: false
        onPressed: (m) => { px = m.x; py = m.y; dragging = false; c.pressedItem() }
        onPositionChanged: (m) => {
            if (!pressed)
                return
            if (!dragging && Math.hypot(m.x - px, m.y - py) > 6) {
                dragging = true
                var s = mapToItem(null, m.x, m.y)
                c.dragStarted(s.x, s.y)
            }
            if (dragging) {
                var p = mapToItem(null, m.x, m.y)
                c.dragMoved(p.x, p.y)
            }
        }
        onReleased: (m) => {
            if (dragging) {
                dragging = false
                var p = mapToItem(null, m.x, m.y)
                c.dragEnded(p.x, p.y)
            }
        }
        onCanceled: { if (dragging) { dragging = false; Dnd.cancel() } }
    }
    Rectangle {
        id: handle
        visible: !c.ghost && c.enabled && (area.containsMouse || grip.pressed || c.selected)
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        width: 6
        height: 22
        radius: 3
        anchors.rightMargin: 3
        color: grip.containsMouse || grip.pressed ? Theme.ac : Theme.line2
        MouseArea {
            id: grip
            anchors.fill: parent
            anchors.margins: -5
            hoverEnabled: true
            cursorShape: Qt.SizeHorCursor
            onPressed: c.pressedItem()
            onPositionChanged: (m) => { if (pressed) c.resizeMoved(mapToItem(null, m.x, m.y).x) }
        }
    }
    Rectangle {
        anchors.fill: parent
        anchors.margins: -3
        radius: 12
        color: "transparent"
        border.width: 2
        border.color: Theme.ac
        visible: c.activeFocus
    }
}
