//! Fresh picker targets while real GTK fields/windows change in private Wayland.
use super::*;
use std::{
    path::PathBuf,
    process::{Child, Command},
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn editor(workspace: &std::path::Path, directory: &std::path::Path, name: &str) -> Process {
    let log = std::fs::File::create(directory.join(format!("{name}.log"))).unwrap();
    Process(
        Command::new("python3")
            .arg(workspace.join("tools/linux-field-fixture.py"))
            .arg(directory.join(format!("{name}.json")))
            .arg(directory.join(format!("{name}.command")))
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    )
}
fn wait_target(desktop: &LinuxDesktop, pid: u32) -> InputTarget {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let state = desktop.refresh().unwrap();
        if state.pid == pid && state.control != 0 {
            return desktop.input_target().unwrap().unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "fixture editor unavailable: {state:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires private GNOME/KDE Wayland; use tools/test-linux-focus-capture.sh"]
fn wayland_picker_capture_recovers_metadata_and_rejects_stale_fields() {
    use crate::services::LinuxClipboard;
    use okbs_platform::Clipboard;
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scratch = workspace.join("tmp/linux-capture-tests");
    std::fs::create_dir_all(&scratch).unwrap();
    let directory = tempfile::TempDir::new_in(scratch).unwrap().keep();
    let _bridge =
        (SessionInfo::detect().desktop == Desktop::Kde).then(|| serve_kde_bridge().unwrap());
    let desktop = LinuxDesktop::connect().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !desktop.integration_ready() {
        assert!(Instant::now() < deadline);
        desktop.refresh().unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    let first = editor(&workspace, &directory, "first");
    let original = wait_target(&desktop, first.0.id());
    let observing_desktop = desktop.clone();
    let observing = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let ongoing = observing.clone();
    let observer = std::thread::spawn(move || {
        let mut windows = std::collections::BTreeSet::new();
        while ongoing.load(std::sync::atomic::Ordering::Acquire) {
            let state = observing_desktop.refresh().unwrap();
            windows.insert((state.window, state.pid));
            std::thread::sleep(Duration::from_millis(5));
        }
        windows
    });
    let mut clipboard = LinuxClipboard::new().unwrap();
    let synthetic = "Привет, clipboard\nsecond line\\tail";
    for _ in 0..16 {
        clipboard.set_text(synthetic).unwrap();
        assert_eq!(clipboard.text().unwrap().as_deref(), Some(synthetic));
        std::thread::sleep(Duration::from_millis(10));
    }
    observing.store(false, std::sync::atomic::Ordering::Release);
    assert_eq!(
        observer.join().unwrap(),
        std::collections::BTreeSet::from([(original.window, first.0.id())]),
        "clipboard access moved focus to a helper surface"
    );
    desktop
        .accessibility
        .make_temporarily_unavailable(Duration::from_millis(100));
    assert_eq!(
        desktop.refresh().unwrap().control,
        0,
        "missing metadata was not simulated"
    );
    assert_eq!(desktop.capture_input_target().unwrap(), Some(original));

    desktop
        .accessibility
        .make_temporarily_unavailable(Duration::from_millis(180));
    std::fs::write(directory.join("first.command"), "second").unwrap();
    let changed = desktop.capture_input_target().unwrap().unwrap();
    assert_eq!(changed.window, original.window);
    assert_ne!(
        changed.control, original.control,
        "capture retained the old field"
    );
    assert!(
        desktop.activate_target(original).is_err(),
        "old field unexpectedly accepted"
    );
    assert_eq!(desktop.input_target().unwrap(), Some(changed));

    desktop
        .accessibility
        .make_temporarily_unavailable(Duration::from_millis(600));
    let started = Instant::now();
    assert!(
        desktop.capture_input_target().is_err(),
        "identified editor degraded to window-only target"
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "capture retry exceeded its bound"
    );
    assert_eq!(wait_target(&desktop, first.0.id()), changed);

    let second = editor(&workspace, &directory, "other");
    let other = wait_target(&desktop, second.0.id());
    desktop.activate_target(changed).unwrap();
    desktop
        .accessibility
        .make_temporarily_unavailable(Duration::from_millis(250));
    let switch_desktop = desktop.clone();
    let switching = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        switch_desktop
            .command("ActivateWindow", other.window)
            .unwrap();
    });
    assert!(
        desktop.capture_input_target().is_err(),
        "picker crossed to another window"
    );
    switching.join().unwrap();
    assert_eq!(wait_target(&desktop, second.0.id()), other);
}
