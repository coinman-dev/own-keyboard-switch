//! Opt-in integration check against real Windows windows; no keyboard injection.
#![cfg(windows)]
#![allow(unsafe_code)]

use okbs_core::{
    Lang,
    config::{Config, Theme, UiLanguage},
};
use okbs_ui::{
    autoreplace_list::{AutoreplaceListConfig, AutoreplaceListLabels},
    settings::Section,
    window::SettingsWindow,
};
use std::{
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
    Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, MonitorFromWindow,
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
        PostMessageW, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos, WM_CLOSE,
    },
};

struct Search {
    title: String,
    found: Option<HWND>,
}

unsafe extern "system" fn find_window(hwnd: HWND, data: LPARAM) -> windows::core::BOOL {
    // SAFETY: EnumWindows synchronously passes the Search pointer supplied below.
    let search = unsafe { &mut *(data.0 as *mut Search) };
    let mut pid = 0;
    let mut title = [0_u16; 256];
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let len = GetWindowTextW(hwnd, &mut title);
        if pid == std::process::id()
            && String::from_utf16_lossy(&title[..len as usize]) == search.title
            && IsWindowVisible(hwnd).as_bool()
        {
            search.found = Some(hwnd);
        }
    }
    true.into()
}

fn visible(title: &str) -> Option<HWND> {
    let mut search = Search {
        title: title.into(),
        found: None,
    };
    // SAFETY: stack data remains live throughout the synchronous enumeration.
    unsafe {
        EnumWindows(
            Some(find_window),
            LPARAM((&mut search as *mut Search) as isize),
        )
    }
    .expect("enumerate windows");
    search.found
}

fn wait_for(title: &str, shown: bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while visible(title).is_some() != shown {
        assert!(
            Instant::now() < deadline,
            "window {title:?}: expected visible={shown}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn close(title: &str) {
    let window = visible(title).expect("visible test window");
    // SAFETY: only closes a window owned by this test process.
    unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) }.expect("close window");
    wait_for(title, false);
}

fn work_area(window: Option<HWND>) -> RECT {
    // SAFETY: query only; all output buffers are initialized and correctly sized.
    unsafe {
        let monitor = window.map_or_else(
            || MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTONEAREST),
            |window| MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST),
        );
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        assert!(GetMonitorInfoW(monitor, &mut info).as_bool());
        info.rcWork
    }
}

fn wait_inside(title: &str) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let window = visible(title).expect("visible window");
        let work = work_area(Some(window));
        let mut bounds = RECT::default();
        // SAFETY: our test window and a valid output buffer.
        unsafe { GetWindowRect(window, &mut bounds) }.expect("window bounds");
        if bounds.left >= work.left
            && bounds.top >= work.top
            && bounds.right <= work.right
            && bounds.bottom <= work.bottom
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "window outside working area: {bounds:?} vs {work:?}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[ignore = "opens real GUI windows; run separately with OKBS_GUI_FIRST=list and settings"]
fn settings_and_list_share_one_event_loop_and_shutdown() {
    const SETTINGS: &str = "Own Keyboard Switch Settings";
    const LIST: &str = "OKBS UI smoke list";
    let mut config = Config::default();
    config.general.ui_language = UiLanguage::En;
    config.general.theme = Theme::Dark;
    let (events, _rx) = crossbeam_channel::unbounded();
    let window = SettingsWindow::spawn(events, Lang::En).expect("GUI thread");
    let list = window.autoreplace_list();
    let list_config = AutoreplaceListConfig {
        settings: config.autoreplace.clone(),
        theme: config.general.theme,
        labels: AutoreplaceListLabels {
            title: LIST.into(),
            insert: "Insert".into(),
            close: "Close".into(),
            empty: "No entries".into(),
            ..Default::default()
        },
    };
    let work = work_area(None);
    let position = [(work.right - 2) as f32, (work.bottom - 2) as f32];
    if std::env::var("OKBS_GUI_FIRST").as_deref() == Ok("list") {
        list.show(list_config.clone(), true, position);
        wait_for(LIST, true);
        window.open(&config, Section::General, None, Vec::new());
    } else {
        window.open(&config, Section::General, None, Vec::new());
        wait_for(SETTINGS, true);
        list.show(list_config.clone(), true, position);
    }
    wait_for(SETTINGS, true);
    wait_for(LIST, true);
    wait_inside(LIST);
    for position in [[-20000, -20000], [30000, 30000]] {
        // Simulate an invalid saved position/disconnected monitor without changing
        // the user's display configuration, pointer or any other application's UI.
        unsafe {
            SetWindowPos(
                visible(LIST).expect("test list"),
                None,
                position[0],
                position[1],
                0,
                0,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSIZE,
            )
        }
        .expect("move test list off-screen");
        wait_inside(LIST);
    }
    close(SETTINGS);
    close(LIST);
    for _ in 0..2 {
        list.show(list_config.clone(), true, position);
        wait_for(LIST, true);
        wait_inside(LIST);
        assert!(visible(SETTINGS).is_none());
        window.open(&config, Section::General, None, Vec::new());
        wait_for(SETTINGS, true);
        close(LIST);
        close(SETTINGS);
    }
    // Exercise shutdown with a visible root, which previously canceled Close.
    list.show(list_config, true, position);
    window.open(&config, Section::General, None, Vec::new());
    wait_for(LIST, true);
    wait_for(SETTINGS, true);
    let (done, stopped) = std::sync::mpsc::channel();
    thread::spawn(move || {
        drop(list);
        drop(window);
        let _ = done.send(());
    });
    stopped
        .recv_timeout(Duration::from_secs(8))
        .expect("GUI shutdown must finish");
    assert!(visible(SETTINGS).is_none());
    assert!(visible(LIST).is_none());
}
