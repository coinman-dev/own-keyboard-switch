//! End-to-end tests of the processor against a simulated desktop.
//!
//! The desktop is a text field that receives the user's keys (as an
//! application would) and the keys injected by the engine, types characters
//! with the active layout, and supports Ctrl+C / Ctrl+V on a selection.

use crate::{Backends, ConversionKind, Event, Processor, TextOp, Timing};
use okbs_core::KeyMap;
use okbs_core::config::Config;
use okbs_core::data;
use okbs_core::layouts::{KeyPress, builtin_keymap, char_for, keys_for_text, render};
use okbs_core::{Lang, PhysKey};
use okbs_platform::{
    Clipboard, FocusInfo, Injector, InputEvent, InputTarget, KeyStroke, LayoutId, LayoutInfo,
    LayoutManager, PlatformError, Result, WindowInfo,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[derive(Debug)]
struct Desktop {
    text: Vec<char>,
    selection: Option<std::ops::Range<usize>>,
    layout: Lang,
    clipboard: Option<String>,
    shift: bool,
    ctrl: bool,
    alt: bool,
    caps: bool,
    injected: usize,
    window_pid: u32,
    window_exe: Option<String>,
    window_title: Option<String>,
    password: bool,
    caret: Option<usize>,
    fail_copy: bool,
    fail_paste: bool,
    fail_clipboard_read: bool,
    fail_clipboard_write: bool,
    copy_during_paste: Option<String>,
    fail_layout: bool,
    focus_change_on_layout: bool,
    enter_count: usize,
    terminal: bool,
    /// Language of the access keys of the window's menu bar, if it has one.
    menu_language: Option<Lang>,
    submitted_lines: Vec<String>,
    tab_count: usize,
    f12_count: usize,
}

impl Desktop {
    fn new(layout: Lang) -> Self {
        Self {
            text: Vec::new(),
            selection: None,
            layout,
            clipboard: None,
            shift: false,
            ctrl: false,
            alt: false,
            caps: false,
            injected: 0,
            window_pid: 1,
            window_exe: None,
            window_title: None,
            password: false,
            caret: None,
            fail_copy: false,
            fail_paste: false,
            fail_clipboard_read: false,
            fail_clipboard_write: false,
            copy_during_paste: None,
            fail_layout: false,
            focus_change_on_layout: false,
            enter_count: 0,
            terminal: false,
            menu_language: None,
            submitted_lines: Vec::new(),
            tab_count: 0,
            f12_count: 0,
        }
    }

    fn insert(&mut self, s: &str) {
        let caret = self.caret.unwrap_or(self.text.len());
        let range = self.selection.take().unwrap_or(caret..caret);
        let end = range.start + s.chars().count();
        self.text.splice(range, s.chars());
        self.caret = (end < self.text.len()).then_some(end);
    }

    fn apply(&mut self, key: PhysKey, pressed: bool) {
        match key {
            PhysKey::ShiftLeft | PhysKey::ShiftRight => self.shift = pressed,
            PhysKey::ControlLeft | PhysKey::ControlRight => self.ctrl = pressed,
            PhysKey::AltLeft | PhysKey::AltRight => self.alt = pressed,
            _ if !pressed => {}
            PhysKey::CapsLock => self.caps = !self.caps,
            PhysKey::F12 => self.f12_count += 1,
            PhysKey::KeyC if self.ctrl => {
                if let Some(range) = self.selection.clone() {
                    self.clipboard = Some(self.text[range].iter().collect());
                }
            }
            PhysKey::KeyV if self.ctrl => {
                let clip = self.clipboard.clone().unwrap_or_default();
                self.insert(&clip);
                if let Some(newer) = self.copy_during_paste.take() {
                    self.clipboard = Some(newer);
                }
            }
            PhysKey::Backspace => {
                if let Some(range) = self.selection.take() {
                    self.text.drain(range);
                } else {
                    let caret = self.caret.unwrap_or(self.text.len());
                    if caret > 0 {
                        self.text.remove(caret - 1);
                        self.caret = (caret - 1 < self.text.len()).then_some(caret - 1);
                    }
                }
            }
            PhysKey::ArrowLeft => {
                self.caret = Some(self.caret.unwrap_or(self.text.len()).saturating_sub(1));
            }
            _ if self.ctrl || self.alt => {}
            PhysKey::Enter | PhysKey::NumpadEnter => {
                self.enter_count += 1;
                if self.terminal {
                    self.submitted_lines.push(self.text.iter().collect());
                    self.text.clear();
                    self.selection = None;
                    self.caret = None;
                } else {
                    self.insert("\n");
                }
            }
            PhysKey::Tab => {
                self.tab_count += 1;
                self.insert("\t");
            }
            _ => {
                let press = KeyPress {
                    key,
                    shift: self.shift,
                    caps: self.caps,
                };
                if let Some(c) = char_for(builtin_keymap(self.layout), press) {
                    self.insert(&c.to_string());
                }
            }
        }
    }
}

#[derive(Clone)]
struct Shared(Arc<Mutex<Desktop>>);

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Desktop> {
        self.0.lock().expect("desktop lock")
    }
}

struct FakeInjector(Shared);
impl Injector for FakeInjector {
    fn send(&mut self, strokes: &[KeyStroke]) -> Result<()> {
        let mut d = self.0.lock();
        if strokes.iter().any(|s| {
            s.pressed
                && ((s.key == PhysKey::KeyC && d.fail_copy)
                    || (s.key == PhysKey::KeyV && d.fail_paste))
        }) {
            return Err(PlatformError::Other("simulated input failure".into()));
        }
        for s in strokes {
            d.injected += 1;
            d.apply(s.key, s.pressed);
        }
        Ok(())
    }
}

struct FakeLayouts(Shared);
fn id(lang: Lang) -> LayoutId {
    LayoutId(match lang {
        Lang::Ru => 0x0419,
        Lang::En => 0x0409,
    })
}
impl LayoutManager for FakeLayouts {
    fn layouts(&self) -> Result<Vec<LayoutInfo>> {
        Ok(Lang::ALL
            .iter()
            .map(|&l| LayoutInfo {
                id: id(l),
                lang: Some(l),
                locale: match l {
                    Lang::Ru => "ru-RU".to_string(),
                    Lang::En => "en-US".to_string(),
                },
                name: l.code().to_string(),
                short: l.code().to_string(),
            })
            .collect())
    }
    fn current(&self) -> Result<LayoutId> {
        Ok(id(self.0.lock().layout))
    }
    fn set(&mut self, layout: LayoutId) -> Result<()> {
        if self.0.lock().fail_layout {
            return Err(PlatformError::Other("simulated layout failure".into()));
        }
        let lang = Lang::ALL
            .into_iter()
            .find(|&l| id(l) == layout)
            .ok_or_else(|| PlatformError::Other("unknown layout".into()))?;
        let mut d = self.0.lock();
        d.layout = lang;
        if d.focus_change_on_layout {
            d.window_pid += 1;
        }
        Ok(())
    }
    fn keymap(&self, layout: LayoutId) -> Result<KeyMap> {
        let lang = if layout == id(Lang::Ru) {
            Lang::Ru
        } else {
            Lang::En
        };
        Ok(builtin_keymap(lang).clone())
    }
}

struct FakeFocus(Shared);
impl FocusInfo for FakeFocus {
    fn is_terminal(&self) -> Result<bool> {
        Ok(self.0.lock().terminal)
    }
    fn input_target(&self) -> Result<Option<InputTarget>> {
        let pid = self.0.lock().window_pid;
        Ok((pid != std::process::id()).then_some(InputTarget {
            window: u64::from(pid),
            control: 1,
        }))
    }
    fn activate_target(&self, target: InputTarget) -> Result<()> {
        self.0.lock().window_pid = target.window as u32;
        Ok(())
    }
    fn active_window(&self) -> Result<Option<WindowInfo>> {
        let d = self.0.lock();
        Ok(Some(WindowInfo {
            pid: Some(d.window_pid),
            executable: d.window_exe.as_ref().map(PathBuf::from),
            title: d.window_title.clone(),
            app_id: None,
        }))
    }
    fn is_password_field(&self) -> Result<Option<bool>> {
        Ok(Some(self.0.lock().password))
    }
    fn menu_access_language(&self) -> Result<Option<Lang>> {
        Ok(self.0.lock().menu_language)
    }
}

struct FakeClipboard(Shared);
impl Clipboard for FakeClipboard {
    fn text(&mut self) -> Result<Option<String>> {
        let d = self.0.lock();
        if d.fail_clipboard_read {
            return Err(PlatformError::Other("clipboard is busy".into()));
        }
        Ok(d.clipboard.clone())
    }
    fn set_text(&mut self, text: &str) -> Result<()> {
        let mut d = self.0.lock();
        if d.fail_clipboard_write {
            return Err(PlatformError::Other("clipboard is busy".into()));
        }
        d.clipboard = Some(text.to_string());
        Ok(())
    }
    fn clear(&mut self) -> Result<()> {
        self.0.lock().clipboard = None;
        Ok(())
    }
}

struct Harness {
    gate: Option<Arc<okbs_platform::autoreplace_gate::AutoReplaceGate>>,
    desktop: Shared,
    processor: Processor,
    events: Vec<Event>,
    swallows: bool,
}

impl Harness {
    fn with_config(layout: Lang, mut config: Config) -> Self {
        config.switching.layout_switch_delay_ms = 0;
        let desktop = Shared(Arc::new(Mutex::new(Desktop::new(layout))));
        let backends = Backends {
            injector: Box::new(FakeInjector(desktop.clone())),
            layouts: Box::new(FakeLayouts(desktop.clone())),
            clipboard: Some(Box::new(FakeClipboard(desktop.clone()))),
            sound: None,
            focus: Some(Box::new(FakeFocus(desktop.clone()))),
        };
        let mut processor = Processor::new(config, backends);
        processor.set_timing(Timing {
            copy_timeout: Duration::from_millis(50),
            paste_settle: Duration::ZERO,
            poll: Duration::from_millis(1),
        });
        Self {
            gate: None,
            desktop,
            processor,
            events: Vec::new(),
            swallows: false,
        }
    }

    fn new(layout: Lang) -> Self {
        Self::with_config(layout, Config::default())
    }

    /// A physical key event seen by both the application and the engine.
    fn key(&mut self, key: PhysKey, pressed: bool) {
        self.key_at(key, pressed, Instant::now());
    }

    /// Like [`key`](Self::key) at a given time. Physical Caps Lock does not reach
    /// the application when the (Windows) hook suppresses it.
    fn key_at(&mut self, key: PhysKey, pressed: bool, time: Instant) {
        let event = self.source_event(key, pressed, false, time);
        let events = self.processor.handle_input(event);
        self.events.extend(events);
    }

    fn enable_gate(&mut self) {
        let gate = Arc::new(okbs_platform::autoreplace_gate::AutoReplaceGate::new(
            self.processor.config(),
        ));
        self.processor.set_input_gate(gate.clone());
        self.gate = Some(gate);
    }

    fn source_event(
        &mut self,
        key: PhysKey,
        pressed: bool,
        repeat: bool,
        time: Instant,
    ) -> InputEvent {
        let config = self.processor.config();
        let swallowed = key == PhysKey::CapsLock
            && self.swallows
            && (config.advanced.disable_capslock
                || config.switching.switch_key == okbs_core::config::SwitchKey::CapsLock);
        let raw = InputEvent::Key {
            key,
            pressed,
            repeat,
            injected: false,
            time,
        };
        if self.gate.as_ref().is_some_and(|gate| gate.discard(raw)) {
            return InputEvent::Key {
                key,
                pressed,
                repeat,
                injected: true,
                time,
            };
        }
        let target = FakeFocus(self.desktop.clone()).input_target().unwrap();
        if let Some(gate) = &self.gate
            && !swallowed
            && gate.capture(raw, Some(self.layout()), target)
        {
            return InputEvent::CapturedKey {
                epoch: gate.epoch(),
                key,
                pressed,
                repeat,
                time,
                target,
            };
        }
        if !swallowed {
            self.desktop.lock().apply(key, pressed);
        }
        raw
    }

    fn tap(&mut self, key: PhysKey) {
        self.key(key, true);
        self.key(key, false);
    }

    fn combo(&mut self, modifier: PhysKey, key: PhysKey) {
        self.key(modifier, true);
        self.tap(key);
        self.key(modifier, false);
    }

    /// Types `text` by the key positions of `keys_of` layout, whatever layout is active.
    fn type_as(&mut self, text: &str, keys_of: Lang) {
        for c in text.chars() {
            let (key, shift) = match c {
                ' ' => (PhysKey::Space, false),
                '\n' => (PhysKey::Enter, false),
                _ => builtin_keymap(keys_of)
                    .find(c)
                    .unwrap_or_else(|| panic!("cannot type {c:?}")),
            };
            if shift {
                self.key(PhysKey::ShiftLeft, true);
            }
            self.tap(key);
            if shift {
                self.key(PhysKey::ShiftLeft, false);
            }
        }
    }

    fn text(&self) -> String {
        self.desktop.lock().text.iter().collect()
    }

    fn layout(&self) -> Lang {
        self.desktop.lock().layout
    }

    fn select_all(&mut self) {
        let mut d = self.desktop.lock();
        d.selection = Some(0..d.text.len());
    }

    fn conversions(&self) -> Vec<ConversionKind> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::Converted { kind, .. } => Some(*kind),
                _ => None,
            })
            .collect()
    }
}

fn test_input_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tmp")
        .join(name)
}

fn csw_words() -> Vec<String> {
    let path = test_input_path("CSW24-2-3.txt");
    let mut words: Vec<_> = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .split_whitespace()
        .filter(|word| matches!(word.len(), 2 | 3))
        .filter(|word| word.bytes().all(|byte| byte.is_ascii_alphabetic()))
        .map(str::to_ascii_lowercase)
        .collect();
    words.sort_unstable();
    words.dedup();
    words
}

fn marked_reference_words() -> BTreeSet<String> {
    let path = test_input_path("слова .txt");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .lines()
        .filter(|line| line.trim_end().ends_with('<'))
        .filter_map(|line| line.split_whitespace().nth(1))
        .filter_map(|shown| keys_for_text(shown, builtin_keymap(Lang::Ru)))
        .map(|keys| render(&keys, builtin_keymap(Lang::En)).to_ascii_lowercase())
        .collect()
}

fn ignored_abbreviations() -> BTreeSet<String> {
    let path = test_input_path("ignored-abbreviations.txt");
    match std::fs::read_to_string(&path) {
        Ok(text) => text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_lowercase)
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
        Err(error) => panic!("cannot read {}: {error}", path.display()),
    }
}

#[derive(Debug)]
struct MissedWord {
    russian_reading: String,
    was_marked: bool,
    separators: BTreeSet<String>,
}

fn missed_words_from_report() -> BTreeMap<String, MissedWord> {
    let path = test_input_path("CSW24-2-3.lowercase-results.tsv");
    let mut missed = BTreeMap::new();
    for line in std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .lines()
    {
        if line.starts_with('#') || line.starts_with("word\t") {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        let [word, separator, russian_reading, was_marked, _, _, result] = fields.as_slice() else {
            panic!("unexpected report row: {line:?}");
        };
        if *result != "missed" {
            continue;
        }
        let entry = missed
            .entry((*word).to_owned())
            .or_insert_with(|| MissedWord {
                russian_reading: (*russian_reading).to_owned(),
                was_marked: false,
                separators: BTreeSet::new(),
            });
        assert_eq!(
            entry.russian_reading, *russian_reading,
            "inconsistent reading: {word}"
        );
        entry.was_marked |= *was_marked == "true";
        entry.separators.insert((*separator).to_owned());
    }
    assert!(!missed.is_empty(), "the first report must contain misses");
    missed
}

/// Produces a local report for every lower-case two- and three-letter CSW word
/// and each marked reference example, even if it is outside the CSW list.
///
/// Run explicitly because the input and report remain in the ignored `tmp/` directory:
/// `cargo test -p okbs-engine report_csw_lowercase_after_russian_context -- --ignored --nocapture`.
#[test]
#[ignore = "requires local tmp/CSW24-2-3.txt and writes a local report"]
fn report_csw_lowercase_after_russian_context() {
    let marked = marked_reference_words();
    let mut words = csw_words();
    words.extend(marked.iter().cloned());
    words.sort_unstable();
    words.dedup();
    assert!(!words.is_empty(), "the CSW list must contain words");

    let report_path = test_input_path("CSW24-2-3.lowercase-results.tsv");
    let mut report = String::from(
        "# context=ура; input=lowercase English key positions; initial_layout=ru\n\
         word\tseparator\tbefore_separator\twas_marked\tautomatic_conversions\tfinal_layout\tresult\n",
    );
    let mut total = [0_usize; 2];
    let mut missed = [0_usize; 2];
    let mut marked_missed = [0_usize; 2];

    for word in &words {
        for (index, (separator_name, separator_key, ending)) in [
            ("space", PhysKey::Space, " "),
            ("enter", PhysKey::Enter, "\n"),
        ]
        .into_iter()
        .enumerate()
        {
            let mut harness = Harness::new(Lang::Ru);
            harness.type_as("ура ", Lang::Ru);
            assert_eq!(
                harness.layout(),
                Lang::Ru,
                "context must keep Russian layout"
            );
            harness.type_as(word, Lang::En);
            let before_separator = harness
                .text()
                .strip_prefix("ура ")
                .expect("context prefix")
                .to_owned();
            harness.tap(separator_key);

            let automatic_conversions = harness
                .conversions()
                .into_iter()
                .filter(|kind| *kind == ConversionKind::Automatic)
                .count();
            let switched = harness.text() == format!("ура {word}{ending}")
                && harness.layout() == Lang::En
                && automatic_conversions == 1;
            total[index] += 1;
            if !switched {
                missed[index] += 1;
                if marked.contains(word) {
                    marked_missed[index] += 1;
                }
            }
            writeln!(
                report,
                "{word}\t{separator_name}\t{before_separator}\t{}\t{automatic_conversions}\t{:?}\t{}",
                marked.contains(word),
                harness.layout(),
                if switched { "switched" } else { "missed" },
            )
            .expect("write report row");
        }
    }
    writeln!(
        report,
        "# summary\tspace\ttotal\t{}\tswitched\t{}\tmissed\t{}\tmarked_missed\t{}\n\
         # summary\tenter\ttotal\t{}\tswitched\t{}\tmissed\t{}\tmarked_missed\t{}",
        total[0],
        total[0] - missed[0],
        missed[0],
        marked_missed[0],
        total[1],
        total[1] - missed[1],
        missed[1],
        marked_missed[1],
    )
    .expect("write report summary");
    std::fs::write(&report_path, report)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", report_path.display()));

    println!(
        "{} words; space misses: {}; enter misses: {}; report: {}",
        words.len(),
        missed[0],
        missed[1],
        report_path.display(),
    );
}

/// Analyses only words missed by [`report_csw_lowercase_after_russian_context`].
/// A collision means that the Russian rendering of the same physical keys is
/// recognised by the built-in Russian dictionary or frequency list.
#[test]
#[ignore = "requires the local lower-case report and writes a local collision report"]
fn report_csw_lowercase_missed_collisions() {
    let missed = missed_words_from_report();
    let missed_count = missed.len();
    let ru_dictionary = data::dictionary(Lang::Ru);
    let en_dictionary = data::dictionary(Lang::En);
    let ru_model = data::language_model(Lang::Ru);
    let en_model = data::language_model(Lang::En);
    let ignored_abbreviations = ignored_abbreviations();
    let report_path = test_input_path("CSW24-2-3.lowercase-missed-collisions.tsv");
    let mut report = String::from(
        "# collision means the Russian reading is known to the built-in dictionary or frequency list and is not an abbreviation\n\
         word\trussian_reading\tmissed_after\twas_marked\tenglish_dictionary\tenglish_rank\t\
         russian_dictionary\trussian_rank\trussian_abbreviation\trussian_known\tboth_known\n",
    );
    let mut russian_known_count = 0_usize;
    let mut both_known_count = 0_usize;
    let mut abbreviation_count = 0_usize;

    for (word, missed_word) in missed {
        let english_dictionary = en_dictionary.check(&word);
        let english_rank = en_model.rank(&word);
        let russian_dictionary = ru_dictionary.check(&missed_word.russian_reading);
        let russian_rank = ru_model.rank(&missed_word.russian_reading);
        let russian_abbreviation = ignored_abbreviations.contains(&missed_word.russian_reading)
            || (ru_dictionary.check(&missed_word.russian_reading.to_uppercase())
                && !russian_dictionary);
        let russian_known = !russian_abbreviation && (russian_dictionary || russian_rank.is_some());
        let both_known = russian_known && (english_dictionary || english_rank.is_some());
        russian_known_count += usize::from(russian_known);
        both_known_count += usize::from(both_known);
        abbreviation_count += usize::from(russian_abbreviation);
        writeln!(
            report,
            "{word}\t{}\t{}\t{}\t{english_dictionary}\t{}\t{russian_dictionary}\t{}\t{russian_abbreviation}\t{russian_known}\t{both_known}",
            missed_word.russian_reading,
            missed_word.separators.into_iter().collect::<Vec<_>>().join(","),
            missed_word.was_marked,
            english_rank.map_or_else(String::new, |rank| rank.to_string()),
            russian_rank.map_or_else(String::new, |rank| rank.to_string()),
        )
        .expect("write collision row");
    }
    writeln!(
        report,
        "# summary\tmissed_words\t{}\tabbreviations_excluded\t{}\trussian_known\t{}\tboth_known\t{}",
        missed_count, abbreviation_count, russian_known_count, both_known_count,
    )
    .expect("write collision summary");
    std::fs::write(&report_path, report)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", report_path.display()));

    println!(
        "{} missed words; Russian reading known: {}; both readings known: {}; report: {}",
        missed_count,
        russian_known_count,
        both_known_count,
        report_path.display(),
    );
}

#[test]
fn extra_rule_correction_is_undoable_and_configurable_live() {
    let mut config = Config::default();
    config.rules_options.extra_rules = false;
    let mut h = Harness::with_config(Lang::En, config.clone());
    h.type_as("кен ", Lang::Ru);
    assert_eq!(h.text(), "rty ");
    config.rules_options.extra_rules = true;
    h.processor.apply_config(config);
    h.type_as("кен ", Lang::Ru);
    assert_eq!(h.text(), "rty кен ");
    assert_eq!(h.layout(), Lang::Ru);
    h.tap(PhysKey::Pause);
    assert_eq!(h.text(), "rty rty ");
    assert_eq!(h.layout(), Lang::En);
}

fn improved_config() -> Config {
    let mut config = Config::default();
    config.rules_options.improve_switching = true;
    config
}

#[test]
#[ignore = "writes a local before/after report from tmp/CSW24-2-3.txt"]
fn report_improved_short_words() {
    let mut words = csw_words();
    words.extend(marked_reference_words());
    words.sort_unstable();
    words.dedup();
    let mut report = String::from(
        "# exact English targets; Russian words and names protected; lowercase input\nword\trussian_reading\tprefix\tseparator\tbaseline\timproved\n",
    );
    let mut totals = [[0_usize; 2]; 4];
    for (prefix_index, prefix) in ["ура ", "в "].iter().enumerate() {
        for (separator_index, (separator, key, ending)) in [
            ("space", PhysKey::Space, " "),
            ("enter", PhysKey::Enter, "\n"),
        ]
        .iter()
        .enumerate()
        {
            for word in &words {
                let mut success = [false; 2];
                let mut typed = String::new();
                for (i, config) in [Config::default(), improved_config()]
                    .into_iter()
                    .enumerate()
                {
                    let mut h = Harness::with_config(Lang::Ru, config);
                    h.type_as(prefix, Lang::Ru);
                    h.type_as(word, Lang::En);
                    typed = h.text().strip_prefix(prefix).expect("prefix").into();
                    h.tap(*key);
                    success[i] =
                        h.text() == format!("{prefix}{word}{ending}") && h.layout() == Lang::En;
                    totals[prefix_index * 2 + separator_index][i] += usize::from(success[i]);
                }
                assert!(!success[0] || success[1], "lost correction: {prefix}{word}");
                writeln!(
                    report,
                    "{word}\t{typed}\t{}\t{separator}\t{}\t{}",
                    prefix.trim(),
                    success[0],
                    success[1]
                )
                .expect("row");
            }
            let counts = totals[prefix_index * 2 + separator_index];
            writeln!(report, "# summary\tprefix={}\tseparator={separator}\ttotal={}\tbaseline_switched={}\timproved_switched={}\tremaining={}", prefix.trim(), words.len(), counts[0], counts[1], words.len()-counts[1]).expect("summary");
            println!(
                "{prefix:?} {separator}: {} -> {} switched out of {}",
                counts[0],
                counts[1],
                words.len()
            );
            assert!(counts[1] > counts[0]);
        }
    }
    std::fs::write(test_input_path("CSW24-2-3.improved-results.tsv"), report).expect("save report");
}

#[test]
fn improved_correction_waits_for_shift_and_is_cancelled_by_focus() {
    for change_focus in [false, true] {
        let mut h = Harness::with_config(Lang::Ru, improved_config());
        h.type_as("ура ", Lang::Ru);
        h.type_as("iso", Lang::En);
        h.key(PhysKey::ShiftLeft, true);
        h.tap(PhysKey::Space);
        assert_eq!(h.text(), "ура шыщ ");
        if change_focus {
            h.processor
                .handle_focus(&okbs_platform::FocusEvent::WindowChanged(None));
        }
        h.key(PhysKey::ShiftLeft, false);
        assert_eq!(
            h.text(),
            if change_focus {
                "ура шыщ "
            } else {
                "ура iso "
            }
        );
    }
}

#[test]
fn undo_after_a_newline_does_not_restore_russian_context_on_the_new_line() {
    let mut h = Harness::with_config(Lang::Ru, improved_config());
    h.type_as("ура ", Lang::Ru);
    h.type_as("iso\n ", Lang::En);
    h.tap(PhysKey::Pause);
    h.type_as("iso ", Lang::En);
    assert_eq!(h.text(), "ура шыщ\n шыщ ");
}

#[test]
fn improves_after_previous_russian_word_or_letter_on_space_and_enter() {
    for prefix in ["ура ", "в ", "б "] {
        for word in ["iso", "dir", "ing"] {
            for (key, ending) in [
                (PhysKey::Space, " "),
                (PhysKey::Enter, "\n"),
                (PhysKey::NumpadEnter, "\n"),
            ] {
                let mut h = Harness::with_config(Lang::Ru, improved_config());
                h.type_as(prefix, Lang::Ru);
                h.type_as(word, Lang::En);
                assert_eq!(h.layout(), Lang::Ru, "no early correction");
                h.tap(key);
                assert_eq!(h.text(), format!("{prefix}{word}{ending}"));
                assert_eq!(h.layout(), Lang::En);
                assert_eq!(h.conversions(), vec![ConversionKind::Automatic]);
                h.tap(PhysKey::Pause);
                let rendered = render(
                    &keys_for_text(word, builtin_keymap(Lang::En)).expect("fixture"),
                    builtin_keymap(Lang::Ru),
                );
                assert_eq!(h.text(), format!("{prefix}{rendered}{ending}"));
                assert_eq!(h.layout(), Lang::Ru);
            }
        }
    }
}

#[test]
fn terminal_corrects_before_submitting_and_cannot_replay_a_submitted_command() {
    for prefix in ["ура ", "в "] {
        for word in ["dir", "iso", "ing"] {
            for enter in [PhysKey::Enter, PhysKey::NumpadEnter] {
                let mut h = Harness::with_config(Lang::Ru, improved_config());
                h.desktop.lock().terminal = true;
                h.enable_gate();
                h.type_as(prefix, Lang::Ru);
                h.type_as(word, Lang::En);
                h.tap(enter);
                assert_eq!(
                    h.desktop.lock().submitted_lines,
                    [format!("{prefix}{word}")]
                );
                assert_eq!(h.desktop.lock().enter_count, 1);
                assert_eq!(h.conversions(), [ConversionKind::Automatic]);
                assert_eq!(h.text(), "");
                h.tap(PhysKey::Pause);
                assert_eq!(h.desktop.lock().enter_count, 1);
                assert_eq!(h.text(), "");
                assert_eq!(h.conversions(), [ConversionKind::Automatic]);
            }
        }
    }
}

#[test]
fn gated_boundaries_preserve_editor_undo_and_corrected_prefix_context() {
    for enter in [PhysKey::Space, PhysKey::Enter] {
        let mut h = Harness::with_config(Lang::Ru, improved_config());
        h.enable_gate();
        h.type_as("ура ", Lang::Ru);
        h.type_as("dir", Lang::En);
        h.tap(enter);
        let suffix = if enter == PhysKey::Space { " " } else { "\n" };
        assert_eq!(h.text(), format!("ура dir{suffix}"));
        h.tap(PhysKey::Pause);
        assert_eq!(h.text(), format!("ура вшк{suffix}"));
    }
    let mut h = Harness::with_config(Lang::En, improved_config());
    h.enable_gate();
    h.type_as("привет ", Lang::Ru);
    assert_eq!(h.text(), "привет ");
    h.type_as("dir ", Lang::En);
    assert_eq!(h.text(), "привет dir ");
}

#[test]
fn terminal_preserves_protections_and_switch_toggle() {
    for protection in 0..5 {
        let mut config = improved_config();
        if protection == 0 {
            config.general.autoswitch = false;
        }
        if protection == 1 {
            config.troubleshooting.no_switch_on_tab_enter = true;
        }
        if protection == 4 {
            config
                .exclusions
                .executables
                .push(okbs_core::config::ExecutableExclusion {
                    path: "powershell.exe".into(),
                });
        }
        let mut h = Harness::with_config(Lang::Ru, config);
        h.desktop.lock().terminal = true;
        h.enable_gate();
        if protection == 2 {
            h.desktop.lock().password = true;
        }
        if protection == 3 {
            h.processor.set_autoswitch(false);
        }
        if protection == 4 {
            h.desktop.lock().window_exe = Some("powershell.exe".into());
        }
        h.type_as("ура ", Lang::Ru);
        h.type_as("dir\n", Lang::En);
        assert_eq!(
            h.desktop.lock().submitted_lines,
            ["ура вшк"],
            "protection {protection}"
        );
        assert_eq!(h.desktop.lock().enter_count, 1);
        assert!(h.conversions().is_empty());
        if protection == 3 {
            h.processor.set_autoswitch(true);
            h.type_as("ура ", Lang::Ru);
            h.type_as("dir\n", Lang::En);
            assert_eq!(h.desktop.lock().submitted_lines, ["ура вшк", "ура dir"]);
        }
    }
}

#[test]
fn terminal_never_changes_a_line_after_unintercepted_enter() {
    let mut h = Harness::with_config(Lang::Ru, improved_config());
    h.desktop.lock().terminal = true;
    h.enable_gate();
    h.type_as("ура ", Lang::Ru);
    h.type_as("dir", Lang::En);
    h.combo(PhysKey::ShiftLeft, PhysKey::Enter);
    h.tap(PhysKey::Pause);
    assert_eq!(h.desktop.lock().submitted_lines, ["ура вшк"]);
    assert_eq!(h.text(), "");
    assert!(h.conversions().is_empty());
}

#[test]
fn terminal_does_not_submit_when_layout_switch_fails() {
    let mut h = Harness::with_config(Lang::Ru, improved_config());
    h.desktop.lock().terminal = true;
    h.enable_gate();
    h.type_as("ура ", Lang::Ru);
    h.type_as("dir", Lang::En);
    h.desktop.lock().fail_layout = true;
    h.tap(PhysKey::Enter);
    assert!(h.desktop.lock().submitted_lines.is_empty());
    assert_eq!(h.text(), "ура вшк");
    assert!(h.events.iter().any(|e| matches!(e, Event::Error(_))));
    h.desktop.lock().fail_layout = false;
    h.tap(PhysKey::Enter);
    assert_eq!(h.desktop.lock().submitted_lines, ["ура вшк"]);
}

#[test]
fn terminal_aborts_correction_when_focus_changes_while_switching_layout() {
    let mut h = Harness::with_config(Lang::Ru, improved_config());
    h.desktop.lock().terminal = true;
    h.enable_gate();
    h.type_as("ура ", Lang::Ru);
    h.type_as("dir", Lang::En);
    h.desktop.lock().focus_change_on_layout = true;
    h.tap(PhysKey::Enter);
    assert!(h.desktop.lock().submitted_lines.is_empty());
    assert_eq!(h.text(), "ура вшк");
    assert!(h.events.iter().any(|e| matches!(e, Event::Error(_))));
}

#[test]
fn terminal_burst_preserves_the_russian_prefix_before_the_engine_reads_it() {
    let mut h = Harness::with_config(Lang::Ru, improved_config());
    h.desktop.lock().terminal = true;
    h.enable_gate();
    let mut keys = keys_for_text("ура ", builtin_keymap(Lang::Ru)).expect("prefix");
    keys.extend(keys_for_text("dir", builtin_keymap(Lang::En)).expect("command"));
    keys.push(KeyPress::plain(PhysKey::Enter));
    let mut queue = Vec::new();
    for key in keys {
        for pressed in [true, false] {
            queue.push(h.source_event(key.key, pressed, false, Instant::now()));
        }
    }
    assert_eq!(h.text(), "ура");
    for event in queue {
        h.events.extend(h.processor.handle_input(event));
    }
    assert_eq!(h.desktop.lock().submitted_lines, ["ура dir"]);
    assert_eq!(h.desktop.lock().enter_count, 1);
}

#[test]
fn terminal_queued_input_keeps_command_order() {
    let mut h = Harness::new(Lang::En);
    h.desktop.lock().terminal = true;
    h.enable_gate();
    h.type_as("привет", Lang::Ru);
    let mut queue = Vec::new();
    for key in [
        PhysKey::Enter,
        PhysKey::KeyL,
        PhysKey::KeyF,
        PhysKey::Space,
        PhysKey::Enter,
    ] {
        for pressed in [true, false] {
            queue.push(h.source_event(key, pressed, false, Instant::now()));
        }
    }
    assert!(h.desktop.lock().submitted_lines.is_empty());
    for event in queue {
        h.events.extend(h.processor.handle_input(event));
    }
    assert_eq!(h.desktop.lock().submitted_lines, ["привет", "да "]);
    assert_eq!(h.desktop.lock().enter_count, 2);
    assert_eq!(h.text(), "");
}

#[test]
fn improvement_never_uses_an_older_word_or_another_input_location() {
    for prefix in ["", "ура\n", "ура 123 ", "ура жжжж ", "ура / "] {
        let mut h = Harness::with_config(Lang::Ru, improved_config());
        h.type_as(prefix, Lang::Ru);
        h.type_as("iso ", Lang::En);
        assert!(h.text().ends_with("шыщ "), "{prefix:?}: {}", h.text());
    }
    for action in 0..4 {
        let mut h = Harness::with_config(Lang::Ru, improved_config());
        h.type_as("ура ", Lang::Ru);
        match action {
            0 => {
                h.processor
                    .handle_focus(&okbs_platform::FocusEvent::WindowChanged(None));
            }
            1 => h.tap(PhysKey::ArrowLeft),
            2 => {
                h.processor.handle_layout_change(id(Lang::Ru));
            }
            _ => {
                h.events
                    .extend(h.processor.handle_input(InputEvent::MouseButton {
                        time: Instant::now(),
                        in_own_window: false,
                    }));
            }
        }
        h.type_as("iso ", Lang::En);
        assert_eq!(h.layout(), Lang::Ru, "action {action}");
        assert!(h.conversions().is_empty(), "action {action}");
    }
}

#[test]
fn improvement_preserves_legacy_single_letter_context() {
    let mut baseline = Harness::new(Lang::En);
    let mut improved = Harness::with_config(Lang::En, improved_config());
    for h in [&mut baseline, &mut improved] {
        h.type_as("plan qwerty b ", Lang::En);
    }
    assert_eq!(baseline.text(), "plan qwerty b ");
    assert_eq!(improved.text(), baseline.text());
    assert_eq!(improved.layout(), baseline.layout());
}

#[test]
fn improvement_preserves_russian_phrases_and_protection_settings() {
    for phrase in [
        "из рук ",
        "в суд ",
        "не ищи ",
        "принимает душ ",
        "ура ну ",
        "из уфы ",
        "это луи ",
        "там рур ",
        "привет мир ",
    ] {
        let mut h = Harness::with_config(Lang::Ru, improved_config());
        h.type_as(phrase, Lang::Ru);
        assert_eq!(h.text(), phrase);
        assert!(h.conversions().is_empty());
    }
    for case in 0..5 {
        let mut config = improved_config();
        if case == 0 {
            config.general.autoswitch = false;
        }
        if case == 4 {
            config.troubleshooting.no_switch_on_tab_enter = true;
        }
        let mut h = Harness::with_config(Lang::Ru, config);
        if case == 1 {
            h.desktop.lock().password = true;
        }
        if case == 2 {
            h.desktop.lock().window_pid = std::process::id();
        }
        h.type_as("ура ", Lang::Ru);
        if case == 3 {
            h.type_as("isx", Lang::En);
            h.tap(PhysKey::Backspace);
            h.type_as("o", Lang::En);
        } else {
            h.type_as("iso", Lang::En);
        }
        h.tap(if case == 4 {
            PhysKey::Enter
        } else {
            PhysKey::Space
        });
        assert!(h.conversions().is_empty(), "case {case}");
        assert_eq!(h.layout(), Lang::Ru);
    }
}

#[test]
fn improvement_updates_live_and_leaves_autoreplace_first() {
    let mut h = Harness::new(Lang::Ru);
    h.type_as("ура ", Lang::Ru);
    h.type_as("iso ", Lang::En);
    assert_eq!(h.text(), "ура шыщ ");
    h.processor.apply_config(improved_config());
    h.type_as("ура ", Lang::Ru);
    h.type_as("iso ", Lang::En);
    assert!(h.text().ends_with("ура iso "));
    let mut config = autoreplace_config();
    config.rules_options.improve_switching = true;
    let expected = format!("ура {} ", config.autoreplace.items[0].to);
    let mut h = Harness::with_config(Lang::Ru, config);
    h.type_as("ура снп ", Lang::Ru);
    assert_eq!(h.text(), expected);
    assert!(h.events.contains(&Event::Autoreplaced));
}

#[test]
fn extra_rule_corrections_obey_focus_autoswitch_and_separators() {
    for case in 0..4 {
        let mut config = Config::default();
        if case == 0 {
            config.general.autoswitch = false;
        }
        let mut h = Harness::with_config(Lang::En, config);
        if case == 1 {
            h.desktop.lock().password = true;
        }
        if case == 2 {
            h.desktop.lock().window_pid = std::process::id();
        }
        if case == 3 {
            // A user edit blocks automatic conversion of this word.
            h.type_as("rtx", Lang::En);
            h.tap(PhysKey::Backspace);
            h.type_as("y ", Lang::En);
        } else {
            h.type_as("кен ", Lang::Ru);
        }
        assert_eq!(h.text(), "rty ", "case {case}");
        assert!(h.conversions().is_empty());
    }
    for separator in [PhysKey::Space, PhysKey::Enter, PhysKey::Tab] {
        let mut h = Harness::new(Lang::En);
        h.type_as("кен", Lang::Ru);
        assert_eq!(h.text(), "rty", "no early conversion");
        h.tap(separator);
        let ending = match separator {
            PhysKey::Enter => "\n",
            PhysKey::Tab => "\t",
            _ => " ",
        };
        assert_eq!(h.text(), format!("кен{ending}"));
    }
}

#[test]
fn converts_russian_typed_in_english_layout() {
    let mut h = Harness::new(Lang::En);
    h.type_as("привет мир ", Lang::Ru);
    assert_eq!(h.text(), "привет мир ");
    assert_eq!(h.layout(), Lang::Ru);
    assert_eq!(h.conversions(), vec![ConversionKind::Automatic]);
}

#[test]
fn converts_english_typed_in_russian_layout() {
    let mut h = Harness::new(Lang::Ru);
    h.type_as("Hello, world! ", Lang::En);
    assert_eq!(h.text(), "Hello, world! ");
    assert_eq!(h.layout(), Lang::En);
}

/// Words the customer reported: abbreviations and short words have no
/// frequency rank, so only the dictionary can place them.
#[test]
fn converts_abbreviations_and_short_words() {
    for (word, keys_of, from) in [
        ("ISO ", Lang::En, Lang::Ru),
        ("You ", Lang::En, Lang::Ru),
        ("id ", Lang::En, Lang::Ru),
        ("НДС ", Lang::Ru, Lang::En),
        ("ФСБ ", Lang::Ru, Lang::En),
        ("лук ", Lang::Ru, Lang::En),
    ] {
        let mut h = Harness::new(from);
        h.type_as(word, keys_of);
        assert_eq!(h.text(), word, "{word}");
        assert_eq!(h.layout(), keys_of, "{word}");
        assert_eq!(h.conversions(), vec![ConversionKind::Automatic], "{word}");
    }
}

/// Both readings are ordinary words, so what the user typed is kept.
#[test]
fn keeps_words_that_read_as_words_in_both_layouts() {
    let mut h = Harness::new(Lang::Ru);
    h.type_as("ye ", Lang::En);
    assert_eq!(h.text(), "ну ");
    assert_eq!(h.layout(), Lang::Ru);
    assert!(h.conversions().is_empty());
}

/// Reported by the customer: a rare English word after an English one still
/// gives way to a very common Russian word.
#[test]
fn rare_english_word_after_english_text_yields_to_a_common_russian_one() {
    let mut h = Harness::new(Lang::En);
    h.type_as("Path ye ", Lang::En);
    assert_eq!(h.text(), "Path ну ");
    assert_eq!(h.layout(), Lang::Ru);
}

#[test]
fn converts_one_letter_words() {
    let mut h = Harness::new(Lang::En);
    h.type_as("so need check\n", Lang::En);
    h.type_as("я думаю что\n", Lang::Ru);
    assert_eq!(h.text(), "so need check\nя думаю что\n");
    assert_eq!(h.layout(), Lang::Ru);
    h.type_as("i know ", Lang::En);
    assert_eq!(h.text(), "so need check\nя думаю что\ni know ");
    assert_eq!(h.layout(), Lang::En);
}

#[test]
fn stray_letter_in_english_text_stays() {
    let mut h = Harness::new(Lang::En);
    h.type_as("plan b is fine ", Lang::En);
    assert_eq!(h.text(), "plan b is fine ");
    assert!(h.conversions().is_empty());
}

#[test]
fn break_converts_one_letter() {
    let mut h = Harness::new(Lang::En);
    h.type_as("plan c ", Lang::En);
    h.tap(PhysKey::Pause);
    assert_eq!(h.text(), "plan с ");
    assert_eq!(h.layout(), Lang::Ru);
    h.tap(PhysKey::Pause);
    assert_eq!(h.text(), "plan c ");

    let mut h = Harness::new(Lang::Ru);
    h.type_as("ч", Lang::Ru);
    h.tap(PhysKey::Pause);
    assert_eq!(h.text(), "x");
    assert_eq!(h.layout(), Lang::En);
}

#[test]
fn no_autoswitch_in_own_windows() {
    let mut h = Harness::new(Lang::En);
    h.desktop.lock().window_pid = std::process::id();
    h.type_as("ghbdtn ", Lang::En);
    assert_eq!(h.text(), "ghbdtn ");
    h.tap(PhysKey::Pause);
    assert_eq!(h.text(), "привет ", "manual conversion still works");
}

fn config_with(change: impl FnOnce(&mut Config)) -> Config {
    let mut config = Config::default();
    change(&mut config);
    config
}

#[test]
fn right_ctrl_short_press_toggles_layout() {
    use okbs_core::config::SwitchKey;
    let mut h = Harness::with_config(
        Lang::En,
        config_with(|c| c.switching.switch_key = SwitchKey::RightCtrl),
    );
    h.tap(PhysKey::ControlRight);
    assert_eq!(h.layout(), Lang::Ru);
    h.tap(PhysKey::ControlRight);
    assert_eq!(h.layout(), Lang::En);

    let start = Instant::now();
    h.key_at(PhysKey::ControlRight, true, start);
    h.key_at(
        PhysKey::ControlRight,
        false,
        start + Duration::from_millis(600),
    );
    assert_eq!(
        h.layout(),
        Lang::En,
        "long press is a modifier, not a switch"
    );

    h.key(PhysKey::ControlRight, true);
    h.tap(PhysKey::KeyC);
    h.key(PhysKey::ControlRight, false);
    assert_eq!(h.layout(), Lang::En, "RCtrl+C does not switch");
}

#[test]
fn switched_layout_blocks_conversion_of_next_word() {
    use okbs_core::config::SwitchKey;
    let mut h = Harness::with_config(
        Lang::Ru,
        config_with(|c| c.switching.switch_key = SwitchKey::RightCtrl),
    );
    h.tap(PhysKey::ControlRight);
    assert_eq!(h.layout(), Lang::En);
    h.type_as("ghbdtn ", Lang::En);
    assert_eq!(h.text(), "ghbdtn ", "the user chose the layout explicitly");
    h.type_as("ghbdtn ", Lang::En);
    assert_eq!(h.text(), "ghbdtn привет ");
}

#[test]
fn direct_shift_keys_choose_layouts() {
    let mut h = Harness::with_config(
        Lang::En,
        config_with(|c| c.switching.direct_keys.enabled = true),
    );
    h.tap(PhysKey::ShiftLeft);
    assert_eq!(h.layout(), Lang::Ru);
    h.tap(PhysKey::ShiftLeft);
    assert_eq!(h.layout(), Lang::Ru);
    h.tap(PhysKey::ShiftRight);
    assert_eq!(h.layout(), Lang::En);
    h.type_as("Hello ", Lang::En);
    assert_eq!(
        h.layout(),
        Lang::En,
        "Shift used for a capital letter does not switch"
    );
}

#[test]
fn caps_lock_as_switch_key_is_swallowed() {
    use okbs_core::config::SwitchKey;
    let mut h = Harness::with_config(
        Lang::En,
        config_with(|c| c.switching.switch_key = SwitchKey::CapsLock),
    );
    h.swallows = true;
    h.processor.set_swallows_capslock(true);
    h.tap(PhysKey::CapsLock);
    assert_eq!(h.layout(), Lang::Ru);
    assert!(!h.desktop.lock().caps);
    h.type_as("привет ", Lang::Ru);
    assert_eq!(h.text(), "привет ");
}

#[test]
fn disabled_caps_lock_and_scroll_lock_as_caps_lock() {
    let mut h = Harness::with_config(
        Lang::Ru,
        config_with(|c| {
            c.advanced.disable_capslock = true;
            c.advanced.scrolllock_as_capslock = true;
        }),
    );
    h.swallows = true;
    h.processor.set_swallows_capslock(true);
    h.tap(PhysKey::CapsLock);
    assert!(!h.desktop.lock().caps);
    h.tap(PhysKey::ScrollLock);
    assert!(h.desktop.lock().caps, "Scroll Lock toggles Caps Lock");
    h.type_as("мир ", Lang::Ru);
    assert_eq!(h.text(), "МИР ");
}

#[test]
fn two_initial_capitals_are_fixed() {
    let mut h = Harness::new(Lang::Ru);
    h.type_as("ПРивет СПб ", Lang::Ru);
    assert_eq!(h.text(), "Привет СПб ");
    assert!(h.events.contains(&Event::CaseFixed));
}

#[test]
fn accidental_caps_lock_is_fixed() {
    let mut h = Harness::new(Lang::Ru);
    h.tap(PhysKey::CapsLock);
    h.type_as("Привет ", Lang::Ru);
    assert_eq!(h.text(), "Привет ");
    assert!(!h.desktop.lock().caps, "Caps Lock turned off");
    h.type_as("мир ", Lang::Ru);
    assert_eq!(h.text(), "Привет мир ");
}

#[test]
fn caps_fix_and_conversion_in_one_retype() {
    let mut h = Harness::new(Lang::En);
    h.tap(PhysKey::CapsLock);
    h.type_as("Привет ", Lang::Ru);
    assert_eq!(h.text(), "Привет ");
    assert_eq!(h.layout(), Lang::Ru);
    assert!(!h.desktop.lock().caps);
}

#[test]
fn case_fixes_can_be_disabled() {
    let mut h = Harness::with_config(
        Lang::Ru,
        config_with(|c| c.advanced.fix_two_capitals = false),
    );
    h.type_as("ПРивет ", Lang::Ru);
    assert_eq!(h.text(), "ПРивет ");
}

#[test]
fn double_space_becomes_comma() {
    let mut h = Harness::with_config(
        Lang::Ru,
        config_with(|c| c.advanced.double_space_comma = true),
    );
    h.type_as("привет  как дела", Lang::Ru);
    assert_eq!(h.text(), "привет, как дела");
    let mut h = Harness::with_config(
        Lang::En,
        config_with(|c| c.advanced.double_space_comma = true),
    );
    h.type_as("hello  world", Lang::En);
    assert_eq!(h.text(), "hello, world");
    let mut h = Harness::new(Lang::Ru);
    h.type_as("привет  ", Lang::Ru);
    assert_eq!(h.text(), "привет  ", "off by default");
}

#[test]
fn excluded_programs_are_left_alone() {
    type Case = (fn(&mut Config), Option<&'static str>, Option<&'static str>);
    let cases: [Case; 4] = [
        (
            |c| {
                c.exclusions
                    .executables
                    .push(okbs_core::config::ExecutableExclusion {
                        path: r"C:\Program Files\Game\game.exe".into(),
                    })
            },
            Some(r"c:\program files\game\GAME.exe"),
            None,
        ),
        (
            |c| {
                c.exclusions
                    .executables
                    .push(okbs_core::config::ExecutableExclusion {
                        path: "code.exe".into(),
                    })
            },
            Some(r"C:\Users\me\AppData\Local\Programs\Code\Code.exe"),
            None,
        ),
        (
            |c| {
                c.exclusions.titles.push(okbs_core::config::TitleExclusion {
                    contains: "Visual Studio".into(),
                })
            },
            None,
            Some("main.rs - Visual Studio Code"),
        ),
        (
            |c| {
                c.exclusions
                    .folders
                    .push(okbs_core::config::FolderExclusion {
                        path: r"D:\Games\".into(),
                    })
            },
            Some(r"D:\Games\Chess\chess.exe"),
            None,
        ),
    ];
    for (setup, exe, title) in cases {
        let mut h = Harness::with_config(Lang::En, config_with(setup));
        {
            let mut d = h.desktop.lock();
            d.window_exe = exe.map(str::to_string);
            d.window_title = title.map(str::to_string);
        }
        h.type_as("ghbdtn ", Lang::En);
        assert_eq!(h.text(), "ghbdtn ", "{exe:?} {title:?}");
        h.desktop.lock().window_exe = Some(r"C:\Windows\notepad.exe".into());
        h.desktop.lock().window_title = Some("Notepad".into());
        h.type_as("ghbdtn ", Lang::En);
        assert_eq!(h.text(), "ghbdtn привет ");
    }
}

#[test]
fn ignore_excluded_programs_completely() {
    let mut h = Harness::with_config(
        Lang::En,
        config_with(|c| {
            c.troubleshooting.ignore_excluded_apps_completely = true;
            c.exclusions.titles.push(okbs_core::config::TitleExclusion {
                contains: "Game".into(),
            });
        }),
    );
    h.desktop.lock().window_title = Some("Game".into());
    h.type_as("ghbdtn ", Lang::En);
    h.tap(PhysKey::Pause);
    assert_eq!(h.text(), "ghbdtn ", "hotkeys do not work either");
}

#[test]
fn password_fields_are_left_alone() {
    let mut h = Harness::new(Lang::En);
    h.desktop.lock().password = true;
    h.type_as("ghbdtn ", Lang::En);
    assert_eq!(h.text(), "ghbdtn ");
}

#[test]
fn single_layout_follows_focus() {
    use okbs_core::config::SwitchKey;
    let mut h = Harness::with_config(
        Lang::En,
        config_with(|c| {
            c.switching.switch_key = SwitchKey::RightCtrl;
            c.switching.single_layout_for_all_windows = true;
        }),
    );
    h.tap(PhysKey::ControlRight);
    assert_eq!(h.layout(), Lang::Ru);
    h.desktop.lock().layout = Lang::En;
    h.processor
        .handle_focus(&okbs_platform::FocusEvent::WindowChanged(None));
    assert_eq!(h.layout(), Lang::Ru);
}

#[test]
fn user_layout_change_blocks_next_word() {
    let mut h = Harness::new(Lang::En);
    let events = h.processor.handle_layout_change(id(Lang::En));
    assert!(
        matches!(events.as_slice(), [Event::LayoutChanged(Some(info))] if info.lang == Some(Lang::En) && info.locale == "en-US")
    );
    h.type_as("ghbdtn ", Lang::En);
    assert_eq!(h.text(), "ghbdtn ");
    h.type_as("ghbdtn ", Lang::En);
    assert_eq!(h.text(), "ghbdtn привет ");
    let before = h.events.len();
    let ours = h.processor.handle_layout_change(id(Lang::Ru));
    assert!(
        matches!(ours.as_slice(), [Event::LayoutChanged(Some(info))] if info.lang == Some(Lang::Ru))
    );
    assert_eq!(h.events.len(), before);
    h.type_as("руки ", Lang::Ru);
    h.tap(PhysKey::Pause);
    assert_eq!(
        h.text(),
        "ghbdtn привет herb ",
        "our own change does not block"
    );
}

#[test]
fn keeps_correct_text() {
    let mut h = Harness::new(Lang::Ru);
    h.type_as("это правильный текст ", Lang::Ru);
    assert_eq!(h.text(), "это правильный текст ");
    assert_eq!(h.desktop.lock().injected, 0);
}

#[test]
fn shifted_separator_waits_for_shift_release() {
    let mut h = Harness::new(Lang::En);
    h.type_as("Привет, как дела? ", Lang::Ru);
    assert_eq!(h.text(), "Привет, как дела? ");
}

#[test]
fn break_undoes_and_redoes_conversion() {
    let mut h = Harness::new(Lang::En);
    h.type_as("привет ", Lang::Ru);
    assert_eq!(h.text(), "привет ");
    h.tap(PhysKey::Pause);
    assert_eq!(h.text(), "ghbdtn ");
    assert_eq!(h.layout(), Lang::En);
    h.tap(PhysKey::Pause);
    assert_eq!(h.text(), "привет ");
    assert_eq!(
        h.conversions(),
        vec![
            ConversionKind::Automatic,
            ConversionKind::Undo,
            ConversionKind::Manual
        ]
    );
}

#[test]
fn break_converts_word_being_typed() {
    let mut h = Harness::new(Lang::En);
    h.type_as("прив", Lang::Ru);
    assert_eq!(h.text(), "ghbd");
    h.tap(PhysKey::Pause);
    assert_eq!(h.text(), "прив");
    assert_eq!(h.layout(), Lang::Ru);
    h.type_as("ет ", Lang::Ru);
    assert_eq!(h.text(), "привет ");
    assert_eq!(h.conversions(), vec![ConversionKind::Manual]);
}

#[test]
fn break_converts_last_word_that_stayed() {
    let mut h = Harness::new(Lang::Ru);
    h.type_as("руки ", Lang::Ru);
    h.tap(PhysKey::Pause);
    assert_eq!(h.text(), "herb ");
    assert_eq!(h.layout(), Lang::En);
}

#[test]
fn repeated_undo_suggests_rule() {
    let mut h = Harness::new(Lang::En);
    for _ in 0..2 {
        h.type_as("ghbdtn ", Lang::En);
        h.tap(PhysKey::Pause);
        h.type_as(" ", Lang::En);
    }
    let rules: Vec<_> = h
        .events
        .iter()
        .filter_map(|e| match e {
            Event::SuggestRule(rule) => Some(rule.pattern.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(rules, vec!["ghbdtn".to_string()]);
}

#[test]
fn backspace_inside_word_blocks_conversion() {
    let mut h = Harness::new(Lang::En);
    h.type_as("ghbdtx", Lang::En);
    h.tap(PhysKey::Backspace);
    h.type_as("n ", Lang::En);
    assert_eq!(h.text(), "ghbdtn ");
    assert!(h.conversions().is_empty());
}

#[test]
fn backspace_blocking_can_be_disabled() {
    let mut config = Config::default();
    config.troubleshooting.no_switch_after.backspace = false;
    let mut h = Harness::with_config(Lang::En, config);
    h.type_as("ghbdtx", Lang::En);
    h.tap(PhysKey::Backspace);
    h.type_as("n ", Lang::En);
    assert_eq!(h.text(), "привет ");
}

#[test]
fn autoswitch_off_does_nothing() {
    let mut config = Config::default();
    config.general.autoswitch = false;
    let mut h = Harness::with_config(Lang::En, config);
    h.type_as("ghbdtn ", Lang::En);
    assert_eq!(h.text(), "ghbdtn ");
    h.tap(PhysKey::Pause);
    assert_eq!(h.text(), "привет ");
}

#[test]
fn mouse_click_and_ctrl_combos_reset_the_word() {
    let mut h = Harness::new(Lang::En);
    h.type_as("ghbd", Lang::En);
    h.events
        .extend(h.processor.handle_input(InputEvent::MouseButton {
            time: Instant::now(),
            in_own_window: false,
        }));
    h.type_as("tn ", Lang::En);
    assert_eq!(h.text(), "ghbdtn ");

    h.type_as("ghbd", Lang::En);
    h.combo(PhysKey::ControlLeft, PhysKey::KeyZ);
    h.type_as("tn ", Lang::En);
    assert!(h.conversions().is_empty());
}

#[test]
fn shift_break_converts_selection() {
    let mut h = Harness::new(Lang::En);
    {
        let mut d = h.desktop.lock();
        d.text = "ghbdtn? vbh!".chars().collect();
        d.clipboard = Some("keep me".into());
    }
    h.select_all();
    h.combo(PhysKey::ShiftLeft, PhysKey::Pause);
    assert_eq!(h.text(), "привет, мир!");
    assert_eq!(h.layout(), Lang::Ru);
    assert_eq!(h.desktop.lock().clipboard.as_deref(), Some("keep me"));
    assert!(
        h.events
            .contains(&Event::SelectionConverted(TextOp::Layout))
    );
}

#[test]
fn alt_break_inverts_case_and_alt_scroll_lock_transliterates() {
    let mut h = Harness::new(Lang::Ru);
    h.desktop.lock().text = "пРИВЕТ".chars().collect();
    h.select_all();
    h.key(PhysKey::AltLeft, true);
    h.tap(PhysKey::Pause);
    h.key(PhysKey::AltLeft, false);
    assert_eq!(h.text(), "Привет");

    h.select_all();
    h.key(PhysKey::AltLeft, true);
    h.tap(PhysKey::ScrollLock);
    h.key(PhysKey::AltLeft, false);
    assert_eq!(h.text(), "Privet");
}

#[test]
fn nothing_selected_restores_clipboard() {
    let mut h = Harness::new(Lang::En);
    h.desktop.lock().clipboard = Some("saved".into());
    h.combo(PhysKey::ShiftLeft, PhysKey::Pause);
    assert_eq!(h.desktop.lock().clipboard.as_deref(), Some("saved"));
    assert_eq!(h.text(), "");
}

#[test]
fn clipboard_operation_from_menu() {
    let mut h = Harness::new(Lang::En);
    h.desktop.lock().clipboard = Some("ghbdtn".into());
    let events = h.processor.clipboard_op(TextOp::Layout);
    assert_eq!(h.desktop.lock().clipboard.as_deref(), Some("привет"));
    assert!(matches!(
        events.as_slice(),
        [Event::ClipboardConverted { .. }]
    ));
}

#[test]
fn enter_converts_unless_disabled() {
    let mut h = Harness::new(Lang::En);
    h.type_as("ghbdtn\n", Lang::En);
    assert_eq!(h.text(), "привет\n");

    let mut config = Config::default();
    config.troubleshooting.no_switch_on_tab_enter = true;
    let mut h = Harness::with_config(Lang::En, config);
    h.type_as("ghbdtn\n", Lang::En);
    assert_eq!(h.text(), "ghbdtn\n");
}

#[test]
fn caps_lock_words_are_converted_with_caps() {
    let mut h = Harness::new(Lang::En);
    h.tap(PhysKey::CapsLock);
    h.type_as("ghbdtn ", Lang::En);
    assert_eq!(h.text(), "ПРИВЕТ ");
}

#[test]
fn toggle_autoswitch_hotkey() {
    let mut config = Config::default();
    config.hotkeys.toggle_autoswitch = "Ctrl+Alt+K"
        .parse()
        .map(okbs_core::config::HotkeyBinding::some)
        .unwrap();
    let mut h = Harness::with_config(Lang::En, config);
    h.key(PhysKey::ControlLeft, true);
    h.key(PhysKey::AltLeft, true);
    h.tap(PhysKey::KeyK);
    h.key(PhysKey::AltLeft, false);
    h.key(PhysKey::ControlLeft, false);
    assert!(h.events.contains(&Event::AutoswitchChanged(false)));
    h.type_as("ghbdtn ", Lang::En);
    assert_eq!(h.text(), "ghbdtn ");
}

#[test]
fn captures_hotkeys_without_acting() {
    let mut h = Harness::new(Lang::En);
    h.type_as("ghbdtn ", Lang::En);
    h.processor.set_capture(true);
    h.combo(PhysKey::ShiftLeft, PhysKey::Pause);
    assert_eq!(h.text(), "привет ");
    assert!(
        h.events
            .contains(&Event::HotkeyCaptured("Shift+Break".parse().unwrap()))
    );
    h.processor.set_capture(true);
    h.key(PhysKey::ControlRight, true);
    h.key(PhysKey::AltLeft, true);
    h.tap(PhysKey::F11);
    assert!(
        h.events
            .contains(&Event::HotkeyCaptured("Ctrl+Alt+F11".parse().unwrap()))
    );
    h.key(PhysKey::AltLeft, false);
    h.key(PhysKey::ControlRight, false);
    h.processor.set_capture(true);
    h.tap(PhysKey::Escape);
    assert!(h.events.contains(&Event::CaptureCancelled));
    h.type_as("руки ", Lang::Ru);
    h.tap(PhysKey::Pause);
    assert_eq!(
        h.text(),
        "привет herb ",
        "capture mode ended, Break works again"
    );
}

#[test]
fn engine_thread_processes_input() {
    use crossbeam_channel::unbounded;
    let h = Harness::new(Lang::En);
    let desktop = h.desktop.clone();
    let (input_tx, input_rx) = unbounded();
    let handle = crate::EngineHandle::spawn(
        h.processor,
        crate::Inputs {
            input: input_rx,
            layout: None,
            focus: None,
            clipboard: None,
        },
    )
    .unwrap();
    let events = handle.events().clone();
    assert_eq!(
        events.recv_timeout(Duration::from_secs(5)).unwrap(),
        Event::Started
    );
    for key in [
        PhysKey::KeyG,
        PhysKey::KeyH,
        PhysKey::KeyB,
        PhysKey::KeyD,
        PhysKey::KeyT,
        PhysKey::KeyN,
        PhysKey::Space,
    ] {
        for pressed in [true, false] {
            desktop.lock().apply(key, pressed);
            input_tx
                .send(InputEvent::Key {
                    key,
                    pressed,
                    repeat: false,
                    injected: false,
                    time: Instant::now(),
                })
                .unwrap();
        }
    }
    let converted = events.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(
        matches!(converted, Event::Converted { to: Lang::Ru, .. }),
        "{converted:?}"
    );
    assert_eq!(desktop.lock().text.iter().collect::<String>(), "привет ");
    assert!(handle.send(crate::Command::ToggleAutoswitch));
    assert_eq!(
        events.recv_timeout(Duration::from_secs(5)).unwrap(),
        Event::AutoswitchChanged(false)
    );
    handle.shutdown();
    assert_eq!(
        events.recv_timeout(Duration::from_secs(5)).unwrap(),
        Event::Stopped
    );
}

fn autoreplace_config() -> Config {
    config_with(|c| {
        c.autoreplace.trigger = okbs_core::config::AutoReplaceTrigger::Space;
        c.autoreplace.items = vec![okbs_core::config::AutoReplaceItem {
            from: "снп".into(),
            to: "С наилучшими пожеланиями,\nИван".into(),
            cursor_pos: -1,
        }];
    })
}

#[test]
fn autoreplace_on_space_precedes_layout_detection_and_preserves_clipboard() {
    for lang in Lang::ALL {
        let mut h = Harness::with_config(lang, autoreplace_config());
        h.desktop.lock().clipboard = Some("saved".into());
        h.type_as("снп ", Lang::Ru);
        assert_eq!(h.text(), "С наилучшими пожеланиями,\nИван ");
        assert_eq!(
            h.layout(),
            lang,
            "expansion does not change the active layout"
        );
        assert_eq!(h.desktop.lock().clipboard.as_deref(), Some("saved"));
        assert_eq!(h.events, vec![Event::Autoreplaced]);
        h.tap(PhysKey::Pause);
        assert_eq!(
            h.text(),
            "С наилучшими пожеланиями,\nИван ",
            "stale word was cleared"
        );
    }
}

#[test]
fn autoreplace_places_the_caret_inside_unicode_text() {
    let mut config = autoreplace_config();
    config.autoreplace.items[0].to = "Здравствуйте, !".into();
    config.autoreplace.items[0].cursor_pos = 14;
    let mut h = Harness::with_config(Lang::Ru, config);
    h.type_as("снп Иван", Lang::Ru);
    assert_eq!(h.text(), "Здравствуйте, Иван! ");
    assert_eq!(h.desktop.lock().clipboard, None);
}

#[test]
fn autoreplace_respects_options_and_reloads_its_table() {
    let mut config = autoreplace_config();
    config.general.autoswitch = false;
    config.autoreplace.also_in_other_layout = false;
    let mut h = Harness::with_config(Lang::En, config.clone());
    h.type_as("снп ", Lang::Ru);
    assert_eq!(h.text(), "cyg ");
    config.autoreplace.also_in_other_layout = true;
    config.autoreplace.items[0].to = "changed".into();
    h.processor.apply_config(config.clone());
    h.type_as("снп ", Lang::Ru);
    assert_eq!(
        h.text(),
        "cyg changed ",
        "works independently of autoswitch"
    );
    config.autoreplace.enabled = false;
    h.processor.apply_config(config);
    h.type_as("снп ", Lang::Ru);
    assert_eq!(h.text(), "cyg changed cyg ");
}

#[test]
fn autoreplace_does_not_run_for_other_triggers_or_separators() {
    use okbs_core::config::AutoReplaceTrigger;
    for trigger in [
        AutoReplaceTrigger::Tooltip,
        AutoReplaceTrigger::Enter,
        AutoReplaceTrigger::Tab,
        AutoReplaceTrigger::Hotkey,
    ] {
        let mut config = autoreplace_config();
        config.general.autoswitch = false;
        config.autoreplace.trigger = trigger;
        let mut h = Harness::with_config(Lang::Ru, config);
        h.type_as("снп ", Lang::Ru);
        assert_eq!(h.text(), "снп ");
        assert!(!h.events.contains(&Event::Autoreplaced));
    }
    let mut h = Harness::with_config(Lang::Ru, autoreplace_config());
    h.type_as("снп\n", Lang::Ru);
    assert_eq!(h.text(), "снп\n");
}

#[test]
fn autoreplace_respects_password_fields_own_windows_and_exclusions() {
    for case in 0..3 {
        let mut config = autoreplace_config();
        config
            .exclusions
            .titles
            .push(okbs_core::config::TitleExclusion {
                contains: "Excluded".into(),
            });
        let mut h = Harness::with_config(Lang::Ru, config);
        {
            let mut d = h.desktop.lock();
            match case {
                0 => d.password = true,
                1 => d.window_pid = std::process::id(),
                _ => d.window_title = Some("Excluded".into()),
            }
        }
        h.type_as("снп ", Lang::Ru);
        assert_eq!(h.text(), "снп ");
        assert_eq!(h.desktop.lock().injected, 0);
    }
}

#[test]
fn autoreplace_is_cancelled_by_navigation_backspace_and_layout_change() {
    let mut h = Harness::with_config(Lang::Ru, autoreplace_config());
    h.type_as("снп", Lang::Ru);
    h.tap(PhysKey::Backspace);
    h.type_as("п ", Lang::Ru);
    assert_eq!(h.text(), "снп ");
    h.type_as("снп", Lang::Ru);
    h.processor
        .handle_focus(&okbs_platform::FocusEvent::WindowChanged(None));
    h.tap(PhysKey::Space);
    assert!(!h.events.contains(&Event::Autoreplaced));
    let mut h = Harness::with_config(Lang::Ru, autoreplace_config());
    h.type_as("снп", Lang::Ru);
    h.desktop.lock().layout = Lang::En;
    h.tap(PhysKey::Space);
    assert_eq!(h.text(), "снп ");
}

#[test]
fn busy_clipboard_does_not_erase_an_abbreviation() {
    for read_error in [true, false] {
        let mut h = Harness::with_config(Lang::Ru, autoreplace_config());
        {
            let mut d = h.desktop.lock();
            d.fail_clipboard_read = read_error;
            d.fail_clipboard_write = !read_error;
        }
        h.type_as("снп ", Lang::Ru);
        assert_eq!(h.text(), "снп ");
        assert!(h.events.iter().any(|e| matches!(e, Event::Error(_))));
        assert_eq!(h.desktop.lock().injected, 0);
    }
}

#[test]
fn clipboard_cleanup_handles_empty_clipboard_and_input_failures() {
    for saved in [None, Some("saved".to_owned())] {
        let mut h = Harness::new(Lang::En);
        h.desktop.lock().clipboard = saved.clone();
        h.combo(PhysKey::ShiftLeft, PhysKey::Pause);
        assert_eq!(h.desktop.lock().clipboard, saved, "copy timeout");
        h.desktop.lock().fail_copy = true;
        h.combo(PhysKey::ShiftLeft, PhysKey::Pause);
        assert_eq!(h.desktop.lock().clipboard, saved, "copy error");
        h.desktop.lock().fail_copy = false;
        h.desktop.lock().fail_paste = true;
        h.desktop.lock().text = "ghbdtn".chars().collect();
        h.select_all();
        h.combo(PhysKey::ShiftLeft, PhysKey::Pause);
        assert_eq!(h.desktop.lock().clipboard, saved, "paste error");
        assert_eq!(h.text(), "ghbdtn");
    }
}

#[test]
fn clipboard_restoration_keeps_a_newer_copy() {
    let mut h = Harness::with_config(Lang::Ru, autoreplace_config());
    h.desktop.lock().clipboard = Some("old".into());
    h.desktop.lock().copy_during_paste = Some("new copy".into());
    h.type_as("снп ", Lang::Ru);
    assert_eq!(h.desktop.lock().clipboard.as_deref(), Some("new copy"));
    assert!(h.events.contains(&Event::Autoreplaced));
}

#[test]
fn plain_paste_does_not_inject_when_clipboard_has_no_text() {
    let mut h = Harness::new(Lang::En);
    h.key(PhysKey::ControlLeft, true);
    h.key(PhysKey::AltLeft, true);
    h.tap(PhysKey::KeyV);
    h.key(PhysKey::AltLeft, false);
    h.key(PhysKey::ControlLeft, false);
    assert_eq!(h.desktop.lock().injected, 0);
}

#[test]
fn failed_layout_switch_does_not_erase_text() {
    let mut h = Harness::new(Lang::En);
    h.desktop.lock().fail_layout = true;
    h.type_as("ghbdtn ", Lang::En);
    assert_eq!(h.text(), "ghbdtn ");
    assert_eq!(h.desktop.lock().injected, 0);
    assert!(h.events.iter().any(|e| matches!(e, Event::Error(_))));
}

#[test]
fn double_space_does_not_modify_passwords_or_excluded_apps() {
    for password in [true, false] {
        let mut config = config_with(|c| c.advanced.double_space_comma = true);
        config
            .exclusions
            .titles
            .push(okbs_core::config::TitleExclusion {
                contains: "Excluded".into(),
            });
        let mut h = Harness::with_config(Lang::En, config);
        if password {
            h.desktop.lock().password = true;
        } else {
            h.desktop.lock().window_title = Some("Excluded".into());
        }
        h.type_as("hello  ", Lang::En);
        assert_eq!(h.text(), "hello  ");
        assert_eq!(h.desktop.lock().injected, 0);
    }
}

#[test]
fn spelling_requests_snapshot_options_and_preserve_newer_clipboard() {
    let mut h = Harness::new(Lang::En);
    h.desktop.lock().clipboard = Some("wrold".into());
    assert!(matches!(h.processor.spellcheck_clipboard().as_slice(),
        [Event::CheckSpelling { text, settings, interactive: false, target: None }] if text == "wrold" && settings.languages == Lang::ALL));
    h.processor.correct_clipboard("wrold", "world");
    assert_eq!(h.desktop.lock().clipboard.as_deref(), Some("world"));
    h.desktop.lock().clipboard = Some("newer".into());
    h.processor.correct_clipboard("wrold", "world");
    assert_eq!(h.desktop.lock().clipboard.as_deref(), Some("newer"));
    let mut config = h.processor.config().clone();
    config.spellcheck.check_on_command = false;
    h.processor.apply_config(config);
    assert!(h.processor.spellcheck_clipboard().is_empty());
}

#[test]
fn tray_layout_choice_applies_to_the_editor_typed_in() {
    let mut h = Harness::new(Lang::Ru);
    h.type_as("да ", Lang::Ru);
    let editor = h.desktop.lock().window_pid;
    // The tray menu is in front while the item is chosen.
    h.desktop.lock().window_pid = std::process::id();
    let events = h.processor.select_layout(id(Lang::En));
    assert_eq!(h.desktop.lock().window_pid, editor);
    assert_eq!(h.layout(), Lang::En);
    assert!(
        events.iter().any(|event| matches!(event,
            Event::LayoutChanged(Some(layout)) if layout.lang == Some(Lang::En))),
        "{events:?}"
    );
}

#[test]
fn spelling_hotkey_checks_the_selection_before_the_clipboard() {
    for prefer_selection in [true, false] {
        let mut h = Harness::with_config(
            Lang::En,
            config_with(|c| {
                c.spellcheck.prefer_selection = prefer_selection;
                c.hotkeys.spellcheck_clipboard =
                    okbs_core::config::HotkeyBinding::some("Ctrl+F11".parse().unwrap());
            }),
        );
        h.desktop.lock().text = "wrold".chars().collect();
        h.desktop.lock().clipboard = Some("clipbaord".into());
        h.select_all();
        h.combo(PhysKey::ControlLeft, PhysKey::F11);
        let checked: Vec<_> = h
            .events
            .iter()
            .filter_map(|event| match event {
                Event::CheckSpelling { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        let expected = if prefer_selection {
            "wrold"
        } else {
            "clipbaord"
        };
        assert_eq!(checked, [expected]);
        assert_eq!(h.desktop.lock().clipboard.as_deref(), Some("clipbaord"));
    }
}

#[test]
fn typed_spelling_uses_final_text_and_respects_enabled_modes() {
    for gate in [false, true] {
        for autoswitch in [false, true] {
            for enabled in [false, true] {
                let mut config = Config::default();
                config.general.autoswitch = autoswitch;
                config.spellcheck.check_typed_words = enabled;
                let mut h = Harness::with_config(Lang::Ru, config);
                if gate {
                    h.enable_gate();
                }
                h.type_as("хочеш ", Lang::Ru);
                assert_eq!(h.text(), "хочеш ");
                let requests: Vec<_> = h
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        Event::CheckSpelling {
                            text,
                            interactive: true,
                            ..
                        } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(requests, if enabled { vec!["хочеш"] } else { vec![] });
            }
        }
        let mut config = Config::default();
        config.spellcheck.check_typed_words = true;
        let mut h = Harness::with_config(Lang::En, config);
        if gate {
            h.enable_gate();
        }
        h.type_as("привет ", Lang::Ru);
        assert_eq!(h.text(), "привет ");
        assert!(h.events.iter().any(|event| matches!(event,
            Event::CheckSpelling { text, .. } if text == "привет")));

        // A word ended by Enter or punctuation can never be replaced later.
        let mut config = Config::default();
        config.spellcheck.check_typed_words = true;
        let mut h = Harness::with_config(Lang::Ru, config);
        if gate {
            h.enable_gate();
        }
        h.type_as("хочеш\nхочеш, ", Lang::Ru);
        assert_eq!(h.text(), "хочеш\nхочеш, ");
        assert!(
            !h.events
                .iter()
                .any(|event| matches!(event, Event::CheckSpelling { .. })),
            "{:?}",
            h.events
        );
    }
}

#[test]
fn auto_spelling_replaces_only_the_current_space_terminated_word() {
    let mut h = Harness::with_config(
        Lang::Ru,
        config_with(|config| config.spellcheck.check_typed_words = true),
    );
    h.type_as("хочеш ", Lang::Ru);
    let target = h
        .events
        .iter()
        .find_map(|event| match event {
            Event::CheckSpelling {
                text,
                target: Some(target),
                ..
            } if text == "хочеш" => Some(*target),
            _ => None,
        })
        .expect("finished word has an input target");
    let events = h
        .processor
        .correct_typed_spelling(target, "хочеш", "хочешь");
    assert!(matches!(events.as_slice(), [Event::SpellingCorrected]));
    assert_eq!(h.text(), "хочешь ");

    h.type_as("ещё", Lang::Ru);
    assert!(
        h.processor
            .correct_typed_spelling(target, "хочеш", "хочешь")
            .is_empty()
    );
    assert_eq!(h.text(), "хочешь ещё");
}

#[test]
fn spelling_choice_restores_its_editor_after_the_popup_was_used() {
    for gated in [false, true] {
        let mut h = Harness::with_config(
            Lang::Ru,
            config_with(|config| config.spellcheck.check_typed_words = true),
        );
        if gated {
            h.enable_gate();
        }
        h.type_as("хочеш ", Lang::Ru);
        let target = h
            .events
            .iter()
            .find_map(|event| match event {
                Event::CheckSpelling {
                    text,
                    target: Some(target),
                    ..
                } if text == "хочеш" => Some(*target),
                _ => None,
            })
            .expect("finished word has an input target");

        // Windows delivers the hook event before activating the clicked popup.
        h.processor.handle_input(InputEvent::MouseButton {
            time: Instant::now(),
            in_own_window: true,
        });
        h.desktop.lock().window_pid = std::process::id();
        h.processor
            .handle_focus(&okbs_platform::FocusEvent::WindowChanged(Some(
                WindowInfo {
                    pid: Some(std::process::id()),
                    executable: None,
                    title: Some("Possible spelling error".into()),
                    app_id: None,
                },
            )));
        h.processor.handle_input(InputEvent::MouseButton {
            time: Instant::now(),
            in_own_window: true,
        });

        // Hiding the popup returns foreground focus before the controller has
        // forwarded its replacement command to the engine.
        h.desktop.lock().window_pid = target.window as u32;
        h.processor
            .handle_focus(&okbs_platform::FocusEvent::WindowChanged(Some(
                WindowInfo {
                    pid: Some(target.window as u32),
                    executable: None,
                    title: Some("Document editor".into()),
                    app_id: None,
                },
            )));

        let events = h
            .processor
            .correct_typed_spelling(target, "хочеш", "хочешь");
        assert!(matches!(events.as_slice(), [Event::SpellingCorrected]));
        assert_eq!(h.text(), "хочешь ");
        assert_eq!(h.desktop.lock().window_pid, target.window as u32);
    }
}

#[test]
fn spelling_choice_rejects_edits_after_the_checked_word() {
    for gated in [false, true] {
        for action in [
            PhysKey::KeyA,
            PhysKey::Space,
            PhysKey::Enter,
            PhysKey::ArrowLeft,
        ] {
            let mut h = Harness::with_config(
                Lang::Ru,
                config_with(|config| config.spellcheck.check_typed_words = true),
            );
            if gated {
                h.enable_gate();
            }
            h.type_as("хочеш ", Lang::Ru);
            let target = h
                .events
                .iter()
                .find_map(|event| match event {
                    Event::CheckSpelling { target, .. } => *target,
                    _ => None,
                })
                .expect("spelling target");
            h.tap(action);
            let before = h.text();
            let injected = h.desktop.lock().injected;
            assert!(
                h.processor
                    .correct_typed_spelling(target, "хочеш", "хочешь")
                    .is_empty()
            );
            assert_eq!(h.text(), before);
            assert_eq!(h.desktop.lock().injected, injected);
        }
    }
}

#[test]
fn spelling_click_back_into_editor_invalidates_before_activation() {
    let mut h = Harness::with_config(
        Lang::Ru,
        config_with(|config| config.spellcheck.check_typed_words = true),
    );
    h.enable_gate();
    h.type_as("хочеш ", Lang::Ru);
    let target = h
        .events
        .iter()
        .find_map(|event| match event {
            Event::CheckSpelling { target, .. } => *target,
            _ => None,
        })
        .expect("spelling target");
    h.desktop.lock().window_pid = std::process::id();
    h.processor.handle_input(InputEvent::MouseButton {
        time: Instant::now(),
        in_own_window: false,
    });
    let events = crate::handle_command(
        &mut h.processor,
        crate::Command::ReplaceTypedSpelling {
            request_id: 7,
            target,
            original: "хочеш".into(),
            corrected: "хочешь".into(),
        },
    );
    assert_eq!(
        events,
        vec![Event::SpellingReplacementFinished {
            request_id: 7,
            success: false
        }]
    );
    assert_eq!(h.text(), "хочеш ");
    assert_eq!(h.desktop.lock().window_pid, std::process::id());
}

#[test]
fn explicit_spelling_acknowledges_after_restoring_editor_and_pasting() {
    let mut h = Harness::with_config(
        Lang::Ru,
        config_with(|config| config.spellcheck.check_typed_words = true),
    );
    h.enable_gate();
    h.type_as("хочеш ", Lang::Ru);
    let target = h
        .events
        .iter()
        .find_map(|event| match event {
            Event::CheckSpelling { target, .. } => *target,
            _ => None,
        })
        .expect("spelling target");
    h.processor.handle_input(InputEvent::MouseButton {
        time: Instant::now(),
        in_own_window: true,
    });
    h.desktop.lock().window_pid = std::process::id();
    let events = crate::handle_command(
        &mut h.processor,
        crate::Command::ReplaceTypedSpelling {
            request_id: 9,
            target,
            original: "хочеш".into(),
            corrected: "хочешь".into(),
        },
    );
    assert_eq!(
        events,
        vec![
            Event::SpellingCorrected,
            Event::SpellingReplacementFinished {
                request_id: 9,
                success: true
            }
        ]
    );
    assert_eq!(h.text(), "хочешь ");
    assert_eq!(h.desktop.lock().window_pid, target.window as u32);
}

fn add_selection_config() -> Config {
    config_with(|c| {
        c.hotkeys.add_selection_to_autoreplace =
            okbs_core::config::HotkeyBinding::some("Ctrl+F12".parse().unwrap());
    })
}

#[test]
fn selection_to_autoreplace_waits_for_modifiers_and_restores_clipboard() {
    let mut h = Harness::with_config(Lang::Ru, add_selection_config());
    let selected = "С наилучшими пожеланиями,\nИван 👋";
    h.desktop.lock().text = selected.chars().collect();
    h.desktop.lock().clipboard = Some("previous clipboard".into());
    h.select_all();
    h.key(PhysKey::ControlLeft, true);
    h.tap(PhysKey::F12);
    assert!(
        h.events.is_empty(),
        "copy must wait for Ctrl to be released"
    );
    h.key(PhysKey::ControlLeft, false);
    assert_eq!(h.events, vec![Event::AutoreplaceSelection(selected.into())]);
    assert_eq!(h.text(), selected);
    assert!(h.desktop.lock().selection.is_some());
    assert_eq!(
        h.desktop.lock().clipboard.as_deref(),
        Some("previous clipboard")
    );
    assert!(
        h.processor.config().autoreplace.items.is_empty(),
        "only the UI may save the entry"
    );
}

#[test]
fn selection_to_autoreplace_leaves_no_draft_on_empty_selection_or_copy_error() {
    for saved in [None, Some("saved".to_owned())] {
        for fail in [false, true] {
            let mut h = Harness::with_config(Lang::En, add_selection_config());
            h.desktop.lock().clipboard = saved.clone();
            h.desktop.lock().fail_copy = fail;
            h.combo(PhysKey::ControlLeft, PhysKey::F12);
            assert!(
                !h.events
                    .iter()
                    .any(|event| matches!(event, Event::AutoreplaceSelection(_)))
            );
            assert_eq!(h.desktop.lock().clipboard, saved);
        }
    }
}

#[test]
fn selection_to_autoreplace_respects_passwords_and_complete_exclusions() {
    for password in [true, false] {
        let mut config = add_selection_config();
        config.troubleshooting.ignore_excluded_apps_completely = true;
        config
            .exclusions
            .titles
            .push(okbs_core::config::TitleExclusion {
                contains: "Excluded".into(),
            });
        let mut h = Harness::with_config(Lang::En, config);
        h.desktop.lock().text = "selected".chars().collect();
        h.select_all();
        if password {
            h.desktop.lock().password = true;
        } else {
            h.desktop.lock().window_title = Some("Excluded".into());
        }
        h.combo(PhysKey::ControlLeft, PhysKey::F12);
        assert!(h.events.is_empty());
        assert_eq!(h.desktop.lock().injected, 0);
    }
}

fn gated_autoreplace(trigger: okbs_core::config::AutoReplaceTrigger, lang: Lang) -> Harness {
    let mut config = autoreplace_config();
    config.general.autoswitch = false;
    config.autoreplace.trigger = trigger;
    config.hotkeys.show_autoreplace_menu =
        okbs_core::config::HotkeyBinding::some("Ctrl+F12".parse().unwrap());
    let mut h = Harness::with_config(lang, config);
    h.enable_gate();
    h
}

#[test]
fn autoreplace_intercepts_confirmations_before_the_application() {
    use okbs_core::config::AutoReplaceTrigger::*;
    for (trigger, key) in [
        (Space, PhysKey::Space),
        (Enter, PhysKey::Enter),
        (Enter, PhysKey::NumpadEnter),
        (Tab, PhysKey::Tab),
        (Tooltip, PhysKey::Enter),
        (Tooltip, PhysKey::Tab),
    ] {
        for lang in Lang::ALL {
            let mut h = gated_autoreplace(trigger, lang);
            h.desktop.lock().clipboard = Some("saved".into());
            h.type_as("снп", Lang::Ru);
            h.tap(key);
            assert_eq!(
                h.text(),
                format!(
                    "С наилучшими пожеланиями,\nИван{}",
                    if key == PhysKey::Space { " " } else { "" }
                )
            );
            assert!(h.events.contains(&Event::Autoreplaced));
            assert_eq!(
                h.desktop.lock().enter_count,
                0,
                "must not send a message or newline before replacement"
            );
            assert_eq!(
                h.desktop.lock().tab_count,
                0,
                "must not move input focus before replacement"
            );
            assert_eq!(h.desktop.lock().clipboard.as_deref(), Some("saved"));
            assert_eq!(h.layout(), lang);
        }
    }
}

#[test]
fn tooltip_shows_hides_and_escape_dismisses_without_expanding() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Tooltip, Lang::En);
    h.type_as("снп", Lang::Ru);
    assert_eq!(h.events, vec![Event::AutoreplaceHint(Some(0))]);
    h.tap(PhysKey::Escape);
    assert_eq!(h.events.last(), Some(&Event::AutoreplaceHint(None)));
    h.tap(PhysKey::Enter);
    assert_eq!(h.text(), "cyg\n");
    assert_eq!(h.desktop.lock().enter_count, 1);
    assert!(!h.events.contains(&Event::Autoreplaced));
}

#[test]
fn unrecognized_words_and_other_separators_keep_their_original_behavior() {
    use okbs_core::config::AutoReplaceTrigger::*;
    for trigger in [Enter, Tab, Tooltip, Hotkey] {
        let mut h = gated_autoreplace(trigger, Lang::En);
        h.type_as("unknown\n", Lang::En);
        assert_eq!(h.text(), "unknown\n");
        assert_eq!(h.desktop.lock().enter_count, 1);
        h.type_as("снп ", Lang::Ru);
        assert_eq!(h.text(), "unknown\ncyg ");
        assert!(!h.events.contains(&Event::Autoreplaced));
    }
}

#[test]
fn autoreplace_hotkey_waits_for_release_and_falls_back_to_the_menu() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Hotkey, Lang::En);
    h.type_as("снп", Lang::Ru);
    h.key(PhysKey::ControlLeft, true);
    h.tap(PhysKey::F12);
    assert!(!h.events.contains(&Event::Autoreplaced));
    h.key(PhysKey::ControlLeft, false);
    assert_eq!(h.text(), "С наилучшими пожеланиями,\nИван");
    assert_eq!(
        h.desktop.lock().f12_count,
        0,
        "the application must not execute the assigned shortcut"
    );
    assert!(h.events.contains(&Event::Autoreplaced));
    h.combo(PhysKey::ControlLeft, PhysKey::F12);
    assert!(matches!(
        h.events.last(),
        Some(Event::AutoreplaceList {
            toggle: false,
            target: Some(_)
        })
    ));
    assert!(!h.desktop.lock().ctrl);
}

#[test]
fn queued_typing_is_replayed_in_order_and_can_expand_another_abbreviation() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Enter, Lang::En);
    h.type_as("снп", Lang::Ru);
    let mut pending = Vec::new();
    // Simulate typing while the engine is still checking focus/clipboard.
    for key in [
        PhysKey::Enter,
        PhysKey::Space,
        PhysKey::KeyC,
        PhysKey::KeyY,
        PhysKey::KeyG,
        PhysKey::Enter,
        PhysKey::KeyA,
    ] {
        for pressed in [true, false] {
            let event = h.source_event(key, pressed, false, Instant::now());
            assert!(matches!(event, InputEvent::CapturedKey { .. }));
            pending.push(event);
        }
    }
    assert_eq!(
        h.text(),
        "cyg",
        "no queued text may reach the application early"
    );
    for event in pending {
        h.events.extend(h.processor.handle_input(event));
    }
    assert_eq!(
        h.text(),
        "С наилучшими пожеланиями,\nИван С наилучшими пожеланиями,\nИванa"
    );
    assert_eq!(
        h.events
            .iter()
            .filter(|e| **e == Event::Autoreplaced)
            .count(),
        2
    );
    assert_eq!(h.desktop.lock().enter_count, 0);
    let event = h.source_event(PhysKey::KeyB, true, false, Instant::now());
    assert!(
        matches!(event, InputEvent::Key { .. }),
        "gate must be released after the queue drains"
    );
}

#[test]
fn confirmation_repeat_is_swallowed_until_release() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Enter, Lang::En);
    h.type_as("снп", Lang::Ru);
    h.key(PhysKey::Enter, true);
    for _ in 0..3 {
        let event = h.source_event(PhysKey::Enter, true, true, Instant::now());
        h.events.extend(h.processor.handle_input(event));
    }
    h.key(PhysKey::Enter, false);
    assert_eq!(h.desktop.lock().enter_count, 0);
    assert_eq!(
        h.events
            .iter()
            .filter(|e| **e == Event::Autoreplaced)
            .count(),
        1
    );
    h.tap(PhysKey::Enter);
    assert_eq!(
        h.desktop.lock().enter_count,
        1,
        "a new Enter press works normally"
    );
}

#[test]
fn delayed_confirmation_does_not_type_into_a_new_focus_target() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Enter, Lang::En);
    h.type_as("снп", Lang::Ru);
    let event = h.source_event(PhysKey::Enter, true, false, Instant::now());
    {
        let mut d = h.desktop.lock();
        d.window_pid = 2;
        d.text = "other application".chars().collect();
    }
    h.events.extend(h.processor.handle_input(event));
    assert_eq!(h.text(), "other application");
    assert_eq!(h.desktop.lock().enter_count, 0);
    assert!(!h.events.contains(&Event::Autoreplaced));
}

#[test]
fn guarded_confirmation_is_replayed_in_password_fields_and_exclusions() {
    for password in [true, false] {
        let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Tooltip, Lang::En);
        let mut config = h.processor.config().clone();
        config
            .exclusions
            .titles
            .push(okbs_core::config::TitleExclusion {
                contains: "Excluded".into(),
            });
        h.processor.apply_config(config);
        if password {
            h.desktop.lock().password = true;
        } else {
            h.desktop.lock().window_title = Some("Excluded".into());
        }
        h.type_as("снп", Lang::Ru);
        assert!(
            !h.events
                .iter()
                .any(|e| matches!(e, Event::AutoreplaceHint(Some(_))))
        );
        h.tap(PhysKey::Enter);
        assert_eq!(h.text(), "cyg\n");
        assert_eq!(h.desktop.lock().enter_count, 1);
        assert!(!h.events.contains(&Event::Autoreplaced));
    }
}

#[test]
fn complete_exclusion_keeps_the_autoreplace_hotkey_in_the_original_application() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Hotkey, Lang::En);
    let mut config = h.processor.config().clone();
    config.troubleshooting.ignore_excluded_apps_completely = true;
    config
        .exclusions
        .titles
        .push(okbs_core::config::TitleExclusion {
            contains: "Excluded".into(),
        });
    h.processor.apply_config(config);
    h.desktop.lock().window_title = Some("Excluded".into());
    h.type_as("снп", Lang::Ru);
    h.combo(PhysKey::ControlLeft, PhysKey::F12);
    assert_eq!(h.text(), "cyg");
    assert_eq!(h.desktop.lock().f12_count, 1);
    assert!(
        !h.events
            .iter()
            .any(|e| matches!(e, Event::AutoreplaceList { .. }))
    );
}

#[test]
fn busy_clipboard_does_not_forward_a_confirmation_or_erase_the_abbreviation() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Enter, Lang::En);
    h.desktop.lock().fail_clipboard_write = true;
    h.type_as("снп", Lang::Ru);
    h.tap(PhysKey::Enter);
    assert_eq!(h.text(), "cyg");
    assert_eq!(h.desktop.lock().enter_count, 0);
    assert!(h.events.iter().any(|e| matches!(e, Event::Error(_))));
}

#[test]
fn list_insertion_restores_the_editor_and_preserves_caret_and_clipboard() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Tooltip, Lang::En);
    let mut config = h.processor.config().clone();
    config.autoreplace.items[0].to = "<b></b>".into();
    config.autoreplace.items[0].cursor_pos = 3;
    let item = config.autoreplace.items[0].clone();
    h.processor.apply_config(config);
    h.type_as("hello ", Lang::En);
    h.desktop.lock().clipboard = Some("saved".into());
    h.desktop.lock().window_pid = std::process::id(); // Activated picker.
    h.events.extend(h.processor.insert_autoreplace(item, None));
    h.type_as("text", Lang::En);
    assert_eq!(h.text(), "hello <b>text</b>");
    assert_eq!(h.desktop.lock().clipboard.as_deref(), Some("saved"));
    assert!(h.events.contains(&Event::Autoreplaced));
}

#[test]
fn stale_or_disabled_list_entries_are_not_inserted() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Tooltip, Lang::En);
    h.type_as("hello", Lang::En);
    let original = h.processor.config().autoreplace.items[0].clone();
    let mut changed = original.clone();
    changed.to = "stale".into();
    assert!(h.processor.insert_autoreplace(changed, None).is_empty());
    let mut config = h.processor.config().clone();
    config.autoreplace.enabled = false;
    h.processor.apply_config(config);
    assert!(h.processor.insert_autoreplace(original, None).is_empty());
    assert_eq!(h.text(), "hello");
    assert_eq!(h.desktop.lock().injected, 0);
}

#[test]
fn a_click_in_the_same_control_invalidates_delayed_replacement() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Enter, Lang::En);
    h.type_as("снп", Lang::Ru);
    let confirm = h.source_event(PhysKey::Enter, true, false, Instant::now());
    let click = InputEvent::MouseButton {
        time: Instant::now(),
        in_own_window: false,
    };
    h.gate.as_ref().unwrap().capture(click, None, None);
    h.desktop.lock().caret = Some(0); // Same HWND, different insertion point.
    h.events.extend(h.processor.handle_input(confirm));
    h.processor.handle_input(click);
    assert_eq!(h.text(), "cyg");
    assert_eq!(h.desktop.lock().injected, 0);
    assert!(!h.events.contains(&Event::Autoreplaced));
}

#[test]
fn burst_input_can_be_intercepted_before_the_engine_has_read_the_word() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Tab, Lang::En);
    let mut pending = Vec::new();
    for key in [
        PhysKey::ArrowRight,
        PhysKey::KeyC,
        PhysKey::KeyY,
        PhysKey::KeyG,
        PhysKey::Tab,
    ] {
        for pressed in [true, false] {
            pending.push(h.source_event(key, pressed, false, Instant::now()));
        }
    }
    assert_eq!(h.text(), "cyg");
    assert_eq!(h.desktop.lock().tab_count, 0);
    for event in pending {
        h.events.extend(h.processor.handle_input(event));
    }
    assert_eq!(h.text(), "С наилучшими пожеланиями,\nИван");
    assert!(h.events.contains(&Event::Autoreplaced));
}

#[test]
fn capturing_a_hotkey_does_not_leave_autoreplace_interception_disabled() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Enter, Lang::En);
    h.processor.set_capture(true);
    h.combo(PhysKey::ControlLeft, PhysKey::F12);
    assert!(
        h.events
            .iter()
            .any(|event| matches!(event, Event::HotkeyCaptured(_)))
    );
    h.type_as("снп", Lang::Ru);
    h.tap(PhysKey::Enter);
    assert!(h.events.contains(&Event::Autoreplaced));
    assert_eq!(h.desktop.lock().enter_count, 0);
}

#[test]
fn backspace_option_and_master_switch_apply_to_intercepted_confirmations() {
    for block in [true, false] {
        let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Enter, Lang::En);
        let mut config = h.processor.config().clone();
        config.troubleshooting.no_switch_after.backspace = block;
        h.processor.apply_config(config);
        h.type_as("cygg", Lang::En);
        h.tap(PhysKey::Backspace);
        h.tap(PhysKey::Enter);
        assert_eq!(h.events.contains(&Event::Autoreplaced), !block);
        assert_eq!(h.desktop.lock().enter_count, usize::from(block));
    }
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Enter, Lang::En);
    let mut config = h.processor.config().clone();
    config.autoreplace.enabled = false;
    h.processor.apply_config(config);
    h.type_as("снп\n", Lang::Ru);
    assert_eq!(h.text(), "cyg\n");
    assert!(!h.events.contains(&Event::Autoreplaced));
}

#[test]
fn switching_autodetection_off_does_not_break_a_pending_autoreplace_hint() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Tooltip, Lang::En);
    h.type_as("снп", Lang::Ru);
    h.processor.set_autoswitch(false);
    h.tap(PhysKey::Tab);
    assert!(h.events.contains(&Event::Autoreplaced));
    assert_eq!(h.desktop.lock().tab_count, 0);
}

#[test]
fn picker_confirmation_does_not_repeat_in_the_restored_editor() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Tooltip, Lang::En);
    h.gate
        .as_ref()
        .unwrap()
        .suppress_until_release(PhysKey::Enter);
    for _ in 0..3 {
        let event = h.source_event(PhysKey::NumpadEnter, true, true, Instant::now());
        h.processor.handle_input(event);
    }
    h.key(PhysKey::NumpadEnter, false);
    assert_eq!(h.desktop.lock().enter_count, 0);
    h.tap(PhysKey::Enter);
    assert_eq!(h.desktop.lock().enter_count, 1);
}

#[test]
fn explicit_autoreplace_hotkey_works_after_editing_and_in_partial_exclusions() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Hotkey, Lang::En);
    let mut config = h.processor.config().clone();
    config
        .exclusions
        .titles
        .push(okbs_core::config::TitleExclusion {
            contains: "Editor".into(),
        });
    h.processor.apply_config(config);
    h.desktop.lock().window_title = Some("Editor".into());
    h.type_as("cygg", Lang::En);
    h.tap(PhysKey::Backspace);
    h.combo(PhysKey::ControlLeft, PhysKey::F12);
    assert_eq!(h.text(), "С наилучшими пожеланиями,\nИван");
    assert!(h.events.contains(&Event::Autoreplaced));
}

#[test]
fn tray_insertion_uses_the_editor_at_the_click_even_before_any_typing() {
    let mut h = gated_autoreplace(okbs_core::config::AutoReplaceTrigger::Tooltip, Lang::En);
    let item = h.processor.config().autoreplace.items[0].clone();
    h.gate.as_ref().unwrap().capture(
        InputEvent::MouseButton {
            time: Instant::now(),
            in_own_window: false,
        },
        None,
        Some(InputTarget {
            window: 1,
            control: 1,
        }),
    );
    h.desktop.lock().window_pid = std::process::id(); // Tray's menu now owns focus.
    h.events.extend(h.processor.insert_autoreplace(item, None));
    assert_eq!(h.text(), "С наилучшими пожеланиями,\nИван");
    assert!(h.events.contains(&Event::Autoreplaced));
}

/// Clipboard marker of the processor, repeated here so a change of the private
/// constant is noticed by a failing test instead of silently reaching the list.
const MARKER: &str = "\u{2063}okbswitch\u{2063}";

fn clipboard_texts(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ClipboardText(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn clipboard_history_reports_new_texts_once_and_skips_our_own() {
    let mut h = Harness::new(Lang::En);
    for (contents, expected) in [
        (Some("first".to_owned()), vec!["first".to_owned()]),
        // The same notification twice, e.g. a program setting several formats.
        (Some("first".to_owned()), Vec::new()),
        (Some("second".to_owned()), vec!["second".to_owned()]),
        // Back to an older text: it is new again for the list in front.
        (Some("first".to_owned()), vec!["first".to_owned()]),
        (Some(MARKER.to_owned()), Vec::new()),
        (Some(String::new()), Vec::new()),
        (Some("   \n ".to_owned()), Vec::new()),
        (None, Vec::new()),
    ] {
        h.desktop.lock().clipboard = contents;
        let events = h.processor.handle_clipboard_change();
        assert_eq!(clipboard_texts(&events), expected);
    }
}

#[test]
fn clipboard_history_is_off_without_the_option_and_survives_read_errors() {
    let mut h = Harness::with_config(
        Lang::En,
        config_with(|c| c.advanced.clipboard_history = false),
    );
    h.desktop.lock().clipboard = Some("ignored".into());
    assert!(h.processor.handle_clipboard_change().is_empty());

    let mut h = Harness::new(Lang::En);
    h.desktop.lock().clipboard = Some("readable".into());
    h.desktop.lock().fail_clipboard_read = true;
    assert!(h.processor.handle_clipboard_change().is_empty());
    h.desktop.lock().fail_clipboard_read = false;
    assert_eq!(
        clipboard_texts(&h.processor.handle_clipboard_change()),
        ["readable"]
    );
}

/// Copying a selection puts a marker and the selected text on the clipboard for
/// a moment. Notifications are handled after that operation, when the previous
/// text is already back, so nothing of it reaches the history.
#[test]
fn a_copied_selection_does_not_enter_the_history() {
    let mut h = Harness::with_config(Lang::Ru, add_selection_config());
    h.desktop.lock().clipboard = Some("previous clipboard".into());
    // The text on the clipboard before the operation is the newest entry.
    assert_eq!(
        clipboard_texts(&h.processor.handle_clipboard_change()),
        ["previous clipboard"]
    );
    h.desktop.lock().text = "selected words".chars().collect();
    h.select_all();
    h.combo(PhysKey::ControlLeft, PhysKey::F12);
    assert_eq!(
        h.desktop.lock().clipboard.as_deref(),
        Some("previous clipboard")
    );
    // Three notifications arrived meanwhile: marker, copy, restore.
    for _ in 0..3 {
        assert!(clipboard_texts(&h.processor.handle_clipboard_change()).is_empty());
    }
}

#[test]
fn inserting_a_history_entry_pastes_it_and_restores_the_clipboard() {
    let mut h = Harness::new(Lang::En);
    h.desktop.lock().clipboard = Some("current clipboard".into());
    let events = h.processor.insert_text("older text", None);
    assert!(!events.iter().any(|e| matches!(e, Event::Error(_))));
    assert_eq!(h.text(), "older text");
    assert_eq!(
        h.desktop.lock().clipboard.as_deref(),
        Some("current clipboard")
    );
    assert!(h.processor.insert_text("", None).is_empty());
}

#[test]
fn a_history_entry_waits_for_modifiers_and_respects_passwords() {
    let mut config = config_with(|c| {
        c.hotkeys.show_clipboard_history =
            okbs_core::config::HotkeyBinding::some("Ctrl+F12".parse().unwrap());
    });
    config.troubleshooting.ignore_excluded_apps_completely = true;
    let mut h = Harness::with_config(Lang::En, config);
    h.key(PhysKey::ControlLeft, true);
    h.tap(PhysKey::F12);
    assert!(
        !h.events
            .iter()
            .any(|e| matches!(e, Event::ClipboardHistory { .. })),
        "the window waits for the hotkey to be released"
    );
    h.key(PhysKey::ControlLeft, false);
    assert!(
        h.events
            .iter()
            .any(|e| matches!(e, Event::ClipboardHistory { .. })),
        "releasing the hotkey opens the window"
    );
    h.events.clear();
    h.key(PhysKey::ControlLeft, true);
    h.events.extend(h.processor.insert_text("deferred", None));
    assert_eq!(h.text(), "", "the paste waits for Ctrl to be released");
    h.key(PhysKey::ControlLeft, false);
    assert_eq!(h.text(), "deferred");

    let mut h = Harness::new(Lang::En);
    h.desktop.lock().password = true;
    let events = h.processor.insert_text("secret", None);
    assert_eq!(h.text(), "");
    assert!(events.iter().any(|e| matches!(e, Event::Error(_))));
}

fn menu_config() -> Config {
    config_with(|c| c.advanced.fix_layout_in_menus = true)
}

#[test]
fn alt_matches_the_layout_to_the_menu_and_puts_it_back() {
    let mut h = Harness::with_config(Lang::Ru, menu_config());
    h.desktop.lock().menu_language = Some(Lang::En);
    h.key(PhysKey::AltLeft, true);
    assert_eq!(h.layout(), Lang::En, "Alt+Ф must reach «&File»");
    h.key(PhysKey::AltLeft, false);
    assert_eq!(h.layout(), Lang::En, "the menu is still open after Alt");
    h.processor
        .handle_focus(&okbs_platform::FocusEvent::MenuClosed);
    assert_eq!(h.layout(), Lang::Ru, "the typing layout comes back");
}

#[test]
fn a_russian_menu_or_a_matching_layout_changes_nothing() {
    for (menu, layout) in [
        (Some(Lang::Ru), Lang::Ru),
        (Some(Lang::En), Lang::En),
        (None, Lang::Ru),
    ] {
        let mut h = Harness::with_config(layout, menu_config());
        h.desktop.lock().menu_language = menu;
        h.key(PhysKey::AltLeft, true);
        assert_eq!(h.layout(), layout);
        h.key(PhysKey::AltLeft, false);
        h.processor
            .handle_focus(&okbs_platform::FocusEvent::MenuClosed);
        assert_eq!(h.layout(), layout);
    }
}

#[test]
fn the_menu_layout_is_left_alone_without_the_option_and_in_combinations() {
    // Switched off: Punto's option is off by default.
    let mut h = Harness::new(Lang::Ru);
    h.desktop.lock().menu_language = Some(Lang::En);
    h.key(PhysKey::AltLeft, true);
    assert_eq!(h.layout(), Lang::Ru);

    // Ctrl+Alt+… is an ordinary combination, not the menu bar.
    let mut h = Harness::with_config(Lang::Ru, menu_config());
    h.desktop.lock().menu_language = Some(Lang::En);
    h.key(PhysKey::ControlLeft, true);
    h.key(PhysKey::AltLeft, true);
    assert_eq!(h.layout(), Lang::Ru);
    h.key(PhysKey::AltLeft, false);
    h.key(PhysKey::ControlLeft, false);

    // Leaving the window restores the layout as well.
    let mut h = Harness::with_config(Lang::Ru, menu_config());
    h.desktop.lock().menu_language = Some(Lang::En);
    h.key(PhysKey::AltRight, true);
    assert_eq!(h.layout(), Lang::En);
    h.processor
        .handle_focus(&okbs_platform::FocusEvent::WindowChanged(None));
    assert_eq!(h.layout(), Lang::Ru);
}
