// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

RowBase {
    id: r
    property alias text: field.text
    property string placeholder: ""
    property int fieldWidth: 260
    property bool invalid: false
    signal edited(string value)
    Input {
        id: field
        fieldWidth: r.fieldWidth
        placeholder: r.placeholder
        label: r.title
        invalid: r.invalid
        onEdited: (v) => r.edited(v)
    }
}
