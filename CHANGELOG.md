[English](CHANGELOG.md) | [Русский](CHANGELOG.ru_RU.md)

# Changelog

## Unreleased

### Added

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
  change applies without a restart.
- Download and removal errors of optional dictionaries are shown in Settings.

### Fixed

- Assigning a hotkey by pressing it: the dialog no longer waits forever,
  because Windows does not pass keys to the program's hook while its own
  window is active.
- The spell check hotkey honours «check selection first»; the tray command
  checks the clipboard only.
- Spelling fixes are typed into the original input field only; a failed
  replacement is reported instead of being lost.
- Personal words match in any letter case, Cyrillic included.
- Optional Hunspell dictionaries with comments in their affix files load.
- The Windows 11 hidden-icons flyout is no longer taken for the input window.

## 0.1.0-beta — 2026-09-21

First Windows prerelease: automatic RU/EN layout switching with Break undo,
switching rules, excluded programs and password-field checks; autoreplace;
clipboard layout conversion, transliteration, spell checking and history;
floating indicator, tray flags and sounds; portable and installer editions
that keep settings and logs in the program folder.
