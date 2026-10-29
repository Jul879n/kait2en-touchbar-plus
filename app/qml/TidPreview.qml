// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

// The Touch ID prompt as the daemon draws it: the right half of the bar, where the sensor is.
Item {
    id: p
    property string prefix: ""   // file URL (already encoded) of the snapshot prefix
    property string look: "F"
    property string state: "waiting"
    property int viewWidth: 620
    width: viewWidth
    height: Math.round(viewWidth * 60 / 1000)
    clip: true

    Rectangle { anchors.fill: parent; radius: 8; color: "#000000" }
    Image {
        anchors.fill: parent
        source: p.prefix ? p.prefix + "-" + p.look + "-" + p.state + ".png" : ""
        sourceClipRect: Qt.rect(1008, 0, 1000, 60)
        smooth: true
        cache: false
    }
    Rectangle { anchors.fill: parent; radius: 8; color: "transparent"; border.width: 1; border.color: Theme.line2 }
}
