// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

Rectangle {
    id: b
    property string text: ""
    property string icon: ""
    property string kind: "ghost"   // ghost | primary | tab
    property bool checked: false
    property bool danger: false
    signal clicked

    readonly property bool solid: kind === "primary" || checked
    implicitHeight: kind === "tab" ? 34 : 36
    implicitWidth: row.implicitWidth + (kind === "tab" ? 28 : 26)
    radius: kind === "tab" ? height / 2 : 10
    color: solid ? Theme.ac : (area.containsMouse ? (kind === "tab" ? Theme.s2 : Theme.chipHover) : (kind === "tab" ? Theme.s1 : "transparent"))
    border.width: 1
    border.color: solid ? Theme.ac : (danger ? Theme.danger : (kind === "ghost" ? Theme.mu : Theme.line2))
    scale: area.pressed ? 0.97 : 1
    activeFocusOnTab: true
    Accessible.role: Accessible.Button
    Accessible.name: text
    Accessible.onPressAction: b.clicked()
    Behavior on color { ColorAnimation { duration: 120 } }
    Behavior on scale { NumberAnimation { duration: 90; easing.type: Easing.OutQuad } }

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 8
        Icon {
            visible: b.icon !== ""
            anchors.verticalCenter: parent.verticalCenter
            name: b.icon
            size: 16
            tint: b.solid ? Theme.acb : (b.danger ? Theme.danger : Theme.tx)
        }
        Text {
            visible: b.text !== ""
            anchors.verticalCenter: parent.verticalCenter
            text: b.text
            color: b.solid ? Theme.acb : (b.danger ? Theme.danger : Theme.tx)
            font.pixelSize: 14
            font.weight: b.solid ? Font.DemiBold : Font.Medium
        }
    }
    Rectangle {
        anchors.fill: parent
        anchors.margins: -3
        radius: b.radius + 3
        color: "transparent"
        border.width: 2
        border.color: Theme.ac
        visible: b.activeFocus
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: b.clicked()
    }
    Keys.onReturnPressed: b.clicked()
    Keys.onSpacePressed: b.clicked()
}
