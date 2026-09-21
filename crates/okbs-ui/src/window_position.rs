//! Keep native popup bounds inside the monitor's working area. All geometry
//! here is in physical pixels, including the title bar and window borders.

#[cfg(any(windows, test))]
fn fit(bounds: [i32; 4], work: [i32; 4]) -> [i32; 4] {
    let [x, y, width, height] = bounds;
    let [left, top, right, bottom] = work;
    let width = width.min((right - left).max(1)).max(1);
    let height = height.min((bottom - top).max(1)).max(1);
    [
        x.clamp(left, right - width),
        y.clamp(top, bottom - height),
        width,
        height,
    ]
}

#[cfg(windows)]
pub(crate) use native::keep_visible;

#[cfg(windows)]
mod native {
    #![allow(unsafe_code)]
    use windows::Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT},
        Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
            MonitorFromWindow,
        },
        System::Threading::GetCurrentThreadId,
        UI::{
            HiDpi::{
                DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
                SetThreadDpiAwarenessContext,
            },
            WindowsAndMessaging::{
                EnumThreadWindows, GetWindowRect, GetWindowTextW, SWP_NOACTIVATE, SWP_NOZORDER,
                SetWindowPos,
            },
        },
    };

    struct DpiScope(DPI_AWARENESS_CONTEXT);
    impl Drop for DpiScope {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                // SAFETY: restore this thread's previous DPI mode.
                unsafe {
                    SetThreadDpiAwarenessContext(self.0);
                }
            }
        }
    }

    struct Search<'a> {
        title: &'a str,
        found: Option<HWND>,
    }
    unsafe extern "system" fn find(window: HWND, param: LPARAM) -> windows::core::BOOL {
        // SAFETY: the caller keeps Search alive for synchronous enumeration.
        let search = unsafe { &mut *(param.0 as *mut Search<'_>) };
        let mut text = [0u16; 256];
        // SAFETY: text is a valid output buffer for this thread's window.
        let len = unsafe { GetWindowTextW(window, &mut text) };
        if String::from_utf16_lossy(&text[..len as usize]) == search.title {
            search.found = Some(window);
        }
        true.into()
    }

    pub(crate) fn keep_visible(title: &str, requested: Option<[f32; 2]>) -> bool {
        // Only windows on our GUI thread are eligible. Never move another app.
        let mut search = Search { title, found: None };
        // SAFETY: enumeration is synchronous; DPI virtualization is scoped to
        // these coordinate queries and restored before returning to winit.
        unsafe {
            let _dpi = DpiScope(SetThreadDpiAwarenessContext(
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            ));
            let _ = EnumThreadWindows(
                GetCurrentThreadId(),
                Some(find),
                LPARAM((&mut search as *mut Search<'_>) as isize),
            );
            let Some(window) = search.found else {
                return false;
            };
            let mut rect = RECT::default();
            if GetWindowRect(window, &mut rect).is_err() {
                return false;
            }
            let monitor = if let Some([x, y]) = requested {
                MonitorFromPoint(
                    POINT {
                        x: x as i32,
                        y: y as i32,
                    },
                    MONITOR_DEFAULTTONEAREST,
                )
            } else {
                MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST)
            };
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return false;
            }
            let old = [
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ];
            let mut desired = old;
            if let Some([x, y]) = requested {
                desired[0] = (x as i32).saturating_add(12);
                desired[1] = (y as i32).saturating_add(12);
            }
            let work = info.rcWork;
            if work.right <= work.left || work.bottom <= work.top {
                return false;
            }
            let next = super::fit(desired, [work.left, work.top, work.right, work.bottom]);
            if next != old {
                tracing::debug!(
                    x = next[0],
                    y = next[1],
                    width = next[2],
                    height = next[3],
                    "popup positioned within work area"
                );
                return SetWindowPos(
                    window,
                    None,
                    next[0],
                    next[1],
                    next[2],
                    next[3],
                    SWP_NOACTIVATE | SWP_NOZORDER,
                )
                .is_ok();
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn fits_all_edges_scales_and_monitor_origins() {
        // Physical work areas include bottom/left taskbars and negative origins.
        for work in [
            [0, 0, 2482, 1440],
            [-1920, -200, 0, 1040],
            [80, 0, 1920, 1080],
            [2560, 0, 4480, 1040],
        ] {
            for percent in [100, 125, 150, 175, 200, 250] {
                for point in [
                    [work[0] - 100, work[1] - 100],
                    [work[2] - 1, work[3] - 1],
                    [30000, 30000],
                    [-30000, -30000],
                ] {
                    let result = fit(
                        [point[0], point[1], 496 * percent / 100, 438 * percent / 100],
                        work,
                    );
                    assert!(result[0] >= work[0] && result[1] >= work[1]);
                    assert!(result[0] + result[2] <= work[2]);
                    assert!(result[1] + result[3] <= work[3]);
                }
            }
        }
    }

    #[test]
    fn leaves_visible_windows_in_place_and_shrinks_for_small_work_areas() {
        assert_eq!(
            fit([30, 40, 496, 438], [0, 0, 1920, 1040]),
            [30, 40, 496, 438]
        );
        assert_eq!(
            fit([1900, 900, 990, 880], [0, 40, 640, 480]),
            [0, 40, 640, 440]
        );
    }
}
