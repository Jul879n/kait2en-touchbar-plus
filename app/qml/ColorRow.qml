// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

// A color among a few presets, or any #rrggbb. `auto` follows the key color.
RowBase {
    id: r
    property string value: "auto"
    property bool allowAuto: false
    readonly property var presets: ["#dce6ff", "#4da3ff", "#34c759", "#ffc83d", "#ff9f0a", "#ff453a", "#ff6b81", "#b88cff"]
    signal picked(string color)

    Row {
        spacing: 6
        Btn {
            visible: r.allowAuto
            kind: "tab"
            text: "auto"
            checked: r.value === "auto"
            onClicked: r.picked("auto")
        }
        Repeater {
            model: r.presets
            Rectangle {
                width: 26; height: 26; radius: 13
                anchors.verticalCenter: parent.verticalCenter
                color: modelData
                border.width: r.value.toLowerCase() === modelData ? 3 : 1
                border.color: r.value.toLowerCase() === modelData ? Theme.tx : Theme.line2
                activeFocusOnTab: true
                Accessible.role: Accessible.Button
                Accessible.name: modelData
                Accessible.onPressAction: r.picked(modelData)
                Keys.onReturnPressed: r.picked(modelData)
                Keys.onSpacePressed: r.picked(modelData)
                Keys.onLeftPressed: r.picked(r.presets[Math.max(0, index - 1)])
                Keys.onRightPressed: r.picked(r.presets[Math.min(r.presets.length - 1, index + 1)])
                Rectangle { anchors.fill: parent; anchors.margins: -4; radius: width / 2; color: "transparent"; border.width: 2; border.color: Theme.ac; visible: parent.activeFocus }
                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: r.picked(modelData) }
            }
        }
        Input {
            fieldWidth: 96
            anchors.verticalCenter: parent.verticalCenter
            placeholder: "#rrggbb"
            text: r.value === "auto" ? "" : r.value
            onEdited: (v) => { if (/^#[0-9a-fA-F]{6}$/.test(v.trim())) r.picked(v.trim().toLowerCase()) }
        }
    }
}
