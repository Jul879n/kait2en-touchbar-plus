// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick
import QtQuick.Window

Window {
    id: root
    width: 1100
    height: 820
    minimumWidth: 900
    minimumHeight: 660
    visible: true
    color: Theme.bg
    title: tt("Touch Bar Plus")

    property var st: ({ config: {}, layout: { strip: [], "default": [], profile: [], controls: { items: [] }, emoji: { favorites: [] }, clock: {}, weather: {}, pomodoro: {}, now_playing: {} }, keys: [], suggest_langs: "" })
    property var meta: ({ types: {}, toggles: {}, icons: [] })
    property int rev: 0
    property int page: 0
    property bool ready: false
    readonly property bool modalOpen: iconPicker.open || appPicker.open || itemDialog.open
        || profileDialog.open || restoreDialog.open || confirmDialog.open

    function tt(text) { return backend.tr(text) }
    function bump() { rev++ }
    function get(object, key, fallback) {
        rev
        return object && object[key] !== undefined ? object[key] : fallback
    }
    function load(json) {
        var data = JSON.parse(json)
        st = data.state
        meta = data.meta
        rev++
        ready = false
        ready = true
        if (data.problem)
            toast(data.problem)
    }
    function toast(text) {
        toastText.text = text
        toastTimer.restart()
        toastBox.shown = true
    }
    function save() {
        var problem = backend.save(JSON.stringify(st))
        toast(problem !== "" ? problem : tt("Guardado y aplicado."))
    }
    function pickIcon(cb) { iconPicker.show(cb) }
    function pickApp(cb) { appPicker.show(cb) }
    function editItem(item, cb, keysOnly, noIcon) { itemDialog.show(item, cb, keysOnly, noIcon) }
    function confirm(title, message, confirmText, cb) {
        confirmDialog.title = title
        confirmDialog.message = message
        confirmDialog.confirmText = confirmText
        confirmDialog.action = cb
        confirmDialog.open = true
    }
    function newProfile() { profileDialog.show() }

    Component.onCompleted: load(backend.load())

    Shortcut { sequence: "Ctrl+S"; onActivated: root.save() }

    // ---- header ------------------------------------------------------------
    Rectangle {
        id: header
        enabled: !root.modalOpen
        width: parent.width
        height: 60
        color: Theme.s1
        Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
        Text {
            x: 22
            anchors.verticalCenter: parent.verticalCenter
            text: root.tt("Touch Bar Plus")
            color: Theme.tx
            font.pixelSize: 18
            font.weight: Font.DemiBold
        }
        Row {
            anchors.centerIn: parent
            spacing: 6
            Repeater {
                model: ["Barra", "General", "Widgets", "Teclas", "Touch ID"]
                Btn {
                    kind: "tab"
                    text: root.tt(modelData)
                    checked: root.page === index
                    onClicked: root.page = index
                }
            }
        }
        Row {
            anchors.right: parent.right
            anchors.rightMargin: 18
            anchors.verticalCenter: parent.verticalCenter
            spacing: 8
            Btn { icon: "rotate-cw"; text: root.tt("Restaurar"); onClicked: restoreDialog.open = true }
            Btn { kind: "primary"; text: root.tt("Guardar y aplicar"); onClicked: root.save() }
        }
    }

    // ---- pages -------------------------------------------------------------
    Item {
        id: body
        enabled: !root.modalOpen
        anchors.top: header.bottom
        anchors.bottom: parent.bottom
        width: parent.width
        Loader {
            anchors.fill: parent
            active: root.ready
            sourceComponent: Item {
                BarPage { anchors.fill: parent; app: root; visible: root.page === 0 }
                GeneralPage { anchors.fill: parent; app: root; visible: root.page === 1 }
                WidgetsPage { anchors.fill: parent; app: root; visible: root.page === 2 }
                KeysPage { anchors.fill: parent; app: root; visible: root.page === 3 }
                TouchIdPage { anchors.fill: parent; app: root; visible: root.page === 4 }
            }
        }
    }

    // ---- dialogs -----------------------------------------------------------
    IconPicker { id: iconPicker; app: root }
    AppPicker { id: appPicker; app: root }
    ItemDialog { id: itemDialog; app: root }
    ProfileDialog {
        id: profileDialog
        app: root
        onChosen: (name) => {
            root.st.layout.profile.push({ name: name, apps: [], items: [{ type: "playback" }, { type: "controls" }] })
            root.bump()
            root.toast(root.tt("Perfil agregado."))
        }
    }
    ConfirmDialog {
        id: restoreDialog
        app: root
        title: root.tt("¿Restaurar valores por defecto?")
        message: root.tt("Se reemplazan la configuración y la disposición actuales.")
        confirmText: root.tt("Restaurar")
        danger: true
        onConfirmed: { root.load(backend.restore()); root.toast(root.tt("Valores restaurados.")) }
    }

    ConfirmDialog {
        id: confirmDialog
        property var action: null
        app: root
        danger: true
        onConfirmed: { if (action) action() }
    }

    // ---- drag ghost and toast ----------------------------------------------
    Rectangle {
        id: ghost
        visible: Dnd.active
        z: 200
        x: Dnd.gx - width / 2
        y: Dnd.gy - height / 2
        width: ghostRow.implicitWidth + 24
        height: 40
        radius: 10
        color: "#e612151c"
        border.width: 2
        border.color: Theme.ac
        Row {
            id: ghostRow
            anchors.centerIn: parent
            spacing: 8
            Icon { visible: Dnd.icon !== ""; anchors.verticalCenter: parent.verticalCenter; name: Dnd.icon; size: 20; tint: Dnd.tint }
            Text { anchors.verticalCenter: parent.verticalCenter; text: Dnd.label; color: Theme.tx; font.pixelSize: 13 }
        }
    }
    Rectangle {
        id: toastBox
        property bool shown: false
        z: 300
        anchors.horizontalCenter: parent.horizontalCenter
        y: parent.height - (shown ? height + 24 : 0)
        width: Math.min(parent.width - 48, Math.max(220, toastText.implicitWidth + 40))
        height: Math.max(44, toastText.contentHeight + 24)
        radius: 12
        color: Theme.s2
        border.width: 1
        border.color: Theme.line2
        opacity: shown ? 1 : 0
        visible: opacity > 0
        Behavior on y { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
        Behavior on opacity { NumberAnimation { duration: 180 } }
        Text {
            id: toastText
            anchors.centerIn: parent
            width: parent.width - 32
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            maximumLineCount: 4
            elide: Text.ElideRight
            color: Theme.tx
            font.pixelSize: 14
        }
        Timer { id: toastTimer; interval: 3500; onTriggered: toastBox.shown = false }
    }
}
