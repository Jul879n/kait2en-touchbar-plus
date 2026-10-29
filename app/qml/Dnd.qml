// SPDX-License-Identifier: GPL-3.0-or-later
pragma Singleton
import QtQuick

// Drag and drop shared by the bar and the palette: the source reports the
// pointer in window coordinates, the bar decides what the drop means.
QtObject {
    property bool active: false
    property real gx: 0
    property real gy: 0
    property string icon: ""
    property string label: ""
    property color tint: "#dce6ff"
    property var payload: null
    signal moved(real x, real y)
    signal dropped(real x, real y)
    signal canceled

    function begin(text, iconName, color, data) {
        label = text
        icon = iconName
        tint = color
        payload = data
        active = true
    }
    function move(x, y) {
        gx = x
        gy = y
        moved(x, y)
    }
    // The system took the pointer away (no drop): forget the drag without acting on it.
    function cancel() {
        active = false
        payload = null
        canceled()
    }
    function end(x, y) {
        gx = x
        gy = y
        dropped(x, y)
        active = false
        payload = null
    }
}
