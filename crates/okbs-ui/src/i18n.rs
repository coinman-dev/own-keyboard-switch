//! Interface texts in Russian and English.
//!
//! Russian and English strings use familiar wording for each setting.

use okbs_core::Lang;
use okbs_core::config::HotkeyAction;

macro_rules! texts {
    ($( $id:ident => $ru:literal, $en:literal; )*) => {
        /// Identifier of a user-visible string.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Text {
            $( #[doc = $ru] $id, )*
        }

        impl Text {
            /// All texts.
            pub const ALL: &'static [Text] = &[$(Text::$id,)*];
        }

        /// The string for `text` in `lang`.
        pub const fn tr(text: Text, lang: Lang) -> &'static str {
            match lang {
                Lang::Ru => match text { $(Text::$id => $ru,)* },
                Lang::En => match text { $(Text::$id => $en,)* },
            }
        }
    };
}

texts! {
    // Application
    AppName => "Own Keyboard Switch", "Own Keyboard Switch";
    SettingsWindowTitle => "Настройки Own Keyboard Switch", "Own Keyboard Switch Settings";

    // Tray menu (right click)
    MenuSettings => "Настройки...", "Settings...";
    MenuAutoswitch => "Автопереключение", "Auto switch";
    MenuSounds => "Звуковые эффекты", "Sound effects";
    MenuClipboard => "Буфер обмена", "Clipboard";
    MenuClipboardConvertLayout => "Изменить раскладку", "Change layout";
    MenuClipboardTransliterate => "Транслитерировать", "Transliterate";
    MenuClipboardSpellcheck => "Проверить орфографию", "Check spelling";
    MenuClipboardHistory => "Посмотреть историю...", "View history...";
    MenuMore => "Дополнительно", "More";
    MenuAutoreplaceList => "Список автозамены", "Autoreplace list";
    MenuSystemKeyboardSettings => "Системные настройки клавиатуры", "System keyboard settings";
    MenuAbout => "О программе", "About";
    MenuExit => "Выйти", "Exit";

    // Settings sections
    SectionGeneral => "Общие", "General";
    SectionHotkeys => "Горячие клавиши", "Hotkeys";
    SectionRules => "Правила переключения", "Switching rules";
    SectionExclusions => "Программы-исключения", "Excluded programs";
    SectionTroubleshooting => "Устранение проблем", "Troubleshooting";
    SectionAutoreplace => "Автозамена", "Autoreplace";
    SectionSounds => "Звуки", "Sounds";
    SectionSpellcheck => "Проверка орфографии", "Spell checking";
    TabBasic => "Основные", "Basic";
    TabAdvanced => "Дополнительные", "Advanced";
    GroupLayoutSwitching => "Переключение раскладки", "Layout switching";

    // General → Basic
    OptAutostart => "Запускаться при старте", "Start with the system";
    OptAutoswitch => "Автопереключение", "Auto switch";
    OptImproveSwitching => "Улучшить переключение", "Improve layout switching";
    ImproveSwitchingHint => "Дополнительно распознавать короткие английские слова после русского слова или буквы.", "Recognise additional short English words after a Russian word or letter.";
    OptRunElevated => "Запускать с правами Администратора", "Run with administrator rights";
    OptFloatingIndicator => "Показывать плавающий индикатор", "Show floating indicator";
    OptFloatingIndicatorAutohide => "Скрывать плавающий индикатор после смены раскладки", "Hide floating indicator after layout change";
    OptTrayFlags => "Сделать значок в виде флагов стран", "Show country flags as the icon";
    OptTrayFlagsFullBrightness => "Всегда показывать флаги в полную яркость", "Always show flags at full brightness";
    OptHotkeysOffWhenAutoswitchOff => "Отключать горячие клавиши при отключении автопереключения", "Disable hotkeys when auto switch is off";
    OptLockIndicator => "Закрепить индикатор", "Lock indicator";

    // General → Advanced
    OptFixAbbreviations => "Исправлять аббревиатуры", "Correct abbreviations";
    OptFixTwoCapitals => "Исправлять две заглавные буквы в начале слова", "Correct TWo INitial CApitals";
    OptFixAccidentalCapsLock => "Исправлять случайное нажатие Caps Lock", "Correct accidental Caps Lock";
    OptDisableCapsLock => "Отключить кнопку Caps Lock", "Disable the Caps Lock key";
    OptScrollLockAsCapsLock => "Использовать Scroll Lock как Caps Lock", "Use Scroll Lock as Caps Lock";
    OptFixLayoutInMenus => "Исправлять раскладку при работе с меню, содержащим горячие клавиши", "Correct layout in menus with access keys";
    OptShowClipboardConversionWindow => "Показывать окно с результатами конвертации буфера", "Show clipboard conversion result window";
    OptSpellcheckEnabled => "Проверять выделение / буфер по команде", "Check selection / clipboard on command";
    OptSpellcheckTypedWords => "Проверять завершённые слова при наборе", "Check finished words while typing";
    SpellcheckTypedMode => "При наборе:", "While typing:";
    SpellcheckSuggestions => "Показывать варианты", "Show suggestions";
    SpellcheckAuto => "Автоисправление только однозначных ошибок", "Automatically fix only unambiguous errors";
    OptSpellcheckSelectionFirst => "По горячей клавише проверять выделение, затем буфер обмена", "On hotkey, check selection before clipboard";
    SpellcheckHint => "Проверка идёт локально. При наборе показываются варианты для завершённого слова; автоматически текст не меняется. Для проверки готового текста скопируйте его и выберите в трее «Буфер обмена → Проверить орфографию». Слова не отправляются в сеть.", "Checking is local. While typing, suggestions are shown for a finished word; text is not changed automatically. To check existing text, copy it and choose Clipboard → Check spelling in the tray. Words are never sent over the network.";
    SpellcheckLanguages => "Проверять языки:", "Languages to check:";
    SpellcheckExtraDictionaries => "Дополнительные словари", "Additional dictionaries";
    SpellcheckDownloadEnGb => "Скачать английский (Великобритания)", "Download English (United Kingdom)";
    SpellcheckBuiltinEn => "Встроенный английский (США)", "Built-in English (United States)";
    SpellcheckEnGb => "Английский (Великобритания)", "English (United Kingdom)";
    SpellcheckPersonalWords => "Мои слова", "My words";
    SpellcheckAddWord => "Добавить", "Add";
    DictionaryNotInstalled => "Словарь не установлен", "Dictionary is not installed";
    DictionaryDownloading => "Словарь скачивается…", "Downloading dictionary…";
    DictionaryInstalled => "Словарь установлен", "Dictionary is installed";
    DictionaryDownloadFailed => "Не удалось скачать словарь", "Dictionary download failed";
    OptClipboardHistory => "Следить за буфером обмена", "Watch the clipboard";
    OptClipboardHistoryPersist => "Сохранять историю буфера обмена после перезагрузки", "Keep clipboard history after restart";
    OptShowTooltips => "Показывать всплывающие подсказки", "Show tooltips";
    OptDoubleSpaceComma => "Запятая по двойному нажатию клавиши Пробел", "Comma on double Space";

    // General → Layout switching
    OptSwitchKey => "Переключать по:", "Switch with:";
    OptSwitchKeyOnlyPair => "Только русский/английский", "Russian/English only";
    OptSingleLayout => "Единая раскладка", "Same layout for all windows";
    OptDirectKeys => "Дополнительно переключать по:", "Also switch with:";
    DirectKeysShifts => "левому Shift - рус., правому Shift - англ.", "left Shift - Russian, right Shift - English";
    KeyRightCtrl => "правому Ctrl", "right Ctrl";
    KeyLeftCtrl => "левому Ctrl", "left Ctrl";
    KeyRightShift => "правому Shift", "right Shift";
    KeyLeftShift => "левому Shift", "left Shift";
    KeyCapsLock => "Caps Lock", "Caps Lock";
    KeyLeftAlt => "левому Alt", "left Alt";
    KeyRightAlt => "правому Alt", "right Alt";
    KeySpace => "пробелу", "Space";

    // Hotkeys
    HotkeysHint => "Выберите действие и назначьте для него комбинацию клавиш.", "Select an action and assign a key combination to it.";
    ColumnAction => "Действие", "Action";
    ColumnCombination => "Комбинация", "Combination";
    ActCancelOrConvertLastWord => "Отменить конвертацию раскладки", "Undo layout conversion";
    ActConvertSelectionLayout => "Сменить раскладку выделенного текста", "Change layout of selected text";
    ActInvertSelectionCase => "Сменить регистр выделенного текста", "Change case of selected text";
    ActTransliterateSelection => "Транслитерировать выделенный текст", "Transliterate selected text";
    ActPastePlain => "Вставка текста без форматирования", "Paste text without formatting";
    ActNumberToWords => "Преобразовать число в текст", "Convert number to words";
    ActToggleAutoswitch => "Включить/выключить автопереключение", "Turn auto switch on/off";
    ActToggleSounds => "Включить/выключить звуковые эффекты", "Turn sound effects on/off";
    ActOpenSettings => "Открыть настройки", "Open settings";
    ActToggleMaximizeWindow => "Развернуть/восстановить активное окно", "Maximize/restore active window";
    ActMinimizeWindow => "Свернуть активное окно", "Minimize active window";
    ActOpenAutoreplaceSettings => "Открыть настройки автозамены", "Open autoreplace settings";
    ActToggleAutoreplaceList => "Показать/скрыть список автозамены", "Show/hide autoreplace list";
    ActShowAutoreplaceMenu => "Показать меню вставки автозамены", "Show autoreplace insert menu";
    ActAddSelectionToAutoreplace => "Добавить выделенный текст в автозамену", "Add selected text to autoreplace";
    ActShowClipboardHistory => "Показать историю буфера обмена", "Show clipboard history";
    ActConvertClipboardLayout => "Сменить раскладку буфера обмена", "Change clipboard layout";
    ActTransliterateClipboard => "Транслитерировать текст в буфере обмена", "Transliterate clipboard text";
    ActSpellcheckClipboard => "Проверить орфографию буфера обмена", "Check clipboard spelling";

    // Switching rules
    RulesHint => "Правила определяют, в каких случаях переключать раскладку, а в каких нет.", "Rules define when the layout is switched and when it is not.";
    RuleMatchContains => "Содержать данное сочетание букв", "Contain these letters";
    RuleMatchStartsWith => "Начинаться с данных букв", "Start with these letters";
    RuleMatchEquals => "Совпадать с данным сочетанием", "Match exactly";
    RuleCaseSensitive => "Учитывать регистр", "Case sensitive";
    RuleActionSwitch => "Переводить в другую раскладку", "Convert to the other layout";
    RuleActionStay => "Не переводить в другую раскладку", "Do not convert";
    OptSuggestRuleAfterCancels => "Предлагать добавить правило после отмен подряд:", "Suggest a rule after consecutive undos:";

    // Excluded programs
    ExclusionsHint => "Программы, в которых отключается автопереключение раскладки.", "Programs where auto switch is disabled.";
    ExclusionsByExecutable => "По файлу приложения", "By application file";
    ExclusionsByTitle => "По заголовку окна", "By window title";
    ExclusionsByFolder => "По папке с программами", "By program folder";
    ColumnApplication => "Приложение", "Application";
    ColumnPath => "Путь", "Path";
    ExclusionsUnavailable => "На этом рабочем столе программа не может определить активное окно. Установите расширение GNOME Shell из настроек.", "The active window cannot be determined on this desktop. Install the GNOME Shell extension from the settings.";

    // Troubleshooting
    NoSwitchAfterHint => "Не переключать раскладку, если перед вводом были нажаты", "Do not switch the layout if these were pressed before typing";
    KeyBackspace => "Backspace", "Backspace";
    KeyArrowLeft => "Стрелка влево", "Left arrow";
    KeyArrowRight => "Стрелка вправо", "Right arrow";
    KeyArrowUp => "Стрелка вверх", "Up arrow";
    KeyArrowDown => "Стрелка вниз", "Down arrow";
    KeyDelete => "Delete", "Delete";
    NoSwitchAfterLayoutChange => "Вы сменили раскладку", "You changed the layout";
    OptOnlyPairLayouts => "Учитывать ввод только в русской и английской раскладках", "Only consider Russian and English layouts";
    OptNoSwitchOnTabEnter => "Не переключать раскладку по клавишам Tab и Enter", "Do not switch the layout on Tab and Enter";
    OptIgnoreExcludedApps => "Не взаимодействовать с программами-исключениями", "Do not interact with excluded programs";
    OptSensitivity => "Чувствительность анализа текста", "Text analysis sensitivity";

    // Autoreplace
    AutoreplaceHint => "Автозамена облегчает набор часто встречающихся фрагментов текста. Укажите сокращение в поле «Что заменять», и при наборе оно будет автоматически заменено на текст из поля «На что заменять».", "Autoreplace speeds up typing of frequent text. Enter an abbreviation in «Replace» and it will be replaced with the text from «With» while typing.";
    ColumnReplaceWhat => "Что заменять", "Replace";
    ColumnReplaceWith => "На что заменять", "With";
    OptRememberCursor => "Запоминать позицию курсора", "Remember cursor position";
    OptReplaceInOtherLayout => "Заменять при наборе в другой раскладке", "Replace when typed in the other layout";
    OptReplaceOn => "Заменять по:", "Replace on:";
    OptAutoreplaceEnabled => "Включить автозамену", "Enable autoreplace";
    AutoreplaceStageHint => "В режиме подсказки: Enter или Tab — заменить, Esc — отменить. Для замены по горячей клавише назначьте действие «Показать меню вставки автозамены».", "In tooltip mode: Enter or Tab accepts, Esc dismisses. For replacement by hotkey, assign the “Show autoreplace insert menu” action.";
    AutoreplaceHintHelp => "Enter / Tab — заменить; Esc — отменить", "Enter / Tab — replace; Esc — dismiss";
    AutoreplaceMenuHelp => "Enter или двойной щелчок — вставить; Esc — закрыть", "Enter or double-click — insert; Esc — close";
    AutoreplaceListHelp => "Двойной щелчок или кнопка «Вставить»", "Double-click an entry or click Insert";
    AutoreplaceEmpty => "Список пуст. Добавьте записи в настройках автозамены.", "The list is empty. Add entries in Autoreplace settings.";
    AutoreplaceDisabled => "Автозамена отключена в настройках.", "Autoreplace is disabled in settings.";
    BtnInsert => "Вставить", "Insert";
    BtnClose => "Закрыть", "Close";

    // Clipboard history window
    ClipboardHistoryTitle => "История буфера обмена", "Clipboard history";
    ClipboardHistoryEmpty => "История пуста. Скопируйте текст, чтобы он появился здесь.", "The history is empty. Copy some text and it appears here.";
    ClipboardHistoryHelp => "Enter или двойной щелчок — вставить; Esc — закрыть", "Enter or double-click — insert; Esc — close";
    ClipboardHistoryOff => "«Следить за буфером обмена» выключено в разделе «Общие → Дополнительные».", "“Watch the clipboard” is off under General → Advanced.";

    // Floating indicator
    IndicatorHide => "Скрыть индикатор", "Hide indicator";

    // Troubleshooting → diagnostics
    GroupDiagnostics => "Диагностика", "Diagnostics";
    OptLogDebug => "Подробный журнал (Debug)", "Detailed log (debug)";
    LogDebugHint => "Включайте, только чтобы приложить журнал к сообщению о проблеме. Журнал пишется в папку «log» рядом с программой; набранный текст и пароли в него не попадают.", "Turn this on only to attach the log to a problem report. The log goes to the “log” folder next to the program; typed text and passwords never reach it.";

    // Administrator rights
    ElevationTitle => "Права Администратора", "Administrator rights";
    ElevationRestart => "Чтобы программа работала в окнах с правами Администратора, её нужно перезапустить. Перезапустить сейчас?", "To work in windows running as Administrator the program has to restart. Restart now?";
    ElevationRestartNow => "Перезапустить", "Restart";
    ElevationLater => "Позже", "Later";
    ElevationFailed => "Не удалось перезапустить программу с правами Администратора.", "Could not restart the program with administrator rights.";
    ElevationHint => "Без этого права набор в окнах, запущенных от имени Администратора, не обрабатывается.", "Without this, typing in windows running as Administrator is not processed.";
    OptShowInTrayMenu => "Показывать список в меню", "Show the list in the menu";
    TriggerTooltip => "подсказке", "tooltip";
    TriggerSpace => "Пробелу", "Space";
    TriggerEnter => "Enter", "Enter";
    TriggerTab => "Tab", "Tab";
    TriggerHotkey => "горячей клавише", "hotkey";

    // Sounds
    SoundsHint => "Отметьте события, которые сопровождаются звуком.", "Select events that play a sound.";
    SoundPlayFile => "Проиграть звуковой файл", "Play a sound file";
    SoundBeep => "Сигнал динамика системного блока", "System speaker beep";
    SoundEvAutoswitch => "Автопереключение раскладки", "Automatic layout switch";
    SoundEvManualConvert => "Конвертация по горячей клавише", "Conversion by hotkey";
    SoundEvLayoutChanged => "Смена раскладки", "Layout change";
    SoundEvCancel => "Отмена конвертации", "Conversion undone";
    SoundEvSuspicious => "Возможная опечатка", "Possible typo";
    SoundEvAutoreplace => "Автозамена", "Autoreplace";
    SoundEvCaseFixed => "Исправление регистра", "Case corrected";
    SoundEvClipboardConvert => "Конвертация буфера обмена", "Clipboard converted";
    SoundEvError => "Ошибка конвертации", "Conversion error";
    SoundEvSpellingError => "Найдена орфографическая ошибка", "Spelling error found";
    SoundEvSpellingCorrected => "Орфографическая ошибка исправлена", "Spelling correction applied";

    // Settings window, additions
    InDevelopment => "в разработке", "in development";
    ClipboardResultTitle => "Результат конвертации буфера обмена", "Clipboard conversion result";
    SpellcheckTitle => "Проверка орфографии буфера обмена", "Clipboard spell check";
    SpellcheckWordTitle => "Возможная орфографическая ошибка", "Possible spelling error";
    SpellcheckReplaceWord => "Заменить слово", "Replace word";
    SpellcheckTargetChanged => "Поле ввода изменилось", "The input field changed";
    SpellcheckReplacementFailed => "Слово не заменено. Повторите проверку в редакторе.", "The word was not replaced. Check it again in the editor.";
    CopyResult => "Скопировать результат", "Copy result";
    ResultCopied => "Результат скопирован", "Result copied";
    SpellingNoErrors => "Ошибок не найдено.", "No spelling errors found.";
    SpellingKeep => "Оставить как есть", "Keep unchanged";
    SpellingApplyAll => "Выбрать первые подсказки", "Choose first suggestions";
    SpellingPreview => "Результат:", "Result:";
    OptUiLanguage => "Язык интерфейса:", "Interface language:";
    OptTheme => "Оформление:", "Theme:";
    ThemeSystem => "как в системе", "system";
    ThemeLight => "светлое", "light";
    ThemeDark => "тёмное", "dark";
    LangRussian => "Русский", "Russian";
    LangEnglish => "Английский", "English";
    LangSystem => "Автоматически", "Automatic";
    OptLayoutSwitchDelay => "Пауза перед перепечаткой слова, мс:", "Pause before retyping a word, ms:";
    OptInjectDelay => "Пауза между нажатиями при перепечатке, мс:", "Pause between retyped keys, ms:";
    KeyNone => "не выбрано", "not set";
    OptMinWordLen => "Не переключать слова короче, букв:", "Do not convert words shorter than, letters:";
    OptPasswordHeuristic => "Не переключать слова с цифрами и смешанным регистром (пароли)", "Do not convert words with digits or mixed case (passwords)";
    SensitivityCautious => "осторожно", "cautious";
    SensitivityEager => "смело", "eager";
    HotkeyDialogTitle => "Назначить комбинацию клавиш", "Assign key combination";
    HotkeyPressPrompt => "Нажмите нужную комбинацию клавиш. Esc — отмена.", "Press the key combination. Esc cancels.";
    HotkeyWaiting => "Ожидание нажатия…", "Waiting for a key press…";
    HotkeyTypeHint => "Или введите вручную, например Ctrl+Alt+K:", "Or type it, e.g. Ctrl+Alt+K:";
    HotkeyPressAgain => "Нажать заново", "Press again";
    HotkeyConflict => "Эта комбинация уже назначена действию:", "This combination is already assigned to:";
    HotkeyInvalid => "Не удалось разобрать комбинацию:", "Cannot parse the combination:";
    BtnClear => "Очистить", "Clear";
    RuleDialogTitle => "Правило переключения", "Switching rule";
    RulePattern => "Слово или сочетание букв:", "Word or letter combination:";
    RulePatternHint => "Совет: укажите характерную часть слова без окончания.", "Tip: use the characteristic part of the word without its ending.";
    RuleSuggestion => "Вы несколько раз подряд отменили конвертацию этого слова. Добавить правило?", "You undid the conversion of this word several times. Add a rule?";
    ColumnRulePattern => "Сочетание", "Combination";
    ColumnRuleCondition => "Условие", "Condition";
    ColumnRuleAction => "Действие", "Action";
    ExclusionDialogTitle => "Программа-исключение", "Excluded program";
    ExclusionExePrompt => "Полный путь к файлу программы:", "Full path of the program file:";
    ExclusionTitlePrompt => "Часть заголовка окна (с учётом регистра):", "Part of the window title (case-sensitive):";
    ExclusionFolderPrompt => "Папка с программами:", "Program folder:";
    ColumnWindowTitle => "Заголовок окна", "Window title";
    ColumnFolder => "Папка", "Folder";
    AutoreplaceDialogTitle => "Автозамена", "Autoreplace";
    AutoreplaceCursorAt => "Курсор после символа номер:", "Caret after character number:";
    OptListOpacity => "Прозрачность списка автозамены:", "Autoreplace list opacity:";
    ColumnEvent => "Событие", "Event";
    SoundFile => "Свой звуковой файл WAV (пусто — встроенный звук):", "Custom WAV file (empty = built-in sound):";
        GroupDetector => "Анализ текста", "Text analysis";
    OptLayoutFlags => "Флаг для каждой раскладки:", "Flag for each layout:";
    FlagNotSet => "не выбран", "not set";
    GroupTiming => "Перепечатка", "Retyping";

    // Buttons
    BtnOk => "ОК", "OK";
    BtnCancel => "Отмена", "Cancel";
    BtnApply => "Применить", "Apply";
    BtnAdd => "Добавить...", "Add...";
    BtnEdit => "Изменить", "Edit";
    BtnDelete => "Удалить", "Delete";
    BtnAssign => "Назначить...", "Assign...";
    BtnDefault => "По умолчанию", "Default";
    BtnPlay => "Проиграть", "Play";
    BtnChange => "Изменить...", "Change...";
    BtnBrowse => "Обзор...", "Browse...";
}

/// Label of a hotkey action in the «Горячие клавиши» list.
pub const fn hotkey_action_text(action: HotkeyAction) -> Text {
    match action {
        HotkeyAction::CancelOrConvertLastWord => Text::ActCancelOrConvertLastWord,
        HotkeyAction::ConvertSelectionLayout => Text::ActConvertSelectionLayout,
        HotkeyAction::InvertSelectionCase => Text::ActInvertSelectionCase,
        HotkeyAction::TransliterateSelection => Text::ActTransliterateSelection,
        HotkeyAction::PastePlain => Text::ActPastePlain,
        HotkeyAction::NumberToWords => Text::ActNumberToWords,
        HotkeyAction::ToggleAutoswitch => Text::ActToggleAutoswitch,
        HotkeyAction::ToggleSounds => Text::ActToggleSounds,
        HotkeyAction::OpenSettings => Text::ActOpenSettings,
        HotkeyAction::ToggleMaximizeWindow => Text::ActToggleMaximizeWindow,
        HotkeyAction::MinimizeWindow => Text::ActMinimizeWindow,
        HotkeyAction::OpenAutoreplaceSettings => Text::ActOpenAutoreplaceSettings,
        HotkeyAction::ToggleAutoreplaceList => Text::ActToggleAutoreplaceList,
        HotkeyAction::ShowAutoreplaceMenu => Text::ActShowAutoreplaceMenu,
        HotkeyAction::AddSelectionToAutoreplace => Text::ActAddSelectionToAutoreplace,
        HotkeyAction::ShowClipboardHistory => Text::ActShowClipboardHistory,
        HotkeyAction::ConvertClipboardLayout => Text::ActConvertClipboardLayout,
        HotkeyAction::TransliterateClipboard => Text::ActTransliterateClipboard,
        HotkeyAction::SpellcheckClipboard => Text::ActSpellcheckClipboard,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_text_is_translated() {
        for &text in Text::ALL {
            for lang in Lang::ALL {
                let s = tr(text, lang);
                assert!(!s.trim().is_empty(), "{text:?} is empty in {lang}");
                assert_eq!(s, s.trim(), "{text:?} has surrounding spaces in {lang}");
            }
        }
    }

    #[test]
    fn english_interface_strings_do_not_fall_back_to_russian() {
        for &text in Text::ALL {
            assert!(
                !tr(text, Lang::En)
                    .chars()
                    .any(|c| ('\u{0400}'..='\u{04ff}').contains(&c)),
                "{text:?}"
            );
        }
    }

    #[test]
    fn russian_texts_are_russian() {
        let latin_only_allowed: HashSet<Text> = [
            Text::AppName,
            Text::KeyCapsLock,
            Text::KeyBackspace,
            Text::KeyDelete,
            Text::TriggerEnter,
            Text::TriggerTab,
            Text::BtnOk,
        ]
        .into_iter()
        .collect();
        for &text in Text::ALL {
            let ru = tr(text, Lang::Ru);
            let has_cyrillic = ru
                .chars()
                .any(|c| ('а'..='я').contains(&c.to_lowercase().next().unwrap_or(c)) || c == 'ё');
            assert!(
                has_cyrillic || latin_only_allowed.contains(&text),
                "{text:?} = {ru:?}"
            );
        }
    }

    #[test]
    fn hotkey_actions_have_distinct_labels() {
        let mut seen = HashSet::new();
        for &action in HotkeyAction::ALL {
            let label = tr(hotkey_action_text(action), Lang::Ru);
            assert!(seen.insert(label), "duplicate label {label}");
        }
        assert_eq!(
            tr(
                hotkey_action_text(HotkeyAction::CancelOrConvertLastWord),
                Lang::Ru
            ),
            "Отменить конвертацию раскладки"
        );
    }
}
