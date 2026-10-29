// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

Dialog {
    id: d
    property var app
    property var cb: null
    property var all: []
    property string query: ""
    title: app.tt("Elegir app")
    panelWidth: 520
    z: 110

    function show(callback) {
        cb = callback
        query = ""
        search.text = ""
        if (all.length === 0)
            all = JSON.parse(backend.apps())
        open = true
        search.focusInput.forceActiveFocus()
    }

    Input {
        id: search
        width: parent.width
        placeholder: d.app.tt("Buscar por nombre")
        onEdited: (v) => d.query = v.toLowerCase()
    }
    ListView {
        width: parent.width
        height: 340
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        model: d.all.filter((a) => a.name.toLowerCase().indexOf(d.query) >= 0 || a.id.toLowerCase().indexOf(d.query) >= 0)
        delegate: Rectangle {
            width: ListView.view.width
            height: 48
            radius: 8
            color: row.containsMouse ? Theme.s2 : "transparent"
            Icon { id: appIcon; name: "app:" + modelData.id; size: 28; x: 12; anchors.verticalCenter: parent.verticalCenter }
            Column {
                anchors.verticalCenter: parent.verticalCenter
                x: 52
                Text { text: modelData.name; color: Theme.tx; font.pixelSize: 14 }
                Text { text: modelData.id; color: Theme.mu; font.pixelSize: 11 }
            }
            MouseArea {
                id: row
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: { d.open = false; if (d.cb) d.cb(modelData.id) }
            }
        }
    }
}
