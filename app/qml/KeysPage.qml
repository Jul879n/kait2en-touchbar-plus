// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

PageColumn {
    id: page
    property var app

    Card {
        title: app.tt("Teclas personales de la tercera fila")
        RowBase {
            title: app.tt("Hasta cuatro caben junto a las teclas especiales.")
            Btn {
                kind: "primary"
                text: app.tt("Agregar")
                onClicked: app.editItem(null, (item) => {
                    var entry = {}
                    for (var k in item) if (k === "label" || k === "send" || k === "run") entry[k] = item[k]
                    if (!entry.label) entry.label = (entry.send || entry.run || "key").substring(0, 12)
                    app.st.keys.push(entry)
                    app.bump()
                }, true, true)
            }
        }
        Repeater {
            model: { app.rev; return app.st.keys }
            RowBase {
                readonly property int at: index
                last: index === app.st.keys.length - 1
                title: modelData.label || "?"
                subtitle: (modelData.send ? app.tt("atajo") + " " + modelData.send : app.tt("ejecuta") + " " + (modelData.run || ""))
                Btn {
                    icon: "x"
                    danger: true
                    Accessible.name: app.tt("Quitar")
                    onClicked: { app.st.keys.splice(at, 1); app.bump() }
                }
            }
        }
    }
}
