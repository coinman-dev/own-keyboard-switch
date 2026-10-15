[English](README.md) | [Русский](README.ru_RU.md)

<p align="center">
  <img src="images/logo-settings.png" alt="Own Keyboard Switch" width="160">
</p>

<h1 align="center">Own Keyboard Switch</h1>
<p align="center">Automatic Russian ↔ English keyboard layout switching for Windows and Linux.</p>

[![Release](https://img.shields.io/badge/release-0.4.0--beta-orange)](https://github.com/coinman-dev/own-keyboard-switch/releases/tag/v0.4.0-beta)
[![Build](https://github.com/coinman-dev/own-keyboard-switch/actions/workflows/release.yml/badge.svg)](https://github.com/coinman-dev/own-keyboard-switch/actions/workflows/release.yml)
[![Windows](https://img.shields.io/badge/Windows-10%20%2F%2011%20x64-blue)](#download)
[![Linux](https://img.shields.io/badge/Linux-X11%20%7C%20GNOME%20%7C%20KDE-blue)](#linux)
[![Rust](https://img.shields.io/badge/Rust-1.95%2B-orange)](#building)
[![License](https://img.shields.io/badge/license-PolyForm%20Noncommercial-blue)](LICENSE)

**Own Keyboard Switch** corrects words typed with the wrong keyboard layout:
`ghbdtn` → `привет`, `руддщ` → `hello`. It runs in the tray, supports custom rules
and text expansions, and includes clipboard tools and a floating layout indicator.

> **0.4.0-beta is a prerelease for Windows and, for the first time, Linux**
> (X11, GNOME and KDE Plasma 6 on Wayland). The Linux version is new: it was
> tested in isolated sessions, not yet on many real computers.

## Download

| Edition | Download | Usage |
|---|---|---|
| Portable | [okbswitch-portable.exe](https://github.com/coinman-dev/own-keyboard-switch/releases/download/v0.4.0-beta/okbswitch-portable.exe) | Put the file in a writable folder and run it. No installation required. |
| Installer | [okbswitch-install.exe](https://github.com/coinman-dev/own-keyboard-switch/releases/download/v0.4.0-beta/okbswitch-install.exe) | Choose a current-user or all-users installation, shortcuts and startup options. |
| Linux package | [okbswitch_0.4.0-beta_amd64.deb](https://github.com/coinman-dev/own-keyboard-switch/releases/download/v0.4.0-beta/okbswitch_0.4.0-beta_amd64.deb) | Ubuntu 24.04 or newer: `sudo apt install ./okbswitch_0.4.0-beta_amd64.deb`. |
| Linux AppImage | [okbswitch-0.4.0-beta-x86_64.AppImage](https://github.com/coinman-dev/own-keyboard-switch/releases/download/v0.4.0-beta/okbswitch-0.4.0-beta-x86_64.AppImage) | Make it executable and run it; settings are kept next to the file. |

Windows 10/11 x64; Linux x86_64 with X11, GNOME 45–50 or KDE Plasma 6. On Linux
the first start asks for permission (pkexec) to give your session access to the
keyboards. Dictionaries and language models are built in. Typed text never
leaves the computer; the network is used only when you download an optional
dictionary in Settings. No advertising, no update checks.
[All releases](https://github.com/coinman-dev/own-keyboard-switch/releases) ·
[Changelog](CHANGELOG.md).

## Quick start

1. Run the portable file or complete the installer. On Linux see [Linux](#linux).
2. Double-click the tray icon to open Settings (a single click in KDE Plasma).
3. Keep automatic switching enabled and type a word followed by Space or Enter.
4. Press **Break** to undo an automatic correction or convert the current word.

The interface supports English and Russian, and light, dark and system themes.
You can change shortcuts and excluded applications in Settings.

## Features

- RU/EN detection using built-in dictionaries, language models and additional rules.
- Optional **Improve switching**: recognizes more lowercase two/three-letter English
  words after a Russian word or letter. Russian words and names remain protected.
- Correction before Enter reaches PowerShell, a Windows terminal or a Linux
  terminal.
- Single-key layout switching, case correction and manual conversion of selections.
- **Break** converts the text since the last space, symbols included; without a
  remembered word it converts the word left of the caret.
- Passwords typed in the Russian layout are retyped in English, and password
  fields get the English layout from the first character (can be switched off).
- Autoreplace with Space, Enter, Tab, a tooltip or a shortcut; multiline entries,
  a remembered caret position and a floating insertion list.
- Clipboard layout conversion, transliteration, spellchecking and history.
- Spell checking while typing: suggestions at the text caret or automatic fixes
  of unambiguous typing slips, personal words and optional dictionaries.
- Floating layout indicator, configurable tray flags and sounds; the tray menu
  switches between the installed layouts.
- Application exclusions (terminals and IDEs are offered on the first start),
  password-field checks, startup and optional elevation.
- Switching rules can be exported to a file and imported back.
- Floating lists stay inside the monitor working area, including at different DPI.

Clipboard operations preserve **plain text**; preservation of images and rich text
is not implemented. Saving clipboard history between runs is off by default; when
enabled, copied sensitive text can also be stored in that history.

## Linux

The Linux version is a first prerelease. It works on x86_64 in these sessions:

| Session | How the layout is switched | Notes |
|---|---|---|
| X11 (any desktop) | XKB | Nothing to install. |
| GNOME 45–50 on Wayland | Bundled GNOME Shell extension | Installed by the program; sign out and back in once after the first start. |
| KDE Plasma 6 on Wayland | KDE layout service and a bundled KWin script | Installed and loaded by the program in the running session. |

Other Wayland compositors (Sway, Hyprland and others) are not supported yet.
Add the Russian and English layouts in the system keyboard settings first.

**Install.** On Ubuntu 24.04 or newer install the package:

```sh
sudo apt install ./okbswitch_0.4.0-beta_amd64.deb
```

It adds the program to the application menu and installs a udev rule
(`/usr/lib/udev/rules.d/70-okbswitch.rules`) that gives the user of the active
local session access to keyboards and to `/dev/uinput`. Membership in the
`input` group or root is not needed. The AppImage needs no installation:

```sh
chmod +x okbswitch-0.4.0-beta-x86_64.AppImage
./okbswitch-0.4.0-beta-x86_64.AppImage
```

**First start.** A setup window checks the keyboard access, the desktop
connection and the layouts. Without access to the keyboards it offers
**Set up keyboard access**: the system asks for the administrator password
(pkexec) for a small setup helper, which installs the same udev rule and loads
the `evdev` and `uinput` modules. The program itself always runs as a regular
user. **Set up desktop integration** installs the GNOME extension or the KWin
script for your user; on GNOME sign out and back in, then press **Check again**.
Keys are not captured until setup is complete.

**How it works.** The program reads the keyboards through evdev and passes
everything you type on through a virtual uinput keyboard, so it works in all
applications, including terminals and Wayland windows. Note that, as with any
layout switcher on Wayland, the access granted by the udev rule lets programs
of your user read keyboard events. In the **Linux** tab of Settings you can
choose the switching method and limit the program to particular keyboards.

**Data.** The installed program keeps its files in the XDG folders of your user;
the AppImage and `okbswitch --portable` keep them next to the program, like the
Windows portable edition:

| File | Installed (`.deb`) | AppImage, `--portable` |
|---|---|---|
| Settings | `~/.config/okbswitch/config.toml` | `data/config.toml` |
| Clipboard history, if saved | `~/.local/state/okbswitch/` | `data/` |
| Logs | `~/.local/state/okbswitch/Logs/` | `Logs/` |

Autostart is a desktop file in `~/.config/autostart/`. Remove the package with
`sudo apt remove okbswitch`; your settings stay in the folders above.

**Linux commands.** `okbswitch --diagnose` reports the session, the layouts and
the device access; `okbswitch --setup-linux-input` and
`okbswitch --install-linux-integration` repeat the two setup steps.

## Settings and data

On Windows all runtime files are stored **inside the program directory**:

| File | Relative path |
|---|---|
| Settings and autoreplace entries | `data/config.toml` |
| Persistent clipboard history, if enabled | `data/clipboard-history.txt` |
| Instance lock | `data/okbswitch.lock` |
| Daily logs | `Logs/okbswitch.<date>.log` |

The portable edition needs a writable folder. The all-users installer grants
write access to `data` and `log` while keeping the executable protected.
There is **no automatic fallback to AppData**. If the installation cannot write
its data, startup reports the problem so you can choose a writable folder or
repair its permissions.

Older profile settings and history are migrated on normal startup. Existing local
settings are never overwritten; old logs, backups and conflicting files are
preserved under `data/legacy-profile`. Migration waits for the old application
to be closed. Only copied files and empty old directories are removed.

The uninstaller keeps settings by default and asks separately about deleting them.
Silent removal also keeps them. Copy the program folder to back up your data.

## Diagnostics

```powershell
.\okbswitch-portable.exe --settings
.\okbswitch-portable.exe --paths
.\okbswitch-portable.exe --diagnose
.\okbswitch-portable.exe --debug
.\okbswitch-portable.exe --licenses
```

The installed executable is named `okbswitch.exe`. `--paths` is read-only and
shows the resolved locations. `--diagnose` does not migrate profile data.
An optional `--config` path must remain inside the program directory.

Enable **Write a program diagnostic log** under Troubleshooting and choose
Error, Info, or Debug. Changes apply without restarting. When disabled, no log
file is created. Logs contain events and detection decisions, not typed text.

## Building

Rust 1.95 or newer is required by the UI dependencies. Windows releases use the MSVC toolchain with the
Visual Studio C++ build tools and Windows SDK. NSIS is needed for the installer.

```powershell
cargo test --workspace --locked
cargo build --release --locked -p okbswitch --target x86_64-pc-windows-msvc
```

See [tools/build-release.ps1](tools/build-release.ps1) for packaging. Linux/WSL
developers can use `tools/cross-windows.sh` with llvm-mingw and
`tools/build-installer.sh` with NSIS for the Windows files. The Linux files are
built in a clean Ubuntu 24.04 root by `tools/build-linux-baseline.sh` and packaged
by `tools/package-linux.py` (`.deb` and AppImage).

## Automated releases

Pushing a version tag such as `v0.1.0-beta` starts the [release workflow](.github/workflows/release.yml).
The tag must match the version in `Cargo.toml`. GitHub tests the project, builds
Windows x64 binaries, packages the installer, computes SHA-256 checksums, and
publishes the release only after the artifacts are ready. Prerelease versions
are marked as prereleases automatically. The workflow can also be run manually
for an existing version tag. Linux files are built locally and attached to the
release by hand.

## License

Original project code is licensed under the **[PolyForm Noncommercial License 1.0.0](LICENSE)**.
The license permits uses covered by its noncommercial terms; it does not grant
general commercial-use rights. See [NOTICE](NOTICE) for the required notice.

Third-party libraries, fonts and language data retain their own licenses.
See [data/LICENSES.md](data/LICENSES.md) and the notices included with releases.
