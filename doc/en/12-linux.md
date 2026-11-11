# Linux

The Linux version is a preview. It works on x86_64 in these sessions:

| Session | How the layout is switched | What is needed |
|---|---|---|
| X11 (any desktop) | XKB | Nothing extra |
| GNOME 45–50 on Wayland | Built-in GNOME Shell extension | The program installs it itself; after the first installation, sign out and back in once |
| KDE Plasma 6 on Wayland | KDE layout service and a built-in KWin script | The program installs and loads the script in the current session |

Other Wayland compositors (Sway, Hyprland and others) are not supported yet.
Russian and English layouts must be added in the system keyboard settings.

## How it works

The program reads keyboards through evdev and passes everything typed on
through a uinput virtual keyboard. That is why it works in all applications,
including terminals and Wayland windows. As with any layout switcher on
Wayland, keyboard access granted by a udev rule lets programs of your user
read key presses. Own Keyboard Switch itself always runs as a regular user;
only a small setup helper needs administrator rights.

## First run

The **Set up Linux access** window shows the status item by item: keyboard,
desktop, layouts, session. While something is not ready, the program does not
capture keys.

- **Set up keyboard access** — the system asks for the administrator password
  (pkexec) for the helper, which installs a udev rule (`70-okbswitch.rules`)
  and loads the `evdev` and `uinput` modules. The `.deb` package installs the
  rule itself, so this step is usually not needed.
- **Set up desktop integration** — install the GNOME Shell extension or the
  KWin script for your user. In GNOME, sign out after that, sign back in and
  click **Check again**.
- **Start switching** — when everything is ready.

The **General → Basic → Set up keyboard access on startup** option opens this
window on every start if there is no access.

## The Linux tab

**General → Linux**:

- **Switching method** — “Automatic selection” uses XKB in X11 and GNOME or
  KDE integration in Wayland. You can explicitly choose “X11 (XKB)”, “KDE
  desktop” or “GNOME extension”. The change applies after checking the
  desktop connection; if the selected method is unavailable, the previous
  settings remain active.
- A note on whether the GNOME extension was installed by the program.
- **Keyboards** — one device path per line, such as `/dev/input/event3` or a
  link under `/dev/input/by-id`. An empty list uses all accessible keyboards.
  Applies without a restart.

## Differences from Windows

- Tray icon: in KDE Plasma the settings open with a single click; in GNOME a
  single click opens the menu.
- There are no “Run with administrator rights” and “Correct layout in menus
  with access keys” options.
- “Same layout for all windows” is not supported in GNOME on Wayland.
- Excluded programs in GNOME on Wayland work only with the GNOME Shell
  extension.

## Commands

```sh
okbswitch --diagnose                   # session, layouts, device access
okbswitch --setup-linux-input          # repeat the keyboard access setup
okbswitch --install-linux-integration  # reinstall the GNOME extension / KWin script
okbswitch --portable                   # keep the settings next to the program
```

The full list is in the [Command line](13-command-line.md) chapter.
