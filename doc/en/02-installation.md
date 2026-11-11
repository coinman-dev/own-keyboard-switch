# Installation

Release files are on the
[Releases](https://github.com/coinman-dev/own-keyboard-switch/releases) page.

| File | Purpose |
|---|---|
| `okbswitch-portable.exe` | Windows without installation |
| `okbswitch-install.exe` | Installer for Windows |
| `okbswitch_<version>_amd64.deb` | Package for Ubuntu 24.04 and newer, Debian |
| `okbswitch-<version>-x86_64.AppImage` | Linux without installation |

## Windows

### Portable

Put `okbswitch-portable.exe` in a folder **you can write to** (for example,
`D:\Tools\Okbswitch`) and run it. A `data` folder with the settings and a
`Logs` folder for the log appear next to it. Do not put the portable version
in `C:\Program Files`: a regular user cannot write there, and the program
reports that it cannot save the settings.

### Installer

1. Run `okbswitch-install.exe` and choose who to install for:
   - **Only for me** — into `%LOCALAPPDATA%\Programs\Okbswitch`, no
     administrator rights needed;
   - **For all users** — into `C:\Program Files\Okbswitch`, Windows asks for
     administrator rights. The installer allows writing only to the `data` and
     `Logs` folders; `okbswitch.exe` itself stays protected.
2. On the components page you can select **Desktop shortcut** and **Start at
   logon**. Autostart can be changed later in the program settings.
3. If the program is already running, the installer closes it before
   replacing the files.

### Updating

Run the installer of the new version over the old one, or replace
`okbswitch-portable.exe` with the new file after exiting the program (tray
menu → **Exit**). The settings are kept.

Settings and clipboard history from old versions that stored them in the user
profile are moved to the program folder on the first run. Existing settings
are not overwritten; old logs and conflicting files are set aside in
`data\legacy-profile`.

### Uninstalling

**Windows Settings → Apps → Own Keyboard Switch → Uninstall.** The uninstaller
asks whether to delete the settings, autoreplace entries and clipboard history
as well; if you are going to install the program again, answer “No”. A silent
uninstall (`uninstall.exe /S`) keeps the data. For the portable version, just
close it and delete it together with its folder.

## Linux

First add Russian and English layouts in the system keyboard settings.

### .deb package

```sh
sudo apt install ./okbswitch_0.5.0-beta_amd64.deb
```

The package adds the program to the application menu and installs a udev rule
that gives the user of the active local session access to keyboards and
`/dev/uinput`. The program needs neither the `input` group nor root rights.

### AppImage

```sh
chmod +x okbswitch-0.5.0-beta-x86_64.AppImage
./okbswitch-0.5.0-beta-x86_64.AppImage
```

The AppImage keeps its settings and logs next to itself, like the portable
version for Windows.

### First run on Linux

The **Set up Linux access** window checks keyboard access, the desktop
connection and the layouts, and offers the missing steps: **Set up keyboard
access** (a system password prompt for the setup helper) and **Set up desktop
integration** (a GNOME Shell extension or a KWin script). Until setup is
complete, keys are not captured. More in the [Linux](12-linux.md) chapter.

### Updating and uninstalling

A new package is installed with the same `sudo apt install ./…deb` command.
To uninstall: `sudo apt remove okbswitch`; the settings in
`~/.config/okbswitch/` remain. The AppImage is removed together with its
folder.
