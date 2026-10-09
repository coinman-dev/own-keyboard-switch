//! Placement of this process's X11 windows in physical root pixels.
//! Never connect to XWayland for native Wayland application coordinates.
use crate::desktop::error;
use okbs_platform::Result;
use std::cell::RefCell;
use x11rb::{
    connection::Connection,
    protocol::{
        randr,
        xproto::{self, ConnectionExt},
    },
    rust_connection::RustConnection,
};

thread_local! {
    static CONNECTION: RefCell<Option<(RustConnection, u32)>> = const { RefCell::new(None) };
}

pub fn is_x11_session() -> bool {
    crate::session::SessionInfo::detect().session_type == crate::session::SessionType::X11
}

/// False means the window has not been created yet, or placement must be retried.
pub fn keep_visible(title: &str, requested: Option<[f32; 2]>) -> bool {
    if !is_x11_session() {
        return false;
    }
    CONNECTION.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let Ok((connection, screen)) = x11rb::connect(None) else {
                return false;
            };
            let root = connection.setup().roots[screen].root;
            *slot = Some((connection, root));
        }
        let Some((connection, root)) = slot.as_ref() else {
            return false;
        };
        match place(connection, *root, title, requested) {
            Ok(placed) => placed,
            Err(err) => {
                tracing::debug!(%err, "X11 popup placement will retry");
                *slot = None;
                false
            }
        }
    })
}

fn atom(conn: &RustConnection, name: &str) -> Result<u32> {
    Ok(conn
        .intern_atom(false, name.as_bytes())
        .map_err(error)?
        .reply()
        .map_err(error)?
        .atom)
}

/// Prepare the focus hint before eframe maps a hidden spelling viewport.
pub fn show_inactive(title: &str) -> bool {
    use x11rb::wrapper::ConnectionExt as _;
    if !is_x11_session() {
        return false;
    }
    let Ok((conn, screen)) = x11rb::connect(None) else {
        return false;
    };
    let root = conn.setup().roots[screen].root;
    let Some(window) = owned_window(&conn, root, title) else {
        return false;
    };
    let Ok(user_time) = atom(&conn, "_NET_WM_USER_TIME") else {
        return false;
    };
    let Ok(cookie) = conn.change_property32(
        xproto::PropMode::REPLACE,
        window,
        user_time,
        xproto::AtomEnum::CARDINAL,
        &[0],
    ) else {
        return false;
    };
    cookie.check().is_ok() && conn.flush().is_ok()
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
fn numbers(conn: &RustConnection, window: u32, name: &str) -> Vec<u32> {
    property(conn, window, name)
        .ok()
        .and_then(|p| p.value32().map(|values| values.collect()))
        .unwrap_or_default()
}
fn name(conn: &RustConnection, window: u32, property_name: &str) -> String {
    property(conn, window, property_name)
        .map(|p| {
            String::from_utf8_lossy(&p.value)
                .trim_end_matches('\0')
                .into()
        })
        .unwrap_or_default()
}
fn owned_window(conn: &RustConnection, root: u32, title: &str) -> Option<u32> {
    let mut candidates = numbers(conn, root, "_NET_CLIENT_LIST");
    candidates.extend(conn.query_tree(root).ok()?.reply().ok()?.children);
    // A WM can reparent even hidden clients. Search its frames as well, with a
    // bound on work in case another client creates a pathological hierarchy.
    let mut index = 0;
    while index < candidates.len().min(1024) {
        let window = candidates[index];
        index += 1;
        if numbers(conn, window, "_NET_WM_PID").first() == Some(&std::process::id())
            && (name(conn, window, "_NET_WM_NAME") == title
                || name(conn, window, "WM_NAME") == title)
        {
            return Some(window);
        }
        if let Ok(cookie) = conn.query_tree(window)
            && let Ok(tree) = cookie.reply()
        {
            for child in tree.children {
                if candidates.len() >= 1024 {
                    break;
                }
                if !candidates.contains(&child) {
                    candidates.push(child);
                }
            }
        }
    }
    None
}

fn nearest_area(point: [i32; 2], areas: &[[i32; 4]]) -> [i32; 4] {
    *areas
        .iter()
        .min_by_key(|&&[x, y, width, height]| {
            let dx =
                i64::from(point[0]) - i64::from(point[0].clamp(x, x.saturating_add(width - 1)));
            let dy =
                i64::from(point[1]) - i64::from(point[1].clamp(y, y.saturating_add(height - 1)));
            dx * dx + dy * dy
        })
        .expect("at least the root area")
}

pub(crate) fn place(
    conn: &RustConnection,
    root: u32,
    title: &str,
    requested: Option<[f32; 2]>,
) -> Result<bool> {
    let Some(window) = owned_window(conn, root, title) else {
        return Ok(false);
    };
    if conn
        .get_window_attributes(window)
        .map_err(error)?
        .reply()
        .map_err(error)?
        .map_state
        != xproto::MapState::VIEWABLE
    {
        return Ok(false);
    }
    let geometry = conn
        .get_geometry(window)
        .map_err(error)?
        .reply()
        .map_err(error)?;
    let origin = conn
        .translate_coordinates(window, root, 0, 0)
        .map_err(error)?
        .reply()
        .map_err(error)?;
    let extents = numbers(conn, window, "_NET_FRAME_EXTENTS");
    let [left, right, top, bottom] = match extents.as_slice() {
        [l, r, t, b, ..] => [*l, *r, *t, *b].map(|v| v.min(4096) as i32),
        _ => [0; 4],
    };
    let old = [
        i32::from(origin.dst_x) - left,
        i32::from(origin.dst_y) - top,
    ];
    let point = requested.map(|p| p.map(|v| v as i32)).unwrap_or(old);
    let root_geometry = conn
        .get_geometry(root)
        .map_err(error)?
        .reply()
        .map_err(error)?;
    let mut monitors = randr::get_monitors(conn, root, true)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| {
            reply
                .monitors
                .into_iter()
                .filter(|m| m.width > 0 && m.height > 0)
                .map(|m| {
                    [
                        i32::from(m.x),
                        i32::from(m.y),
                        i32::from(m.width),
                        i32::from(m.height),
                    ]
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if monitors.is_empty() {
        monitors.push([
            0,
            0,
            i32::from(root_geometry.width),
            i32::from(root_geometry.height),
        ]);
    }
    let mut area = nearest_area(point, &monitors);
    let desktop = numbers(conn, root, "_NET_CURRENT_DESKTOP")
        .first()
        .copied()
        .unwrap_or(0) as usize;
    let work = numbers(conn, root, "_NET_WORKAREA");
    if let Some(work) = desktop
        .checked_mul(4)
        .and_then(|offset| work.get(offset..offset + 4))
    {
        let x = area[0].max(work[0] as i32);
        let y = area[1].max(work[1] as i32);
        let right = (area[0] + area[2]).min((work[0] as i32).saturating_add(work[2] as i32));
        let bottom = (area[1] + area[3]).min((work[1] as i32).saturating_add(work[3] as i32));
        if right > x && bottom > y {
            area = [x, y, right - x, bottom - y];
        }
    }
    let width = i32::from(geometry.width).min((area[2] - left - right).max(1));
    let height = i32::from(geometry.height).min((area[3] - top - bottom).max(1));
    let next = crate::placement::fit(point, area, [width + left + right, height + top + bottom]);
    if next == old && width == i32::from(geometry.width) && height == i32::from(geometry.height) {
        return Ok(true);
    }
    let moveresize = atom(conn, "_NET_MOVERESIZE_WINDOW")?;
    if numbers(conn, root, "_NET_SUPPORTED").contains(&moveresize) {
        // NorthWestGravity: x/y denote the outside of the frame, width/height
        // remain client sizes. Moving/resizing never activates the window.
        let event = xproto::ClientMessageEvent::new(
            32,
            window,
            moveresize,
            [
                1 | (15 << 8) | (1 << 12),
                next[0] as u32,
                next[1] as u32,
                width as u32,
                height as u32,
            ],
        );
        conn.send_event(
            false,
            root,
            xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )
        .map_err(error)?
        .check()
        .map_err(error)?;
    } else {
        conn.configure_window(
            window,
            &xproto::ConfigureWindowAux::new()
                .x(next[0])
                .y(next[1])
                .width(width as u32)
                .height(height as u32),
        )
        .map_err(error)?
        .check()
        .map_err(error)?;
    }
    conn.flush().map_err(error)?;
    // Stop requesting the original caret position after submitting the move.
    // The WM processes EWMH asynchronously; repeating it on the next repaint
    // could undo a user's drag. Later frames still clamp the actual bounds.
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chooses_requested_monitor_including_negative_origins_and_gaps() {
        let areas = [
            [-1600, -200, 1600, 1000],
            [0, 0, 1920, 1080],
            [2000, 0, 1280, 900],
        ];
        assert_eq!(nearest_area([-800, 100], &areas), areas[0]);
        assert_eq!(nearest_area([2200, 100], &areas), areas[2]);
        assert_eq!(nearest_area([1980, 100], &areas), areas[2]);
        assert_eq!(nearest_area([50000, -50000], &areas), areas[2]);
    }
}
