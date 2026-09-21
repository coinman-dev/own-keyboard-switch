//! Checks of access to input devices.

use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Result of trying to open a device node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceAccess {
    /// Opened successfully.
    Granted,
    /// Exists but permission was denied.
    Denied,
    /// Does not exist.
    Missing,
    /// Another error.
    Failed,
}

fn classify(result: std::io::Result<std::fs::File>) -> DeviceAccess {
    match result {
        Ok(_) => DeviceAccess::Granted,
        Err(e) if e.kind() == ErrorKind::PermissionDenied => DeviceAccess::Denied,
        Err(e) if e.kind() == ErrorKind::NotFound => DeviceAccess::Missing,
        Err(_) => DeviceAccess::Failed,
    }
}

/// Whether `/dev/uinput` can be opened for writing.
pub fn uinput_access() -> DeviceAccess {
    let result = OpenOptions::new().write(true).open("/dev/uinput");
    match classify(result) {
        DeviceAccess::Missing => classify(OpenOptions::new().write(true).open("/dev/input/uinput")),
        other => other,
    }
}

/// An evdev device that reports letter keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardDevice {
    /// Device node, e.g. `/dev/input/event3`.
    pub path: PathBuf,
    /// Kernel device name.
    pub name: String,
}

/// Summary of `/dev/input/event*` access.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputScan {
    /// `/dev/input` exists.
    pub input_dir_exists: bool,
    /// Number of `event*` nodes.
    pub event_nodes: usize,
    /// Nodes that could not be opened because of permissions.
    pub denied: usize,
    /// Readable devices that look like keyboards.
    pub keyboards: Vec<KeyboardDevice>,
}

/// Enumerates event devices and finds readable keyboards.
pub fn scan_input_devices() -> InputScan {
    scan_dir(Path::new("/dev/input"))
}

fn scan_dir(dir: &Path) -> InputScan {
    let mut scan = InputScan::default();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return scan;
    };
    scan.input_dir_exists = true;
    let mut nodes: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("event"))
        })
        .collect();
    nodes.sort();
    scan.event_nodes = nodes.len();
    for path in nodes {
        match evdev::Device::open(&path) {
            Ok(device) => {
                let is_keyboard = device.supported_keys().is_some_and(|keys| {
                    [
                        evdev::KeyCode::KEY_A,
                        evdev::KeyCode::KEY_Z,
                        evdev::KeyCode::KEY_SPACE,
                    ]
                    .iter()
                    .all(|&k| keys.contains(k))
                });
                if is_keyboard {
                    scan.keyboards.push(KeyboardDevice {
                        name: device.name().unwrap_or("unknown").to_string(),
                        path,
                    });
                }
            }
            Err(e) if e.kind() == ErrorKind::PermissionDenied => scan.denied += 1,
            Err(_) => {}
        }
    }
    scan
}

/// Whether the process belongs to the `input` group.
pub fn in_input_group() -> Option<bool> {
    let group = std::fs::read_to_string("/etc/group").ok()?;
    let gid = group_gid(&group, "input")?;
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    Some(process_groups(&status).contains(&gid))
}

fn group_gid(group_file: &str, name: &str) -> Option<u32> {
    group_file.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.next()? == name).then_some(())?;
        fields.nth(1)?.parse().ok()
    })
}

fn process_groups(status: &str) -> Vec<u32> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("Groups:"))
        .map(|rest| {
            rest.split_whitespace()
                .filter_map(|g| g.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Paths where the udev rule of the package may be installed.
pub const UDEV_RULE_PATHS: [&str; 2] = [
    "/usr/lib/udev/rules.d/70-okbswitch.rules",
    "/etc/udev/rules.d/70-okbswitch.rules",
];

/// Installed udev rule, if any.
pub fn udev_rule() -> Option<&'static str> {
    UDEV_RULE_PATHS.into_iter().find(|p| Path::new(p).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_group_file() {
        let file = "root:x:0:\ninput:x:104:alice\nplugdev:x:46:bob\n";
        assert_eq!(group_gid(file, "input"), Some(104));
        assert_eq!(group_gid(file, "video"), None);
    }

    #[test]
    fn parses_process_groups() {
        let status = "Name:\tbash\nGroups:\t4 24 27 104 \nVmPeak:\t1 kB\n";
        assert_eq!(process_groups(status), vec![4, 24, 27, 104]);
        assert_eq!(process_groups("Name: x\n"), Vec::<u32>::new());
    }

    #[test]
    fn missing_dir_scan() {
        let scan = scan_dir(Path::new("/nonexistent/okbswitch/input"));
        assert!(!scan.input_dir_exists);
        assert_eq!(scan.event_nodes, 0);
    }
}
