// Window IDs belong to this script instance and are never reused.
const windows = new Map();
const placements = new Map();
const placementState = new Map();
// KWin uses QRectF/QPointF, including for windows on an unscaled output.
// Snapshot coordinates are desktop pixels; retain the fractional geometry
// for actual placement, but serialize rounded numbers for the common API.
function geometry(rect) {
    return rect ? [rect.x, rect.y, rect.width, rect.height].map(Math.round) : [0,0,0,0];
}
let nextId = Math.floor(Math.random() * 1048576) * 16777216;
function windowId(window) {
    if (!window) return 0;
    if (!windows.has(window.internalId.toString()))
        windows.set(window.internalId.toString(), ++nextId);
    return windows.get(window.internalId.toString());
}
function place(window) {
    const request = placements.get(`${window.pid}:${window.caption}`);
    if (!request) return;
    const rect = window.frameGeometry;
    if (rect.width <= 0 || rect.height <= 0) return;
    const first = placementState.get(window.internalId.toString()) !== request;
    const point = first ? {x:request.x, y:request.y} : {x:rect.x, y:rect.y};
    const output = workspace.screenAt(point);
    const area = output ? workspace.clientArea(0, output, workspace.currentDesktop)
        : workspace.clientArea(0, window);
    const width = Math.min(rect.width, area.width), height = Math.min(rect.height, area.height);
    const x = Math.max(area.x, Math.min(point.x, area.x + area.width - width));
    const y = Math.max(area.y, Math.min(point.y, area.y + area.height - height));
    placementState.set(window.internalId.toString(), request);
    if (x !== rect.x || y !== rect.y || width !== rect.width || height !== rect.height)
        window.frameGeometry = {x, y, width, height};
    window.keepAbove = true;
    if (request.passive) {
        window.skipTaskbar = true;
        if (Date.now() - request.created < 500 && workspace.activeWindow === window) {
            const editor = workspace.windowList().find(editor => windowId(editor) === request.restore);
            if (editor) workspace.activeWindow = editor;
        }
    }
}
const watched = new Set();
function watch(window) {
    const id = window.internalId.toString();
    if (watched.has(id)) return;
    watched.add(id);
    window.captionChanged.connect(() => place(window));
    window.frameGeometryChanged.connect(() => place(window));
    place(window);
}
workspace.windowAdded.connect(watch);
workspace.windowList().forEach(watch);
workspace.windowRemoved.connect(window => {
    windows.delete(window.internalId.toString());
    placementState.delete(window.internalId.toString());
    watched.delete(window.internalId.toString());
});
workspace.screensChanged.connect(() => workspace.windowList().forEach(place));
workspace.windowActivated.connect(window => { if (window) place(window); });
function pulse() {
    const window = workspace.activeWindow;
    const cursor = workspace.cursorPos;
    const client = window ? window.clientGeometry : null;
    const placed = workspace.windowList().filter(w => placements.has(`${w.pid}:${w.caption}`)).map(w => {
        const rect = w.frameGeometry;
        return {pid:w.pid, title:w.caption, window:windowId(w), frame:geometry(rect)};
    });
    const state = JSON.stringify({protocol:4, placed_windows:placed, window:windowId(window), pid:window ? window.pid : 0,
        title:window ? window.caption : '', app_id:window ? window.resourceClass.toString() : '',
        cursor:cursor ? [Math.round(cursor.x),Math.round(cursor.y)] : [0,0], frame:geometry(window?.frameGeometry),
        client:geometry(client)});
    callDBus('org.own_keyboard_switch.Kde', '/org/own_keyboard_switch/Kde',
        'org.own_keyboard_switch.Kde', 'UpdateState', state, function () {
            callDBus('org.own_keyboard_switch.Kde', '/org/own_keyboard_switch/Kde',
                'org.own_keyboard_switch.Kde', 'TakeCommand', function (command) {
                    if (command) {
                        const action = JSON.parse(command);
                        if (action.operation === 'PlaceWindow') {
                            action.created = Date.now();
                            placements.set(`${action.pid}:${action.title}`, action);
                            workspace.windowList().forEach(place);
                        } else {
                            const target = workspace.windowList().find(window => windowId(window) === action.window);
                            if (target) {
                                if (action.operation === 'ActivateWindow') workspace.activeWindow = target;
                                if (action.operation === 'MinimizeWindow') target.minimized = true;
                                if (action.operation === 'ToggleMaximizeWindow') {
                                    workspace.activeWindow = target;
                                    workspace.slotWindowMaximize();
                                }
                            }
                        }
                    }
                    pulse();
                });
        });
}
pulse();
