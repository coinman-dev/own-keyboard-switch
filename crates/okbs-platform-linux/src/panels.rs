//! Passive panels: Shell actors in GNOME, GTK/layer-shell in KDE and X11.
use crate::desktop::{LinuxDesktop, error};
use crate::session::{SessionInfo, SessionType};
use gtk::prelude::*;

use okbs_platform::{FloatingIndicator, IndicatorEvent, IndicatorLabels, IndicatorState, Result};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Panel {
    pub id: String,
    pub visible: bool,
    pub position: [i32; 2],
    pub text: String,
    pub title: String,
    pub rows: Vec<String>,
    pub opacity: f64,
    pub locked: bool,
    pub settings: String,
    pub hide: String,
    pub lock: String,
    pub icon: Vec<u8>,
    pub theme: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PanelEvent {
    pub id: String,
    pub action: String,
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub position: [i32; 2],
}
pub struct Panels {
    desktop: LinuxDesktop,
    native: RefCell<HashMap<String, gtk::Window>>,
    models: Rc<RefCell<HashMap<String, Panel>>>,
    events: Rc<RefCell<VecDeque<PanelEvent>>>,
}
impl std::fmt::Debug for Panels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Panels").finish_non_exhaustive()
    }
}
impl Panels {
    fn gtk_scale() -> i32 {
        if SessionInfo::detect().session_type == SessionType::X11 {
            gtk::gdk::Display::default()
                .and_then(|display| display.primary_monitor())
                .map(|monitor| monitor.scale_factor())
                .unwrap_or(1)
        } else {
            1
        }
    }
    fn native_point(position: [i32; 2]) -> [i32; 2] {
        crate::placement::x11_to_gtk(position, Self::gtk_scale())
    }
    fn source_point(position: [i32; 2]) -> [i32; 2] {
        crate::placement::gtk_to_x11(position, Self::gtk_scale())
    }
    pub fn new(desktop: LinuxDesktop) -> Result<Rc<Self>> {
        if !desktop.is_gnome() {
            gtk::init().map_err(error)?;
        }
        let panels = Rc::new(Self {
            desktop,
            native: RefCell::default(),
            models: Rc::default(),
            events: Rc::default(),
        });
        if !panels.desktop.is_gnome()
            && let Some(display) = gtk::gdk::Display::default()
        {
            for removed in [false, true] {
                let weak = Rc::downgrade(&panels);
                let notify = move || {
                    let weak = weak.clone();
                    gtk::glib::idle_add_local_once(move || {
                        if let Some(panels) = weak.upgrade() {
                            let visible: Vec<_> = panels
                                .models
                                .borrow()
                                .values()
                                .filter(|panel| panel.visible)
                                .cloned()
                                .collect();
                            for panel in visible {
                                if let Err(err) = panels.show(panel) {
                                    tracing::debug!(
                                        "cannot move panel after monitor change: {err}"
                                    );
                                }
                            }
                        }
                    });
                };
                if removed {
                    display.connect_monitor_removed(move |_, _| notify());
                } else {
                    display.connect_monitor_added(move |_, _| notify());
                }
            }
        }
        Ok(panels)
    }
    pub fn show(&self, panel: Panel) -> Result<()> {
        self.models
            .borrow_mut()
            .insert(panel.id.clone(), panel.clone());
        if self.desktop.is_gnome() {
            return self
                .desktop
                .gnome_request("SetPanel", &serde_json::to_string(&panel).map_err(error)?);
        }
        gtk::init().map_err(error)?;
        let mut windows = self.native.borrow_mut();
        let window = windows.entry(panel.id.clone()).or_insert_with(|| {
            let window = gtk::Window::new(gtk::WindowType::Toplevel);
            window.set_decorated(false);
            window.set_resizable(false);
            window.set_keep_above(true);
            window.set_accept_focus(false);
            window.set_focus_on_map(false);
            window.set_skip_taskbar_hint(true);
            window.set_skip_pager_hint(true);
            window.set_type_hint(gtk::gdk::WindowTypeHint::Notification);
            if crate::layer::supported() {
                crate::layer::initialize(&window);
            }
            let id = panel.id.clone();
            let events = self.events.clone();
            let models = self.models.clone();
            let dragging = Rc::new(RefCell::new(None));
            let drag = dragging.clone();
            window.add_events(
                gtk::gdk::EventMask::BUTTON_PRESS_MASK
                    | gtk::gdk::EventMask::BUTTON_RELEASE_MASK
                    | gtk::gdk::EventMask::POINTER_MOTION_MASK,
            );
            window.connect_button_press_event(move |window, event| {
                if id != "indicator" {
                    return gtk::glib::Propagation::Proceed;
                }
                if event.event_type() == gtk::gdk::EventType::DoubleButtonPress {
                    events.borrow_mut().push_back(PanelEvent {
                        id: id.clone(),
                        action: "settings".into(),
                        ..PanelEvent::default()
                    });
                } else if event.button() == 3 {
                    let model = models.borrow().get(&id).cloned().unwrap_or_default();
                    let menu = gtk::Menu::new();
                    for (label, action) in [
                        (model.settings, "settings"),
                        (model.lock, "lock"),
                        (model.hide, "hide"),
                    ] {
                        let item = gtk::MenuItem::with_label(&label);
                        let queue = events.clone();
                        let id = id.clone();
                        item.connect_activate(move |_| {
                            queue.borrow_mut().push_back(PanelEvent {
                                id: id.clone(),
                                action: action.into(),
                                ..PanelEvent::default()
                            })
                        });
                        menu.append(&item);
                    }
                    menu.show_all();
                    menu.popup_at_pointer(Some(event));
                } else if event.button() == 1
                    && !models.borrow().get(&id).is_some_and(|model| model.locked)
                {
                    if crate::layer::is_layer(window) {
                        *drag.borrow_mut() = Some(event.position());
                    } else {
                        let (x, y) = event.root();
                        window.begin_move_drag(1, x as i32, y as i32, event.time());
                    }
                }
                gtk::glib::Propagation::Stop
            });
            let drag = dragging.clone();
            let id = panel.id.clone();
            let models = self.models.clone();
            window.connect_motion_notify_event(move |window, event| {
                if let Some((start_x, start_y)) = *drag.borrow() {
                    let (x, y) = event.position();
                    if let Some(model) = models.borrow_mut().get_mut(&id) {
                        model.position[0] = (model.position[0] + (x - start_x) as i32).max(0);
                        model.position[1] = (model.position[1] + (y - start_y) as i32).max(0);
                        model.position = crate::layer::position(window, model.position);
                    }
                }
                gtk::glib::Propagation::Proceed
            });
            let id = panel.id.clone();
            let queue = self.events.clone();
            let models = self.models.clone();
            window.connect_button_release_event(move |window, _| {
                *dragging.borrow_mut() = None;
                if id == "indicator" {
                    let (x, y) = window.position();
                    let position = if crate::layer::is_layer(window) {
                        models
                            .borrow()
                            .get(&id)
                            .map(|model| model.position)
                            .unwrap_or([x, y])
                    } else {
                        Self::source_point([x, y])
                    };
                    queue.borrow_mut().push_back(PanelEvent {
                        id: id.clone(),
                        action: "moved".into(),
                        position,
                        ..PanelEvent::default()
                    });
                }
                gtk::glib::Propagation::Proceed
            });
            window
        });
        if !panel.visible {
            window.hide();
            return Ok(());
        }
        if let Some(child) = window.child() {
            window.remove(&child);
        }
        let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
        if matches!(panel.theme.as_str(), "light" | "dark") {
            let css = gtk::CssProvider::new();
            let colors = if panel.theme == "light" {
                "#f5f5f5;color:#202020"
            } else {
                "#232323;color:#ffffff"
            };
            css.load_from_data(
                format!("window {{background:{colors};}} label {{color:inherit;}}").as_bytes(),
            )
            .map_err(error)?;
            window
                .style_context()
                .add_provider(&css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        content.set_border_width(6);
        if !panel.icon.is_empty() {
            let loader = gtk::gdk_pixbuf::PixbufLoader::new();
            loader.write(&panel.icon).map_err(error)?;
            loader.close().map_err(error)?;
            content.add(&gtk::Image::from_pixbuf(loader.pixbuf().as_ref()));
        }
        if !panel.text.is_empty() {
            content.add(&gtk::Label::new(Some(&panel.text)));
        }
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 2);
        for (index, row) in panel.rows.iter().enumerate() {
            let button = gtk::Button::with_label(row);
            button.set_can_focus(false);
            let queue = self.events.clone();
            let id = panel.id.clone();
            button.connect_clicked(move |_| {
                queue.borrow_mut().push_back(PanelEvent {
                    id: id.clone(),
                    action: "insert".into(),
                    index,
                    ..PanelEvent::default()
                })
            });
            rows.add(&button);
        }
        if !panel.rows.is_empty() {
            let scroller =
                gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
            scroller.set_size_request(360, 240);
            scroller.add(&rows);
            content.add(&scroller);
        }
        window.add(&content);
        content.show_all();
        window.set_title(&panel.title);
        window.set_opacity(panel.opacity.clamp(0.1, 1.0));
        if crate::layer::is_layer(window) {
            let point = crate::layer::position(window, panel.position);
            if let Some(model) = self.models.borrow_mut().get_mut(&panel.id) {
                model.position = point;
            }
        } else {
            let Some(display) = gtk::gdk::Display::default() else {
                return Err(error("no display for native panel"));
            };
            let point = Self::native_point(panel.position);
            let monitor = display
                .monitor_at_point(point[0], point[1])
                .or_else(|| display.primary_monitor());
            let point = monitor.map_or(point, |monitor| {
                let area = monitor.workarea();
                let (_, natural) = window.preferred_size();
                crate::placement::fit(
                    point,
                    [area.x(), area.y(), area.width(), area.height()],
                    [natural.width, natural.height],
                )
            });
            window.move_(point[0], point[1]);
            if let Some(model) = self.models.borrow_mut().get_mut(&panel.id) {
                model.position = Self::source_point(point);
            }
        }
        window.show_all();
        Ok(())
    }
    pub fn hide(&self, id: &str) -> Result<()> {
        let panel = self
            .models
            .borrow()
            .get(id)
            .cloned()
            .unwrap_or_else(|| Panel {
                id: id.into(),
                ..Panel::default()
            });
        self.show(Panel {
            visible: false,
            ..panel
        })
    }
    pub fn suspend(&self) {
        let ids: Vec<String> = self.models.borrow().keys().cloned().collect();
        for id in ids {
            if let Err(err) = self.hide(&id) {
                tracing::debug!("cannot hide old desktop panel: {err}");
            }
        }
        for window in self.native.borrow().values() {
            window.hide();
        }
        self.events.borrow_mut().clear();
    }
    pub fn visible(&self, id: &str) -> bool {
        self.models
            .borrow()
            .get(id)
            .is_some_and(|panel| panel.visible)
    }
    pub fn poll(&self, id: &str) -> Option<PanelEvent> {
        if self.desktop.is_gnome() {
            if let Ok(events) = self.desktop.gnome_events() {
                self.events.borrow_mut().extend(events);
            }
        } else {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
        }
        let mut events = self.events.borrow_mut();
        let index = events.iter().position(|event| event.id == id)?;
        events.remove(index)
    }
}
impl Drop for Panels {
    fn drop(&mut self) {
        if self.desktop.is_gnome() {
            for id in self.models.borrow().keys() {
                let _ = self.desktop.gnome_request(
                    "SetPanel",
                    &serde_json::to_string(&Panel {
                        id: id.clone(),
                        ..Panel::default()
                    })
                    .unwrap_or_default(),
                );
            }
        } else {
            for window in self.native.borrow().values() {
                window.close();
            }
        }
    }
}

#[derive(Debug)]
pub struct LinuxIndicator {
    panels: Rc<Panels>,
    state: IndicatorState,
    labels: IndicatorLabels,
    icon: Vec<u8>,
    until: Option<Instant>,
}
impl LinuxIndicator {
    pub fn new(
        panels: Rc<Panels>,
        state: &IndicatorState,
        labels: IndicatorLabels,
    ) -> Result<Self> {
        let indicator = Self {
            panels,
            state: state.clone(),
            labels,
            icon: Vec::new(),
            until: None,
        };
        indicator.update()?;
        Ok(indicator)
    }
    fn update(&self) -> Result<()> {
        self.panels.show(Panel {
            id: "indicator".into(),
            visible: self.state.visible
                && (!self.state.autohide || self.until.is_some_and(|until| until > Instant::now())),
            position: self.state.position.unwrap_or([24, 24]),
            title: self.labels.title.clone(),
            locked: self.state.locked,
            icon: self.icon.clone(),
            opacity: 1.0,
            settings: self.labels.settings.clone(),
            lock: self.labels.lock.clone(),
            hide: self.labels.hide.clone(),
            ..Panel::default()
        })
    }
}
impl FloatingIndicator for LinuxIndicator {
    fn configure(&mut self, state: &IndicatorState, labels: IndicatorLabels) -> Result<()> {
        self.state = state.clone();
        self.labels = labels;
        self.update()
    }
    fn set_icon(&mut self, rgba: &[u8], size: u32, layout_changed: bool) -> Result<()> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .map_err(error)?
            .write_image_data(rgba)
            .map_err(error)?;
        self.icon = bytes;
        if layout_changed {
            self.until =
                Some(Instant::now() + Duration::from_millis(u64::from(self.state.autohide_ms)));
        }
        self.update()
    }
    fn poll(&mut self) -> Option<IndicatorEvent> {
        if self.state.autohide && self.until.is_some_and(|until| until <= Instant::now()) {
            self.until = None;
            let _ = self.update();
        }
        let event = self.panels.poll("indicator")?;
        match event.action.as_str() {
            "settings" => Some(IndicatorEvent::OpenSettings),
            "hide" => {
                self.state.visible = false;
                let _ = self.update();
                Some(IndicatorEvent::Hidden)
            }
            "lock" => {
                self.state.locked = !self.state.locked;
                let _ = self.update();
                Some(IndicatorEvent::Locked(self.state.locked))
            }
            "moved" => {
                self.state.position = Some(event.position);
                Some(IndicatorEvent::Moved(event.position))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires isolated KDE Wayland with two virtual monitors and layer shell"]
    fn layer_shell_popup_uses_the_secondary_monitor_and_local_margins() {
        gtk::init().unwrap();
        let display = gtk::gdk::Display::default().unwrap();
        assert!(
            display.n_monitors() >= 2,
            "test compositor has no second output"
        );
        let first = display.monitor(0).unwrap().geometry();
        let second = display.monitor(1).unwrap().geometry();
        assert_ne!((first.x(), first.y()), (second.x(), second.y()));
        let point = [
            second.x() + second.width() - 2,
            second.y() + second.height() - 2,
        ];
        let desktop = LinuxDesktop::connect().unwrap();
        let panels = Panels::new(desktop.clone()).unwrap();
        panels
            .show(Panel {
                id: "monitor-proof".into(),
                visible: true,
                position: point,
                text: "isolated monitor fixture".into(),
                opacity: 1.0,
                ..Panel::default()
            })
            .unwrap();
        for _ in 0..5 {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let window = panels.native.borrow().get("monitor-proof").unwrap().clone();
        assert!(crate::layer::is_layer(&window));
        let actual = display
            .monitor_at_window(&window.window().unwrap())
            .unwrap()
            .geometry();
        assert_eq!((actual.x(), actual.y()), (second.x(), second.y()));
        let (_, natural) = window.preferred_size();
        let expected = crate::placement::fit(
            point,
            [second.x(), second.y(), second.width(), second.height()],
            [natural.width, natural.height],
        );
        assert_eq!(panels.models.borrow()["monitor-proof"].position, expected);
        assert_ne!(
            desktop.refresh().unwrap().pid,
            std::process::id(),
            "passive popup took the editor focus"
        );
    }
    #[test]
    #[ignore = "requires isolated X11 with GDK_SCALE=2"]
    fn x11_panel_maps_physical_coordinates_to_gtk_scale_two() {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;
        assert_eq!(std::env::var("GDK_SCALE").as_deref(), Ok("2"));
        gtk::init().unwrap();
        let display = gtk::gdk::Display::default().unwrap();
        let monitor = display.primary_monitor().unwrap();
        assert_eq!(monitor.scale_factor(), 2);
        let (connection, screen) = x11rb::connect(None).unwrap();
        let root = connection.setup().roots[screen].root;
        let geometry = connection.get_geometry(root).unwrap().reply().unwrap();
        let area = monitor.geometry();
        assert_eq!(area.width() * 2, i32::from(geometry.width));
        assert_eq!(area.height() * 2, i32::from(geometry.height));
        let panels = Panels::new(LinuxDesktop::connect().unwrap()).unwrap();
        panels
            .show(Panel {
                id: "edge".into(),
                visible: true,
                position: [
                    i32::from(geometry.width) - 2,
                    i32::from(geometry.height) - 2,
                ],
                text: "isolated edge fixture".into(),
                opacity: 1.0,
                ..Panel::default()
            })
            .unwrap();
        for _ in 0..4 {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let window = panels.native.borrow().get("edge").unwrap().clone();
        let (x, y) = window.position();
        let (w, h) = window.size();
        assert!(x >= 0 && y >= 0 && x + w <= area.width() && y + h <= area.height());
        assert_eq!(
            panels.models.borrow()["edge"].position,
            Panels::source_point([x, y])
        );
    }
    #[test]
    #[ignore = "requires isolated X11 display; opens passive GTK panels"]
    fn passive_panels_preserve_focus_report_choices_and_autohide() {
        use x11rb::{
            connection::Connection,
            protocol::xproto::{self, ConnectionExt},
        };
        let (connection, screen) = x11rb::connect(None).unwrap();
        let root = connection.setup().roots[screen].root;
        let editor = connection.generate_id().unwrap();
        connection
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                editor,
                root,
                0,
                0,
                400,
                240,
                0,
                xproto::WindowClass::INPUT_OUTPUT,
                0,
                &xproto::CreateWindowAux::new(),
            )
            .unwrap()
            .check()
            .unwrap();
        connection.map_window(editor).unwrap().check().unwrap();
        connection
            .set_input_focus(xproto::InputFocus::PARENT, editor, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        let desktop = LinuxDesktop::connect().unwrap();
        let panels = Panels::new(desktop).unwrap();
        let state = IndicatorState {
            visible: true,
            autohide: true,
            autohide_ms: 100,
            position: Some([24, 24]),
            locked: false,
        };
        let mut indicator =
            LinuxIndicator::new(panels.clone(), &state, IndicatorLabels::default()).unwrap();
        indicator.set_icon(&[255, 0, 0, 255], 1, true).unwrap();
        panels
            .show(Panel {
                id: "list".into(),
                visible: true,
                position: [80, 80],
                rows: vec!["first".into(), "second".into()],
                opacity: 1.0,
                ..Panel::default()
            })
            .unwrap();
        for _ in 0..4 {
            let _ = panels.poll("list");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            connection.get_input_focus().unwrap().reply().unwrap().focus,
            editor
        );
        fn click(widget: &gtk::Widget) -> bool {
            if let Ok(button) = widget.clone().downcast::<gtk::Button>() {
                button.emit_clicked();
                return true;
            }
            if let Ok(container) = widget.clone().downcast::<gtk::Container>() {
                for child in container.children() {
                    if click(&child) {
                        return true;
                    }
                }
            }
            false
        }
        assert!(click(
            panels.native.borrow().get("list").unwrap().upcast_ref()
        ));
        assert_eq!(panels.poll("list").unwrap().index, 0);
        std::thread::sleep(Duration::from_millis(100));
        let _ = indicator.poll();
        assert!(!panels.visible("indicator"));
        panels.hide("list").unwrap();
        assert!(!panels.visible("list"));
    }
}
