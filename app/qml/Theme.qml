// SPDX-License-Identifier: GPL-3.0-or-later
pragma Singleton
import QtQuick

QtObject {
    readonly property color bg: "#0f1115"
    readonly property color s1: "#171a21"
    readonly property color s2: "#1f232c"
    readonly property color tx: "#eef1f6"
    readonly property color mu: "#a3acbd"
    readonly property color ac: "#7aa2ff"
    readonly property color acb: "#0b1a44"
    readonly property color line: "#2a2f3a"
    readonly property color line2: "#3a4150"
    readonly property color chip: "#12151c"
    readonly property color chipHover: "#1c212b"
    readonly property color glow: "#dce6ff"
    readonly property color danger: "#ff6b81"

    // Icon colors of the "full color" mode; must match `full_tint` in the daemon.
    readonly property var full: ({
        "volume-2": "#4da3ff", "bluetooth": "#4da3ff", "bluetooth-off": "#4da3ff", "sliders-horizontal": "#4da3ff",
        "wifi": "#4da3ff", "wifi-off": "#4da3ff", "clock": "#4da3ff",
        "sun": "#ffc83d", "sun-moon": "#ffc83d", "coffee": "#ffc83d", "cloud-sun": "#ffc83d", "keyboard": "#ffc83d", "moon": "#ffc83d",
        "play": "#ff9f45", "pause": "#ff9f45", "timer": "#ff9f45",
        "battery-full": "#4cd964", "battery-medium": "#4cd964", "battery-charging": "#4cd964", "check": "#4cd964", "thumbs-up": "#4cd964",
        "camera": "#ff6b81", "smile": "#ff6b81", "laugh": "#ff6b81", "heart": "#ff6b81", "flame": "#ff6b81",
        "party-popper": "#ff6b81", "thermometer": "#ff6b81", "battery-low": "#ff6b81",
        "bell-off": "#b88cff", "calendar": "#b88cff", "app-window": "#b88cff", "cpu": "#b88cff", "memory-stick": "#b88cff",
        "cloud": "#7fd4ff", "cloud-fog": "#7fd4ff", "cloud-drizzle": "#7fd4ff", "cloud-rain": "#7fd4ff",
        "cloud-snow": "#7fd4ff", "cloud-lightning": "#7fd4ff", "fan": "#7fd4ff", "arrow-down-up": "#7fd4ff"
    })

    function tint(name, fullColor) {
        return fullColor && full[name] ? full[name] : glow
    }

    readonly property var chipIcons: ({
        "controls": "sliders-horizontal", "playback": "play", "brightness": "sun", "volume": "volume-2",
        "keyboard_light": "keyboard", "screenshot": "camera", "emoji": "smile", "windows": "app-window",
        "pomodoro": "timer", "weather": "cloud-sun", "battery": "battery-full", "cpu": "cpu",
        "memory": "memory-stick", "temperature": "thermometer", "fan": "fan", "network": "arrow-down-up",
        "next_event": "calendar", "clock": "clock", "now_playing": "play"
    })
    readonly property var toggleIcons: ({
        "dnd": "bell-off", "night_light": "sun-moon", "caffeine": "coffee", "wifi": "wifi", "bluetooth": "bluetooth"
    })
    // The bar is a grid of 24 columns; widths below must match `widget_cols` and the cells of the daemon.
    readonly property int gridCols: 24
    readonly property var itemCols: ({
        "playback": 6, "volume": 8, "brightness": 4, "keyboard_light": 4, "now_playing": 8, "network": 4, "next_event": 4
    })
    // Keys an item is made of: a span can't be narrower than that.
    readonly property var itemCells: ({ "playback": 3, "volume": 4, "brightness": 2, "keyboard_light": 2 })
    function naturalCols(item) { return itemCols[item.type] || 2 }
    function minCols(item) { return itemCells[item.type] || 1 }
    function colsOf(item) {
        var span = item.span ? item.span : naturalCols(item)
        return Math.max(minCols(item), Math.min(gridCols, span))
    }
    function iconOf(item) {
        if (item.type === "key")
            return item.app ? "app:" + item.app : (item.icon || "")
        if (item.type === "toggle")
            return toggleIcons[item.what] || ""
        return chipIcons[item.type] || ""
    }
}
