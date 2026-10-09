//! Desktop-specific layout and focus operations. Wayland never falls back to
//! XWayland: its X11 focus and group would describe only some applications.
use crate::session::{Desktop, SessionInfo};
use crate::watch::Watch;
use okbs_core::{KeyMap, Lang, PhysKey};
use okbs_platform::{
    FocusEvent, FocusInfo, InputTarget, LayoutId, LayoutInfo, LayoutManager, PlatformError, Result,
    StopGuard, WindowControl, WindowInfo,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use x11rb::{
    connection::Connection,
    protocol::{
        xkb,
        xproto::{self, ConnectionExt},
    },
    rust_connection::RustConnection,
};

pub(crate) fn error(e: impl std::fmt::Display) -> PlatformError {
    PlatformError::Other(e.to_string())
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub variant: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub protocol: u32,
    #[serde(default)]
    pub group: u32,
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(default)]
    pub window: u64,
    #[serde(default)]
    pub control: u64,
    #[serde(default)]
    pub pid: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub cursor: [i32; 2],
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub caps: Option<bool>,
    #[serde(default)]
    pub frame: [i32; 4],
    #[serde(default)]
    pub client: [i32; 4],
    #[serde(default)]
    pub session_inactive: bool,
    #[serde(default)]
    pub placed_windows: Vec<PlacedWindow>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlacedWindow {
    pub pid: u32,
    pub title: String,
    pub window: u64,
    pub frame: [i32; 4],
}
#[derive(Debug)]
enum Backend {
    X11(Box<RustConnection>, u32),
    Gnome(zbus::blocking::Connection),
    Kde(zbus::blocking::Connection),
}
pub const INTEGRATION_PROTOCOL_VERSION: u32 = 5;
pub const KDE_INTEGRATION_PROTOCOL_VERSION: u32 = 5;
#[derive(Debug, Clone)]
pub struct LinuxDesktop {
    backend: Arc<Mutex<Backend>>,
    snapshot: Arc<Mutex<Snapshot>>,
    accessibility: Arc<crate::accessibility::Accessibility>,
    seat: Arc<Mutex<crate::seat::Seat>>,
    menu_keys: Arc<Mutex<MenuKeys>>,
    setting: Arc<Mutex<okbs_core::config::LayoutBackend>>,
}
/// A checked connection, held separately until the input gate is invalidated.
#[derive(Debug)]
pub struct PreparedLayout {
    desktop: LinuxDesktop,
    setting: okbs_core::config::LayoutBackend,
}
#[derive(Debug, Default)]
struct MenuKeys {
    left: bool,
    right: bool,
    released: Option<Instant>,
    armed: bool,
    cancelled: Option<Instant>,
    opened_at: Option<Instant>,
}
impl LinuxDesktop {
    #[cfg(test)]
    pub(crate) fn place_fixture_window(
        &self,
        pid: u32,
        title: &str,
        point: [i32; 2],
    ) -> Result<()> {
        let request = serde_json::json!({"operation":"PlaceWindow", "pid":pid, "title":title,
            "x":point[0], "y":point[1], "passive":false});
        if self.is_gnome() {
            return self.gnome_request("PlaceWindow", &request.to_string());
        }
        let backend = self.backend.lock().map_err(error)?;
        match &*backend {
            Backend::Kde(_) => {
                bridge_commands()
                    .lock()
                    .map_err(error)?
                    .push_back(request.to_string());
                Ok(())
            }
            _ => Err(PlatformError::Unsupported(
                "native Wayland fixture placement",
            )),
        }
    }
    #[cfg(test)]
    pub(crate) fn fixture_window(&self, pid: u32, title: &str) -> Result<Option<PlacedWindow>> {
        let backend = self.backend.lock().map_err(error)?;
        match &*backend {
            Backend::Gnome(bus) => {
                let proxy = zbus::blocking::Proxy::new(
                    bus,
                    "org.own_keyboard_switch.Gnome",
                    "/org/own_keyboard_switch/Gnome",
                    "org.own_keyboard_switch.Gnome",
                )
                .map_err(error)?;
                let reply: String = proxy
                    .call(
                        "GetPlacedWindow",
                        &(serde_json::json!({"pid":pid,"title":title}).to_string(),),
                    )
                    .map_err(error)?;
                serde_json::from_str(&reply).map_err(error)
            }
            Backend::Kde(_) => Ok(kde_snapshot()
                .placed_windows
                .into_iter()
                .find(|w| w.pid == pid && w.title == title)),
            _ => Err(PlatformError::Unsupported(
                "native Wayland fixture geometry",
            )),
        }
    }
    pub fn effective_backend(&self) -> Result<okbs_core::config::LayoutBackend> {
        use okbs_core::config::LayoutBackend;
        Ok(match &*self.backend.lock().map_err(error)? {
            Backend::X11(_, _) => LayoutBackend::X11,
            Backend::Kde(_) => LayoutBackend::Kde,
            Backend::Gnome(_) => LayoutBackend::GnomeExtension,
        })
    }
    pub fn prepare_layout(
        &self,
        config: &okbs_core::config::Config,
    ) -> Result<Option<PreparedLayout>> {
        let setting = config.linux.layout_backend;
        if *self.setting.lock().map_err(error)? == setting {
            return Ok(None);
        }
        let desktop = Self::connect_with(setting)?;
        if !desktop.integration_ready() {
            return Err(error("desktop window integration is unavailable"));
        }
        let state = desktop.refresh()?;
        if state.locked || state.session_inactive {
            return Err(error(
                "layout backend requires an active unlocked graphical session",
            ));
        }
        let layouts = desktop.layouts()?;
        if !config
            .general
            .language_pair
            .iter()
            .all(|language| layouts.iter().any(|layout| layout.lang == Some(*language)))
        {
            return Err(error(
                "both configured languages must be installed in the desktop",
            ));
        }
        Ok(Some(PreparedLayout { desktop, setting }))
    }
    pub fn apply_layout(&self, prepared: PreparedLayout) -> Result<()> {
        let state = prepared.desktop.cached();
        let mut ours = self.backend.lock().map_err(error)?;
        let mut theirs = prepared.desktop.backend.lock().map_err(error)?;
        let mut snapshot = self.snapshot.lock().map_err(error)?;
        let mut setting = self.setting.lock().map_err(error)?;
        std::mem::swap(&mut *ours, &mut *theirs);
        *snapshot = state;
        *setting = prepared.setting;
        self.reset_menu_keys();
        Ok(())
    }
    pub fn integration_ready(&self) -> bool {
        let Ok(backend) = self.backend.lock() else {
            return false;
        };
        match &*backend {
            Backend::Gnome(bus) => zbus::blocking::Proxy::new(
                bus,
                "org.own_keyboard_switch.Gnome",
                "/org/own_keyboard_switch/Gnome",
                "org.own_keyboard_switch.Gnome",
            )
            .ok()
            .and_then(|proxy| proxy.call::<_, _, u32>("GetVersion", &()).ok())
            .is_some_and(|version| version >= INTEGRATION_PROTOCOL_VERSION),
            Backend::Kde(_) => KDE_STATE.lock().is_ok_and(|state| {
                state
                    .1
                    .is_some_and(|time| time.elapsed() < Duration::from_millis(500))
                    && state.0.protocol >= KDE_INTEGRATION_PROTOCOL_VERSION
            }),
            Backend::X11(_, _) => true,
        }
    }
    pub(crate) fn enable_menu_access(&self, enabled: bool) {
        self.accessibility.enable_menus(enabled);
        if !enabled
            && let Ok(mut keys) = self.menu_keys.lock()
            && keys.armed
        {
            keys.cancelled = keys.opened_at;
            keys.armed = false;
        }
    }
    fn take_cancelled_menu(&self) -> Option<Instant> {
        self.menu_keys
            .lock()
            .ok()
            .and_then(|mut keys| keys.cancelled.take())
    }
    pub(crate) fn reset_menu_keys(&self) {
        if let Ok(mut state) = self.menu_keys.lock() {
            *state = MenuKeys::default();
        }
    }
    pub(crate) fn note_menu_key(&self, key: PhysKey, pressed: bool) {
        if !matches!(key, PhysKey::AltLeft | PhysKey::AltRight) {
            return;
        }
        if let Ok(mut state) = self.menu_keys.lock() {
            let was_pressed = state.left || state.right;
            if key == PhysKey::AltLeft {
                state.left = pressed;
            } else {
                state.right = pressed;
            }
            if pressed {
                state.released = None;
                if !was_pressed {
                    state.armed = false;
                }
            } else if was_pressed && !state.left && !state.right {
                state.released = Some(Instant::now());
            }
        }
    }
    fn menu_opened_at(&self) -> Option<Instant> {
        self.menu_keys.lock().ok().and_then(|state| state.opened_at)
    }
    fn menu_closed(
        &self,
        open: bool,
        observed_open: bool,
        observed_time: Option<Instant>,
    ) -> Option<Instant> {
        let Ok(mut state) = self.menu_keys.lock() else {
            return None;
        };
        if observed_open && !open {
            if observed_time == state.opened_at {
                state.armed = false;
                state.released = None;
            }
            return observed_time;
        }
        if open {
            return None;
        }
        if state.armed
            && state
                .released
                .is_some_and(|time| time.elapsed() >= Duration::from_millis(250))
        {
            state.armed = false;
            state.released = None;
            return state.opened_at;
        }
        None
    }
    #[cfg(test)]
    pub(crate) fn set_test_session_inactive(&self, inactive: bool) {
        self.snapshot.lock().unwrap().session_inactive = inactive;
    }
    #[cfg(test)]
    pub(crate) fn set_test_session_locked(&self, locked: bool) {
        self.snapshot.lock().unwrap().locked = locked;
    }
    pub fn is_gnome(&self) -> bool {
        self.backend
            .lock()
            .is_ok_and(|backend| matches!(*backend, Backend::Gnome(_)))
    }
    pub fn gnome_request(&self, method: &str, data: &str) -> Result<()> {
        let backend = self.backend.lock().map_err(error)?;
        let Backend::Gnome(bus) = &*backend else {
            return Err(error("GNOME panel service is unavailable"));
        };
        let proxy = zbus::blocking::Proxy::new(
            bus,
            "org.own_keyboard_switch.Gnome",
            "/org/own_keyboard_switch/Gnome",
            "org.own_keyboard_switch.Gnome",
        )
        .map_err(error)?;
        proxy.call::<_, _, ()>(method, &(data,)).map_err(error)
    }
    pub fn gnome_events(&self) -> Result<Vec<crate::panels::PanelEvent>> {
        let backend = self.backend.lock().map_err(error)?;
        let Backend::Gnome(bus) = &*backend else {
            return Ok(Vec::new());
        };
        let proxy = zbus::blocking::Proxy::new(
            bus,
            "org.own_keyboard_switch.Gnome",
            "/org/own_keyboard_switch/Gnome",
            "org.own_keyboard_switch.Gnome",
        )
        .map_err(error)?;
        let events: String = proxy.call("TakePanelEvents", &()).map_err(error)?;
        serde_json::from_str(&events).map_err(error)
    }
    pub fn place_owned_window(&self, title: &str, position: [f32; 2]) -> Result<()> {
        self.place_window(title, position, false)
    }
    pub fn place_passive_window(&self, title: &str, position: [f32; 2]) -> Result<()> {
        self.place_window(title, position, true)
    }
    /// Geometry of a window for which this process requested placement. No
    /// editor contents or unrelated application windows are queried.
    pub fn placed_window(&self, title: &str) -> Result<Option<PlacedWindow>> {
        let backend = self.backend.lock().map_err(error)?;
        match &*backend {
            Backend::Gnome(bus) => {
                let proxy = zbus::blocking::Proxy::new(
                    bus,
                    "org.own_keyboard_switch.Gnome",
                    "/org/own_keyboard_switch/Gnome",
                    "org.own_keyboard_switch.Gnome",
                )
                .map_err(error)?;
                let request =
                    serde_json::json!({"pid":std::process::id(), "title":title}).to_string();
                let reply: String = proxy.call("GetPlacedWindow", &(request,)).map_err(error)?;
                serde_json::from_str(&reply).map_err(error)
            }
            Backend::Kde(_) => Ok(kde_snapshot()
                .placed_windows
                .into_iter()
                .find(|window| window.pid == std::process::id() && window.title == title)),
            Backend::X11(_, _) => Err(PlatformError::Unsupported("Wayland placed window geometry")),
        }
    }
    fn place_window(&self, title: &str, position: [f32; 2], passive: bool) -> Result<()> {
        let request = serde_json::json!({"operation":"PlaceWindow", "pid":std::process::id(), "title":title, "x":position[0] as i32, "y":position[1] as i32,"passive":passive,"restore":self.cached().window});
        if self.is_gnome() {
            return self.gnome_request("PlaceWindow", &request.to_string());
        }
        let backend = self.backend.lock().map_err(error)?;
        match &*backend {
            Backend::Kde(_) => {
                bridge_commands()
                    .lock()
                    .map_err(error)?
                    .push_back(request.to_string());
                Ok(())
            }
            Backend::X11(conn, root) => {
                crate::x11_windows::place(conn, *root, title, Some(position)).map(|_| ())
            }
            Backend::Gnome(_) => Ok(()),
        }
    }
    pub fn connect() -> Result<Self> {
        Self::connect_with(okbs_core::config::LayoutBackend::Auto)
    }
    pub fn connect_with(setting: okbs_core::config::LayoutBackend) -> Result<Self> {
        use okbs_core::config::LayoutBackend;
        let session = SessionInfo::detect();
        let resolved = crate::session::validate_layout_backend(setting, &session)?;
        let use_x11 = resolved == LayoutBackend::X11;
        let backend = if use_x11 {
            let (connection, screen) = x11rb::connect(None).map_err(error)?;
            let root = connection.setup().roots[screen].root;
            xkb::use_extension(&connection, 1, 0)
                .map_err(error)?
                .reply()
                .map_err(error)?;
            Backend::X11(Box::new(connection), root)
        } else {
            let bus = zbus::blocking::connection::Builder::session()
                .map_err(error)?
                .method_timeout(Duration::from_millis(300))
                .build()
                .map_err(error)?;
            let desktop = match resolved {
                LayoutBackend::Kde => Desktop::Kde,
                LayoutBackend::GnomeExtension => Desktop::Gnome,
                _ => session.desktop,
            };
            match desktop {
                Desktop::Gnome => Backend::Gnome(bus),
                Desktop::Kde => Backend::Kde(bus),
                _ => {
                    return Err(PlatformError::Unsupported(
                        "Wayland desktop requires GNOME or KDE integration",
                    ));
                }
            }
        };
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let accessibility = Arc::new(crate::accessibility::Accessibility::spawn(Arc::downgrade(
            &snapshot,
        )));
        let desktop = Self {
            backend: Arc::new(Mutex::new(backend)),
            snapshot,
            accessibility,
            seat: Arc::new(Mutex::new(crate::seat::Seat::connect())),
            menu_keys: Arc::new(Mutex::new(MenuKeys::default())),
            setting: Arc::new(Mutex::new(setting)),
        };
        desktop.refresh()?;
        Ok(desktop)
    }
    pub fn cached(&self) -> Snapshot {
        self.snapshot.lock().map(|s| s.clone()).unwrap_or_default()
    }
    pub fn refresh(&self) -> Result<Snapshot> {
        match self.read_snapshot() {
            Ok(mut snapshot) => {
                let seat = self.seat.lock().map_err(error)?.state();
                snapshot.session_inactive = !seat.active;
                snapshot.locked |= seat.locked;
                if let Some(focus) = self.accessibility.current(snapshot.window, snapshot.pid) {
                    snapshot.control = focus.id;
                }
                *self.snapshot.lock().map_err(error)? = snapshot.clone();
                Ok(snapshot)
            }
            Err(err) => {
                if let Ok(mut snapshot) = self.snapshot.lock() {
                    snapshot.locked = true;
                    snapshot.window = 0;
                    snapshot.control = 0;
                    snapshot.pid = 0;
                }
                Err(err)
            }
        }
    }
    fn read_snapshot(&self) -> Result<Snapshot> {
        let backend = self.backend.lock().map_err(error)?;
        let snapshot = match &*backend {
            Backend::X11(conn, root) => x11_snapshot(conn, *root)?,
            Backend::Gnome(bus) => {
                let proxy = zbus::blocking::Proxy::new(
                    bus,
                    "org.own_keyboard_switch.Gnome",
                    "/org/own_keyboard_switch/Gnome",
                    "org.own_keyboard_switch.Gnome",
                )
                .map_err(error)?;
                let text: String = proxy.call("GetState", &()).map_err(error)?;
                serde_json::from_str(&text).map_err(error)?
            }
            Backend::Kde(bus) => {
                let mut snapshot = kde_snapshot();
                let lock = zbus::blocking::Proxy::new(
                    bus,
                    "org.freedesktop.ScreenSaver",
                    "/ScreenSaver",
                    "org.freedesktop.ScreenSaver",
                );
                snapshot.locked |= lock
                    .ok()
                    .and_then(|proxy| proxy.call::<_, _, bool>("GetActive", &()).ok())
                    .unwrap_or(!SessionInfo::detect().wsl);
                let proxy = zbus::blocking::Proxy::new(
                    bus,
                    "org.kde.keyboard",
                    "/Layouts",
                    "org.kde.KeyboardLayouts",
                )
                .map_err(error)?;
                snapshot.group = proxy.call("getLayout", &()).map_err(error)?;
                let sources: Vec<(String, String, String)> =
                    proxy.call("getLayoutsList", &()).map_err(error)?;
                snapshot.sources = sources
                    .into_iter()
                    .map(|(code, _, name)| Source {
                        code,
                        name,
                        variant: String::new(),
                    })
                    .collect();
                snapshot
            }
        };
        Ok(snapshot)
    }
    pub fn target(&self) -> Option<InputTarget> {
        let s = self.cached();
        (s.window != 0 && !s.locked && !s.session_inactive).then_some(InputTarget {
            window: s.window,
            control: s.control,
        })
    }
    pub fn caret_position(&self, target: Option<InputTarget>) -> Option<[f32; 2]> {
        let s = self.cached();
        if s.locked
            || s.session_inactive
            || target.is_some_and(|target| Some(target) != self.target())
        {
            return None;
        }
        self.accessibility
            .current(s.window, s.pid)?
            .caret
            .map(|point| [point[0] as f32, point[1] as f32])
    }
    /// Prefer the caret. If accessibility cannot provide it, keep input hints
    /// in the active application's client area even when the pointer was left
    /// on another monitor during keyboard-only window switching.
    pub fn popup_position(&self, target: Option<InputTarget>) -> Option<[f32; 2]> {
        if let Some(caret) = self.caret_position(target) {
            return Some(caret);
        }
        let state = self.cached();
        if state.locked
            || state.session_inactive
            || target.is_some_and(|target| Some(target) != self.target())
        {
            return None;
        }
        Some(fallback_popup_position(state.cursor, state.client, state.frame).map(|v| v as f32))
    }
    pub fn command(&self, operation: &str, window: u64) -> Result<()> {
        let state = self.cached();
        if window == 0 || state.locked || state.session_inactive {
            return Err(error("no unlocked input target"));
        }
        let backend = self.backend.lock().map_err(error)?;
        match &*backend {
            Backend::X11(conn, root) => {
                let window = u32::try_from(window).map_err(error)?;
                let (atom_name, data) = match operation {
                    "ActivateWindow" => ("_NET_ACTIVE_WINDOW", [2, x11rb::CURRENT_TIME, 0, 0, 0]),
                    "MinimizeWindow" => ("WM_CHANGE_STATE", [3, 0, 0, 0, 0]),
                    "ToggleMaximizeWindow" => (
                        "_NET_WM_STATE",
                        [
                            2,
                            atom(conn, "_NET_WM_STATE_MAXIMIZED_VERT")?,
                            atom(conn, "_NET_WM_STATE_MAXIMIZED_HORZ")?,
                            2,
                            0,
                        ],
                    ),
                    _ => return Err(error("unknown desktop operation")),
                };
                let event =
                    xproto::ClientMessageEvent::new(32, window, atom(conn, atom_name)?, data);
                conn.send_event(
                    false,
                    *root,
                    xproto::EventMask::SUBSTRUCTURE_REDIRECT
                        | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
                    event,
                )
                .map_err(error)?
                .check()
                .map_err(error)?;
                conn.flush().map_err(error)
            }
            Backend::Gnome(bus) => {
                let proxy = zbus::blocking::Proxy::new(
                    bus,
                    "org.own_keyboard_switch.Gnome",
                    "/org/own_keyboard_switch/Gnome",
                    "org.own_keyboard_switch.Gnome",
                )
                .map_err(error)?;
                proxy.call::<_, _, ()>(operation, &(window,)).map_err(error)
            }
            Backend::Kde(_) => {
                bridge_commands().lock().map_err(error)?.push_back(
                    serde_json::json!({ "operation": operation, "window": window }).to_string(),
                );
                Ok(())
            }
        }
    }
}
fn fallback_popup_position(cursor: [i32; 2], client: [i32; 4], frame: [i32; 4]) -> [i32; 2] {
    let area = if client[2] > 0 && client[3] > 0 {
        client
    } else {
        frame
    };
    if area[2] <= 0 || area[3] <= 0 {
        return cursor.map(|v| v.saturating_add(12));
    }
    let [x, y, w, h] = area;
    let inside = cursor[0] >= x
        && cursor[1] >= y
        && i64::from(cursor[0]) < i64::from(x) + i64::from(w)
        && i64::from(cursor[1]) < i64::from(y) + i64::from(h);
    let point = if inside {
        cursor.map(|v| v.saturating_add(12))
    } else {
        [x.saturating_add(12), y.saturating_add(12)]
    };
    crate::placement::fit(point, area, [1, 1])
}
impl LayoutManager for LinuxDesktop {
    fn layouts(&self) -> Result<Vec<LayoutInfo>> {
        Ok(self
            .refresh()?
            .sources
            .iter()
            .enumerate()
            .map(|(index, source)| layout(index, source))
            .collect())
    }
    fn current(&self) -> Result<LayoutId> {
        Ok(LayoutId(u64::from(self.refresh()?.group)))
    }
    fn set(&mut self, layout: LayoutId) -> Result<()> {
        let state = self.cached();
        if state.locked || state.session_inactive {
            return Err(error(
                "cannot switch layout outside an active unlocked graphical session",
            ));
        }
        let group = u32::try_from(layout.0).map_err(error)?;
        if group as usize >= self.cached().sources.len() {
            return Err(error("unknown layout index"));
        }
        {
            let backend = self.backend.lock().map_err(error)?;
            match &*backend {
                Backend::X11(conn, _) => {
                    xkb::latch_lock_state(
                        conn.as_ref(),
                        0x100,
                        0u8.into(),
                        0u8.into(),
                        true,
                        u8::try_from(group).map_err(error)?.into(),
                        0u8.into(),
                        false,
                        0,
                    )
                    .map_err(error)?
                    .check()
                    .map_err(error)?;
                    conn.flush().map_err(error)?;
                }
                Backend::Gnome(bus) => {
                    let proxy = zbus::blocking::Proxy::new(
                        bus,
                        "org.own_keyboard_switch.Gnome",
                        "/org/own_keyboard_switch/Gnome",
                        "org.own_keyboard_switch.Gnome",
                    )
                    .map_err(error)?;
                    proxy
                        .call::<_, _, ()>("SetLayout", &(group,))
                        .map_err(error)?;
                }
                Backend::Kde(bus) => {
                    let proxy = zbus::blocking::Proxy::new(
                        bus,
                        "org.kde.keyboard",
                        "/Layouts",
                        "org.kde.KeyboardLayouts",
                    )
                    .map_err(error)?;
                    let success: bool = proxy.call("setLayout", &(group,)).map_err(error)?;
                    if !success {
                        return Err(error("KDE rejected the requested layout"));
                    }
                }
            }
        }
        for _ in 0..20 {
            if self.refresh()?.group == group {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(error("desktop did not confirm the layout change"))
    }
    fn keymap(&self, id: LayoutId) -> Result<KeyMap> {
        let sources = self.cached().sources;
        let source = sources
            .get(id.0 as usize)
            .ok_or_else(|| error("unknown keyboard layout"))?;
        crate::services::keymap(&source.code, &source.variant)
    }
    fn subscribe(
        &mut self,
        sink: crossbeam_channel::Sender<LayoutId>,
    ) -> Result<Box<dyn StopGuard>> {
        let desktop = self.clone();
        let mut previous = self.cached();
        Ok(Box::new(Watch::spawn(
            move || {
                if let Ok(s) = desktop.refresh()
                    && (s.group != previous.group || s.sources != previous.sources)
                {
                    previous = s.clone();
                    return sink.send(LayoutId(u64::from(s.group))).is_ok();
                }
                true
            },
            Duration::from_millis(40),
        )))
    }
}
impl FocusInfo for LinuxDesktop {
    fn menu_access_language_for(&self, time: Instant) -> Result<Option<Lang>> {
        let language = self.menu_access_language()?;
        if language.is_some()
            && let Ok(mut keys) = self.menu_keys.lock()
        {
            keys.armed = true;
            keys.opened_at = Some(time);
        }
        Ok(language)
    }
    fn menu_access_language(&self) -> Result<Option<Lang>> {
        let snapshot = self.refresh()?;
        if snapshot.locked || snapshot.session_inactive {
            return Ok(None);
        }
        let language = self
            .accessibility
            .menu(snapshot.window, snapshot.pid)
            .and_then(|menu| menu.language);
        Ok(language)
    }
    fn input_target(&self) -> Result<Option<InputTarget>> {
        Ok(self.target())
    }
    fn activate_target(&self, target: InputTarget) -> Result<()> {
        self.command("ActivateWindow", target.window)?;
        for _ in 0..50 {
            self.refresh()?;
            if self.target() == Some(target) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(error("the input target did not regain focus"))
    }
    fn active_window(&self) -> Result<Option<WindowInfo>> {
        let s = self.refresh()?;
        Ok((s.window != 0).then(|| WindowInfo {
            pid: (s.pid != 0).then_some(s.pid),
            executable: std::fs::read_link(format!("/proc/{}/exe", s.pid)).ok(),
            title: Some(s.title),
            app_id: Some(s.app_id),
        }))
    }
    fn is_password_field(&self) -> Result<Option<bool>> {
        let snapshot = self.cached();
        Ok(self
            .accessibility
            .current(snapshot.window, snapshot.pid)
            .map(|focus| focus.password))
    }
    fn is_terminal(&self) -> Result<bool> {
        let name = self.cached().app_id.to_ascii_lowercase();
        Ok([
            "terminal",
            "console",
            "ptyxis",
            "kgx",
            "konsole",
            "kitty",
            "alacritty",
            "wezterm",
            "xterm",
            "foot",
            "tilix",
            "ghostty",
            "contour",
            "guake",
            "yakuake",
        ]
        .iter()
        .any(|part| name.contains(part)))
    }
    fn subscribe(
        &mut self,
        sink: crossbeam_channel::Sender<FocusEvent>,
    ) -> Result<Box<dyn StopGuard>> {
        let desktop = self.clone();
        let mut previous = self.cached();
        let mut menu_open = false;
        let mut menu_time = None;
        Ok(Box::new(Watch::spawn(
            move || {
                if let Ok(s) = desktop.refresh() {
                    if let Some(time) = desktop.take_cancelled_menu()
                        && sink.send(FocusEvent::MenuClosedFor(time)).is_err()
                    {
                        return false;
                    }
                    let same_window = s.window == previous.window && s.pid == previous.pid;
                    if let Some(menu) = desktop.accessibility.menu(s.window, s.pid) {
                        let closed = desktop.menu_closed(menu.open, menu_open, menu_time);
                        if same_window
                            && let Some(time) = closed
                            && sink.send(FocusEvent::MenuClosedFor(time)).is_err()
                        {
                            return false;
                        }
                        if menu.open && !menu_open {
                            menu_time = desktop.menu_opened_at();
                        }
                        menu_open = menu.open;
                    } else if !same_window || s.locked || s.session_inactive {
                        menu_open = false;
                        menu_time = None;
                    }
                    let changed = s.window != previous.window
                        || s.control != previous.control
                        || s.locked != previous.locked;
                    let event = if s.window != previous.window || s.locked != previous.locked {
                        FocusEvent::WindowChanged(desktop.active_window().ok().flatten())
                    } else {
                        FocusEvent::ControlChanged
                    };
                    previous = s;
                    if changed {
                        return sink.send(event).is_ok();
                    }
                }
                true
            },
            Duration::from_millis(40),
        )))
    }
}
impl WindowControl for LinuxDesktop {
    fn minimize_active(&self) -> Result<()> {
        self.command("MinimizeWindow", self.cached().window)
    }
    fn toggle_maximize_active(&self) -> Result<()> {
        self.command("ToggleMaximizeWindow", self.cached().window)
    }
}
fn layout(index: usize, source: &Source) -> LayoutInfo {
    let code = source.code.split('+').next().unwrap_or(&source.code);
    let lang = match code {
        "ru" => Some(Lang::Ru),
        "us" | "gb" => Some(Lang::En),
        _ => None,
    };
    LayoutInfo {
        id: LayoutId(index as u64),
        lang,
        locale: match code {
            "ru" => "ru-RU",
            "gb" => "en-GB",
            "us" => "en-US",
            _ => code,
        }
        .into(),
        name: source.name.clone(),
        short: match lang {
            Some(Lang::Ru) => "Ru",
            Some(Lang::En) => "En",
            _ => code,
        }
        .into(),
    }
}
fn atom(conn: &RustConnection, name: &str) -> Result<u32> {
    Ok(conn
        .intern_atom(false, name.as_bytes())
        .map_err(error)?
        .reply()
        .map_err(error)?
        .atom)
}
fn property(conn: &RustConnection, window: u32, name: &str) -> Result<xproto::GetPropertyReply> {
    conn.get_property(
        false,
        window,
        atom(conn, name)?,
        xproto::AtomEnum::ANY,
        0,
        4096,
    )
    .map_err(error)?
    .reply()
    .map_err(error)
}
fn number(conn: &RustConnection, window: u32, name: &str) -> u32 {
    property(conn, window, name)
        .ok()
        .and_then(|p| p.value32().and_then(|mut v| v.next()))
        .unwrap_or(0)
}
fn text(conn: &RustConnection, window: u32, name: &str) -> String {
    property(conn, window, name)
        .map(|p| {
            String::from_utf8_lossy(&p.value)
                .trim_end_matches('\0')
                .to_string()
        })
        .unwrap_or_default()
}
fn x11_snapshot(conn: &RustConnection, root: u32) -> Result<Snapshot> {
    let keyboard = xkb::get_state(conn, 0x100)
        .map_err(error)?
        .reply()
        .map_err(error)?;
    let group = u32::from(u8::from(keyboard.group));
    let names = property(conn, root, "_XKB_RULES_NAMES")?.value;
    let fields: Vec<_> = names
        .split(|byte| *byte == 0)
        .map(String::from_utf8_lossy)
        .collect();
    let variants: Vec<_> = fields
        .get(3)
        .map(|v| v.split(',').collect())
        .unwrap_or_default();
    let sources = fields
        .get(2)
        .map(|v| {
            v.split(',')
                .enumerate()
                .map(|(i, code)| Source {
                    code: code.into(),
                    name: code.into(),
                    variant: variants.get(i).copied().unwrap_or("").into(),
                })
                .collect()
        })
        .unwrap_or_default();
    let focused = conn
        .get_input_focus()
        .map_err(error)?
        .reply()
        .map_err(error)?
        .focus;
    let mut window = number(conn, root, "_NET_ACTIVE_WINDOW");
    if window == 0 && focused > 1 {
        window = focused;
    }
    let cursor = conn
        .query_pointer(root)
        .map_err(error)?
        .reply()
        .map_err(error)?;
    let client = conn
        .get_geometry(window)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|geometry| {
            conn.translate_coordinates(window, root, 0, 0)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|origin| {
                    [
                        i32::from(origin.dst_x),
                        i32::from(origin.dst_y),
                        i32::from(geometry.width),
                        i32::from(geometry.height),
                    ]
                })
        })
        .unwrap_or_default();
    let borders: Vec<i32> = property(conn, window, "_NET_FRAME_EXTENTS")
        .ok()
        .and_then(|p| {
            p.value32()
                .map(|values| values.take(4).map(|v| v.min(4096) as i32).collect())
        })
        .unwrap_or_default();
    let frame = if let [left, right, top, bottom] = borders.as_slice() {
        [
            client[0] - left,
            client[1] - top,
            client[2] + left + right,
            client[3] + top + bottom,
        ]
    } else {
        client
    };
    Ok(Snapshot {
        group,
        sources,
        window: u64::from(window),
        control: u64::from(focused),
        pid: number(conn, window, "_NET_WM_PID"),
        title: text(conn, window, "_NET_WM_NAME"),
        app_id: text(conn, window, "WM_CLASS"),
        cursor: [i32::from(cursor.root_x), i32::from(cursor.root_y)],
        locked: false,
        caps: Some(keyboard.locked_mods.contains(xproto::ModMask::LOCK)),
        frame,
        client,
        session_inactive: false,
        protocol: 0,
        placed_windows: Vec::new(),
    })
}
static KDE_STATE: std::sync::LazyLock<Mutex<(Snapshot, Option<Instant>)>> =
    std::sync::LazyLock::new(|| Mutex::new((Snapshot::default(), None)));
static KDE_COMMANDS: std::sync::LazyLock<Mutex<std::collections::VecDeque<String>>> =
    std::sync::LazyLock::new(Mutex::default);
fn kde_snapshot() -> Snapshot {
    let Ok(state) = KDE_STATE.lock() else {
        return Snapshot {
            locked: true,
            ..Snapshot::default()
        };
    };
    if state
        .1
        .is_some_and(|time| time.elapsed() < Duration::from_millis(500))
    {
        return state.0.clone();
    }
    Snapshot {
        locked: true,
        ..Snapshot::default()
    }
}
fn bridge_commands() -> &'static Mutex<std::collections::VecDeque<String>> {
    &KDE_COMMANDS
}
#[derive(Debug)]
pub struct KdeBridge;
#[zbus::interface(name = "org.own_keyboard_switch.Kde")]
impl KdeBridge {
    fn update_state(&self, state: &str) -> zbus::fdo::Result<()> {
        let snapshot: Snapshot = serde_json::from_str(state)
            .map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))?;
        if let Ok(mut current) = KDE_STATE.lock() {
            *current = (snapshot, Some(Instant::now()));
        }
        Ok(())
    }
    fn take_command(&self) -> String {
        std::thread::sleep(Duration::from_millis(50));
        bridge_commands()
            .lock()
            .ok()
            .and_then(|mut commands| commands.pop_front())
            .unwrap_or_default()
    }
}
pub type BridgeConnection = zbus::blocking::Connection;
pub fn serve_kde_bridge() -> Result<BridgeConnection> {
    zbus::blocking::connection::Builder::session()
        .map_err(error)?
        .name("org.own_keyboard_switch.Kde")
        .map_err(error)?
        .serve_at("/org/own_keyboard_switch/Kde", KdeBridge)
        .map_err(error)?
        .build()
        .map_err(error)
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires isolated X11 with XKB; creates and reparents only its own test window"]
    fn x11_geometry_uses_root_coordinates_for_reparented_client_windows() {
        use x11rb::wrapper::ConnectionExt as _;
        let (conn, screen) = x11rb::connect(None).unwrap();
        xkb::use_extension(&conn, 1, 0).unwrap().reply().unwrap();
        let root = conn.setup().roots[screen].root;
        let frame = conn.generate_id().unwrap();
        let client = conn.generate_id().unwrap();
        for (id, parent, x, y, w, h) in [
            (frame, root, 600, 300, 330, 210),
            (client, frame, 5, 25, 320, 180),
        ] {
            conn.create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                id,
                parent,
                x,
                y,
                w,
                h,
                0,
                xproto::WindowClass::INPUT_OUTPUT,
                0,
                &xproto::CreateWindowAux::new(),
            )
            .unwrap();
            conn.map_window(id).unwrap();
        }
        conn.change_property32(
            xproto::PropMode::REPLACE,
            client,
            atom(&conn, "_NET_FRAME_EXTENTS").unwrap(),
            xproto::AtomEnum::CARDINAL,
            &[5, 5, 25, 5],
        )
        .unwrap();
        conn.change_property32(
            xproto::PropMode::REPLACE,
            root,
            atom(&conn, "_NET_ACTIVE_WINDOW").unwrap(),
            xproto::AtomEnum::WINDOW,
            &[client],
        )
        .unwrap();
        conn.set_input_focus(xproto::InputFocus::PARENT, client, x11rb::CURRENT_TIME)
            .unwrap();
        conn.flush().unwrap();
        let state = x11_snapshot(&conn, root).unwrap();
        assert_eq!(state.client, [605, 325, 320, 180]);
        assert_eq!(state.frame, [600, 300, 330, 210]);
        assert_eq!(
            fallback_popup_position([20, 20], state.client, state.frame),
            [617, 337]
        );
        conn.delete_property(root, atom(&conn, "_NET_ACTIVE_WINDOW").unwrap())
            .unwrap();
        conn.destroy_window(frame).unwrap();
        conn.flush().unwrap();
    }
    #[test]
    fn popup_fallback_uses_the_input_window_instead_of_another_monitor() {
        let client = [1304, 129, 420, 300];
        assert_eq!(
            fallback_popup_position([512, 384], client, [0; 4]),
            [1316, 141]
        );
        assert_eq!(
            fallback_popup_position([1400, 200], client, [0; 4]),
            [1412, 212]
        );
        assert_eq!(
            fallback_popup_position([1723, 428], client, [0; 4]),
            [1723, 428]
        );
        assert_eq!(
            fallback_popup_position([500, 300], [-1400, -100, 800, 600], [0; 4]),
            [-1388, -88]
        );
        assert_eq!(
            fallback_popup_position([0, 0], [0; 4], [80, 100, 500, 300]),
            [92, 112]
        );
        assert_eq!(
            fallback_popup_position([100, 120], [0; 4], [0; 4]),
            [112, 132]
        );
    }
    use super::*;
    #[test]
    #[ignore = "requires isolated X11; reconnects the layout backend without changing the desktop group"]
    fn x11_backend_change_preserves_clones_and_rejects_unavailable_methods() {
        use okbs_core::config::{Config, LayoutBackend};
        let desktop = LinuxDesktop::connect().unwrap();
        let mut clone = desktop.clone();
        let mut config = Config::default();
        let original = desktop.current().unwrap();
        config.linux.layout_backend = LayoutBackend::X11;
        let prepared = desktop.prepare_layout(&config).unwrap().unwrap();
        assert_eq!(desktop.current().unwrap(), original);
        desktop.apply_layout(prepared).unwrap();
        assert_eq!(clone.effective_backend().unwrap(), LayoutBackend::X11);
        clone.set(LayoutId(1 - original.0)).unwrap();
        assert_eq!(desktop.current().unwrap(), LayoutId(1 - original.0));
        config.linux.layout_backend = LayoutBackend::GnomeFallback;
        assert!(desktop.prepare_layout(&config).is_err());
        assert_eq!(desktop.effective_backend().unwrap(), LayoutBackend::X11);
        clone.set(original).unwrap();
        config.linux.layout_backend = LayoutBackend::Auto;
        desktop
            .apply_layout(desktop.prepare_layout(&config).unwrap().unwrap())
            .unwrap();
        assert_eq!(desktop.current().unwrap(), original);
    }
    #[test]
    #[ignore = "requires an isolated X11 display configured with us,ru layouts"]
    fn x11_reads_switches_and_verifies_the_real_keyboard_group() {
        let mut desktop = LinuxDesktop::connect().unwrap();
        let layouts = desktop.layouts().unwrap();
        assert_eq!(layouts.len(), 2);
        assert_eq!(layouts[0].lang, Some(Lang::En));
        assert_eq!(layouts[1].lang, Some(Lang::Ru));
        let original = desktop.current().unwrap();
        let next = LayoutId(1 - original.0);
        desktop.set(next).unwrap();
        assert_eq!(desktop.current().unwrap(), next);
        assert_eq!(
            desktop
                .keymap(LayoutId(1))
                .unwrap()
                .get(PhysKey::KeyQ, false),
            Some('й')
        );
        assert!(desktop.set(LayoutId(9)).is_err());
        desktop.set(original).unwrap();
    }
    #[test]
    #[ignore = "requires a KDE Wayland session with us,ru layouts"]
    fn kde_reads_switches_and_verifies_the_real_keyboard_group() {
        let _bridge = serve_kde_bridge().unwrap();
        crate::integration::install_current(&Desktop::Kde).unwrap();
        let mut desktop = LinuxDesktop::connect().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while desktop.cached().locked {
            desktop.refresh().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let sources = desktop.layouts().unwrap();
        assert!(sources.iter().any(|source| source.lang == Some(Lang::Ru)));
        assert!(sources.iter().any(|source| source.lang == Some(Lang::En)));
        let original = desktop.current().unwrap();
        let other = sources
            .iter()
            .find(|source| source.id != original)
            .unwrap()
            .id;
        desktop.set(other).unwrap();
        assert_eq!(desktop.current().unwrap(), other);
        desktop.set(original).unwrap();
    }
    #[test]
    #[ignore = "requires isolated KDE Wayland session; loads KWin script and opens GTK window"]
    fn kde_script_tracks_real_windows_and_applies_owned_placement() {
        use gtk::prelude::*;
        gtk::init().unwrap();
        let _bridge = serve_kde_bridge().unwrap();
        crate::integration::install_current(&Desktop::Kde).unwrap();
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("OKBS KDE focus fixture");
        window.set_default_size(320, 200);
        window.show_all();
        window.present();
        let desktop = LinuxDesktop::connect().unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            let snapshot = desktop.refresh().unwrap();
            if snapshot.pid == std::process::id() && snapshot.window != 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "KWin script did not report the fixture window"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        desktop
            .place_owned_window("OKBS KDE focus fixture", [80.0, 90.0])
            .unwrap();
        for _ in 0..20 {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(desktop.cached().pid, std::process::id());
        let placed = desktop.refresh().unwrap();
        assert_eq!(&placed.frame[..2], &[80, 90]);
        let monitor = gtk::gdk::Display::default()
            .unwrap()
            .monitor(1)
            .unwrap()
            .geometry();
        let secondary = [monitor.x() + 80, monitor.y() + 90];
        desktop
            .place_owned_window(
                "OKBS KDE focus fixture",
                [secondary[0] as f32, secondary[1] as f32],
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            let moved = desktop.refresh().unwrap();
            if moved.frame[..2] == secondary {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "window stayed on its original output: {:?}",
                moved.frame
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        window.close();
    }
}
