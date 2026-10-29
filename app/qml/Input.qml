// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

Rectangle {
    id: f
    property alias text: t.text
    property string placeholder: ""
    property int fieldWidth: 240
    property alias focusInput: t
    property string label: ""
    property bool invalid: false
    signal edited(string value)
    signal accepted

    implicitWidth: fieldWidth
    implicitHeight: 34
    radius: 8
    color: Theme.bg
    border.width: 1
    border.color: invalid ? Theme.danger : (t.activeFocus ? Theme.ac : Theme.line2)
    Behavior on border.color { ColorAnimation { duration: 120 } }

    TextInput {
        id: t
        anchors.fill: parent
        anchors.leftMargin: 10
        anchors.rightMargin: 10
        verticalAlignment: TextInput.AlignVCenter
        color: Theme.tx
        selectionColor: Theme.ac
        selectedTextColor: Theme.acb
        selectByMouse: true
        clip: true
        font.pixelSize: 14
        onTextEdited: f.edited(text)
        onAccepted: f.accepted()
        Accessible.name: f.label || f.placeholder
    }
    Text {
        anchors.fill: parent
        anchors.leftMargin: 10
        verticalAlignment: Text.AlignVCenter
        visible: t.text === "" && !t.activeFocus
        text: f.placeholder
        color: Theme.mu
        opacity: 0.7
        font.pixelSize: 14
    }
}
