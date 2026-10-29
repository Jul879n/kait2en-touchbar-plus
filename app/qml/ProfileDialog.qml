// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

Dialog {
    id: d
    property var app
    signal chosen(string name)
    title: app.tt("Nuevo perfil")
    panelWidth: 420

    function show() {
        field.text = app.tt("Nuevo perfil")
        open = true
        field.focusInput.forceActiveFocus()
        field.focusInput.selectAll()
    }
    function accept() {
        if (field.text.trim() === "")
            return
        open = false
        chosen(field.text.trim())
    }

    Text { text: d.app.tt("Nombre del perfil"); color: Theme.mu; font.pixelSize: 12 }
    Input { id: field; width: parent.width; onAccepted: d.accept() }
    Row {
        anchors.right: parent.right
        spacing: 8
        Btn { text: d.app.tt("Cancelar"); onClicked: d.open = false }
        Btn { kind: "primary"; text: d.app.tt("Agregar"); onClicked: d.accept() }
    }
}
