// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

RowBase {
    id: r
    property bool checked: false
    signal toggled(bool value)
    Switch {
        checked: r.checked
        onToggled: (v) => r.toggled(v)
        Accessible.name: r.title
    }
}
