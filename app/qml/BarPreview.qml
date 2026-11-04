// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

// A daemon snapshot of the bar; by default the right half, where the sensor and special keys are.
Item {
    id: p
    property string prefix: ""   // file URL (already encoded) of the snapshot prefix
    property string look: "F"
    property string state: "waiting"
    property rect area: Qt.rect(1008, 0, 1000, 60)
    property int viewWidth: 620
    width: viewWidth
    height: Math.round(viewWidth * area.height / area.width)
    clip: true

    Rectangle { anchors.fill: parent; radius: 8; color: "#000000" }
    Image {
        anchors.fill: parent
        source: p.prefix ? p.prefix + "-" + p.look + "-" + p.state + ".png" : ""
        sourceClipRect: area
        smooth: true
        cache: false
    }
    Rectangle { anchors.fill: parent; radius: 8; color: "transparent"; border.width: 1; border.color: Theme.line2 }
}
