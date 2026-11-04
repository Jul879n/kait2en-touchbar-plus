// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

PageColumn {
    id: page
    property var app
    readonly property var cfg: app.st.config
    readonly property var looks: [
        ["A", "Contorno", "Texto con borde, como ahora."],
        ["B", "Tintado", "Fondo tenue y borde del color de cada tecla."],
        ["C", "Sólido", "Tecla rellena de color con texto oscuro."],
        ["D", "Neón", "Fondo negro con borde de color brillante."]
    ]
    readonly property var layers: [
        ["function", "Teclas F"],
        ["special", "Especiales"]
    ]
    property string prefix: ""
    property string problem: ""
    readonly property string look: app.get(cfg, "key_style", "A")

    // Renders the F and special layers in every style with the settings being edited.
    function refresh() { backend.keysPreview(JSON.stringify(cfg)) }
    function change(key, value) {
        cfg[key] = value
        app.bump()
        later.restart()
    }
    onVisibleChanged: if (visible) refresh()
    Component.onCompleted: if (visible) refresh()
    Connections {
        target: backend
        function onKeysPreviewReady(json) {
            var r = JSON.parse(json)
            page.prefix = r.url || ""
            page.problem = r.error || ""
        }
    }
    Timer { id: later; interval: 300; onTriggered: page.refresh() }

    Card {
        title: app.tt("Estilo de teclas")
        Repeater {
            model: page.looks
            Rectangle {
                width: parent.width
                height: 64
                radius: 10
                color: page.look === modelData[0] ? "#16203a" : (hover.containsMouse ? Theme.chipHover : "transparent")
                border.width: page.look === modelData[0] ? 2 : 1
                border.color: page.look === modelData[0] ? Theme.ac : Theme.line2
                activeFocusOnTab: true
                Accessible.role: Accessible.RadioButton
                Accessible.name: modelData[1]
                Accessible.checked: page.look === modelData[0]
                Accessible.onPressAction: page.change("key_style", modelData[0])
                Keys.onSpacePressed: page.change("key_style", modelData[0])
                MouseArea { id: hover; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                    onClicked: page.change("key_style", modelData[0]) }
                Column {
                    x: 16
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 2
                    Text { text: modelData[0] + " · " + app.tt(modelData[1]); color: Theme.tx; font.pixelSize: 14; font.weight: Font.DemiBold }
                    Text { text: app.tt(modelData[2]); color: Theme.mu; font.pixelSize: 12 }
                }
                Row {
                    anchors.right: parent.right
                    anchors.rightMargin: 10
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 8
                    BarPreview {
                        viewWidth: 300
                        prefix: page.prefix
                        look: modelData[0]
                        state: "function"
                    }
                    BarPreview {
                        viewWidth: 300
                        prefix: page.prefix
                        look: modelData[0]
                        state: "special"
                    }
                }
            }
        }
    }

    Card {
        title: app.tt("Vista previa")
        Text {
            visible: page.problem !== ""
            width: parent.width
            text: page.problem
            color: Theme.danger
            font.pixelSize: 12
            wrapMode: Text.WordWrap
        }
        Repeater {
            model: page.layers
            Column {
                width: parent.width
                spacing: 4
                Text { text: app.tt(modelData[1]); color: Theme.mu; font.pixelSize: 13 }
                BarPreview {
                    viewWidth: parent.width
                    area: Qt.rect(0, 0, 2008, 60)
                    prefix: page.prefix
                    look: page.look
                    state: modelData[0]
                }
            }
        }
    }

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
