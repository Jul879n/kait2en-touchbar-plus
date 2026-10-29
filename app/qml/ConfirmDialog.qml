// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

Dialog {
    id: d
    property var app
    property string message: ""
    property string confirmText: ""
    property bool danger: false
    signal confirmed
    panelWidth: 440

    Text {
        width: parent.width
        text: d.message
        color: Theme.mu
        font.pixelSize: 14
        wrapMode: Text.WordWrap
    }
    Row {
        anchors.right: parent.right
        spacing: 8
        Btn { text: d.app.tt("Cancelar"); onClicked: d.open = false }
        Btn {
            kind: d.danger ? "ghost" : "primary"
            danger: d.danger
            text: d.confirmText
            onClicked: { d.open = false; d.confirmed() }
        }
    }
}
