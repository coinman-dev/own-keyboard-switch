//! Programs offered as exclusions on the first start: terminals and code
//! editors, where commands and identifiers are easily taken for words typed
//! in the other layout.

use super::{Exclusions, ExecutableExclusion};

/// Terminals and command lines (Windows file names).
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

/// Development environments and code editors (Windows file names).
pub const SUGGESTED_EDITORS: &[&str] = &[
    "Code.exe",
    "Code - Insiders.exe",
    "Cursor.exe",
    "devenv.exe",
    "zed.exe",
    "sublime_text.exe",
    "idea64.exe",
    "pycharm64.exe",
    "webstorm64.exe",
    "phpstorm64.exe",
    "clion64.exe",
    "rider64.exe",
    "goland64.exe",
    "rustrover64.exe",
    "datagrip64.exe",
    "studio64.exe",
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
