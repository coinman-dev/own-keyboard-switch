//! Language data embedded into the binary (`builtin-data` feature).
//!
//! Sources and licenses: `data/LICENSES.md`. The data is decompressed lazily
//! on first use; call [`warm_up`] from a background thread at startup.

use crate::lang::Lang;
use crate::lm::LangModel;
use std::sync::OnceLock;

macro_rules! generated {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/generated/",
            $name
        ))
    };
}

static RU_LM: &[u8] = generated!("ru.lm.z");
static EN_LM: &[u8] = generated!("en.lm.z");
static RU_AFF: &[u8] = generated!("ru_RU.aff.z");
static RU_DIC: &[u8] = generated!("ru_RU.dic.z");
static EN_AFF: &[u8] = generated!("en_US.aff.z");
static EN_DIC: &[u8] = generated!("en_US.dic.z");

fn inflate_text(bytes: &[u8], what: &str) -> String {
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(bytes)
        .unwrap_or_else(|_| panic!("built-in {what} is corrupted"));
    String::from_utf8(raw).unwrap_or_else(|_| panic!("built-in {what} is not UTF-8"))
}

/// Built-in trigram model of `lang`.
pub fn language_model(lang: Lang) -> &'static LangModel {
    static RU: OnceLock<LangModel> = OnceLock::new();
    static EN: OnceLock<LangModel> = OnceLock::new();
    let (cell, bytes) = match lang {
        Lang::Ru => (&RU, RU_LM),
        Lang::En => (&EN, EN_LM),
    };
    cell.get_or_init(|| {
        LangModel::from_compressed(bytes)
            .unwrap_or_else(|e| panic!("built-in {lang} language model is invalid: {e}"))
    })
}

/// Built-in Hunspell dictionary of `lang` (ru_RU or en_US).
pub fn dictionary(lang: Lang) -> &'static spellbook::Dictionary {
    static RU: OnceLock<spellbook::Dictionary> = OnceLock::new();
    static EN: OnceLock<spellbook::Dictionary> = OnceLock::new();
    let (cell, aff, dic) = match lang {
        Lang::Ru => (&RU, RU_AFF, RU_DIC),
        Lang::En => (&EN, EN_AFF, EN_DIC),
    };
    cell.get_or_init(|| {
        let aff = inflate_text(aff, "affix file");
        let dic = inflate_text(dic, "dictionary");
        spellbook::Dictionary::new(&aff, &dic)
            .unwrap_or_else(|e| panic!("built-in {lang} dictionary is invalid: {e}"))
    })
}

/// Loads all built-in data now (takes about 100 ms in release builds).
pub fn warm_up() {
    crate::extra_rules::builtin();
    for lang in Lang::ALL {
        language_model(lang);
        dictionary(lang);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_data_loads() {
        let ru = language_model(Lang::Ru);
        let en = language_model(Lang::En);
        assert_eq!(ru.lang(), Lang::Ru);
        assert_eq!(en.lang(), Lang::En);
        assert!(ru.words().len() > 10_000);
        assert!(ru.rank("не").is_some_and(|r| r < 5));
        assert!(en.rank("the").is_some_and(|r| r < 5));
        assert!(dictionary(Lang::Ru).check("привет"));
        assert!(dictionary(Lang::En).check("hello"));
        assert!(!dictionary(Lang::En).check("ghbdtn"));
    }

    #[test]
    fn models_separate_languages() {
        let ru = language_model(Lang::Ru);
        let en = language_model(Lang::En);
        assert_eq!(ru.score("привет").impossible, 0);
        assert!(en.score("ghbdtn").impossible > 0);
        assert_eq!(en.score("hello").impossible, 0);
        assert!(ru.score("руддщ").impossible > 0);
        assert!(ru.score("привет").cost < ru.score("руддщ").cost);
    }
}
