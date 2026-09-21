//! Resolve the system UI language once at startup; never use the keyboard layout.

use okbs_core::Lang;

pub fn system_ui_language() -> Lang {
    #[cfg(windows)]
    {
        okbs_platform_windows::locale::ui_language()
    }
    #[cfg(not(windows))]
    {
        from_environment(|key| std::env::var(key).ok())
    }
}

#[cfg(any(not(windows), test))]
fn from_environment(get: impl Fn(&str) -> Option<String>) -> Lang {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(&get)
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "C".into());
    let language_of = |value: &str| Lang::from_tag(value.split(['.', '@']).next().unwrap_or(value));
    // C/POSIX deliberately asks for the untranslated (English) interface.
    if matches!(locale.split('.').next(), Some("C" | "POSIX")) {
        return Lang::En;
    }
    // GNU gettext's LANGUAGE list is preferred when a localized locale is active.
    if let Some(languages) = get("LANGUAGE")
        && let Some(lang) = languages.split(':').find_map(language_of)
    {
        return lang;
    }
    language_of(&locale).unwrap_or(Lang::En)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_precedence_and_english_fallback() {
        for (vars, expected) in [
            (vec![("LANG", "ru_RU.UTF-8")], Lang::Ru),
            (vec![("LANG", "en_US.UTF-8")], Lang::En),
            (vec![("LANG", "de_DE.UTF-8")], Lang::En),
            (
                vec![("LANG", "en_US.UTF-8"), ("LC_MESSAGES", "ru_RU.UTF-8")],
                Lang::Ru,
            ),
            (
                vec![("LANG", "ru_RU.UTF-8"), ("LC_ALL", "en_GB.UTF-8")],
                Lang::En,
            ),
            (
                vec![("LANG", "en_US.UTF-8"), ("LANGUAGE", "de:ru:en")],
                Lang::Ru,
            ),
            (vec![("LC_ALL", "C"), ("LANGUAGE", "ru")], Lang::En),
            (vec![("LC_ALL", ""), ("LANG", "ru_RU.UTF-8")], Lang::Ru),
            (vec![], Lang::En),
        ] {
            let actual = from_environment(|key| {
                vars.iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, v)| (*v).into())
            });
            assert_eq!(actual, expected, "{vars:?}");
        }
    }
}
