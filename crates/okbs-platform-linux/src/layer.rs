//! Small binding to the maintained system gtk-layer-shell C API. Avoids the
//! archived Rust gtk-layer-shell packages. All calls stay on GTK's UI thread.
#![allow(unsafe_code)]
use gtk::glib::translate::ToGlibPtr;
#[link(name = "gtk-layer-shell")]
unsafe extern "C" {
    fn gtk_layer_is_supported() -> i32;
    fn gtk_layer_init_for_window(window: *mut gtk::ffi::GtkWindow);
    fn gtk_layer_is_layer_window(window: *mut gtk::ffi::GtkWindow) -> i32;
    fn gtk_layer_set_layer(window: *mut gtk::ffi::GtkWindow, layer: i32);
    fn gtk_layer_set_keyboard_mode(window: *mut gtk::ffi::GtkWindow, mode: i32);
    fn gtk_layer_set_anchor(window: *mut gtk::ffi::GtkWindow, edge: i32, enabled: i32);
    fn gtk_layer_set_margin(window: *mut gtk::ffi::GtkWindow, edge: i32, margin: i32);
    fn gtk_layer_set_monitor(
        window: *mut gtk::ffi::GtkWindow,
        monitor: *mut gtk::gdk::ffi::GdkMonitor,
    );
    fn gtk_layer_get_monitor(window: *mut gtk::ffi::GtkWindow) -> *mut gtk::gdk::ffi::GdkMonitor;
    fn gtk_layer_set_namespace(window: *mut gtk::ffi::GtkWindow, name: *const std::ffi::c_char);
}
pub fn supported() -> bool {
    assert!(gtk::is_initialized_main_thread());
    unsafe { gtk_layer_is_supported() != 0 }
}
pub fn initialize(window: &gtk::Window) {
    assert!(gtk::is_initialized_main_thread());
    // SAFETY: GTK owns this live GtkWindow; all arguments are fixed API enums.
    unsafe {
        let window = window.to_glib_none().0;
        gtk_layer_init_for_window(window);
        gtk_layer_set_layer(window, 3); // overlay
        gtk_layer_set_keyboard_mode(window, 0); // none
        gtk_layer_set_anchor(window, 0, 1); // left
        gtk_layer_set_anchor(window, 2, 1); // top
        gtk_layer_set_namespace(window, c"okbswitch".as_ptr());
    }
}
pub fn is_layer(window: &gtk::Window) -> bool {
    assert!(gtk::is_initialized_main_thread());
    unsafe { gtk_layer_is_layer_window(window.to_glib_none().0) != 0 }
}
pub fn position(window: &gtk::Window, position: [i32; 2]) -> [i32; 2] {
    assert!(gtk::is_initialized_main_thread());
    use gtk::prelude::*;
    let Some(display) = gtk::gdk::Display::default() else {
        return position;
    };
    let Some(monitor) = display
        .monitor_at_point(position[0], position[1])
        .or_else(|| display.primary_monitor())
    else {
        return position;
    };
    let geometry = monitor.geometry();
    let area = [
        geometry.x(),
        geometry.y(),
        geometry.width(),
        geometry.height(),
    ];
    let (_, natural) = window.preferred_size();
    let point = crate::placement::fit(position, area, [natural.width, natural.height]);
    let margin = crate::placement::relative(point, area);
    unsafe {
        let window = window.to_glib_none().0;
        if gtk_layer_get_monitor(window) != monitor.to_glib_none().0 {
            gtk_layer_set_monitor(window, monitor.to_glib_none().0);
        }
        gtk_layer_set_margin(window, 0, margin[0]);
        gtk_layer_set_margin(window, 2, margin[1]);
    }
    point
}
