// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

Flickable {
    id: f
    default property alias body: holder.data
    clip: true
    boundsBehavior: Flickable.StopAtBounds
    flickableDirection: Flickable.VerticalFlick
    contentWidth: width
    contentHeight: holder.childrenRect.height
    Item {
        id: holder
        width: f.width
        height: childrenRect.height
    }
    Rectangle {
        parent: f
        z: 5
        x: f.width - 7
        y: 2 + f.visibleArea.yPosition * (f.height - 4)
        width: 4
        radius: 2
        height: Math.max(24, f.visibleArea.heightRatio * (f.height - 4))
        color: Theme.mu
        opacity: f.moving || f.dragging ? 0.6 : 0.0
        visible: f.contentHeight > f.height + 1
        Behavior on opacity { NumberAnimation { duration: 300 } }
    }
}
