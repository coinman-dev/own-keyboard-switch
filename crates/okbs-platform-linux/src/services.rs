//! User-session integrations. No shell interpolation is used for user paths.
#![allow(unsafe_code)]
use crate::desktop::error;
use crate::watch::Watch;
use okbs_core::{KeyMap, PhysKey};
use okbs_platform::{
    Autostart, Clipboard, FileDialogs, FileRequest, Result, SoundPlayer, StopGuard, SystemSettings,
};
use std::ffi::CString;
use std::io::Write;
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
        })
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
            Duration::from_millis(250),
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
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(home).join(".config"));
        Ok(Self {
            path: config.join("autostart/okbswitch.desktop"),
        })
    }
}
fn desktop_exec(path: &Path) -> String {
    format!(
        "\"{}\"",
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('`', "\\`")
            .replace('$', "\\$")
            .replace('%', "%%")
    )
}
impl Autostart for LinuxAutostart {
    fn is_enabled(&self) -> Result<bool> {
        Ok(self.path.exists())
    }
    fn set_enabled(&self, enabled: bool, executable: &Path, _elevated: bool) -> Result<()> {
        let image = std::env::var_os("APPIMAGE").map(PathBuf::from);
        let executable = image.as_deref().unwrap_or(executable);
        if enabled {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent).map_err(error)?;
            }
            std::fs::write(&self.path, format!("[Desktop Entry]\nType=Application\nName=Own Keyboard Switch\nExec={}\nIcon=okbswitch\nTerminal=false\nX-GNOME-Autostart-enabled=true\n", desktop_exec(executable))).map_err(error)
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
        Command::new("canberra-gtk-play")
            .args(["--id", "bell"])
            .spawn()
            .map(|_| ())
            .map_err(error)
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
        Command::new(program)
            .args(args)
            .spawn()
            .map(|_| ())
            .map_err(error)
    }
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
            "\"/home/user/A \\$B%%/okbswitch\""
        );
    }
}
