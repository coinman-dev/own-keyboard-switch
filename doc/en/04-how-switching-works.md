# How auto switch works

## When a word is checked

The program remembers the keys you press, not the text on the screen. A word
counts as finished when it is followed by a space, Enter, Tab or a character
that is not a letter in either the Russian or the English layout. The program
then compares two readings of the same keys — Russian and English:

- whether each of them is in the dictionary and how frequent the word is;
- how usual the letter combinations are for each language;
- what the built-in rules say for known cases.

If the English reading is a real word and the Russian one is nonsense (or the
other way round), the word is erased and typed again in the other layout, and
the layout is switched. If both readings look like words, what you typed
stays: the program does not argue with a word that exists.

The decision depends only on the word itself. At the start of a line, after
Russian or English text, it is the same; the exception is the **Improve
layout switching** option (the [Switching rules](08-rules-and-exclusions.md)
section): with it, more short English words of 2–3 letters are recognised
after a Russian word.

One-letter words are corrected too: `z` → `я`, `ш` → `i`. The program knows
program names, Windows and Linux commands (`dir`, `mkdir`, `chkdsk`, `htop`,
`netstat`…) and words of programming languages even if they are not in the
regular dictionary.

### Terminals

In a terminal, Enter runs the command at once, so the program holds Enter back
while it checks the last word and passes it to the terminal after the
correction. If the terminal is added to the
[exclusions](08-rules-and-exclusions.md#excluded-programs), words in it are not
checked at all.

## The Break key

Break (“Undo layout conversion”, can be reassigned in
[Hotkeys](07-hotkeys-and-mouse.md)) does one of the following:

| When pressed | What happens |
|---|---|
| Right after an automatic correction | The correction is undone: the typed text and the previous layout come back |
| After a word the program left alone | The word is converted to the other layout together with the characters after the last space: `ghbdtn,` → `привет,` |
| In the middle of a word | The typed part is converted, and you can go on typing |
| The program no longer remembers the word (you clicked the mouse, switched windows) | The word to the left of the caret is selected and converted as selected text (except in terminals) |

If the same word is undone with Break several times in a row (twice by
default), the program offers to add a
[rule](08-rules-and-exclusions.md#switching-rules) for it.

## Passwords

With the **Recognise passwords and type them in English** option (on by
default), text without spaces that mixes capitals, lowercase letters, digits
and symbols and was typed in the Russian layout is retyped in English:
`ИшпМщмф2024!` → `BigVova2024!`. Russian words with digits (`Москва-2026`)
stay as they are. In a password field, if the application reports it to the
system, the English layout is turned on from the first character.

Ordinary words with digits and mixed case are not touched by auto switch
(**Troubleshooting → Do not convert words with digits or mixed case
(passwords)**).

## Letter case

Regardless of the layout, the program corrects:

- **two capital letters at the start of a word**: `HEllo` → `Hello`;
- **Caps Lock turned on by accident**: `hELLO` → `Hello`, and Caps Lock is
  turned off;
- **abbreviations** in the wrong layout: `ШЫЩ` → `ISO`.
  If you turn off **Correct abbreviations**, words of capital letters only
  stay as typed.

These options are in the [General → Advanced](06-general-settings.md#advanced)
section.

## When the program does not intervene

The program deliberately leaves a word as it is if:

- **Backspace** was pressed before the word — you are correcting the text
  yourself (the same applies to the arrows and Delete if they are selected in
  [Troubleshooting](11-troubleshooting-sounds-about.md#troubleshooting));
- you **changed the layout yourself** before the word: the program assumes
  you know which language you are writing in (the “You changed the layout”
  option);
- the word is **shorter** than the set number of letters, contains digits or
  looks like a password;
- the window is an **excluded program**, the input field is a password field,
  or it is a window of the program itself;
- a layout other than Russian and English is on;
- the word is finished with Tab or Enter and the **Do not switch the layout on
  Tab and Enter** option is on;
- **both readings** look like words, or neither does. In the second case you
  can turn on the **Possible typo** sound in the
  [Sounds](11-troubleshooting-sounds-about.md#sounds) section.

A mouse click or a change of window or input field resets the remembered
word: after that the program starts counting again.

## Administrator rights (Windows)

Windows does not let a regular program read the keyboard in windows running as
administrator (Task Manager, installers, an administrator console). For auto
switch to work there as well, turn on **General → Run with administrator
rights**.
