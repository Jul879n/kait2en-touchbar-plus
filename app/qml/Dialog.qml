// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

FocusScope {
    id: d
    property bool open: false
    property string title: ""
    property int panelWidth: 480
    default property alias body: content.data
    anchors.fill: parent
    visible: opacity > 0
    opacity: open ? 1 : 0
    z: 100
    focus: open
    Accessible.role: Accessible.Dialog
    Accessible.name: title
    onOpenChanged: if (open) forceActiveFocus()
    Behavior on opacity { NumberAnimation { duration: 140 } }
    Keys.onEscapePressed: d.open = false

    Rectangle {
        anchors.fill: parent
        color: "#b0000000"
        MouseArea { anchors.fill: parent; onClicked: d.open = false }
    }
    Rectangle {
        id: panel
        anchors.centerIn: parent
        width: d.panelWidth
        height: Math.min(d.height - 60, col.implicitHeight + 44)
        radius: 16
        color: Theme.s1
        border.width: 1
        border.color: Theme.line
        scale: d.open ? 1 : 0.96
        Behavior on scale { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
        MouseArea { anchors.fill: parent }
        Column {
            id: col
            x: 22
            y: 22
            width: parent.width - 44
            spacing: 14
            Text {
                text: d.title
                color: Theme.tx
                font.pixelSize: 18
                font.weight: Font.DemiBold
            }
            Column {
                id: content
                width: parent.width
                spacing: 12
            }
        }
    }
}
