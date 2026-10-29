// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

PageColumn {
    id: page
    property var app
    readonly property var cfg: app.st.config
    readonly property var looks: [
        ["A", "Pulso", "Ondas que salen de la huella."],
        ["B", "Tecla", "Una tecla con un destello que la cruza."],
        ["C", "Escáner", "Una línea de luz recorre la huella."],
        ["D", "Hilo de luz", "Un hilo viaja por el borde hacia el lector."],
        ["E", "Cadena de puntos", "Seis puntos se encienden hacia el lector."],
        ["F", "Resplandor", "Un halo ilumina el borde junto al sensor."]
    ]
    readonly property var states: [
        ["waiting", "Esperando", "Desbloquear con Touch ID"],
        ["retry", "Reintentar", "Intentar de nuevo"],
        ["failed", "Fallo", "Usar contraseña"],
        ["matched", "Desbloqueado", "Desbloqueado"]
    ]
    readonly property var animations: ["completa", "sutil", "ninguna"]
    property string prefix: ""
    property string problem: ""
    readonly property string look: app.get(cfg, "touch_id_style", "F")

    // Renders the six looks with the settings being edited.
    function refresh() { backend.touchIdPreview(JSON.stringify(cfg)) }
    function change(key, value) {
        cfg[key] = value
        app.bump()
        later.restart()
    }
    onVisibleChanged: if (visible) refresh()
    Component.onCompleted: if (visible) refresh()
    Connections {
        target: backend
        function onPreviewReady(json) {
            var r = JSON.parse(json)
            page.prefix = r.url || ""
            page.problem = r.error || ""
        }
    }
    Timer { id: later; interval: 300; onTriggered: page.refresh() }

    Card {
        title: app.tt("Estilo")
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
                Accessible.onPressAction: page.change("touch_id_style", modelData[0])
                Keys.onSpacePressed: page.change("touch_id_style", modelData[0])
                MouseArea { id: hover; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                    onClicked: page.change("touch_id_style", modelData[0]) }
                Column {
                    x: 16
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 2
                    Text { text: modelData[0] + " · " + app.tt(modelData[1]); color: Theme.tx; font.pixelSize: 14; font.weight: Font.DemiBold }
                    Text { text: app.tt(modelData[2]); color: Theme.mu; font.pixelSize: 12 }
                }
                TidPreview {
                    anchors.right: parent.right
                    anchors.rightMargin: 10
                    anchors.verticalCenter: parent.verticalCenter
                    viewWidth: 360
                    prefix: page.prefix
                    look: modelData[0]
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
            model: page.states
            Item {
                width: parent.width
                height: 54
                Text { x: 0; anchors.verticalCenter: parent.verticalCenter; text: app.tt(modelData[1]); color: Theme.mu; font.pixelSize: 13 }
                TidPreview {
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    viewWidth: 540
                    prefix: page.prefix
                    look: page.look
                    state: modelData[0]
                }
            }
        }
        Text {
            width: parent.width
            text: app.tt("Imágenes fijas; el movimiento solo se ve en la barra.")
            color: Theme.mu
            font.pixelSize: 12
            wrapMode: Text.WordWrap
        }
    }

    Card {
        title: app.tt("Colores")
        Repeater {
            model: page.states
            ColorRow {
                title: app.tt(modelData[1])
                allowAuto: modelData[0] === "waiting"
                last: modelData[0] === "matched"
                value: app.get(page.cfg, "touch_id_color_" + modelData[0], modelData[0] === "waiting" ? "auto" : "#ffffff")
                onPicked: (c) => page.change("touch_id_color_" + modelData[0], c)
            }
        }
    }

    Card {
        title: app.tt("Textos")
        Repeater {
            model: page.states
            TextRow {
                title: app.tt(modelData[1])
                last: modelData[0] === "matched"
                fieldWidth: 300
                placeholder: app.tt(modelData[2])
                text: app.get(page.cfg, "touch_id_text_" + modelData[0], "")
                invalid: !/^[\x20-\x7e]{0,24}$/.test(text)
                subtitle: invalid ? app.tt("Hasta 24 caracteres ASCII.") : ""
                subtitleColor: Theme.danger
                onEdited: (v) => { if (/^[\x20-\x7e]{0,24}$/.test(v)) page.change("touch_id_text_" + modelData[0], v) }
            }
        }
        Text {
            width: parent.width
            text: app.tt("Vacío usa el texto del idioma. Hasta 24 caracteres, sin acentos.")
            color: Theme.mu
            font.pixelSize: 12
            wrapMode: Text.WordWrap
        }
    }

    Card {
        title: app.tt("Animación")
        ChoiceRow {
            title: app.tt("Movimiento")
            options: [app.tt("Completa"), app.tt("Sutil"), app.tt("Ninguna")]
            index: Math.max(0, page.animations.indexOf(app.get(page.cfg, "touch_id_animation", "completa")))
            onPicked: (i) => page.change("touch_id_animation", page.animations[i])
        }
        StepRow {
            last: true
            title: app.tt("Velocidad (%)")
            value: app.get(page.cfg, "touch_id_speed", 100)
            from: 50; to: 200; step: 10
            onMoved: (v) => page.change("touch_id_speed", v)
        }
    }
}
