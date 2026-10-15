//! User-session integrations. No shell interpolation is used for user paths.
#![allow(unsafe_code)]
use crate::desktop::error;
use crate::watch::Watch;
use okbs_core::{KeyMap, PhysKey};
use okbs_platform::{
    Autostart, Clipboard, FileDialogs, FileRequest, Result, SoundPlayer, StopGuard, SystemSettings,
};
use std::ffi::CString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub fn keymap(code: &str, variant: &str) -> Result<KeyMap> {
    let library =
        xkbcommon_dl::xkbcommon_option().ok_or_else(|| error("libxkbcommon is not installed"))?;
    let (code, inline_variant) = code.split_once('+').unwrap_or((code, ""));
    let code = CString::new(code).map_err(error)?;
    let variant = CString::new(if variant.is_empty() {
        inline_variant
    } else {
        variant
    })
    .map_err(error)?;
    let names = xkbcommon_dl::xkb_rule_names {
        rules: std::ptr::null(),
        model: c"pc105".as_ptr(),
        layout: code.as_ptr(),
        variant: variant.as_ptr(),
        options: c"".as_ptr(),
    };
    // SAFETY: names/CStrings outlive compilation; the library owns contexts
    // and keymaps, and symbol slices are borrowed only until the next call.
    unsafe {
        let context = (library.xkb_context_new)(
            xkbcommon_dl::xkb_context_flags::XKB_CONTEXT_NO_ENVIRONMENT_NAMES,
        );
        if context.is_null() {
            return Err(error("cannot create an XKB context"));
        }
        let raw = (library.xkb_keymap_new_from_names)(
            context,
            &names,
            xkbcommon_dl::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
        );
        (library.xkb_context_unref)(context);
        if raw.is_null() {
            return Err(error("cannot compile the installed keyboard layout"));
        }
        let mut map = KeyMap::new();
        for &key in PhysKey::ALL {
            let mut chars = [None; 2];
            for (level, slot) in chars.iter_mut().enumerate() {
                let mut symbols = std::ptr::null();
                let count = (library.xkb_keymap_key_get_syms_by_level)(
                    raw,
                    u32::from(key.evdev_code()) + 8,
                    0,
                    level as u32,
                    &mut symbols,
                );
                if count > 0 && !symbols.is_null() {
                    *slot = char::from_u32((library.xkb_keysym_to_utf32)(*symbols))
                        .filter(|c| !c.is_control());
                }
            }
            map.set(key, chars[0], chars[1]);
        }
        (library.xkb_keymap_unref)(raw);
        Ok(map)
    }
}

pub struct LinuxClipboard {
    native: Option<arboard::Clipboard>,
    wayland: bool,
    /// How often [`Clipboard::subscribe`] compares the clipboard: Linux
    /// desktops give no portable change notification.
    poll: Duration,
}
impl std::fmt::Debug for LinuxClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinuxClipboard")
            .field("wayland", &self.wayland)
            .finish()
    }
}
impl LinuxClipboard {
    pub fn new() -> Result<Self> {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            && std::env::var("XDG_SESSION_TYPE").as_deref() != Ok("x11");
        Ok(Self {
            native: if wayland {
                None
            } else {
                Some(arboard::Clipboard::new().map_err(error)?)
            },
            wayland,
            poll: Duration::from_millis(500),
        })
    }
    /// «clipboard.poll_interval_ms» for later subscriptions.
    pub fn with_poll_interval(mut self, ms: u32) -> Self {
        self.poll = Duration::from_millis(u64::from(ms.max(1)));
        self
    }
}
impl Clipboard for LinuxClipboard {
    fn text(&mut self) -> Result<Option<String>> {
        if let Some(clipboard) = &mut self.native {
            return match clipboard.get_text() {
                Ok(s) => Ok(Some(s)),
                Err(arboard::Error::ContentNotAvailable) => Ok(None),
                Err(e) => Err(error(e)),
            };
        }
        if let Ok(bus) = zbus::blocking::Connection::session()
            && let Ok(proxy) = zbus::blocking::Proxy::new(
                &bus,
                "org.own_keyboard_switch.Gnome",
                "/org/own_keyboard_switch/Gnome",
                "org.own_keyboard_switch.Gnome",
            )
            && let Ok(text) = proxy.call::<_, _, String>("GetClipboard", &())
        {
            return Ok(Some(text));
        }
        // ext-data-control on current KDE, wlr-data-control on older desktops.
        // These protocols never need the wl-clipboard fallback's focus window.
        use wl_clipboard_rs::paste::{self, ClipboardType, MimeType, Seat};
        match paste::get_contents(ClipboardType::Regular, Seat::Unspecified, MimeType::Text) {
            Ok((mut pipe, _)) => {
                let mut text = String::new();
                pipe.read_to_string(&mut text).map_err(error)?;
                return Ok(Some(text));
            }
            Err(
                paste::Error::NoSeats | paste::Error::ClipboardEmpty | paste::Error::NoMimeType,
            ) => return Ok(None),
            Err(paste::Error::MissingProtocol { .. }) => {}
            Err(err) => return Err(error(err)),
        }
        let output = Command::new("wl-paste")
            .args(["--no-newline", "--type", "text"])
            .output()
            .map_err(error)?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(Some(String::from_utf8(output.stdout).map_err(error)?))
    }
    fn set_text(&mut self, text: &str) -> Result<()> {
        if let Some(clipboard) = &mut self.native {
            return clipboard.set_text(text).map_err(error);
        }
        if let Ok(bus) = zbus::blocking::Connection::session()
            && let Ok(proxy) = zbus::blocking::Proxy::new(
                &bus,
                "org.own_keyboard_switch.Gnome",
                "/org/own_keyboard_switch/Gnome",
                "org.own_keyboard_switch.Gnome",
            )
            && proxy.call::<_, _, ()>("SetClipboard", &(text,)).is_ok()
        {
            return Ok(());
        }
        use wl_clipboard_rs::copy::{self, MimeType, Options, Source};
        match Options::new().copy(Source::Bytes(text.as_bytes().into()), MimeType::Text) {
            Ok(()) => return Ok(()),
            Err(copy::Error::MissingProtocol { .. }) => {}
            Err(err) => return Err(error(err)),
        }
        let mut child = Command::new("wl-copy")
            .args(["--type", "text/plain;charset=utf-8"])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(error)?;
        child
            .stdin
            .take()
            .ok_or_else(|| error("cannot write clipboard"))?
            .write_all(text.as_bytes())
            .map_err(error)?;
        if child.wait().map_err(error)?.success() {
            Ok(())
        } else {
            Err(error("wl-copy failed"))
        }
    }
    fn clear(&mut self) -> Result<()> {
        if let Some(clipboard) = &mut self.native {
            return clipboard.clear().map_err(error);
        }
        self.set_text("")
    }
    fn subscribe(&mut self, sink: crossbeam_channel::Sender<()>) -> Result<Box<dyn StopGuard>> {
        let poll = self.poll;
        let mut clipboard = Self::new()?;
        let mut previous = clipboard_fingerprint(clipboard.text()?);
        Ok(Box::new(Watch::spawn(
            move || {
                if let Ok(text) = clipboard.text() {
                    let fingerprint = clipboard_fingerprint(text);
                    if fingerprint != previous {
                        previous = fingerprint;
                        return sink.send(()).is_ok();
                    }
                }
                true
            },
            poll,
        )))
    }
}
fn clipboard_fingerprint(text: Option<String>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}
#[derive(Debug, Clone)]
pub struct LinuxAutostart {
    path: PathBuf,
}
impl LinuxAutostart {
    pub fn new() -> Result<Self> {
        let home = std::env::var_os("HOME").ok_or_else(|| error("HOME is not set"))?;
        // The base directory specification ignores relative values.
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| PathBuf::from(home).join(".config"));
        Ok(Self::at(config.join("autostart/okbswitch.desktop")))
    }
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
    /// Startup reconciliation: a new profile gets the entry, as the Windows
    /// installer does. Later starts keep what the user chose, including in the
    /// desktop's own startup settings, and only refresh the program path of an
    /// enabled entry: an AppImage may have been moved.
    pub fn refresh(&self, new_profile: bool, wanted: bool, executable: &Path) -> Result<()> {
        if (new_profile && wanted) || self.is_enabled()? {
            self.set_enabled(true, executable, false)?;
        }
        Ok(())
    }
}
fn desktop_exec(path: &Path) -> String {
    // The quoting rule escapes `"`, `` ` ``, `$` and `\` inside a quoted
    // argument; the string escape rule then doubles every backslash again.
    let quoted = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$");
    format!("\"{}\"", quoted.replace('\\', "\\\\").replace('%', "%%"))
}
/// Whether a desktop entry starts at login: `Hidden=true` is the standard way
/// to disable it and GNOME also honours `X-GNOME-Autostart-enabled=false`.
fn starts_at_login(entry: &str) -> bool {
    let mut main = false;
    for line in entry.lines().map(str::trim) {
        if line.starts_with('[') {
            main = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if main
            && matches!(
                (key.trim(), value.trim()),
                ("Hidden", "true") | ("X-GNOME-Autostart-enabled", "false")
            )
        {
            return false;
        }
    }
    true
}
impl Autostart for LinuxAutostart {
    fn is_enabled(&self) -> Result<bool> {
        match std::fs::read_to_string(&self.path) {
            Ok(entry) => Ok(starts_at_login(&entry)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(error(e)),
        }
    }
    fn set_enabled(&self, enabled: bool, executable: &Path, _elevated: bool) -> Result<()> {
        let image = std::env::var_os("APPIMAGE").map(PathBuf::from);
        let executable = image.as_deref().unwrap_or(executable);
        if enabled {
            let entry = format!(
                "[Desktop Entry]\nType=Application\nName=Own Keyboard Switch\nExec={}\nIcon=okbswitch\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
                desktop_exec(executable)
            );
            if std::fs::read_to_string(&self.path).is_ok_and(|current| current == entry) {
                return Ok(());
            }
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent).map_err(error)?;
            }
            std::fs::write(&self.path, entry).map_err(error)
        } else {
            match std::fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(error(e)),
            }
        }
    }
}
#[derive(Debug)]
pub struct LinuxSound;
impl SoundPlayer for LinuxSound {
    fn play_wav(&self, data: &'static [u8]) -> Result<()> {
        let mut file = tempfile::NamedTempFile::new().map_err(error)?;
        file.write_all(data).map_err(error)?;
        std::thread::spawn(move || {
            let _ = play_audio(file.path());
        });
        Ok(())
    }
    fn play_file(&self, path: &Path) -> Result<()> {
        let path = path.to_owned();
        std::thread::spawn(move || {
            let _ = play_audio(&path);
        });
        Ok(())
    }
    fn beep(&self) -> Result<()> {
        spawn_reaped(Command::new("canberra-gtk-play").args(["--id", "bell"])).map(|_| ())
    }
}
fn play_audio(path: &Path) -> Result<()> {
    for program in ["pw-play", "paplay", "aplay"] {
        match Command::new(program).arg(path).status() {
            Ok(s) if s.success() => return Ok(()),
            _ => {}
        }
    }
    Err(error("no supported WAV player is installed"))
}
#[derive(Debug)]
pub struct LinuxFileDialogs;
impl FileDialogs for LinuxFileDialogs {
    fn open(&self, request: &FileRequest) -> Result<Option<PathBuf>> {
        choose_file(request, false)
    }
    fn save(&self, request: &FileRequest) -> Result<Option<PathBuf>> {
        choose_file(request, true)
    }
}
fn choose_file(request: &FileRequest, save: bool) -> Result<Option<PathBuf>> {
    let mut command = Command::new("zenity");
    command
        .args(["--file-selection", "--title"])
        .arg(&request.title);
    if save {
        command
            .args(["--save", "--confirm-overwrite", "--filename"])
            .arg(&request.file_name);
    }
    let output = command.output().map_err(error)?;
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    if !output.status.success() {
        return Err(error("native file dialog failed"));
    }
    let path = String::from_utf8(output.stdout).map_err(error)?;
    Ok(Some(PathBuf::from(path.trim_end_matches(['\n', '\r']))))
}
#[derive(Debug)]
pub struct LinuxSystemSettings;
impl SystemSettings for LinuxSystemSettings {
    fn open_keyboard_settings(&self) -> Result<()> {
        let (program, args) = if std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains("kde")
        {
            ("systemsettings", vec!["kcm_keyboard"])
        } else {
            ("gnome-control-center", vec!["keyboard"])
        };
        spawn_reaped(Command::new(program).args(args)).map(|_| ())
    }
}
/// Starts a program without waiting for it; a thread reaps it on exit, as an
/// unwaited child would stay a zombie until this program ends.
fn spawn_reaped(command: &mut Command) -> Result<u32> {
    let mut child = command.spawn().map_err(error)?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

pub fn password_field(pid: u32) -> Result<Option<bool>> {
    if pid == 0 {
        return Ok(None);
    }
    let session = zbus::blocking::Connection::session().map_err(error)?;
    let bus_proxy =
        zbus::blocking::Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus")
            .map_err(error)?;
    let address: String = match bus_proxy.call("GetAddress", &()) {
        Ok(address) => address,
        Err(_) => return Ok(None),
    };
    let bus = zbus::blocking::connection::Builder::address(address.as_str())
        .map_err(error)?
        .build()
        .map_err(error)?;
    let root = (
        "org.a11y.atspi.Registry".to_string(),
        zbus::zvariant::OwnedObjectPath::try_from("/org/a11y/atspi/accessible/root")
            .map_err(error)?,
    );
    let mut queue = std::collections::VecDeque::from([root]);
    let mut visited = std::collections::HashSet::new();
    let deadline = std::time::Instant::now() + Duration::from_millis(150);
    while let Some((name, path)) = queue.pop_front() {
        if std::time::Instant::now() >= deadline {
            break;
        }
        if !visited.insert((name.clone(), path.clone())) || visited.len() > 512 {
            continue;
        }
        let accessible = zbus::blocking::Proxy::new(
            &bus,
            name.as_str(),
            path.as_str(),
            "org.a11y.atspi.Accessible",
        )
        .map_err(error)?;
        let state: Vec<u32> = accessible.call("GetState", &()).unwrap_or_default();
        if state.first().is_some_and(|bits| bits & (1 << 12) != 0) {
            let owner: u32 = zbus::blocking::fdo::DBusProxy::new(&bus)
                .map_err(error)?
                .get_connection_unix_process_id(name.as_str().try_into().map_err(error)?)
                .map_err(error)?;
            if owner == pid {
                let role: u32 = accessible.call("GetRole", &()).map_err(error)?;
                return Ok(Some(role == 40));
            }
        }
        let children: Vec<(String, zbus::zvariant::OwnedObjectPath)> =
            accessible.call("GetChildren", &()).unwrap_or_default();
        queue.extend(
            children
                .into_iter()
                .filter(|(_, path)| path.as_str() != "/org/a11y/atspi/null"),
        );
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_keymaps_follow_xkb_variants() {
        let ru = keymap("ru", "").unwrap();
        let en = keymap("us", "").unwrap();
        assert_eq!(ru.get(PhysKey::KeyQ, false), Some('й'));
        assert_eq!(en.get(PhysKey::KeyQ, false), Some('q'));
        assert_eq!(
            keymap("us", "dvorak").unwrap().get(PhysKey::KeyQ, false),
            Some('\'')
        );
    }
    #[test]
    fn autostart_quotes_paths_without_shell_execution() {
        assert_eq!(
            desktop_exec(Path::new("/home/user/A $B%/okbswitch")),
            "\"/home/user/A \\\\$B%%/okbswitch\""
        );
        // Read back the way GLib launches it: string unescape, `%%` field
        // code, then the quoting rule.
        let path = "/home/user/A $B%\\ \"q\" `c`/okbswitch";
        let file = gtk::glib::KeyFile::new();
        file.load_from_data(
            &format!("[Desktop Entry]\nExec={}\n", desktop_exec(Path::new(path))),
            gtk::glib::KeyFileFlags::NONE,
        )
        .unwrap();
        let exec = file.string("Desktop Entry", "Exec").unwrap();
        let argv = gtk::glib::shell_parse_argv(exec.replace("%%", "%")).unwrap();
        assert_eq!(argv, [std::ffi::OsString::from(path)]);
    }
    #[test]
    fn systemd_login_autostart_accepts_the_entry_and_its_off_state() {
        // Plasma 6 and GNOME start XDG autostart entries through this generator.
        let generator =
            Path::new("/usr/lib/systemd/user-generators/systemd-xdg-autostart-generator");
        if !generator.exists() {
            eprintln!("systemd XDG autostart generator not installed; skipped");
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config");
        let autostart = LinuxAutostart::at(config.join("autostart/okbswitch.desktop"));
        let program = directory.path().join("Own Keyboard Switch $1/okbswitch");
        let units = |name: &str| {
            let output = directory.path().join(name);
            for unit in ["normal", "early", "late"] {
                std::fs::create_dir_all(output.join(unit)).unwrap();
            }
            let status = Command::new(generator)
                .args(["normal", "early", "late"].map(|unit| output.join(unit)))
                .env("XDG_CONFIG_HOME", &config)
                .env("XDG_CONFIG_DIRS", directory.path().join("none"))
                .status()
                .unwrap();
            assert!(status.success());
            // The generator writes login units into its "late" directory.
            std::fs::read_to_string(output.join("late/app-okbswitch@autostart.service")).ok()
        };
        // It skips entries whose program does not exist.
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(
            &program,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        autostart.set_enabled(true, &program, false).unwrap();
        let unit = units("on").expect("no login unit for the enabled entry");
        let exec = unit
            .lines()
            .find(|line| line.starts_with("ExecStart="))
            .expect("ExecStart");
        assert!(exec.contains("okbswitch"), "{exec}");
        // systemd escapes `$` in ExecStart; the path itself was restored.
        assert!(exec.contains(r"Own Keyboard Switch \$1"), "{exec}");
        let hidden = std::fs::read_to_string(config.join("autostart/okbswitch.desktop")).unwrap()
            + "Hidden=true\n";
        std::fs::write(config.join("autostart/okbswitch.desktop"), hidden).unwrap();
        assert_eq!(units("hidden"), None, "a Hidden entry must not start");
    }
    #[test]
    fn detached_programs_do_not_stay_zombies() {
        let pid = spawn_reaped(&mut Command::new("true")).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        // A zombie keeps its /proc entry until its parent waits for it.
        while Path::new(&format!("/proc/{pid}")).exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "child {pid} was not reaped"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn autostart_respects_entries_disabled_by_the_desktop() {
        assert!(starts_at_login(
            "[Desktop Entry]\nExec=okbswitch\nX-GNOME-Autostart-enabled=true\n"
        ));
        assert!(!starts_at_login("[Desktop Entry]\nHidden=true\n"));
        assert!(!starts_at_login(
            "[Desktop Entry]\nX-GNOME-Autostart-enabled = false\n"
        ));
        assert!(starts_at_login(
            "[Desktop Entry]\nExec=okbswitch\n[Desktop Action x]\nHidden=true\n"
        ));
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("autostart/okbswitch.desktop");
        let autostart = LinuxAutostart::at(path.clone());
        let old = Path::new("/old/okbswitch");
        let new = Path::new("/new/okbswitch");
        // An existing profile without an entry stays without one.
        autostart.refresh(false, true, new).unwrap();
        assert!(!path.exists());
        // A new profile registers itself.
        autostart.refresh(true, true, old).unwrap();
        assert!(autostart.is_enabled().unwrap());
        // A moved program updates its enabled entry.
        autostart.refresh(false, true, new).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("Exec=\"/new/okbswitch\"")
        );
        // An entry disabled in the desktop settings is reported and kept.
        let hidden = "[Desktop Entry]\nExec=\"/new/okbswitch\"\nHidden=true\n";
        std::fs::write(&path, hidden).unwrap();
        assert!(!autostart.is_enabled().unwrap());
        autostart.refresh(false, true, new).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), hidden);
        // Turning it on in the program rewrites it as enabled.
        autostart.set_enabled(true, new, false).unwrap();
        assert!(autostart.is_enabled().unwrap());
        autostart.set_enabled(false, new, false).unwrap();
        assert!(!path.exists());
    }
}
