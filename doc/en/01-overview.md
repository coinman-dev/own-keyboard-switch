# What is Own Keyboard Switch

Own Keyboard Switch (okbswitch) fixes words typed in the wrong layout:
`ghbdtn` → `привет`, `руддщ` → `hello`. You type as usual; when a word is
finished with a space, Enter or a punctuation mark, the program decides which
language it is written in and, if needed, erases it, switches the layout and
types the word again. It runs in the background, with an icon in the
notification area.

## What the program does

- **RU/EN auto switch.** The decision is based on built-in dictionaries, word
  frequencies, letter combination statistics and additional rules. It knows
  program names, Windows and Linux commands and words of programming
  languages.
- **Break** undoes the last correction or, the other way round, converts a word
  the program left alone. If you undo the same word several times in a row,
  the program suggests a rule for it.
- **Passwords.** A password typed in the Russian layout is retyped in English;
  in a password field the English layout is turned on from the first
  character.
- **Letter case.** Fixes `HEllo` and text typed with Caps Lock turned on by
  accident.
- **Working with text:** changing the layout and case of, and transliterating,
  the selected text and the clipboard; pasting without formatting; numbers in
  words.
- **Autoreplace:** abbreviations expand into ready-made text, including
  multi-line text, with a remembered caret position.
- **Spell checking** on command or right while typing.
- **Clipboard history**, a floating layout indicator, flags in the tray,
  sounds, switching the layout with a single key, mouse buttons with assigned
  key combinations.

## What the program does not do

- **It does not send text over the network.** Dictionaries and language
  models are built in. The network is needed only to download an additional
  spelling dictionary when you click **Download** yourself. There are no
  update checks and no ads.
- **It does not write typed text to the log.** The log holds only technical
  details: how many letters the word had, what decision was made and why.
- **It works only with the Russian–English pair.** Other layouts can stay in
  the system: by default the program does not touch input in them.
- Clipboard operations work with **plain text**: images and formatting are not
  kept.

## Everything on one page

| Task | How |
|---|---|
| Open the settings | Double-click the tray icon (in KDE Plasma, a single click) |
| Undo a correction or convert a word by hand | **Break** (Pause) right after the word |
| Convert the selected text to the other layout | **Shift+Break** |
| Change the case of the selected text | **Alt+Break** |
| Transliterate the selected text | **Alt+Scroll Lock** |
| Paste without formatting | **Ctrl+Alt+V** |
| Turn auto switch off for a while | Tray menu → **Auto switch** |
| Do not switch in a particular program | [Excluded programs](08-rules-and-exclusions.md#excluded-programs) |
| A word is switched wrongly | Break, then a [rule](08-rules-and-exclusions.md#switching-rules) |

## System requirements

- Windows 10 or 11, x64.
- Linux x86_64: an X11 session with any desktop, GNOME 45–50 or KDE Plasma 6
  on Wayland. Details are in the [Linux](12-linux.md) chapter.
- Russian and English layouts must be added in the system.

## License

The project's own code is distributed under the
[PolyForm Noncommercial 1.0.0](../../LICENSE) license: you may use the program
free of charge for noncommercial purposes under the terms of the license; it
gives no general permission for commercial use. The built-in libraries, fonts
and Hunspell dictionaries have their own licenses — they are shown by the
**Licenses...** button in the [About](11-troubleshooting-sounds-about.md#about)
section and by the `okbswitch --licenses` command.
