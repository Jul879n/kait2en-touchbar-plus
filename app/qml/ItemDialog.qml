// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

// Adds or edits a personal key or a switch.
Dialog {
    id: d
    property var app
    property var cb: null
    property string kind: "key"
    property bool keyOnly: false
    property bool noIcon: false
    property string icon: ""
    property string mode: "send"
    property string toggle: "dnd"
    property string error: ""
    title: app.tt("Ítem")

    function show(item, callback, onlyKeys, withoutIcon) {
        cb = callback
        keyOnly = !!onlyKeys
        noIcon = !!withoutIcon
        item = item || { type: "key" }
        kind = item.type === "toggle" ? "toggle" : "key"
        icon = noIcon ? "" : (item.icon || "")
        mode = item.run ? "run" : "send"
        toggle = item.what || "dnd"
        label.text = item.label || ""
        value.text = item.send || item.run || ""
        error = ""
        open = true
        if (kind === "key")
            label.focusInput.forceActiveFocus()
    }

    function accept() {
        var item = { type: kind }
        if (kind === "key") {
            if (label.text.trim() !== "")
                item.label = label.text.trim()
            if (icon !== "" && !noIcon)
                item.icon = icon
            item[mode] = value.text.trim()
            var problem = backend.validateKey(JSON.stringify(item))
            if (problem !== "") {
                error = problem
                return
            }
        } else {
            item.what = toggle
        }
        open = false
        if (cb)
            cb(item)
    }

    Row {
        visible: !d.keyOnly
        spacing: 6
        Btn { kind: "tab"; text: d.app.tt("Tecla personalizada"); checked: d.kind === "key"; onClicked: d.kind = "key" }
        Btn { kind: "tab"; text: d.app.tt("Interruptor"); checked: d.kind === "toggle"; onClicked: d.kind = "toggle" }
    }
    Column {
        visible: d.kind === "key"
        width: parent.width
        spacing: 10
        Text { text: d.noIcon ? d.app.tt("Etiqueta (ASCII, 1–12)") : d.app.tt("Etiqueta (ASCII, 1–12; opcional con ícono)"); color: Theme.mu; font.pixelSize: 12 }
        Input { id: label; width: parent.width; label: d.app.tt("Etiqueta") }
        Text { visible: !d.noIcon; text: d.app.tt("Ícono"); color: Theme.mu; font.pixelSize: 12 }
        Row {
            visible: !d.noIcon
            spacing: 8
            Btn {
                icon: d.icon
                text: d.icon !== "" ? d.icon : d.app.tt("Elegir…")
                onClicked: d.app.pickIcon((name) => d.icon = name)
            }
            Btn { visible: d.icon !== ""; icon: "x"; text: d.app.tt("Quitar ícono"); onClicked: d.icon = "" }
        }
        Text { text: d.app.tt("Acción"); color: Theme.mu; font.pixelSize: 12 }
        Row {
            spacing: 6
            Btn { kind: "tab"; text: d.app.tt("Atajo de teclado"); checked: d.mode === "send"; onClicked: d.mode = "send" }
            Btn { kind: "tab"; text: d.app.tt("Comando"); checked: d.mode === "run"; onClicked: d.mode = "run" }
        }
        Input { id: value; width: parent.width; label: d.app.tt("Atajo o comando"); placeholder: d.app.tt("Atajo (ctrl+shift+t) o comando"); onAccepted: d.accept() }
    }
    Flow {
        visible: d.kind === "toggle"
        width: parent.width
        spacing: 6
        Repeater {
            model: Object.keys(d.app.meta.toggles)
            Btn { kind: "tab"; text: d.app.meta.toggles[modelData]; checked: d.toggle === modelData; onClicked: d.toggle = modelData }
        }
    }
    Text {
        visible: d.error !== ""
        width: parent.width
        text: d.error
        color: Theme.danger
        font.pixelSize: 13
        wrapMode: Text.WordWrap
    }
    Row {
        anchors.right: parent.right
        spacing: 8
        Btn { text: d.app.tt("Cancelar"); onClicked: d.open = false }
        Btn { kind: "primary"; text: d.app.tt("Aceptar"); onClicked: d.accept() }
    }
}
