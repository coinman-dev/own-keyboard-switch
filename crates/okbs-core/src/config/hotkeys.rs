//! The «Горячие клавиши» section.

use crate::hotkey::{Hotkey, Modifiers};
use crate::keys::PhysKey;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// A hotkey assignment; `None` (an empty string in the file) means unassigned.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct HotkeyBinding(pub Option<Hotkey>);

impl HotkeyBinding {
    /// Unassigned.
    pub const NONE: HotkeyBinding = HotkeyBinding(None);

    /// Assigned to `hotkey`.
    pub const fn some(hotkey: Hotkey) -> Self {
        Self(Some(hotkey))
    }
}

impl fmt::Display for HotkeyBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(h) => h.fmt(f),
            None => Ok(()),
        }
    }
}

impl Serialize for HotkeyBinding {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for HotkeyBinding {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s.trim().is_empty() {
            return Ok(HotkeyBinding::NONE);
        }
        s.parse::<Hotkey>()
            .map(HotkeyBinding::some)
            .map_err(serde::de::Error::custom)
    }
}

macro_rules! define_hotkeys {
    ($( $(#[$doc:meta])* $field:ident => $variant:ident = $default:expr; )*) => {
        /// An action that can be bound to a hotkey.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum HotkeyAction {
            $( $(#[$doc])* $variant, )*
        }

        impl HotkeyAction {
            /// All actions in settings-window order.
            pub const ALL: &'static [HotkeyAction] = &[$(HotkeyAction::$variant,)*];

            /// Key of the action in `config.toml`.
            pub const fn config_key(self) -> &'static str {
                match self { $(HotkeyAction::$variant => stringify!($field),)* }
            }
        }

        /// Горячие клавиши.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Hotkeys {
            $( $(#[$doc])* pub $field: HotkeyBinding, )*
        }

        impl Default for Hotkeys {
            fn default() -> Self {
                Self { $( $field: $default, )* }
            }
        }

        impl Hotkeys {
            /// Binding of `action`.
            pub fn get(&self, action: HotkeyAction) -> HotkeyBinding {
                match action { $(HotkeyAction::$variant => self.$field,)* }
            }

            /// Mutable binding of `action`.
            pub fn get_mut(&mut self, action: HotkeyAction) -> &mut HotkeyBinding {
                match action { $(HotkeyAction::$variant => &mut self.$field,)* }
            }

            /// All actions with their bindings.
            pub fn iter(&self) -> impl Iterator<Item = (HotkeyAction, HotkeyBinding)> + '_ {
                HotkeyAction::ALL.iter().map(|&a| (a, self.get(a)))
            }
        }
    };
}

const fn bind(mods: Modifiers, key: PhysKey) -> HotkeyBinding {
    HotkeyBinding::some(Hotkey::new(mods, key))
}

define_hotkeys! {
    /// «Отменить конвертацию раскладки» (also converts the last word).
    cancel_or_convert_last_word => CancelOrConvertLastWord = bind(Modifiers::NONE, PhysKey::Pause);
    /// «Сменить раскладку выделенного текста».
    convert_selection_layout => ConvertSelectionLayout = bind(Modifiers::SHIFT, PhysKey::Pause);
    /// «Сменить регистр выделенного текста».
    invert_selection_case => InvertSelectionCase = bind(Modifiers::ALT, PhysKey::Pause);
    /// «Транслитерировать выделенный текст».
    transliterate_selection => TransliterateSelection = bind(Modifiers::ALT, PhysKey::ScrollLock);
    /// «Вставка текста без форматирования».
    paste_plain => PastePlain = bind(Modifiers::CTRL.with(Modifiers::ALT), PhysKey::KeyV);
    /// «Преобразовать число в текст».
    number_to_words => NumberToWords = HotkeyBinding::NONE;
    /// «Включить/выключить автопереключение».
    toggle_autoswitch => ToggleAutoswitch = HotkeyBinding::NONE;
    /// «Включить/выключить звуковые эффекты».
    toggle_sounds => ToggleSounds = HotkeyBinding::NONE;
    /// «Открыть настройки».
    open_settings => OpenSettings = HotkeyBinding::NONE;
    /// «Развернуть/восстановить активное окно».
    toggle_maximize_window => ToggleMaximizeWindow = HotkeyBinding::NONE;
    /// «Свернуть активное окно».
    minimize_window => MinimizeWindow = HotkeyBinding::NONE;
    /// «Открыть настройки автозамены».
    open_autoreplace_settings => OpenAutoreplaceSettings = HotkeyBinding::NONE;
    /// «Показать/скрыть список автозамены».
    toggle_autoreplace_list => ToggleAutoreplaceList = HotkeyBinding::NONE;
    /// «Показать меню вставки автозамены».
    show_autoreplace_menu => ShowAutoreplaceMenu = HotkeyBinding::NONE;
    /// «Добавить выделенный текст в автозамену».
    add_selection_to_autoreplace => AddSelectionToAutoreplace = HotkeyBinding::NONE;
    /// «Показать историю буфера обмена».
    show_clipboard_history => ShowClipboardHistory = HotkeyBinding::NONE;
    /// «Сменить раскладку буфера обмена».
    convert_clipboard_layout => ConvertClipboardLayout = HotkeyBinding::NONE;
    /// «Транслитерировать текст в буфере обмена».
    transliterate_clipboard => TransliterateClipboard = HotkeyBinding::NONE;
    /// «Проверить орфографию буфера обмена».
    spellcheck_clipboard => SpellcheckClipboard = HotkeyBinding::NONE;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkeys() {
        let h = Hotkeys::default();
        assert_eq!(h.cancel_or_convert_last_word.to_string(), "Break");
        assert_eq!(h.convert_selection_layout.to_string(), "Shift+Break");
        assert_eq!(h.invert_selection_case.to_string(), "Alt+Break");
        assert_eq!(h.transliterate_selection.to_string(), "Alt+ScrollLock");
        assert_eq!(h.paste_plain.to_string(), "Ctrl+Alt+V");
        assert_eq!(h.iter().filter(|(_, b)| b.0.is_some()).count(), 5);
        assert_eq!(HotkeyAction::ALL.len(), 19);
    }

    #[test]
    fn get_mut_and_keys() {
        let mut h = Hotkeys::default();
        *h.get_mut(HotkeyAction::ToggleAutoswitch) =
            "Ctrl+Win+Alt+K".parse().map(HotkeyBinding::some).unwrap();
        assert_eq!(h.toggle_autoswitch.to_string(), "Ctrl+Alt+Win+K");
        assert_eq!(HotkeyAction::PastePlain.config_key(), "paste_plain");
    }
}
