# Extra short-word rules

The optional `rules_options.improve_switching` setting defaults to `false`.
It supplements existing detection only after an immediately preceding Russian
word or letter and only for a complete lowercase English target from
`generated/extra-short-en.txt`. The application applies this layer on Space,
Enter and keypad Enter, respecting the existing Enter-disable setting.

The generator reads `missed` rows from a local evaluation report, validates ASCII
lowercase words of two or three letters, removes duplicates and sorts them.
The checked-in artifact has 699 targets and embeds into the executable.
User reports under `tmp/` are ignored and are never loaded by the application.

The new layer keeps existing conversions, user rules and detector filters.
Russian lowercase dictionary/frequency entries and title-case dictionary entries
protect ordinary words, their inflections and names. The surname «Вуд» also has
explicit protection because it occurs in the Russian corpus but is absent from
the spelling dictionary. Reviewed labels «вшк», «гр», «фр», «уч» do not alone
protect a lowercase reading. Uppercase-only entries are not treated as names.

The immediately preceding token is tracked separately from the recent language
used by legacy single-letter rules. Unknown tokens, focus/caret changes, manual
layout changes and line boundaries invalidate the new context. Undo restores the
actual displayed language but does not carry context across a line break.

## Verification

Normal tests cover the option's default and persistence, user-rule precedence,
Russian names/phrases, separate letters, live config changes, focus/password
protection, modifiers, undo, autoreplacement and unchanged legacy letter handling.
A held-out Russian word test rejects new false corrections.

The explicit engine test `report_improved_short_words` compares default and enabled
settings after «ура » and «в » with Space/Enter. Run it with `--ignored --nocapture`
and local inputs present. It writes `tmp/CSW24-2-3.improved-results.tsv` without
overwriting the baseline report. On the current 1,479-word list, each scenario
improves from 780 to 1,417 corrections and leaves 62 protected readings. These are
coverage measurements on that list, not an accuracy estimate for arbitrary text.
