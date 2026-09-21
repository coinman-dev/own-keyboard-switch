[English](README.md) | [Русский](README.ru_RU.md)

<p align="center">
  <img src="images/logo-settings.png" alt="Own Keyboard Switch" width="160">
</p>

<h1 align="center">Own Keyboard Switch</h1>
<p align="center">Automatic Russian ↔ English keyboard layout switching for Windows.</p>

[![Release](https://img.shields.io/badge/release-0.1.0--beta-orange)](https://github.com/coinman-dev/own-keyboard-switch/releases/tag/v0.1.0-beta)
[![Build](https://github.com/coinman-dev/own-keyboard-switch/actions/workflows/release.yml/badge.svg)](https://github.com/coinman-dev/own-keyboard-switch/actions/workflows/release.yml)
[![Windows](https://img.shields.io/badge/Windows-10%20%2F%2011%20x64-blue)](#download)
[![Rust](https://img.shields.io/badge/Rust-1.95%2B-orange)](#building)
[![License](https://img.shields.io/badge/license-PolyForm%20Noncommercial-blue)](LICENSE)

**Own Keyboard Switch** corrects words typed with the wrong keyboard layout:
`ghbdtn` → `привет`, `руддщ` → `hello`. It runs in the tray, supports custom rules
and text expansions, and includes clipboard tools and a floating layout indicator.

> **0.1.0-beta is a Windows prerelease.** Linux support is still in development;
> no Linux binaries are published yet.

## Download

| Edition | Download | Usage |
|---|---|---|
| Portable | [okbswitch-portable.exe](https://github.com/coinman-dev/own-keyboard-switch/releases/download/v0.1.0-beta/okbswitch-portable.exe) | Put the file in a writable folder and run it. No installation required. |
| Installer | [okbswitch-install.exe](https://github.com/coinman-dev/own-keyboard-switch/releases/download/v0.1.0-beta/okbswitch-install.exe) | Choose a current-user or all-users installation, shortcuts and startup options. |

Windows 10/11, x64. Dictionaries and language models are built in. The application
does not make network requests and contains no advertising. [All releases](https://github.com/coinman-dev/own-keyboard-switch/releases).

## Quick start

1. Run the portable file or complete the installer.
2. Double-click the tray icon to open Settings.
3. Keep automatic switching enabled and type a word followed by Space or Enter.
4. Press **Break** to undo an automatic correction or convert the current word.

The interface supports English and Russian, and light, dark and system themes.
You can change shortcuts and excluded applications in Settings.

## Features

- RU/EN detection using built-in dictionaries, language models and additional rules.
- Optional **Improve switching**: recognizes more lowercase two/three-letter English
  words after a Russian word or letter. Russian words and names remain protected.
- Correction before Enter reaches PowerShell or a Windows terminal. WSL terminals
  use the Windows application; the native Linux input backend is unfinished.
- Single-key layout switching, case correction and manual conversion of selections.
- Autoreplace with Space, Enter, Tab, a tooltip or a shortcut; multiline entries,
  a remembered caret position and a floating insertion list.
- Clipboard layout conversion, transliteration, spellchecking and history.
- Floating layout indicator, configurable tray flags and sounds.
- Application exclusions, password-field checks, startup and optional elevation.
- Floating lists stay inside the monitor working area, including at different DPI.

Clipboard operations preserve **plain text**; preservation of images and rich text
is not implemented. Saving clipboard history between runs is off by default; when
enabled, copied sensitive text can also be stored in that history.

## Settings and data

All runtime files are stored **inside the program directory**:

| File | Relative path |
|---|---|
| Settings and autoreplace entries | `data/config.toml` |
| Persistent clipboard history, if enabled | `data/clipboard-history.txt` |
| Instance lock | `data/okbswitch.lock` |
| Daily logs | `log/okbswitch.<date>.log` |

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

Enable **Detailed log (Debug)** under Troubleshooting when reporting a problem.
It applies without restarting. Logs contain events and detection decisions,
not typed text. Debug logging is off by default.

## Building

Rust 1.95 or newer is required by the UI dependencies. Windows releases use the MSVC toolchain with the
Visual Studio C++ build tools and Windows SDK. NSIS is needed for the installer.

```powershell
cargo test --workspace --locked
cargo build --release --locked -p okbswitch --target x86_64-pc-windows-msvc
```

See [tools/build-release.ps1](tools/build-release.ps1) for packaging. Linux/WSL
developers can use `tools/cross-windows.sh` with llvm-mingw and
`tools/build-installer.sh` with NSIS. This produces Windows executables;
it does not imply a usable native Linux version.

## Automated releases

Pushing a version tag such as `v0.1.0-beta` starts the [release workflow](.github/workflows/release.yml).
The tag must match the version in `Cargo.toml`. GitHub tests the project, builds
Windows x64 binaries, packages the installer, computes SHA-256 checksums, and
publishes the release only after the artifacts are ready. Prerelease versions
are marked as prereleases automatically. The workflow can also be run manually
for an existing version tag. Linux artifacts are intentionally not built yet.

## License

Original project code is licensed under the **[PolyForm Noncommercial License 1.0.0](LICENSE)**.
The license permits uses covered by its noncommercial terms; it does not grant
general commercial-use rights. See [NOTICE](NOTICE) for the required notice.

Third-party libraries, fonts and language data retain their own licenses.
See [data/LICENSES.md](data/LICENSES.md) and the notices included with releases.
