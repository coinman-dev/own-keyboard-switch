//! Programs offered as exclusions on the first start: only terminals, where
//! commands are easily taken for words typed in the other layout. Editors are
//! left to the user, who adds them when needed.

use super::{Exclusions, ExecutableExclusion};

/// Terminals and command lines (Windows file names).
#[cfg(not(target_os = "linux"))]
pub const SUGGESTED_TERMINALS: &[&str] = &[
    "WindowsTerminal.exe",
    "OpenConsole.exe",
    "conhost.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "mintty.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "ConEmu64.exe",
    "putty.exe",
];

/// Terminals offered in Linux, using their actual process file names.
#[cfg(target_os = "linux")]
pub const SUGGESTED_TERMINALS: &[&str] = &[
    "gnome-terminal-server",
    "konsole",
    "kitty",
    "alacritty",
    "wezterm-gui",
    "foot",
    "xterm",
    "ptyxis",
    "kgx",
    "tilix",
    "xfce4-terminal",
];

/// Adds the file names that are not excluded yet. Returns how many were added.
pub fn add_executables(exclusions: &mut Exclusions, names: &[&str]) -> usize {
    let mut added = 0;
    for name in names {
        let present = exclusions
            .executables
            .iter()
            .any(|e| e.path.trim().eq_ignore_ascii_case(name));
        if !present {
            exclusions.executables.push(ExecutableExclusion {
                path: (*name).to_string(),
            });
            added += 1;
        }
    }
    added
}
