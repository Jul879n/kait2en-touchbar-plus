// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

Image {
    property string name: ""
    property color tint: "#eef1f6"
    property int size: 20
    width: size
    height: size
    sourceSize: Qt.size(size * 2, size * 2)
    smooth: true
    cache: true
    // "app:<id>.desktop" is an application's own icon, in its colors.
    source: !name ? "" : name.indexOf("app:") === 0
        ? "image://appicon/" + name.substring(4)
        : "image://lucide/" + name + "/" + String(Qt.color(tint)).substring(1, 7)
}
