// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

Item {
    id: r
    property string title: ""
    property string subtitle: ""
    property bool last: false
    property color subtitleColor: Theme.mu
    default property alias control: slot.data
    width: parent ? parent.width : 0
    implicitHeight: Math.max(54, texts.implicitHeight + 22)

    Column {
        id: texts
        anchors.left: parent.left
        anchors.right: slot.left
        anchors.rightMargin: 14
        anchors.verticalCenter: parent.verticalCenter
        spacing: 2
        Text {
            width: parent.width
            text: r.title
            color: Theme.tx
            font.pixelSize: 14
            wrapMode: Text.WordWrap
        }
        Text {
            visible: r.subtitle !== ""
            width: parent.width
            text: r.subtitle
            color: r.subtitleColor
            font.pixelSize: 12
            wrapMode: Text.WordWrap
        }
    }
    Item {
        id: slot
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        width: childrenRect.width
        height: childrenRect.height
    }
    Rectangle {
        anchors.bottom: parent.bottom
        width: parent.width
        height: 1
        color: Theme.line
        visible: !r.last
    }
}
