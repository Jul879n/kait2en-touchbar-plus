// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

RowBase {
    id: r
    property var options: []
    property int index: 0
    signal picked(int i)
    Row {
        spacing: 6
        Repeater {
            model: r.options
            Btn {
                kind: "tab"
                text: modelData
                checked: r.index === index
                onClicked: r.picked(index)
            }
        }
    }
}
