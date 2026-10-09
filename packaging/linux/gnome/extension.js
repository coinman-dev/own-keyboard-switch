import Gio from 'gi://Gio';
import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import St from 'gi://St';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {getInputSourceManager} from 'resource:///org/gnome/shell/ui/status/keyboard.js';

const INTERFACE = `<node><interface name="org.own_keyboard_switch.Gnome">
<method name="GetState"><arg type="s" direction="out"/></method>
<method name="GetVersion"><arg type="u" direction="out"/></method>
<method name="SetLayout"><arg type="u" direction="in"/></method>
<method name="ActivateWindow"><arg type="t" direction="in"/></method>
<method name="MinimizeWindow"><arg type="t" direction="in"/></method>
<method name="ToggleMaximizeWindow"><arg type="t" direction="in"/></method>
<method name="GetClipboard"><arg type="s" direction="out"/></method>
<method name="SetClipboard"><arg type="s" direction="in"/></method>
<method name="SetPanel"><arg type="s" direction="in"/></method>
<method name="GetPanelGeometry"><arg type="s" direction="in"/><arg type="s" direction="out"/></method>
<method name="TakePanelEvents"><arg type="s" direction="out"/></method>
<method name="PlaceWindow"><arg type="s" direction="in"/></method>
<method name="GetPlacedWindow"><arg type="s" direction="in"/><arg type="s" direction="out"/></method>
</interface></node>`;

export default class OkbSwitchExtension extends Extension {
    enable() {
        this._panels = new Map();
        this._monitorChange = Main.layoutManager.connect('monitors-changed', () => {
            for (const panel of this._panels.values()) this._positionPanel(panel);
            for (const window of this._windowSignals.keys()) this._place(window);
        });
        this._events = [];
        this._placements = new Map();
        this._windowSignals = new Map();
        this._placementState = new Map();
        this._windows = global.display.connect('window-created', (_display, window) => {
            this._watchWindow(window);
        });
        this._focusChange = global.display.connect('notify::focus-window', () => {
            const window = global.display.focus_window;
            if (window) this._place(window);
        });
        this._mapChange = global.window_manager.connect('map', (_manager, actor) => {
            const window = actor.meta_window;
            // Initial compositor placement can happen after window-created
            // and title/size notifications. Apply the pending request after
            // the real actor is mapped, so it is not overwritten by centering.
            GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
                if (this._windowSignals.has(window)) {
                    this._placementState.delete(window);
                    this._place(window);
                }
                return GLib.SOURCE_REMOVE;
            });
        });
        for (const actor of global.get_window_actors()) this._watchWindow(actor.meta_window);
        this._export = Gio.DBusExportedObject.wrapJSObject(INTERFACE, this);
        this._export.export(Gio.DBus.session, '/org/own_keyboard_switch/Gnome');
        this._owner = Gio.bus_own_name_on_connection(Gio.DBus.session,
            'org.own_keyboard_switch.Gnome', Gio.BusNameOwnerFlags.NONE, null, null);
    }
    disable() {
        if (this._windows) global.display.disconnect(this._windows);
        if (this._monitorChange) Main.layoutManager.disconnect(this._monitorChange);
        if (this._focusChange) global.display.disconnect(this._focusChange);
        if (this._mapChange) global.window_manager.disconnect(this._mapChange);
        this._mapChange = 0;
        this._focusChange = 0;
        for (const [window, signals] of this._windowSignals) {
            for (const signal of signals) window.disconnect(signal);
        }
        this._windowSignals.clear();
        this._placementState.clear();
        this._monitorChange = 0;
        this._windows = 0;
        for (const panel of this._panels.values()) panel.destroy();
        this._panels.clear();
        if (this._owner) Gio.bus_unown_name(this._owner);
        this._owner = 0;
        this._export?.unexport();
        this._export = null;
    }
    GetState() {
        const manager = getInputSourceManager();
        // Shell overview and modal dialogs own the keyboard while Mutter can
        // retain a background application's focus_window. Never identify
        // that application as the input target of Shell search/password UI.
        const window = Main.overview.visible || Main.modalCount > 0
            ? null : global.display.focus_window;
        const frame = window?.get_frame_rect();
        const client = window?.get_client_content_rect?.() ?? window?.get_buffer_rect();
        const [x, y] = global.get_pointer();
        const keymap = Clutter.get_default_backend()?.get_default_seat?.()?.get_keymap?.();
        return JSON.stringify({
            protocol:5,
            group: manager.currentSource?.index ?? 0,
            sources: Object.values(manager.inputSources).map(source => ({
                code: source.type === 'xkb' ? source.id : 'ibus', name: source.displayName,
                variant: source.id.includes('+') ? source.id.split('+')[1] : ''
            })),
            window: window?.get_stable_sequence() ?? 0,
            control: 0, pid: window?.get_pid() ?? 0,
            title: window?.get_title() ?? '', app_id: window?.get_wm_class() ?? '',
            cursor: [x, y], locked: Main.sessionMode.isLocked,
            caps: keymap?.get_caps_lock_state?.() ?? null
            ,frame: frame ? [frame.x,frame.y,frame.width,frame.height] : [0,0,0,0],
            client:client ? [client.x,client.y,client.width,client.height] : [0,0,0,0]
        });
    }
    GetVersion() { return 5; }
    SetLayout(index) {
        if (Main.sessionMode.isLocked) throw new Error('Session is locked');
        const source = getInputSourceManager().inputSources[index];
        if (!source) throw new Error('Unknown input source');
        source.activate(false);
    }
    _window(id) {
        if (Main.sessionMode.isLocked) throw new Error('Session is locked');
        if (Main.overview.visible || Main.modalCount > 0)
            throw new Error('Shell owns keyboard focus');
        const window = global.get_window_actors().map(actor => actor.meta_window)
            .find(window => window.get_stable_sequence() === id);
        if (!window) throw new Error('Input window no longer exists');
        return window;
    }
    ActivateWindow(id) { this._window(id).activate(global.get_current_time()); }
    MinimizeWindow(id) { this._window(id).minimize(); }
    ToggleMaximizeWindow(id) {
        const window = this._window(id);
        if (window.get_maximized() === Meta.MaximizeFlags.BOTH)
            window.unmaximize(Meta.MaximizeFlags.BOTH);
        else window.maximize(Meta.MaximizeFlags.BOTH);
    }
    GetClipboardAsync(_parameters, invocation) {
        St.Clipboard.get_default().get_text(St.ClipboardType.CLIPBOARD,
            (_clipboard, text) => invocation.return_value(new GLib.Variant('(s)', [text ?? ''])));
    }
    SetClipboard(text) {
        St.Clipboard.get_default().set_text(St.ClipboardType.CLIPBOARD, text);
    }
    PlaceWindow(text) {
        const request = JSON.parse(text);
        request.created = Date.now();
        this._placements.set(`${request.pid}:${request.title}`, request);
        for (const actor of global.get_window_actors()) this._place(actor.meta_window);
    }
    GetPlacedWindow(text) {
        const request = JSON.parse(text);
        if (!this._placements.has(`${request.pid}:${request.title}`)) return 'null';
        const window = global.get_window_actors().map(actor => actor.meta_window)
            .find(window => window.get_pid() === request.pid && window.get_title() === request.title);
        if (!window) return 'null';
        const rect = window.get_frame_rect();
        return JSON.stringify({pid:request.pid, title:request.title, window:window.get_stable_sequence(),
            frame:[rect.x, rect.y, rect.width, rect.height]});
    }
    _watchWindow(window) {
        if (this._windowSignals.has(window)) return;
        const signals = [window.connect('notify::title', () => this._place(window)),
            window.connect('size-changed', () => this._place(window)),
            window.connect('shown', () => {
                GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
                    if (this._windowSignals.has(window)) this._place(window);
                    return GLib.SOURCE_REMOVE;
                });
            }),
            window.connect('unmanaged', () => {
                this._windowSignals.delete(window);
                this._placementState.delete(window);
            })];
        this._windowSignals.set(window, signals);
        this._place(window);
    }
    _place(window) {
        const request = this._placements.get(`${window.get_pid()}:${window.get_title()}`);
        if (!request || Main.sessionMode.isLocked) return;
        // Clutter's mapped flag can lag behind the valid Meta.Window geometry
        // (and stays false for hidden/off-workspace actors). Positioning does
        // not require the surface to be painted; the map hook reapplies it
        // after the compositor's initial placement.
        if (!window.get_compositor_private()) return;
        const rect = window.get_frame_rect();
        if (rect.width <= 0 || rect.height <= 0) return;
        const previous = this._placementState.get(window);
        const first = previous !== request;
        const point = first ? [request.x, request.y] : [rect.x, rect.y];
        const index = Main.layoutManager.monitors.findIndex(m =>
            point[0] >= m.x && point[0] < m.x + m.width &&
            point[1] >= m.y && point[1] < m.y + m.height);
        const area = index >= 0 ? window.get_work_area_for_monitor(index) : window.get_work_area_current_monitor();
        if (request.passive) {
            if (Date.now() - request.created < 500 && global.display.focus_window === window) {
                const editor = global.get_window_actors().map(actor => actor.meta_window).find(editor => editor.get_stable_sequence() === request.restore);
                editor?.activate(global.get_current_time());
            }
        }
        const width = Math.min(rect.width, area.width), height = Math.min(rect.height, area.height);
        const x = Math.max(area.x, Math.min(point[0], area.x + area.width - width));
        const y = Math.max(area.y, Math.min(point[1], area.y + area.height - height));
        this._placementState.set(window, request);
        if (width !== rect.width || height !== rect.height) window.move_resize_frame(true, x, y, width, height);
        else if (x !== rect.x || y !== rect.y) window.move_frame(true, x, y);
        window.make_above();
    }
    TakePanelEvents() { const events = this._events; this._events = []; return JSON.stringify(events); }
    GetPanelGeometry(id) {
        const panel = this._panels.get(id);
        return JSON.stringify({panel:panel ? {x:panel.x,y:panel.y,width:panel.width,height:panel.height} : null,
            monitors:Main.layoutManager.monitors.map((monitor, index) => ({x:monitor.x,y:monitor.y,
                width:monitor.width,height:monitor.height,scale:global.display.get_monitor_scale(index)}))});
    }
    _positionPanel(panel) {
        const requested = panel._requestedPosition;
        const monitor = Main.layoutManager.monitors.find(m => requested[0] >= m.x && requested[0] < m.x + m.width &&
            requested[1] >= m.y && requested[1] < m.y + m.height) ?? Main.layoutManager.primaryMonitor;
        if (!monitor) return;
        panel.set_position(Math.max(monitor.x, Math.min(requested[0], monitor.x + monitor.width - panel.width)),
            Math.max(monitor.y, Math.min(requested[1], monitor.y + monitor.height - panel.height)));
    }
    SetPanel(text) {
        const model = JSON.parse(text);
        this._panels.get(model.id)?.destroy();
        this._panels.delete(model.id);
        if (!model.visible || Main.sessionMode.isLocked) return;
        const colors = model.theme === 'light' ? 'background-color: rgba(245,245,245,0.95); color: #202020;' : 'background-color: rgba(35,35,35,0.95); color: white;';
        const panel = new St.BoxLayout({vertical: true, reactive: true, style_class: 'popup-menu-content',
            style: `padding: 6px; border-radius: 6px; ${colors}`});
        if (model.icon?.length) {
            const icon = new St.Icon({gicon: Gio.BytesIcon.new(new GLib.Bytes(Uint8Array.from(model.icon))), icon_size: 32});
            panel.add_child(icon);
        }
        if (model.text) panel.add_child(new St.Label({text: model.text}));
        const rows = new St.BoxLayout({vertical:true});
        for (const [index, row] of (model.rows ?? []).entries()) {
            const button = new St.Button({label: row, style_class: 'popup-menu-item', can_focus: false});
            button.connect('clicked', () => this._events.push({id:model.id, action:'insert', index}));
            rows.add_child(button);
        }
        if (model.rows?.length) {
            const scroll = new St.ScrollView({style:'max-height: 320px; min-width: 320px;'});
            if (scroll.set_child) scroll.set_child(rows); else scroll.add_actor(rows);
            panel.add_child(scroll);
        }
        if (model.id === 'indicator') {
            panel.connect('button-press-event', (_actor, event) => {
                if (event.get_click_count() >= 2) this._events.push({id:model.id, action:'settings'});
                if (event.get_button() === 3) {
                    for (const [label, action] of [[model.settings,'settings'], [model.lock,'lock'], [model.hide,'hide']]) {
                        const button = new St.Button({label, style_class:'popup-menu-item'});
                        button.connect('clicked', () => this._events.push({id:model.id, action}));
                        panel.add_child(button);
                    }
                    return Clutter.EVENT_STOP;
                }
                if (event.get_button() === 1 && !model.locked) {
                    const [startX, startY] = event.get_coords();
                    const origin = [panel.x, panel.y];
                    if (panel._dragSignal) global.stage.disconnect(panel._dragSignal);
                    panel._dragSignal = global.stage.connect('captured-event', (_stage, motion) => {
                        if (motion.type() === Clutter.EventType.MOTION) {
                            const [x, y] = motion.get_coords();
                            panel.set_position(origin[0] + x - startX, origin[1] + y - startY);
                            return Clutter.EVENT_STOP;
                        }
                        if (motion.type() === Clutter.EventType.BUTTON_RELEASE) {
                            global.stage.disconnect(panel._dragSignal); panel._dragSignal = 0;
                            panel._requestedPosition = [panel.x, panel.y];
                            this._positionPanel(panel);
                            this._events.push({id:model.id, action:'moved', position:[Math.round(panel.x), Math.round(panel.y)]});
                            return Clutter.EVENT_STOP;
                        }
                        return Clutter.EVENT_PROPAGATE;
                    });
                }
                return Clutter.EVENT_PROPAGATE;
            });
            panel.connect('destroy', () => { if (panel._dragSignal) global.stage.disconnect(panel._dragSignal); panel._dragSignal = 0; });
        }
        panel.opacity = Math.round(Math.max(0.1, Math.min(model.opacity || 1, 1)) * 255);
        panel._requestedPosition = model.position;
        for (const property of ['width', 'height']) {
            panel.connect(`notify::${property}`, () => {
                if (this._panels.get(model.id) === panel) this._positionPanel(panel);
            });
        }
        Main.layoutManager.addChrome(panel, {trackFullscreen:true});
        this._positionPanel(panel);
        GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
            if (this._panels.get(model.id) === panel) this._positionPanel(panel);
            return GLib.SOURCE_REMOVE;
        });
        this._panels.set(model.id, panel);
    }
}
