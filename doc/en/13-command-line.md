# Command line

On Windows the installed program is called `okbswitch.exe`, the portable one
`okbswitch-portable.exe`; on Linux it is `okbswitch` (or the AppImage file).

| Parameter | What it does |
|---|---|
| `--settings` | Start and open the settings window at once |
| `-c`, `--config <file>` | Use a different settings file. On Windows it must be inside the program folder |
| `-d`, `--debug` | Detailed log for this run, with a copy in the terminal |
| `--no-tray` | Run without a tray icon (stop with Ctrl+C) |
| `--paths` | Show where the settings, log and lock file are, and exit |
| `--diagnose` | Check the environment: session, layouts, device access, autostart — and exit |
| `--print-default-config` | Print the default settings file and exit |
| `--licenses` | Print the program license and the notices for built-in data, and exit |
| `--version`, `--help` | Version and help |

Linux only:

| Parameter | What it does |
|---|---|
| `--portable` | Keep the settings and logs next to the program (always in the AppImage) |
| `--setup-linux-input` | Request administrator rights and set up keyboard access |
| `--install-linux-integration` | Install the GNOME Shell extension and the KDE script for the current user |

Only one instance of the program runs per data folder: a second start reports
“okbswitch is already running” and exits.

On Windows the program is a windowed application: it prints its output
(`--paths`, `--diagnose`) to the console window it was started from. Run such
commands in an open PowerShell or command prompt window; when redirected to a
file (`> out.txt`), the output may not get there.

```powershell
.\okbswitch-portable.exe --paths
.\okbswitch-portable.exe --diagnose
```
