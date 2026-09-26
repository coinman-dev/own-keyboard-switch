[English](CHANGELOG.md) | [Русский](CHANGELOG.ru_RU.md)

# Changelog

## 0.2.0-beta — 2026-09-26

### Added

- **Spell checking** section in Settings: checking on command and while typing
  are switched separately; languages to check, personal words and optional
  dictionaries (English — United Kingdom, modern expanded Russian). A
  dictionary is downloaded only when you ask for it and is checked against a
  pinned SHA-256; this is the only network access of the program.
- The spell check hotkey can check the selected text before the clipboard.
- Spell checking while typing: after a Space, a misspelled word gets a small
  popup at the text caret with suggestions, **Skip** and **Add to my words**.
  The popup does not take the focus and closes as you keep typing. **Break**
  right after a correction puts the word back; a skipped or undone word is not
  checked again until restart.
- Automatic spelling mode fixes only unambiguous typing slips (neighbouring
  key, swapped, missing, extra or doubled letter). Short words, words with
  capitals, British spellings, addresses, paths, code and terminals are left
  as typed.
- The tray menu lists the installed keyboard layouts with their flags; the
  chosen one is applied to the program you typed in last. **System keyboard
  settings** opens the Windows language settings.
- **About** in the tray menu and in Settings: version, license and the full
  notices of third-party components.
- Switching rules can be exported to a file and imported back; the rules of a
  whole `config.toml` are accepted too.
- On the first start the program offers to exclude terminals and IDEs. The
  same list can be added later under **Excluded programs**.
- The diagnostic log can be switched off or set to Error, Info or Debug; the
  change applies without a restart. Logs are written to the `Logs` folder next
  to the program.

### Fixed

- Assigning a hotkey by pressing it: the dialog no longer waits forever,
  because Windows does not pass keys to the program's hook while its own
  window is active.
- Installing over an earlier version no longer turns autostart on again when
  it was switched off in the program, keeps the desktop shortcut and removes
  the old `log` folder.

### Changed

- The release contains only the two program files and their checksums; the
  license texts are in the repository, in the installed folder and in
  **About → Licenses...**.

## 0.1.0-beta — 2026-09-21

First Windows prerelease: automatic RU/EN layout switching with Break undo,
switching rules, excluded programs and password-field checks; autoreplace;
clipboard layout conversion, transliteration, spell checking and history;
floating indicator, tray flags and sounds; portable and installer editions
that keep settings and logs in the program folder.
