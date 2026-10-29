// SPDX-License-Identifier: GPL-3.0-or-later

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension, InjectionManager} from 'resource:///org/gnome/shell/extensions/extension.js';

const NAME = 'org.kait2en.TouchbarPlus.Shell';
const PATH = '/org/kait2en/TouchbarPlus/Shell';
const XML = `
<node>
  <interface name="${NAME}">
    <signal name="FocusChanged"><arg type="s" name="app_id"/></signal>
    <signal name="Notification">
      <arg type="s" name="app"/><arg type="s" name="title"/><arg type="s" name="body"/>
    </signal>
    <signal name="Suggestions"><arg type="as" name="items"/></signal>
    <signal name="SuggestionsHidden"/>
    <method name="GetFocus"><arg type="s" direction="out" name="app_id"/></method>
    <method name="ListWindows"><arg type="a(uss)" direction="out" name="windows"/></method>
    <method name="ListApps"><arg type="a(ssa(us))" direction="out" name="apps"/></method>
    <method name="ActivateWindow"><arg type="u" direction="in" name="id"/></method>
    <method name="SelectSuggestion"><arg type="u" direction="in" name="index"/></method>
    <method name="InsertText"><arg type="s" direction="in" name="text"/></method>
  </interface>
</node>`;

export default class TouchbarPlusExtension extends Extension {
    enable() {
        this._handlers = [];
        this._sources = new Map();
        this._panelHandlers = [];
        this._panelService = null;
        this._dbus = Gio.DBusExportedObject.wrapJSObject(XML, this);
        this._dbus.export(Gio.DBus.session, PATH);
        this._ownerId = Gio.bus_own_name(
            Gio.BusType.SESSION, NAME, Gio.BusNameOwnerFlags.NONE, null,
            // The first FocusChanged left before the daemon could hear the name.
            () => this._emitFocus(), null);

        this._tracker = Shell.WindowTracker.get_default();
        this._connect(global.display, 'notify::focus-window', () => this._emitFocus());
        this._emitFocus();

        this._watchNotifications();
        this._watchSuggestions();
    }

    disable() {
        for (const [object, id] of this._handlers)
            object.disconnect(id);
        this._handlers = [];
        for (const [source, id] of this._sources ?? []) {
            try {
                source.disconnect(id);
            } catch (_error) {
                // The source may already be gone.
            }
        }
        this._sources = null;
        this._unhookPanel();
        this._injections?.clear();
        this._injections = null;
        if (this._ownerId) {
            Gio.bus_unown_name(this._ownerId);
            this._ownerId = 0;
        }
        this._dbus?.unexport();
        this._dbus = null;
        this._tracker = null;
    }

    _connect(object, signal, callback) {
        this._handlers.push([object, object.connect(signal, callback)]);
    }

    _emit(name, variant) {
        try {
            this._dbus?.emit_signal(name, variant);
        } catch (error) {
            console.error(`touchbar-plus: emit ${name}: ${error}`);
        }
    }

    _appId(win) {
        return win ? this._tracker.get_window_app(win)?.get_id() ?? '' : '';
    }

    _emitFocus() {
        this._emit('FocusChanged',
            GLib.Variant.new('(s)', [this._appId(global.display.focus_window)]));
    }

    _watchNotifications() {
        // A source re-added by the shell is hooked once, and forgotten when removed.
        const hook = source => {
            if (this._sources.has(source))
                return;
            this._sources.set(source, source.connect('notification-added', (_source, notification) => {
                this._emit('Notification', GLib.Variant.new('(sss)', [
                    source.title ?? '',
                    notification.title ?? '',
                    notification.body ?? '',
                ]));
            }));
        };
        this._connect(Main.messageTray, 'source-added', (_tray, source) => hook(source));
        this._connect(Main.messageTray, 'source-removed', (_tray, source) => {
            const id = this._sources.get(source);
            if (id === undefined)
                return;
            this._sources.delete(source);
            try {
                source.disconnect(id);
            } catch (_error) {
                // Already destroyed.
            }
        });
        for (const source of Main.messageTray.getSources())
            hook(source);
    }

    async _watchSuggestions() {
        // Private shell API: when it is missing only this feature goes away.
        try {
            const {getIBusManager} = await import('resource:///org/gnome/shell/misc/ibusManager.js');
            if (!this._handlers)
                return;
            const manager = getIBusManager();
            this._injections = new InjectionManager();
            const extension = this;
            const popup = manager._candidatePopup;
            this._injections.overrideMethod(popup, 'setPanelService', original =>
                function (...args) {
                    original.apply(this, args);
                    extension._hookPanel(args[0]);
                });
            if (manager._panelService)
                this._hookPanel(manager._panelService);
        } catch (error) {
            console.error(`touchbar-plus: suggestions unavailable: ${error}`);
        }
    }

    _unhookPanel() {
        for (const id of this._panelHandlers)
            this._panelService?.disconnect(id);
        this._panelHandlers = [];
        this._panelService = null;
    }

    _hookPanel(panelService) {
        this._unhookPanel();
        if (!panelService)
            return;
        this._panelService = panelService;
        this._panelHandlers.push(
            panelService.connect('update-lookup-table', (_service, table, visible) => {
                try {
                    const size = table.get_page_size();
                    const count = table.get_number_of_candidates();
                    const start = Math.floor(table.get_cursor_pos() / size) * size;
                    const items = [];
                    for (let i = start; i < Math.min(start + size, count); i++)
                        items.push(table.get_candidate(i).get_text());
                    if (visible && items.length > 0)
                        this._emit('Suggestions', GLib.Variant.new('(as)', [items]));
                    else
                        this._emit('SuggestionsHidden', null);
                } catch (error) {
                    console.error(`touchbar-plus: lookup table: ${error}`);
                }
            }),
            panelService.connect('hide-lookup-table', () => {
                this._emit('SuggestionsHidden', null);
            }));
    }

    GetFocus() {
        return GLib.Variant.new('(s)', [this._appId(global.display.focus_window)]);
    }

    ListWindows() {
        const windows = [];
        for (const actor of global.get_window_actors()) {
            const win = actor.meta_window;
            if (win.get_window_type() !== Meta.WindowType.NORMAL || win.skip_taskbar)
                continue;
            windows.push([win.get_stable_sequence(), this._appId(win), win.get_title() ?? '']);
        }
        return GLib.Variant.new('(a(uss))', [windows]);
    }

    /** Running apps, most recently used first, each with its windows. */
    ListApps() {
        const normal = win => win.get_window_type() === Meta.WindowType.NORMAL && !win.skip_taskbar;
        let windows;
        if (typeof global.display.get_tab_list === 'function') {
            // The shell's own switcher: every workspace unless it is set to the current one.
            const current = new Gio.Settings({schema_id: 'org.gnome.shell.app-switcher'})
                .get_boolean('current-workspace-only');
            const workspace = current ? global.workspace_manager.get_active_workspace() : null;
            windows = global.display.get_tab_list(Meta.TabList.NORMAL_ALL, workspace)
                // A dialog stands for the window it belongs to.
                .map(win => (win.is_attached_dialog() ? win.get_transient_for() ?? win : win));
        } else {
            windows = global.get_window_actors().map(actor => actor.meta_window)
                .sort((a, b) => b.get_user_time() - a.get_user_time());
        }
        const apps = new Map();
        for (const win of windows.filter(normal)) {
            const app = this._tracker.get_window_app(win);
            if (!app)
                continue;
            const id = app.get_id() ?? '';
            if (!apps.has(id))
                apps.set(id, [id, app.get_name() ?? id, []]);
            apps.get(id)[2].push([win.get_stable_sequence(), win.get_title() ?? '']);
        }
        return GLib.Variant.new('(a(ssa(us)))', [[...apps.values()]]);
    }

    ActivateWindow(id) {
        for (const actor of global.get_window_actors()) {
            if (actor.meta_window.get_stable_sequence() === id) {
                actor.meta_window.activate(global.get_current_time());
                return;
            }
        }
    }

    SelectSuggestion(index) {
        // The index is relative to the visible page, as in the shell popup.
        this._panelService?.candidate_clicked(index, 1, 0);
    }

    InsertText(text) {
        St.Clipboard.get_default().set_text(St.ClipboardType.CLIPBOARD, text);
    }
}
