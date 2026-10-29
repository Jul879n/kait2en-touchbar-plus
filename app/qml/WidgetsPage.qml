// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

PageColumn {
    id: page
    property var app
    readonly property var lay: app.st.layout

    Card {
        title: app.tt("Reloj")
        TextRow {
            last: true
            title: app.tt("Formato (strftime, p. ej. %H:%M)")
            text: page.lay.clock.format || ""
            fieldWidth: 200
            onEdited: (v) => page.lay.clock.format = v
        }
    }
    Card {
        title: app.tt("Clima")
        ChoiceRow {
            title: app.tt("Ubicación")
            options: [app.tt("Automática (GeoClue)"), app.tt("Manual")]
            index: app.get(page.lay.weather, "location", "auto") === "auto" ? 0 : 1
            onPicked: (i) => { page.lay.weather.location = i === 0 ? "auto" : "manual"; app.bump() }
        }
        StepRow {
            title: app.tt("Latitud (respaldo o manual)")
            value: app.get(page.lay.weather, "latitude", 0)
            from: -90; to: 90; step: 0.01; decimals: 2
            onMoved: (v) => { page.lay.weather.latitude = v; app.bump() }
        }
        StepRow {
            title: app.tt("Longitud (respaldo o manual)")
            value: app.get(page.lay.weather, "longitude", 0)
            from: -180; to: 180; step: 0.01; decimals: 2
            onMoved: (v) => { page.lay.weather.longitude = v; app.bump() }
        }
        StepRow {
            last: true
            title: app.tt("Actualizar cada (minutos)")
            value: app.get(page.lay.weather, "refresh_min", 15)
            from: 1; to: 240
            onMoved: (v) => { page.lay.weather.refresh_min = v; app.bump() }
        }
    }
    Card {
        title: app.tt("Pomodoro")
        StepRow {
            title: app.tt("Trabajo (minutos)")
            value: app.get(page.lay.pomodoro, "work_min", 25)
            from: 1; to: 180
            onMoved: (v) => { page.lay.pomodoro.work_min = v; app.bump() }
        }
        StepRow {
            last: true
            title: app.tt("Descanso (minutos)")
            value: app.get(page.lay.pomodoro, "break_min", 5)
            from: 1; to: 60
            onMoved: (v) => { page.lay.pomodoro.break_min = v; app.bump() }
        }
    }
    Card {
        title: app.tt("Canción actual")
        SwitchRow {
            last: true
            title: app.tt("Visualizador animado")
            checked: !!app.get(page.lay.now_playing, "visualizer", true)
            onToggled: (v) => { page.lay.now_playing.visualizer = v; app.bump() }
        }
    }
    Card {
        title: app.tt("Emojis favoritos")
        RowBase {
            title: app.tt("El ícono se dibuja en la barra; el texto es lo que se inserta.")
            Btn {
                kind: "primary"
                text: app.tt("Agregar")
                onClicked: { page.lay.emoji.favorites.push({ icon: "smile", text: "🙂" }); app.bump() }
            }
        }
        Repeater {
            model: { app.rev; return page.lay.emoji.favorites }
            RowBase {
                readonly property int at: index
                last: index === page.lay.emoji.favorites.length - 1
                title: ""
                implicitHeight: 58
                Row {
                    spacing: 8
                    Btn {
                        icon: modelData.icon
                        text: modelData.icon
                        onClicked: app.pickIcon((name) => { page.lay.emoji.favorites[at].icon = name; app.bump() })
                    }
                    Input {
                        fieldWidth: 180
                        text: modelData.text
                        label: app.tt("Texto a insertar")
                        placeholder: app.tt("Texto a insertar")
                        onEdited: (v) => page.lay.emoji.favorites[at].text = v
                    }
                    Btn {
                        icon: "x"
                        danger: true
                        Accessible.name: app.tt("Quitar")
                        onClicked: { page.lay.emoji.favorites.splice(at, 1); app.bump() }
                    }
                }
            }
        }
    }
}
