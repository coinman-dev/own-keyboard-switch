# Files, folders and logs

`okbswitch --paths` shows the exact paths on your computer.

## Windows

All working files are kept **inside the program folder**:

| What | Where |
|---|---|
| Program (installed for all users) | `C:\Program Files\Okbswitch\okbswitch.exe` |
| Program (installed for yourself) | `%LOCALAPPDATA%\Programs\Okbswitch\okbswitch.exe` |
| Settings, rules, exclusions, autoreplace entries | `data\config.toml` |
| Clipboard history, if saving it is on | `data\clipboard-history.txt` |
| Single-instance lock | `data\okbswitch.lock` |
| Downloaded spelling dictionaries | `data\dictionaries\` |
| Log, one file per day, the last 7 are kept | `Logs\okbswitch.<date>.log` |
| Moved files of old versions | `data\legacy-profile\` |

There is no automatic fallback to `%APPDATA%`: if the program folder is not
writable, the program reports an error. Keep the portable version in a folder
you can write to; the installer for all users grants write access to `data`
and `Logs` itself.

To back up the settings, it is enough to copy the `data` folder.

## Linux

| What | Installed package | AppImage, `--portable` |
|---|---|---|
| Settings | `~/.config/okbswitch/config.toml` | `data/config.toml` next to the program |
| Downloaded dictionaries | `~/.config/okbswitch/dictionaries/` | `data/dictionaries/` |
| Clipboard history, if saved | `~/.local/state/okbswitch/` | `data/` |
| Logs | `~/.local/state/okbswitch/Logs/` | `Logs/` |
| Autostart | `~/.config/autostart/` | `~/.config/autostart/` |
| udev rule | `/usr/lib/udev/rules.d/70-okbswitch.rules` | installed by the setup helper |

## Settings file

`config.toml` is a plain TOML text file; the settings window rewrites it on
**Apply**. Manual edits take effect after the program restarts. If the file
contains an error, the program does not refuse to start: a syntax error resets
all settings to their defaults, an invalid value resets only its own
parameter, and a broken list element is dropped. The original file is then
kept as `config.toml.invalid-<time>`, so you can carry your edits over from
it.

`okbswitch --print-default-config` prints the default settings.

## Log

The log is written only if **Troubleshooting → Write a program diagnostic
log** is on, or for one run with the `--debug` parameter. It contains no typed
text or passwords — only technical details, for example:

```
word finished typed_in=Ru letters=4 stay_reason=None switch_reason=Some(ExtraRule) target=Some(En)
word converted from=ru to=en kind=Automatic
```

This is enough to understand why a word was or was not corrected, and to
attach the log to a bug report.
