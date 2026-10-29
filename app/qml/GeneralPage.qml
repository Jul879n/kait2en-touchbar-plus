// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

PageColumn {
    id: page
    property var app
    readonly property var cfg: app.st.config
    readonly property var languages: ["auto", "es", "en"]

    Card {
        title: app.tt("Encendido y pantalla OLED")
        ChoiceRow {
            title: app.tt("Idioma de la barra y la app")
            subtitle: app.tt("Reinicie la app para cambiar su idioma.")
            options: [app.tt("Automático"), "Español", "English"]
            index: Math.max(0, page.languages.indexOf(app.get(page.cfg, "language", "auto")))
            onPicked: (i) => { page.cfg.language = page.languages[i]; app.bump() }
        }
        TextRow {
            title: app.tt("Idiomas de sugerencias (ej. es_CL,en_US)")
            text: app.st.suggest_langs || ""
            fieldWidth: 220
            onEdited: (v) => app.st.suggest_langs = v.trim()
        }
        SwitchRow {
            title: app.tt("Siempre encendida")
            subtitle: app.tt("Solo se apaga al bloquear y al suspender")
            checked: !!app.get(page.cfg, "always_on", true)
            onToggled: (v) => { page.cfg.always_on = v; app.bump() }
        }
        SwitchRow {
            title: app.tt("Oscura al bloquear la pantalla")
            checked: !!app.get(page.cfg, "screen_lock_dark", true)
            onToggled: (v) => { page.cfg.screen_lock_dark = v; app.bump() }
        }
        ChoiceRow {
            title: app.tt("Brillo normal")
            options: [app.tt("Normal"), app.tt("Tenue")]
            index: app.get(page.cfg, "active_brightness", 128) >= 2 ? 0 : 1
            onPicked: (i) => { page.cfg.active_brightness = i === 0 ? 128 : 1; app.bump() }
        }
        StepRow {
            title: app.tt("Atenuar tras (segundos, 0 = nunca)")
            value: app.get(page.cfg, "dim_after_ms", 120000) / 1000
            from: 0; to: 3600; step: 10
            onMoved: (v) => { page.cfg.dim_after_ms = v * 1000; app.bump() }
        }
        SwitchRow {
            last: true
            title: app.tt("Desplazar píxeles (protege el OLED)")
            checked: !!app.get(page.cfg, "pixel_shift", true)
            onToggled: (v) => { page.cfg.pixel_shift = v; app.bump() }
        }
    }

    Card {
        title: app.tt("Apariencia y gestos")
        RowBase {
            id: colorRow
            title: app.tt("Color de los íconos")
            property string hex: app.get(page.cfg, "key_color", "#dce6ff")
            Row {
                spacing: 10
                Rectangle {
                    width: 34; height: 34; radius: 8
                    color: /^#[0-9a-fA-F]{6}$/.test(colorRow.hex) ? colorRow.hex : "transparent"
                    border.width: 1; border.color: Theme.line2
                }
                Input {
                    fieldWidth: 120
                    text: colorRow.hex
                    label: colorRow.title
                    invalid: !/^#[0-9a-fA-F]{6}$/.test(text)
                    onEdited: (v) => {
                        if (/^#[0-9a-fA-F]{6}$/.test(v)) {
                            page.cfg.key_color = v.toLowerCase()
                            app.bump()
                        }
                    }
                }
            }
        }
        SwitchRow {
            title: app.tt("Respuesta háptica")
            checked: !!app.get(page.cfg, "haptic_feedback", true)
            onToggled: (v) => { page.cfg.haptic_feedback = v; app.bump() }
        }
        SwitchRow {
            title: app.tt("Háptica también al soltar")
            checked: !!app.get(page.cfg, "haptic_touch_up", true)
            onToggled: (v) => { page.cfg.haptic_touch_up = v; app.bump() }
        }
        StepRow {
            title: app.tt("Sensibilidad del deslizamiento de volumen (px por paso)")
            value: app.get(page.cfg, "volume_swipe_step_px", 40)
            from: 10; to: 200; step: 5
            onMoved: (v) => { page.cfg.volume_swipe_step_px = v; app.bump() }
        }
        StepRow {
            title: app.tt("Sensibilidad del deslizamiento de brillo (px por paso)")
            value: app.get(page.cfg, "brightness_swipe_step_px", 40)
            from: 10; to: 200; step: 5
            onMoved: (v) => { page.cfg.brightness_swipe_step_px = v; app.bump() }
        }
        StepRow {
            last: true
            title: app.tt("Mantener para abrir el slider (ms)")
            value: app.get(page.cfg, "hold_slider_ms", 500)
            from: 200; to: 1500; step: 50
            onMoved: (v) => { page.cfg.hold_slider_ms = v; app.bump() }
        }
    }

    Card {
        title: app.tt("Avisos")
        SwitchRow {
            title: app.tt("Notificaciones en la barra")
            checked: !!app.get(page.cfg, "notifications", true)
            onToggled: (v) => { page.cfg.notifications = v; app.bump() }
        }
        SwitchRow {
            title: app.tt("Apps de Cmd+Tab en la barra")
            checked: !!app.get(page.cfg, "app_switcher", true)
            onToggled: (v) => { page.cfg.app_switcher = v; app.bump() }
        }
        SwitchRow {
            title: app.tt("Sugerencias de escritura")
            checked: !!app.get(page.cfg, "suggestions", true)
            onToggled: (v) => { page.cfg.suggestions = v; app.bump() }
        }
        SwitchRow {
            title: app.tt("Canción nueva con la barra oscura")
            checked: !!app.get(page.cfg, "show_track_changes", true)
            onToggled: (v) => { page.cfg.show_track_changes = v; app.bump() }
        }
        RowBase {
            last: true
            title: app.tt("Probar notificación")
            Btn {
                text: app.tt("Enviar aviso de prueba")
                onClicked: {
                    var error = backend.testNotice()
                    app.toast(error !== "" ? error : app.tt("Aviso de prueba"))
                }
            }
        }
    }
}
