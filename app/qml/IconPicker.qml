// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

Dialog {
    id: d
    property var app
    property var cb: null
    property string query: ""
    title: app.tt("Elegir ícono")
    panelWidth: 560
    z: 110

    function show(callback) {
        cb = callback
        query = ""
        search.text = ""
        open = true
        search.focusInput.forceActiveFocus()
    }

    Input {
        id: search
        width: parent.width
        placeholder: d.app.tt("Buscar por nombre")
        onEdited: (v) => d.query = v.toLowerCase()
    }
    GridView {
        id: grid
        width: parent.width
        height: 340
        clip: true
        cellWidth: Math.floor(width / 6)
        cellHeight: 76
        boundsBehavior: Flickable.StopAtBounds
        model: d.app.meta.icons.filter((n) => n.indexOf(d.query) >= 0)
        delegate: Rectangle {
            width: grid.cellWidth - 6
            height: 70
            radius: 10
            color: cell.containsMouse ? Theme.s2 : "transparent"
            border.width: 1
            border.color: cell.containsMouse ? Theme.line2 : "transparent"
            Column {
                anchors.centerIn: parent
                spacing: 6
                Icon { anchors.horizontalCenter: parent.horizontalCenter; name: modelData; size: 26; tint: Theme.tx }
                Text {
                    width: grid.cellWidth - 14
                    horizontalAlignment: Text.AlignHCenter
                    elide: Text.ElideRight
                    text: modelData
                    color: Theme.mu
                    font.pixelSize: 11
                }
            }
            MouseArea {
                id: cell
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: { d.open = false; if (d.cb) d.cb(modelData) }
            }
        }
    }
}
