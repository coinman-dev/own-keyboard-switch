//! Bounded AT-SPI queries. Cache identity/role/geometry and menu mnemonics,
//! never field text.
use crate::desktop::{Snapshot, error};
use okbs_core::Lang;
use okbs_platform::Result;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::OwnedObjectPath,
};

type Object = (String, OwnedObjectPath);
#[derive(Debug, Clone)]
pub struct FocusedControl {
    pub window: u64,
    pub pid: u32,
    pub id: u64,
    pub password: bool,
    pub caret: Option<[i32; 2]>,
    text_input: bool,
    updated: Instant,
}
#[derive(Debug, Clone)]
pub(crate) struct WindowMenu {
    window: u64,
    pid: u32,
    pub language: Option<Lang>,
    pub open: bool,
    updated: Instant,
}
#[derive(Debug, Default)]
struct AccessibilityState {
    focus: Option<FocusedControl>,
    menu: Option<WindowMenu>,
}
#[derive(Debug)]
pub struct Accessibility {
    state: Arc<Mutex<AccessibilityState>>,
    stop: Arc<AtomicBool>,
    menus_enabled: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Accessibility {
    pub fn spawn(desktop: Weak<Mutex<Snapshot>>) -> Self {
        let state = Arc::new(Mutex::new(AccessibilityState::default()));
        let shared = state.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let menus_enabled = Arc::new(AtomicBool::new(false));
        let menu_setting = menus_enabled.clone();
        let thread = std::thread::spawn(move || {
            let mut reader = None;
            let mut retry = Instant::now();
            while !stopping.load(Ordering::Acquire) {
                let Some(desktop) = desktop.upgrade() else {
                    break;
                };
                let snapshot = desktop.lock().map(|s| s.clone()).unwrap_or_default();
                drop(desktop);
                if reader.is_none() && retry.elapsed() > Duration::from_millis(500) {
                    reader = Reader::connect().ok();
                    retry = Instant::now();
                }
                let menus = menu_setting.load(Ordering::Acquire);
                if let Some(reader) = &mut reader {
                    reader.menus_enabled = menus;
                    if !menus {
                        reader.menu = None;
                    }
                }
                let focused = if snapshot.pid != 0 && !snapshot.locked && !snapshot.session_inactive
                {
                    reader
                        .as_mut()
                        .and_then(|reader| reader.focused(&snapshot).ok().flatten())
                } else {
                    None
                };
                let menu =
                    if menus && snapshot.pid != 0 && !snapshot.locked && !snapshot.session_inactive
                    {
                        reader
                            .as_ref()
                            .and_then(|reader| reader.window_menu(&snapshot))
                    } else {
                        None
                    };
                if let Ok(mut state) = shared.lock() {
                    *state = AccessibilityState {
                        focus: focused,
                        menu,
                    };
                }
                std::thread::sleep(Duration::from_millis(40));
            }
        });
        Self {
            state,
            stop,
            menus_enabled,
            thread: Some(thread),
        }
    }
    pub(crate) fn enable_menus(&self, enabled: bool) {
        self.menus_enabled.store(enabled, Ordering::Release);
    }
    pub fn current(&self, window: u64, pid: u32) -> Option<FocusedControl> {
        self.state
            .lock()
            .ok()?
            .focus
            .as_ref()
            .filter(|focus| {
                focus.window == window
                    && focus.pid == pid
                    && focus.updated.elapsed() < Duration::from_millis(500)
            })
            .cloned()
    }
    pub(crate) fn menu(&self, window: u64, pid: u32) -> Option<WindowMenu> {
        self.state
            .lock()
            .ok()?
            .menu
            .as_ref()
            .filter(|menu| {
                menu.window == window
                    && menu.pid == pid
                    && menu.updated.elapsed() < Duration::from_millis(500)
            })
            .cloned()
    }
}
impl Drop for Accessibility {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
struct Reader {
    bus: Connection,
    owners: HashMap<String, u32>,
    previous: Option<Object>,
    previous_window: u64,
    menu: Option<MenuCache>,
    menus_enabled: bool,
    wayland: bool,
    scan: Option<FocusScan>,
}
struct FocusScan {
    window: u64,
    pid: u32,
    queue: VecDeque<Object>,
    menu_queue: VecDeque<Object>,
    visited: HashSet<Object>,
    fallback: Option<(Object, FocusedControl)>,
}
struct MenuCache {
    window: u64,
    pid: u32,
    frame: Object,
    language: Option<Lang>,
    items: Vec<Object>,
    updated: Instant,
}
impl Reader {
    fn connect() -> Result<Self> {
        let session = zbus::blocking::connection::Builder::session()
            .map_err(error)?
            .method_timeout(Duration::from_millis(100))
            .build()
            .map_err(error)?;
        let proxy =
            Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus").map_err(error)?;
        let address: String = proxy.call("GetAddress", &()).map_err(error)?;
        let bus = zbus::blocking::connection::Builder::address(address.as_str())
            .map_err(error)?
            .method_timeout(Duration::from_millis(50))
            .build()
            .map_err(error)?;
        Ok(Self {
            bus,
            owners: HashMap::new(),
            previous: None,
            previous_window: 0,
            menu: None,
            menus_enabled: false,
            wayland: crate::session::SessionInfo::detect().session_type
                == crate::session::SessionType::Wayland,
            scan: None,
        })
    }
    fn proxy<'a>(&'a self, object: &'a Object, interface: &'a str) -> Result<Proxy<'a>> {
        // Direct property reads obey the connection timeout. A cache can wait
        // for its initial GetAll after an accessible has already disappeared,
        // preventing the worker from finishing during application shutdown.
        zbus::blocking::proxy::Builder::new(&self.bus)
            .destination(object.0.as_str())
            .map_err(error)?
            .path(object.1.as_str())
            .map_err(error)?
            .interface(interface)
            .map_err(error)?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .map_err(error)
    }
    fn owner(&mut self, name: &str) -> Option<u32> {
        if let Some(owner) = self.owners.get(name) {
            return Some(*owner);
        }
        let pid = zbus::blocking::fdo::DBusProxy::new(&self.bus)
            .ok()?
            .get_connection_unix_process_id(name.try_into().ok()?)
            .ok()?;
        self.owners.insert(name.to_string(), pid);
        Some(pid)
    }
    fn inspect(&self, object: &Object, snapshot: &Snapshot) -> Result<Option<FocusedControl>> {
        let accessible = self.proxy(object, "org.a11y.atspi.Accessible")?;
        let state: Vec<u32> = accessible.call("GetState", &()).map_err(error)?;
        if !state.first().is_some_and(|bits| bits & (1 << 12) != 0) {
            return Ok(None);
        }
        let role: u32 = accessible.call("GetRole", &()).map_err(error)?;
        let text_input = matches!(role, 40 | 60 | 61 | 79)
            || state.first().is_some_and(|bits| bits & (1 << 7) != 0);
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        object.hash(&mut hash);
        let caret = if role == 40 || !text_input {
            None
        } else {
            let text = self.proxy(object, "org.a11y.atspi.Text")?;
            let coordinates = u32::from(self.wayland);
            text.get_property::<i32>("CaretOffset")
                .ok()
                .and_then(|offset| {
                    let extents = text
                        .call::<_, _, (i32, i32, i32, i32)>(
                            "GetCharacterExtents",
                            &(offset, coordinates),
                        )
                        .ok()
                        .filter(|(_, _, _, height)| *height > 0);
                    if let Some((x, y, _, height)) = extents {
                        return Some([x, y.saturating_add(height)]);
                    }
                    text.call::<_, _, (i32, i32, i32, i32)>(
                        "GetCharacterExtents",
                        &(offset.saturating_sub(1), coordinates),
                    )
                    .ok()
                    .filter(|(_, _, _, height)| *height > 0)
                    .map(|(x, y, width, height)| {
                        [x.saturating_add(width), y.saturating_add(height)]
                    })
                })
                .and_then(|point| {
                    if self.wayland {
                        self.window_caret(object, snapshot, point)
                    } else {
                        Some(point)
                    }
                })
        };
        Ok(Some(FocusedControl {
            window: snapshot.window,
            pid: snapshot.pid,
            id: hash.finish().max(1),
            password: role == 40,
            caret,
            text_input,
            updated: Instant::now(),
        }))
    }
    fn window_caret(
        &self,
        object: &Object,
        snapshot: &Snapshot,
        point: [i32; 2],
    ) -> Option<[i32; 2]> {
        let deadline = Instant::now() + Duration::from_millis(100);
        let parent = self.top_level(object, deadline)?;
        let accessible = self.proxy(&parent, "org.a11y.atspi.Accessible").ok()?;
        let title: String = accessible.get_property("Name").ok()?;
        if title != snapshot.title {
            return None;
        }
        let component = self.proxy(&parent, "org.a11y.atspi.Component").ok()?;
        let (x, y, w, h): (i32, i32, i32, i32) = component.call("GetExtents", &(1u32,)).ok()?;
        map_window_caret(point, [x, y, w, h], snapshot.client)
    }
    fn top_level(&self, object: &Object, deadline: Instant) -> Option<Object> {
        let mut parent = object.clone();
        let mut previous = None;
        for _ in 0..32 {
            if Instant::now() >= deadline {
                return None;
            }
            let accessible = self.proxy(&parent, "org.a11y.atspi.Accessible").ok()?;
            let role: u32 = accessible.call("GetRole", &()).ok()?;
            if matches!(role, 16 | 23 | 69) {
                return Some(parent.clone());
            }
            // Qt can expose a QWidget top-level as FILLER. Its actual parent
            // is APPLICATION; never confuse that application root with a
            // window, since it can own several unrelated text fields.
            if role == 75 {
                return previous;
            }
            let next = accessible.get_property("Parent").ok()?;
            drop(accessible);
            previous = Some(parent.clone());
            parent = next;
            if parent.0.is_empty() {
                parent.0 = object.0.clone();
            }
        }
        None
    }
    fn menu_language(&mut self, focused: &Object, snapshot: &Snapshot) -> Option<Lang> {
        let deadline = Instant::now() + Duration::from_millis(100);
        // Ascend from the actual focused control instead of searching every
        // window owned by its process (editors often have several windows).
        let mut frame = focused.clone();
        for _ in 0..32 {
            if Instant::now() >= deadline || frame.1.as_str() == "/org/a11y/atspi/null" {
                return None;
            }
            let accessible = self.proxy(&frame, "org.a11y.atspi.Accessible").ok()?;
            let role: u32 = accessible.call("GetRole", &()).ok()?;
            if matches!(role, 16 | 23 | 69) {
                let state: Vec<u32> = accessible.call("GetState", &()).ok()?;
                if !state.first().is_some_and(|bits| bits & (1 << 1) != 0) {
                    return None;
                }
                drop(accessible);
                if let Some(cache) = &self.menu
                    && cache.window == snapshot.window
                    && cache.pid == snapshot.pid
                    && cache.frame == frame
                    && cache.updated.elapsed() < Duration::from_millis(250)
                {
                    return cache.language;
                }
                let (language, items) = self.read_menu(&frame, deadline)?;
                self.menu = Some(MenuCache {
                    window: snapshot.window,
                    pid: snapshot.pid,
                    frame,
                    language,
                    items,
                    updated: Instant::now(),
                });
                return language;
            }
            let (name, path): Object = accessible.get_property("Parent").ok()?;
            drop(accessible);
            frame = (if name.is_empty() { frame.0 } else { name }, path);
        }
        None
    }
    fn window_menu(&self, snapshot: &Snapshot) -> Option<WindowMenu> {
        let cache = self.menu.as_ref()?;
        if cache.window != snapshot.window || cache.pid != snapshot.pid {
            return None;
        }
        let deadline = Instant::now() + Duration::from_millis(50);
        let mut open = false;
        for item in &cache.items {
            // Missing menu state must not be reported as an observed closure.
            if Instant::now() >= deadline {
                return None;
            }
            let state: Vec<u32> = self
                .proxy(item, "org.a11y.atspi.Accessible")
                .ok()?
                .call("GetState", &())
                .ok()?;
            open |= state
                .first()
                .is_some_and(|bits| bits & ((1 << 23) | (1 << 12)) != 0);
        }
        Some(WindowMenu {
            window: snapshot.window,
            pid: snapshot.pid,
            language: (cache.updated.elapsed() < Duration::from_millis(500))
                .then_some(cache.language)
                .flatten(),
            open,
            updated: Instant::now(),
        })
    }
    fn read_menu(&self, frame: &Object, deadline: Instant) -> Option<(Option<Lang>, Vec<Object>)> {
        let mut queue = VecDeque::from([(frame.clone(), false)]);
        let mut visited = HashSet::new();
        let (mut latin, mut cyrillic) = (0, 0);
        let mut items = Vec::new();
        while let Some((object, in_bar)) = queue.pop_front() {
            if Instant::now() >= deadline || visited.len() >= 128 {
                // A partial scan must not choose a language.
                return None;
            }
            if object.1.as_str() == "/org/a11y/atspi/null" || !visited.insert(object.clone()) {
                continue;
            }
            let accessible = self.proxy(&object, "org.a11y.atspi.Accessible").ok()?;
            let role: u32 = accessible.call("GetRole", &()).ok()?;
            if in_bar {
                items.push(object.clone());
                let actions: Vec<(String, String, String)> = self
                    .proxy(&object, "org.a11y.atspi.Action")
                    .ok()?
                    .call("GetActions", &())
                    .ok()?;
                for (_, _, binding) in actions {
                    match mnemonic_language(&binding) {
                        Some(Lang::En) => latin += 1,
                        Some(Lang::Ru) => cyrillic += 1,
                        _ => {}
                    }
                }
                // Only top-level access keys determine the menu language.
                continue;
            }
            // Text controls and nested windows cannot contain this window's
            // menu bar. Do not read names, values or field contents.
            if matches!(role, 16 | 23 | 69) && &object != frame || matches!(role, 40 | 61 | 79) {
                continue;
            }
            let children: Vec<Object> = accessible.call("GetChildren", &()).ok()?;
            queue.extend(children.into_iter().map(|(name, path)| {
                (
                    (
                        if name.is_empty() {
                            object.0.clone()
                        } else {
                            name
                        },
                        path,
                    ),
                    role == 34,
                )
            }));
        }
        let language = match (latin, cyrillic) {
            (0, 0) => None,
            (l, c) if l >= c => Some(Lang::En),
            _ => Some(Lang::Ru),
        };
        Some((language, items))
    }
    fn focused(&mut self, snapshot: &Snapshot) -> Result<Option<FocusedControl>> {
        if let Some(previous) = self.previous.clone()
            && self.previous_window == snapshot.window
            && self.owner(&previous.0) == Some(snapshot.pid)
            && let Ok(Some(focus)) = self.inspect(&previous, snapshot)
            && focus.text_input
        {
            self.scan = None;
            if self.menus_enabled {
                self.menu_language(&previous, snapshot);
            }
            return Ok(Some(focus));
        }
        if self
            .scan
            .as_ref()
            .is_some_and(|scan| scan.window != snapshot.window || scan.pid != snapshot.pid)
        {
            self.scan = None;
        }
        let deadline = Instant::now() + Duration::from_millis(100);
        if self.scan.is_none() {
            let root = (
                "org.a11y.atspi.Registry".to_string(),
                OwnedObjectPath::try_from("/org/a11y/atspi/accessible/root").map_err(error)?,
            );
            let children: Vec<Object> = self
                .proxy(&root, "org.a11y.atspi.Accessible")?
                .call("GetChildren", &())
                .map_err(error)?;
            let mut queue = VecDeque::new();
            for child in children {
                if Instant::now() >= deadline {
                    break;
                }
                let owner = self.owner(&child.0);
                if owner == Some(snapshot.pid) {
                    queue.push_back(child);
                }
            }
            self.scan = Some(FocusScan {
                window: snapshot.window,
                pid: snapshot.pid,
                queue,
                menu_queue: VecDeque::new(),
                visited: HashSet::new(),
                fallback: None,
            });
        }
        let Some(mut scan) = self.scan.take() else {
            return Ok(None);
        };
        while !scan.queue.is_empty() || !scan.menu_queue.is_empty() {
            if scan.queue.is_empty() {
                std::mem::swap(&mut scan.queue, &mut scan.menu_queue);
            }
            if Instant::now() >= deadline {
                let fallback = scan
                    .fallback
                    .as_ref()
                    .and_then(|(object, _)| self.inspect(object, snapshot).ok().flatten());
                self.scan = Some(scan);
                return Ok(fallback);
            }
            if scan.visited.len() >= 512 {
                break;
            }
            let Some(object) = scan.queue.pop_front() else {
                break;
            };
            if object.1.as_str() == "/org/a11y/atspi/null" || !scan.visited.insert(object.clone()) {
                continue;
            }
            if let Ok(Some(focus)) = self.inspect(&object, snapshot) {
                if focus.text_input {
                    if self.menus_enabled {
                        self.menu_language(&object, snapshot);
                    }
                    self.previous = Some(object);
                    self.previous_window = snapshot.window;
                    return Ok(Some(focus));
                }
                // Browsers expose focused containers alongside the actual
                // editable child. Keep searching instead of caching a dialog
                // or frame and never reaching its password/text field.
                scan.fallback = Some((object.clone(), focus));
            }
            if let Ok(proxy) = self.proxy(&object, "org.a11y.atspi.Accessible")
                && let Ok(role) = proxy.call::<_, _, u32>("GetRole", &())
                // Hidden office menus can have hundreds of descendants. Do
                // not exhaust the bounded scan before reaching the editor.
                // Showing menus remain searchable, including editable items.
                && (!matches!(role, 33 | 34 | 35 | 41)
                    || proxy.call::<_, _, Vec<u32>>("GetState", &()).ok().is_some_and(|state|
                        state.first().is_some_and(|bits| bits & (1 << 25) != 0)))
                && let Ok(children) = proxy.call::<_, _, Vec<Object>>("GetChildren", &())
            {
                // Search editor/container branches before potentially large
                // visible menu trees. LibreOffice exposes collapsed menu
                // descendants as showing; inspecting all of them first can
                // hit the node bound before the focused paragraph is reached.
                let queue = if matches!(role, 33 | 34 | 35 | 41) {
                    &mut scan.menu_queue
                } else {
                    &mut scan.queue
                };
                let capacity: usize = if matches!(role, 33 | 34 | 35 | 41) {
                    128
                } else {
                    512
                };
                let remaining = capacity.saturating_sub(queue.len());
                queue.extend(children.into_iter().take(remaining).map(|(name, path)| {
                    (
                        if name.is_empty() {
                            object.0.clone()
                        } else {
                            name
                        },
                        path,
                    )
                }));
            }
        }
        if let Some((object, _)) = scan.fallback
            && let Some(focus) = self.inspect(&object, snapshot)?
        {
            if self.menus_enabled {
                self.menu_language(&object, snapshot);
            }
            self.previous = Some(object);
            self.previous_window = snapshot.window;
            return Ok(Some(focus));
        }
        self.previous = None;
        Ok(None)
    }
}

fn map_window_caret(point: [i32; 2], local: [i32; 4], client: [i32; 4]) -> Option<[i32; 2]> {
    let [lx, ly, lw, lh] = local;
    let [x, y, w, h] = client;
    if lw <= 0 || lh <= 0 || w <= 0 || h <= 0 {
        return None;
    }
    // A scale change preserves the aspect ratio. Some Qt Wayland top-levels
    // report only their QWidget content while the compositor includes a
    // client-side title bar. Stretching that content over the whole surface
    // invents a caret position; use the active-window fallback instead.
    let sx = f64::from(w) / f64::from(lw);
    let sy = f64::from(h) / f64::from(lh);
    let rounding = 2.0 / f64::from(lw.min(lh));
    if (sx - sy).abs() > rounding {
        return None;
    }
    let dx = i64::from(point[0]) - i64::from(lx);
    let dy = i64::from(point[1]) - i64::from(ly);
    if dx < 0 || dy < 0 || dx > i64::from(lw) || dy > i64::from(lh) {
        return None;
    }
    Some([
        x.saturating_add((dx as f64 * f64::from(w) / f64::from(lw)).round() as i32),
        y.saturating_add((dy as f64 * f64::from(h) / f64::from(lh)).round() as i32),
    ])
}

/// AT-SPI Action bindings are `mnemonic;sequence;shortcut`. A shortcut
/// without a mnemonic says nothing about the menu's access language.
#[allow(unsafe_code)]
fn mnemonic_language(binding: &str) -> Option<Lang> {
    let mnemonic = binding.split(';').next()?.trim();
    let key = mnemonic.rsplit('>').next()?.rsplit('+').next()?.trim();
    let mut letters = key.chars();
    let mut letter = letters.next()?;
    if letters.next().is_some() {
        // GTK encodes non-Latin mnemonics as XKB keysym names.
        let name = std::ffi::CString::new(key).ok()?;
        let library = xkbcommon_dl::xkbcommon_option()?;
        // SAFETY: a valid terminated name, no retained pointers; both calls
        // return values and have no ownership requirements.
        letter = unsafe {
            let symbol = (library.xkb_keysym_from_name)(
                name.as_ptr(),
                xkbcommon_dl::xkb_keysym_flags::XKB_KEYSYM_NO_FLAGS,
            );
            char::from_u32((library.xkb_keysym_to_utf32)(symbol))?
        };
    }
    if letter.is_ascii_alphabetic() {
        Some(Lang::En)
    } else if ('А'..='я').contains(&letter) || matches!(letter, 'Ё' | 'ё') {
        Some(Lang::Ru)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caret_mapping_handles_origins_scale_and_missing_geometry() {
        assert_eq!(
            map_window_caret([48, 30], [0, 0, 420, 300], [1300, 100, 422, 340]),
            None,
            "a client-side title bar is not a scale change"
        );
        assert_eq!(
            map_window_caret([40, 26], [0, 0, 380, 200], [1068, 89, 380, 200]),
            Some([1108, 115])
        );
        assert_eq!(
            map_window_caret([80, 52], [0, 0, 760, 400], [-1200, -200, 380, 200]),
            Some([-1160, -174])
        );
        assert_eq!(
            map_window_caret([60, 39], [0, 0, 570, 300], [1200, 60, 380, 200]),
            Some([1240, 86])
        );
        assert_eq!(
            map_window_caret([0, 0], [0, 0, 0, 200], [0, 0, 380, 200]),
            None
        );
        assert_eq!(map_window_caret([40, 26], [0, 0, 380, 200], [0; 4]), None);
        assert_eq!(
            map_window_caret([-1, 26], [0, 0, 380, 200], [100, 100, 380, 200]),
            None
        );
    }
    #[test]
    fn menu_language_uses_mnemonics_not_shortcuts_or_labels() {
        for binding in ["f;<Alt>f;", "<Alt>F;;", "Alt+F;;", "f;;<Control>o"] {
            assert_eq!(mnemonic_language(binding), Some(Lang::En), "{binding}");
        }
        for binding in ["Ф;;", "ё;;", "Cyrillic_ef;<Alt>Cyrillic_ef;"] {
            assert_eq!(mnemonic_language(binding), Some(Lang::Ru), "{binding}");
        }
        for binding in [
            ";;<Control>f",
            "",
            "1;;",
            "F10;;",
            "é;;",
            "Cyrillic_unknown;;",
        ] {
            assert_eq!(mnemonic_language(binding), None, "{binding}");
        }
    }
    #[test]
    #[ignore = "requires isolated X11 and accessibility session; opens two GTK fields"]
    fn real_accessibility_distinguishes_fields_password_and_caret_without_reading_text() {
        use gtk::prelude::*;
        use x11rb::{
            connection::Connection,
            protocol::xproto::{self, ConnectionExt},
        };
        gtk::init().unwrap();
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("OKBS accessibility fixture");
        let fields = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let plain = gtk::Entry::new();
        plain.set_text("fixture");
        let password = gtk::Entry::new();
        password.set_visibility(false);
        fields.add(&plain);
        fields.add(&password);
        window.add(&fields);
        window.show_all();
        let (connection, screen) = x11rb::connect(None).unwrap();
        let root = connection.setup().roots[screen].root;
        for _ in 0..10 {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let id = connection
            .query_tree(root)
            .unwrap()
            .reply()
            .unwrap()
            .children
            .into_iter()
            .find(|&id| {
                for name in [b"_NET_WM_NAME".as_slice(), b"WM_NAME".as_slice()] {
                    let atom = connection
                        .intern_atom(false, name)
                        .unwrap()
                        .reply()
                        .unwrap()
                        .atom;
                    let value = connection
                        .get_property(false, id, atom, xproto::AtomEnum::ANY, 0, 256)
                        .unwrap()
                        .reply()
                        .unwrap()
                        .value;
                    if String::from_utf8_lossy(&value).trim_end_matches('\0')
                        == "OKBS accessibility fixture"
                    {
                        return true;
                    }
                }
                false
            })
            .unwrap();
        connection
            .set_input_focus(xproto::InputFocus::PARENT, id, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        plain.grab_focus();
        plain.set_position(-1);
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        let snapshot = Snapshot {
            window: u64::from(id),
            pid: std::process::id(),
            ..Snapshot::default()
        };
        let state = Arc::new(Mutex::new(snapshot.clone()));
        let reader = Accessibility::spawn(Arc::downgrade(&state));
        let read = |expected_password: bool| {
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                while gtk::events_pending() {
                    gtk::main_iteration_do(false);
                }
                if let Some(focused) = reader.current(snapshot.window, snapshot.pid)
                    && focused.password == expected_password
                {
                    return focused;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            panic!("focused GTK field not found");
        };
        let first = read(false);
        assert!(!first.password);
        assert!(first.caret.is_some());
        password.grab_focus();
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        let second = read(true);
        assert!(second.password);
        assert!(second.caret.is_none());
        assert_ne!(first.id, second.id);
        window.close();
    }
}
