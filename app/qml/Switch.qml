// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

Rectangle {
    id: s
    property bool checked: false
    signal toggled(bool value)
    implicitWidth: 46
    implicitHeight: 26
    radius: 13
    color: checked ? Theme.ac : Theme.line2
    activeFocusOnTab: true
    Accessible.role: Accessible.CheckBox
    Accessible.checked: checked
    Accessible.onToggleAction: s.toggled(!s.checked)
    Accessible.onPressAction: s.toggled(!s.checked)
    Behavior on color { ColorAnimation { duration: 140 } }
    Rectangle {
        width: 20
        height: 20
        radius: 10
        y: 3
        x: s.checked ? s.width - width - 3 : 3
        color: s.checked ? Theme.acb : Theme.tx
        Behavior on x { NumberAnimation { duration: 140; easing.type: Easing.OutCubic } }
    }
    Rectangle {
        anchors.fill: parent
        anchors.margins: -3
        radius: 16
        color: "transparent"
        border.width: 2
        border.color: Theme.ac
        visible: s.activeFocus
    }
    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: s.toggled(!s.checked)
    }
    Keys.onSpacePressed: s.toggled(!s.checked)
    Keys.onReturnPressed: s.toggled(!s.checked)
}
