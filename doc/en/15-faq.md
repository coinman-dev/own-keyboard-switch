# Solving problems and FAQ

## Where to look first

- **The tray icon** — whether the program is running and whether auto switch
  is on (a grey or dim icon means off).
- **The log** — turn on **Troubleshooting → Write a program diagnostic log**
  in **Debug** mode, reproduce the problem and look at the latest file in
  `Logs` ([Files, folders and logs](14-files-and-logs.md#log)). For each word
  it shows what decision was made and why.
- `okbswitch --diagnose` — layouts, autostart, and on Linux the session and
  keyboard access.

## A word was not corrected

First check whether one of the cases in
[“When the program does not intervene”](04-how-switching-works.md#when-the-program-does-not-intervene)
applies. Most often:

- **Backspace was pressed before the word** — the program assumes you are
  editing the text yourself;
- **the layout was switched by hand** right before the word. If you are
  testing the program by deliberately turning on the “wrong” layout, this is
  exactly why it does nothing. Clear **Troubleshooting → You changed the
  layout** if this behaviour gets in the way;
- **the program is in the exclusions** — for example, a terminal added on the
  first run;
- **both readings look like words** (`рук` — `her`, `вуд` — `del`) or the word
  is too short.

In the log such a word looks like `word finished … stay_reason=…` without a
following `word converted` line. If it says `layout_chosen=true`, the layout
was chosen by hand before the word.

Press **Break** to convert the word now, and add a
[rule](08-rules-and-exclusions.md#switching-rules) “Convert to the other
layout” if the word should always be corrected.

## The program corrects what it should not

- Press **Break** — the correction is undone. After two undos in a row the
  program offers a “Do not convert” rule.
- If these are terms of one program, add it to the
  [exclusions](08-rules-and-exclusions.md#excluded-programs).
- Short words: increase **Do not convert words shorter than, letters** or move
  **Text analysis sensitivity** towards “cautious”.
- Words of capital letters: clear **Correct abbreviations**.

## Does not work in some windows (Windows)

- Windows running as administrator (Task Manager, installers, an
  administrator `cmd`): turn on **General → Run with administrator rights**.
- The program's own windows (settings) are deliberately not corrected.
- Remote desktops and virtual machines pass keys to another system; add them
  to the exclusions and, if you like, run the program inside.

## The corrected word appears in the old layout or with gaps

The program changes the layout and immediately retypes the word. Some programs
need more time: increase **Troubleshooting → Pause before retyping a word**
(for example, to 80–100 ms) and, if letters are lost, **Pause between retyped
keys** (2–5 ms).

## Break does not work

Many laptops have no Break (Pause) key, or it is pressed via Fn. Assign
another combination to the “Undo layout conversion” action in
[Hotkeys](07-hotkeys-and-mouse.md). Also check that **Disable hotkeys when
auto switch is off** is not on while auto switch is off.

## Two copies are running

Only one instance runs per data folder. A portable copy in another folder and
the installed program are different instances: both will correct the same
words. Keep one.

## The antivirus complains

A layout switcher by its nature watches the keyboard and sends key presses, so
antivirus heuristics sometimes react to it. Download the program only from the
[Releases](https://github.com/coinman-dev/own-keyboard-switch/releases) page
and check the SHA-256 checksums published next to the files.

## Linux: keys are not captured

Open the setup window (it appears at startup if something is not ready) and
go through the flagged items: keyboard access, desktop integration, layouts,
active session. The program works only in an active local session — after
the screen locks, unlock it. More in [Linux](12-linux.md).

## How to report a bug

Create an issue on
[GitHub](https://github.com/coinman-dev/own-keyboard-switch/issues): the
program and system versions, the program you were typing in, which word was
typed and what came out. Attach a log in Debug mode — it contains no typed
text.
