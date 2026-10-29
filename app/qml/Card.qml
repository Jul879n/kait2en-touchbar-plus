// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

Rectangle {
    id: c
    property string title: ""
    default property alias rows: col.data
    width: parent ? parent.width : 0
    implicitHeight: col.implicitHeight + 28
    height: implicitHeight
    color: Theme.s2
    radius: 14
    border.width: 1
    border.color: Theme.line

    Column {
        id: col
        x: 18
        y: 14
        width: parent.width - 36
        Heading {
            visible: c.title !== ""
            height: visible ? 32 : 0
            text: c.title
            verticalAlignment: Text.AlignVCenter
        }
    }
}
