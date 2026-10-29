// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

// A scrolling page whose cards share the same centered column.
Scroll {
    id: s
    default property alias content: col.data
    Column {
        id: col
        x: (s.width - width) / 2
        y: 24
        width: Math.min(760, s.width - 48)
        spacing: 18
        bottomPadding: 28
    }
}
