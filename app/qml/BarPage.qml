// SPDX-License-Identifier: GPL-3.0-or-later
import QtQuick

// The bar drawn large on a 24-column grid: pick a profile, drag items to
// reorder or drop them from the palette, edit the selected one on the right.
Item {
    id: page
    property var app
    property var zone: "default"        // "default" or the profile index
    property var sel: null              // { list: "zone" | "strip", index }
    property bool toStrip: false
    property int tabsRev: 0
    property var indicator: null        // where a drop would land: { list, col, span, ok }

    readonly property var st: app.st
    readonly property int gap: 8
    readonly property bool fullColor: app.get(st.config, "icon_color", "mono") === "full"

    function arr(list) {
        app.rev
        if (list === "strip")
            return st.layout.strip
        if (zone === "default")
            return st.layout["default"]
        var p = st.layout.profile[zone]
        if (!p.items)
            p.items = []
        return p.items
    }

    function spanOf(item) { return Theme.colsOf(item) }

    // Where each item of a list sits, like the daemon: the item's column, else right after the previous one.
    function placeList(items, first) {
        var cursor = first, places = []
        for (var i = 0; i < items.length; i++) {
            var span = spanOf(items[i])
            var at = items[i].col ? Math.max(items[i].col - 1, cursor) : cursor
            places.push({ col: at, span: span })
            cursor = at + span
        }
        return { places: places, end: cursor }
    }
    function layoutAll() {
        var z = arr("zone"), s = arr("strip"), total = 0, i
        for (i = 0; i < s.length; i++) total += spanOf(s[i])
        var zp = placeList(z, 0)
        var sp = placeList(s, Math.max(Theme.gridCols - total, zp.end))
        var cells = []
        for (i = 0; i < z.length; i++) cells.push({ item: z[i], list: "zone", index: i, col: zp.places[i].col, span: zp.places[i].span })
        for (i = 0; i < s.length; i++) cells.push({ item: s[i], list: "strip", index: i, col: sp.places[i].col, span: sp.places[i].span })
        return { cells: cells, columns: Math.max(Theme.gridCols, zp.end, sp.end) }
    }
    readonly property var grid: { app.rev; zone; return layoutAll() }
    readonly property int columns: grid.columns
    readonly property real colW: (track.width - gap * (columns - 1)) / columns
    function colX(col) { return col * (colW + gap) }
    function spanW(span) { return span * colW + (span - 1) * gap }

    // Writes every item's column and width, so later edits can't shift the others.
    function materialize() {
        var g = layoutAll(), i, over = false
        for (i = 0; i < g.cells.length; i++)
            if (g.cells[i].col + g.cells[i].span > Theme.gridCols) over = true
        if (over) {
            // Past the last column the daemon rejects the file: pack each list to the left.
            var ends = { zone: 0, strip: 0 }
            for (i = 0; i < g.cells.length; i++) {
                var c = g.cells[i]
                c.col = Math.max(c.col, ends[c.list])
                if (c.col + c.span > Theme.gridCols) c.col = Theme.gridCols - c.span
                ends[c.list] = c.col + c.span
            }
            app.toast(app.tt("Se reacomodaron elementos para caber en 24 columnas."))
        }
        for (i = 0; i < g.cells.length; i++) {
            g.cells[i].item.col = g.cells[i].col + 1
            g.cells[i].item.span = g.cells[i].span
        }
    }
    function sortLists() {
        var by = (a, b) => (a.col || 0) - (b.col || 0)
        arr("zone").sort(by)
        arr("strip").sort(by)
    }
    // Columns an item may take: free of other items and inside the grid.
    function fits(col, span, skip) {
        if (col < 0 || col + span > columns) return false
        var cells = grid.cells
        for (var i = 0; i < cells.length; i++) {
            if (skip && cells[i].list === skip.list && cells[i].index === skip.index) continue
            if (col < cells[i].col + cells[i].span && cells[i].col < col + span) return false
        }
        return true
    }
    // The valid column nearest to `col`, or -1.
    function nearest(col, span, skip) {
        for (var d = 0; d < columns; d++) {
            if (fits(col - d, span, skip)) return col - d
            if (d > 0 && fits(col + d, span, skip)) return col + d
        }
        return -1
    }

    function selected() {
        if (!sel) return null
        var list = arr(sel.list)
        return sel.index < list.length ? list[sel.index] : null
    }
    function selectedCell() {
        if (!sel) return null
        var cells = grid.cells
        for (var i = 0; i < cells.length; i++)
            if (cells[i].list === sel.list && cells[i].index === sel.index) return cells[i]
        return null
    }
    function labelOf(item) {
        if (item.app) return item.app
        return app.meta.types[item.type] || item.type
    }
    function select(list, index) { sel = { list: list, index: index } }
    function selectItem(item) {
        sortLists()
        var z = arr("zone"), s = arr("strip")
        if (z.indexOf(item) >= 0) select("zone", z.indexOf(item))
        else if (s.indexOf(item) >= 0) select("strip", s.indexOf(item))
    }

    // Puts `item` on the grid at `col` (0-based) with `span` columns, in `list`.
    function place(item, list, col, span, from) {
        materialize()
        if (from) arr(from.list).splice(from.index, 1)
        item.col = col + 1
        item.span = span
        arr(list).push(item)
        sortLists()
        selectItem(item)
        app.bump()
    }

    function addItem(item) {
        var span = Theme.colsOf(item), list = toStrip ? "strip" : "zone", col = -1
        if (toStrip) {
            for (var c = columns - span; c >= 0; c--) if (fits(c, span, null)) { col = c; break }
        } else {
            for (var d = 0; d + span <= columns; d++) if (fits(d, span, null)) { col = d; break }
        }
        if (col < 0) { app.toast(app.tt("No hay espacio libre en la barra.")); return }
        place(item, list, col, span, null)
    }
    function moveSelected(delta) {
        var cell = selectedCell()
        if (!cell) return
        if (!fits(cell.col + delta, cell.span, { list: cell.list, index: cell.index })) return
        var item = arr(cell.list)[cell.index]
        materialize()
        item.col = cell.col + delta + 1
        sortLists()
        selectItem(item)
        app.bump()
    }
    function setPlace(col, span) {
        var cell = selectedCell()
        if (!cell) return false
        var item = arr(cell.list)[cell.index]
        span = Math.max(Theme.minCols(item), span)
        if (!fits(col, span, { list: cell.list, index: cell.index })) { app.toast(app.tt("Esas columnas están ocupadas.")); return false }
        materialize()
        item.col = col + 1
        item.span = span
        sortLists()
        selectItem(item)
        app.bump()
        return true
    }
    function removeSelected() {
        if (!sel) return
        materialize()
        arr(sel.list).splice(sel.index, 1)
        sel = null
        app.bump()
    }

    // Where a drop would land: { list, col, span, ok }.
    function dropInfo(px) {
        var data = Dnd.payload
        if (!data) return null
        var item = data.from ? arr(data.from.list)[data.from.index] : data.item
        var span = Theme.colsOf(item), pitch = colW + gap
        var left = px - data.grabX
        var col = Math.max(0, Math.min(columns - span, Math.round(left / pitch)))
        var good = nearest(col, span, data.from ? data.from : null)
        var list = data.from ? data.from.list : (toStrip ? "strip" : "zone")
        var strip = arr("strip")
        if (data.from && strip.length > 0) {
            var first = grid.cells.filter((c) => c.list === "strip")[0]
            list = (good >= 0 ? good : col) >= first.col ? "strip" : "zone"
        }
        return { list: list, col: good >= 0 ? good : col, span: span, ok: good >= 0 }
    }
    function dropAt(gx, gy) {
        var p = track.mapFromItem(null, gx, gy)
        if (p.y < -50 || p.y > track.height + 50 || p.x < -40 || p.x > track.width + 40)
            return null
        return dropInfo(p.x)
    }
    function commit(info) {
        var data = Dnd.payload
        if (data.from) {
            var item = arr(data.from.list)[data.from.index]
            var moved = data.from.list !== info.list
            place(item, info.list, info.col, info.span, data.from)
            if (moved)
                app.toast(app.tt(info.list === "strip" ? "El elemento pasó al control strip." : "El elemento pasó a la zona de la app."))
        } else {
            place(JSON.parse(JSON.stringify(data.item)), info.list, info.col, info.span, null)
        }
    }

    // Dragging the right edge of the selected chip.
    function resizeTo(cell, gx) {
        var p = track.mapFromItem(null, gx, 0)
        var span = Math.round((p.x - colX(cell.col)) / (colW + gap) + 0.5)
        var item = arr(cell.list)[cell.index]
        span = Math.max(Theme.minCols(item), Math.min(columns - cell.col, span))
        if (span === cell.span || !fits(cell.col, span, { list: cell.list, index: cell.index })) return
        materialize()
        item.col = cell.col + 1
        item.span = span
        app.bump()
    }

    Connections {
        target: Dnd
        function onMoved(x, y) { if (page.visible) page.indicator = page.dropAt(x, y) }
        function onCanceled() { page.indicator = null }
        function onDropped(x, y) {
            if (!page.visible) return
            var info = page.dropAt(x, y)
            page.indicator = null
            if (info && info.ok) page.commit(info)
            else app.bump()
        }
    }

    Shortcut { sequence: "Alt+Left"; enabled: page.visible && !app.modalOpen; onActivated: page.moveSelected(-1) }
    Shortcut { sequence: "Alt+Right"; enabled: page.visible && !app.modalOpen; onActivated: page.moveSelected(1) }
    Shortcut { sequence: "Delete"; enabled: page.visible && !app.modalOpen && page.sel !== null; onActivated: page.removeSelected() }
    Shortcut { sequence: "Escape"; enabled: page.visible && !app.modalOpen && page.sel !== null; onActivated: page.sel = null }

    // ---- stage -------------------------------------------------------------
    Rectangle {
        id: stage
        width: parent.width
        height: stageCol.implicitHeight + 52
        gradient: Gradient {
            GradientStop { position: 0; color: "#222a3d" }
            GradientStop { position: 1; color: "#0a0c10" }
        }
        Column {
            id: stageCol
            x: 28
            y: 26
            width: parent.width - 56
            spacing: 14

            Flow {
                width: parent.width
                spacing: 6
                Repeater {
                    model: {
                        page.tabsRev
                        app.rev
                        var out = [{ key: "default", name: app.tt("Predeterminado") }]
                        var profiles = st.layout.profile
                        for (var i = 0; i < profiles.length; i++) out.push({ key: i, name: profiles[i].name })
                        return out
                    }
                    Btn {
                        kind: "tab"
                        text: modelData.name
                        checked: page.zone === modelData.key
                        onClicked: { page.zone = modelData.key; page.sel = null }
                    }
                }
                Btn {
                    kind: "tab"
                    icon: "plus"
                    Accessible.name: app.tt("Agregar perfil")
                    onClicked: app.newProfile()
                }
            }

            Rectangle {
                width: parent.width
                height: 68
                radius: 12
                color: "#000000"
                border.width: 1
                border.color: Theme.line
                Item {
                    id: track
                    x: 10
                    y: 14
                    width: parent.width - 20
                    height: 40

                    Repeater {
                        model: page.columns
                        Rectangle {
                            x: page.colX(index)
                            width: page.colW
                            height: 40
                            radius: 6
                            color: "transparent"
                            border.width: 1
                            border.color: "#15181f"
                        }
                    }
                    Repeater {
                        // One delegate per cell, kept alive while the grid changes, so a resize drag is not cut short.
                        model: page.grid.cells.length
                        Chip {
                            readonly property var cell: page.grid.cells[index]
                            x: page.colX(cell.col)
                            width: cell.span * page.colW + (cell.span - 1) * page.gap
                            height: 40
                            app: page.app
                            item: cell.item
                            label: page.labelOf(cell.item)
                            strip: cell.list === "strip"
                            fullColor: page.fullColor
                            selected: page.sel !== null && page.sel.list === cell.list && page.sel.index === cell.index
                            ghost: Dnd.active && Dnd.payload && Dnd.payload.from
                                   && Dnd.payload.from.list === cell.list && Dnd.payload.from.index === cell.index
                            onPressedItem: page.select(cell.list, cell.index)
                            onDragStarted: (x, y) => {
                                Dnd.begin(label, iconName, Theme.tint(iconName, page.fullColor),
                                          { from: { list: cell.list, index: cell.index }, grabX: grabX })
                                Dnd.move(x, y)
                            }
                            onResizeMoved: (x) => page.resizeTo(cell, x)
                            onDragMoved: (x, y) => Dnd.move(x, y)
                            onDragEnded: (x, y) => Dnd.end(x, y)
                        }
                    }
                    Rectangle {
                        visible: Dnd.active && page.indicator !== null
                        x: page.indicator ? page.colX(page.indicator.col) : 0
                        y: -4
                        width: page.indicator ? page.spanW(page.indicator.span) : 0
                        height: 48
                        radius: 11
                        color: page.indicator && page.indicator.ok ? "#337aa2ff" : "#33ff6b81"
                        border.width: 2
                        border.color: page.indicator && page.indicator.ok ? Theme.ac : Theme.danger
                        Behavior on x { NumberAnimation { duration: 80; easing.type: Easing.OutCubic } }
                        Behavior on width { NumberAnimation { duration: 80; easing.type: Easing.OutCubic } }
                    }
                }
            }

            Text {
                text: app.tt("Al tocar «Controles»")
                color: Theme.mu
                font.pixelSize: 12
            }
            Rectangle {
                width: parent.width
                height: 68
                radius: 12
                color: "#000000"
                border.width: 1
                border.color: Theme.line
                Row {
                    x: 10
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 16
                    width: parent.width - 20
                    Chip {
                        width: 52
                        height: 40
                        app: page.app
                        item: ({ type: "controls" })
                        fullColor: page.fullColor
                        label: ""
                        enabled: false
                    }
                    Repeater {
                        model: { app.rev; return st.layout.controls.items }
                        Row {
                            readonly property real slot: (parent.width - 52 - 16 * 3) / 3
                            width: slot
                            height: 40
                            spacing: 10
                            readonly property string icn: ({ "volume": "volume-2", "brightness": "sun", "keyboard": "keyboard" })[modelData] || "sliders-horizontal"
                            Icon { anchors.verticalCenter: parent.verticalCenter; name: parent.icn; size: 22; tint: Theme.tint(parent.icn, page.fullColor) }
                            Item {
                                anchors.verticalCenter: parent.verticalCenter
                                width: parent.width - 32
                                height: 20
                                Rectangle { anchors.verticalCenter: parent.verticalCenter; width: parent.width; height: 6; radius: 3; color: Theme.line }
                                Rectangle { anchors.verticalCenter: parent.verticalCenter; width: parent.width * 0.5; height: 6; radius: 3; color: Theme.glow }
                                Rectangle { anchors.verticalCenter: parent.verticalCenter; x: parent.width * 0.5 - 9; width: 18; height: 18; radius: 9; color: Theme.glow }
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- palette + inspector ----------------------------------------------
    Item {
        anchors.top: stage.bottom
        anchors.bottom: parent.bottom
        width: parent.width

        Scroll {
            id: paletteScroll
            anchors.left: parent.left
            anchors.right: inspector.left
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            Column {
                x: 24
                y: 18
                width: paletteScroll.width - 48
                spacing: 12
                bottomPadding: 18
                Heading { text: app.tt("Paleta") }
                Row {
                    spacing: 8
                    Text { anchors.verticalCenter: parent.verticalCenter; text: app.tt("Agregar a:"); color: Theme.tx; font.pixelSize: 14 }
                    Btn { kind: "tab"; text: app.tt("Zona de app"); checked: !page.toStrip; onClicked: page.toStrip = false }
                    Btn { kind: "tab"; text: app.tt("Strip fijo"); checked: page.toStrip; onClicked: page.toStrip = true }
                }
                Flow {
                    width: parent.width
                    spacing: 8
                    Repeater {
                        model: {
                            var out = [], types = app.meta.types, toggles = app.meta.toggles, k
                            for (k in types) {
                                if (["key", "toggle", "spacer", "now_playing"].indexOf(k) >= 0) continue
                                out.push({ item: { type: k }, text: types[k], icon: Theme.chipIcons[k] || "" })
                            }
                            out.push({ item: { type: "now_playing" }, text: types.now_playing, icon: "play" })
                            out.push({ item: { type: "spacer", span: 1 }, text: types.spacer, icon: "move-horizontal" })
                            for (k in toggles)
                                out.push({ item: { type: "toggle", what: k }, text: toggles[k], icon: Theme.toggleIcons[k] || "" })
                            return out
                        }
                        Pill {
                            icon: modelData.icon
                            text: modelData.text
                            tint: Theme.tint(modelData.icon, page.fullColor)
                            onActivated: page.addItem(JSON.parse(JSON.stringify(modelData.item)))
                            onDragStarted: (x, y) => { Dnd.begin(text, icon, tint, { from: null, item: modelData.item, grabX: (Theme.colsOf(modelData.item) * (page.colW + page.gap) - page.gap) / 2 }); Dnd.move(x, y) }
                            onDragMoved: (x, y) => Dnd.move(x, y)
                            onDragEnded: (x, y) => Dnd.end(x, y)
                        }
                    }
                }
                Row {
                    spacing: 8
                    Btn { text: app.tt("Tecla personalizada…"); onClicked: app.editItem(null, (item) => page.addItem(item), true) }
                    Btn { text: app.tt("Aplicación…"); onClicked: app.pickApp((id) => page.addItem({ type: "key", app: id })) }
                }
            }
        }

        Rectangle {
            id: inspector
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            anchors.margins: 18
            width: 340
            radius: 14
            color: Theme.s2
            border.width: 1
            border.color: Theme.line

            Scroll {
                anchors.fill: parent
                anchors.margins: 1
                Column {
                    id: insp
                    x: 20
                    y: 20
                    width: inspector.width - 42
                    spacing: 12
                    bottomPadding: 20
                    readonly property var item: { app.rev; page.sel; return page.selected() }
                    readonly property var profile: { app.rev; return page.zone === "default" ? null : st.layout.profile[page.zone] }

                    Heading { text: app.tt("Inspector") }
                    Text {
                        visible: insp.item === null
                        width: parent.width
                        text: app.tt("Elija un ítem de la barra.")
                        color: Theme.mu
                        font.pixelSize: 14
                        wrapMode: Text.WordWrap
                    }
                    Column {
                        visible: insp.item !== null
                        width: parent.width
                        spacing: 12
                        Text {
                            width: parent.width
                            text: insp.item ? page.labelOf(insp.item) : ""
                            color: Theme.tx
                            font.pixelSize: 17
                            font.weight: Font.DemiBold
                            wrapMode: Text.WordWrap
                        }
                        Text {
                            visible: text !== ""
                            width: parent.width
                            color: Theme.mu
                            font.pixelSize: 13
                            wrapMode: Text.WordWrap
                            text: {
                                var it = insp.item
                                if (!it) return ""
                                if (it.type === "key" && !it.app)
                                    return (it.label || it.icon || "") + " · " + (it.send ? app.tt("atajo") + " " + it.send : app.tt("ejecuta") + " " + (it.run || ""))
                                if (it.type === "toggle") return app.meta.toggles[it.what] || ""
                                return ""
                            }
                        }
                        Flow {
                            width: parent.width
                            spacing: 6
                            Btn { icon: "arrow-left"; Accessible.name: app.tt("Mover a la izquierda"); onClicked: page.moveSelected(-1) }
                            Btn { icon: "arrow-right"; Accessible.name: app.tt("Mover a la derecha"); onClicked: page.moveSelected(1) }
                            Btn {
                                visible: insp.item !== null && (insp.item.type === "key" || insp.item.type === "toggle") && !insp.item.app
                                text: app.tt("Editar")
                                onClicked: {
                                    var list = page.arr(page.sel.list), at = page.sel.index
                                    app.editItem(list[at], (item) => {
                                        var old = list[at], merged = {}, edited = ["label", "icon", "send", "run", "what", "app"]
                                        for (var k in old) if (edited.indexOf(k) < 0) merged[k] = old[k]
                                        list[at] = Object.assign(merged, item)
                                        app.bump()
                                    })
                                }
                            }
                            Btn { icon: "x"; danger: true; text: app.tt("Quitar"); onClicked: page.removeSelected() }
                        }
                        Column {
                            width: parent.width
                            StepRow {
                                title: app.tt("Columna")
                                value: { app.rev; var c = page.selectedCell(); return c ? c.col + 1 : 1 }
                                from: 1; to: Theme.gridCols
                                onMoved: (v) => { if (!page.setPlace(v - 1, page.selectedCell().span)) app.bump() }
                            }
                            StepRow {
                                last: true
                                title: app.tt("Ancho (columnas)")
                                value: { app.rev; var c = page.selectedCell(); return c ? c.span : 1 }
                                from: 1; to: Theme.gridCols
                                onMoved: (v) => { var c = page.selectedCell(); if (!page.setPlace(c.col, v)) app.bump() }
                            }
                        }
                        Column {
                            visible: insp.item !== null && insp.item.type === "controls"
                            width: parent.width
                            Repeater {
                                model: [["volume", "Mostrar el slider de volumen"], ["brightness", "Mostrar el slider de brillo"], ["keyboard", "Mostrar el slider de luz del teclado"]]
                                SwitchRow {
                                    title: app.tt(modelData[1])
                                    checked: app.get(st.layout.controls, "items", []).indexOf(modelData[0]) >= 0
                                    onToggled: (v) => {
                                        var items = st.layout.controls.items, at = items.indexOf(modelData[0])
                                        if (v && at < 0) items.push(modelData[0])
                                        else if (!v && at >= 0) {
                                            if (items.length === 1) { app.bump(); app.toast(app.tt("Deje al menos un slider.")); return }
                                            items.splice(at, 1)
                                        }
                                        app.bump()
                                    }
                                }
                            }
                            StepRow {
                                title: app.tt("Ancho del panel (px, 0 = toda la zona de app)")
                                value: app.get(st.layout.controls, "width_px", 0)
                                from: 0; to: 2000; step: 10
                                onMoved: (v) => { st.layout.controls.width_px = v; app.bump() }
                            }
                            StepRow {
                                last: true
                                title: app.tt("Se cierra tras (ms sin tocar)")
                                value: app.get(st.layout.controls, "timeout_ms", 3000)
                                from: 1000; to: 30000; step: 500
                                onMoved: (v) => { st.layout.controls.timeout_ms = v; app.bump() }
                            }
                        }
                    }

                    Rectangle { width: parent.width; height: 1; color: Theme.line }
                    Heading { text: app.tt("Apariencia") }
                    Row {
                        spacing: 6
                        Btn { kind: "tab"; text: app.tt("Monocromo"); checked: !page.fullColor; onClicked: { st.config.icon_color = "mono"; app.bump() } }
                        Btn { kind: "tab"; text: app.tt("Color completo"); checked: page.fullColor; onClicked: { st.config.icon_color = "full"; app.bump() } }
                    }

                    Column {
                        visible: insp.profile !== null
                        width: parent.width
                        spacing: 10
                        Rectangle { width: parent.width; height: 1; color: Theme.line }
                        Heading { text: app.tt("Perfil") }
                        Text { text: app.tt("Nombre"); color: Theme.mu; font.pixelSize: 12 }
                        Input {
                            width: parent.width
                            text: insp.profile ? insp.profile.name : ""
                            label: app.tt("Nombre")
                            onEdited: (v) => { if (v.trim() !== "") { insp.profile.name = v; page.tabsRev++ } }
                        }
                        Text { text: app.tt("IDs de app (separados por coma)"); color: Theme.mu; font.pixelSize: 12 }
                        Input {
                            id: appsField
                            width: parent.width
                            text: insp.profile ? (insp.profile.apps || []).join(", ") : ""
                            onEdited: (v) => insp.profile.apps = v.split(",").map((a) => a.trim()).filter((a) => a !== "")
                        }
                        Row {
                            spacing: 8
                            Btn {
                                text: app.tt("Elegir app…")
                                onClicked: app.pickApp((id) => {
                                    var cur = (insp.profile.apps || [])
                                    if (cur.indexOf(id) < 0) cur.push(id)
                                    insp.profile.apps = cur
                                    appsField.text = cur.join(", ")
                                })
                            }
                            Btn {
                                danger: true
                                text: app.tt("Quitar perfil")
                                onClicked: {
                                    var index = page.zone
                                    app.confirm(app.tt("¿Quitar el perfil?"), insp.profile.name, app.tt("Quitar perfil"), () => {
                                        st.layout.profile.splice(index, 1)
                                        page.zone = "default"
                                        page.sel = null
                                        app.bump()
                                    })
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
