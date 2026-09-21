//! Numbers in words (Russian): «Преобразовать число в текст».
//!
//! `458` → «Четыреста пятьдесят восемь», `247-23` → «Двести сорок семь рублей 23 копейки».

/// Grammatical gender of the counted noun.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gender {
    /// рубль, миллион.
    Masculine,
    /// копейка, тысяча.
    Feminine,
    /// Neuter nouns.
    Neuter,
}

const UNITS: [&str; 20] = [
    "",
    "один",
    "два",
    "три",
    "четыре",
    "пять",
    "шесть",
    "семь",
    "восемь",
    "девять",
    "десять",
    "одиннадцать",
    "двенадцать",
    "тринадцать",
    "четырнадцать",
    "пятнадцать",
    "шестнадцать",
    "семнадцать",
    "восемнадцать",
    "девятнадцать",
];
const TENS: [&str; 10] = [
    "",
    "",
    "двадцать",
    "тридцать",
    "сорок",
    "пятьдесят",
    "шестьдесят",
    "семьдесят",
    "восемьдесят",
    "девяносто",
];
const HUNDREDS: [&str; 10] = [
    "",
    "сто",
    "двести",
    "триста",
    "четыреста",
    "пятьсот",
    "шестьсот",
    "семьсот",
    "восемьсот",
    "девятьсот",
];

/// Scales with their gender and forms for 1, 2–4 and 5+.
const SCALES: [(Gender, [&str; 3]); 4] = [
    (Gender::Feminine, ["тысяча", "тысячи", "тысяч"]),
    (Gender::Masculine, ["миллион", "миллиона", "миллионов"]),
    (Gender::Masculine, ["миллиард", "миллиарда", "миллиардов"]),
    (Gender::Masculine, ["триллион", "триллиона", "триллионов"]),
];

/// Picks the noun form for `n`: `forms` are for 1, 2–4 and 5+.
pub fn plural(n: u64, forms: [&str; 3]) -> &str {
    let (last2, last) = (n % 100, n % 10);
    if (11..=14).contains(&last2) {
        forms[2]
    } else if last == 1 {
        forms[0]
    } else if (2..=4).contains(&last) {
        forms[1]
    } else {
        forms[2]
    }
}

fn triad(n: u64, gender: Gender, out: &mut Vec<&'static str>) {
    let (h, rest) = ((n / 100) as usize, (n % 100) as usize);
    if h > 0 {
        out.push(HUNDREDS[h]);
    }
    let unit = if rest < 20 {
        rest
    } else {
        out.push(TENS[rest / 10]);
        rest % 10
    };
    match (unit, gender) {
        (0, _) => {}
        (1, Gender::Feminine) => out.push("одна"),
        (1, Gender::Neuter) => out.push("одно"),
        (2, Gender::Feminine) => out.push("две"),
        (u, _) => out.push(UNITS[u]),
    }
}

/// Writes `n` in words, lowercase, agreeing with a noun of `gender`.
pub fn number_to_words(n: u64, gender: Gender) -> String {
    if n == 0 {
        return "ноль".to_string();
    }
    let mut words: Vec<&str> = Vec::new();
    let scales_top = (SCALES.len() as u32 * 3) as usize;
    let mut groups = Vec::new();
    let mut rest = n;
    while rest > 0 {
        groups.push(rest % 1000);
        rest /= 1000;
    }
    if groups.len() > scales_top / 3 + 1 {
        return n.to_string();
    }
    for (i, &group) in groups.iter().enumerate().rev() {
        if group == 0 {
            continue;
        }
        if i == 0 {
            triad(group, gender, &mut words);
        } else {
            let (scale_gender, forms) = SCALES[i - 1];
            triad(group, scale_gender, &mut words);
            words.push(plural(group, forms));
        }
    }
    words.join(" ")
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|f| f.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Rubles and kopecks: «Двести сорок семь рублей 23 копейки».
pub fn money_to_words(rubles: u64, kopecks: u8) -> String {
    format!(
        "{} {} {:02} {}",
        capitalize(&number_to_words(rubles, Gender::Masculine)),
        plural(rubles, ["рубль", "рубля", "рублей"]),
        kopecks,
        plural(u64::from(kopecks), ["копейка", "копейки", "копеек"])
    )
}

/// Converts selected text: an integer (`458`, `1 000 000`, `-5`) or a sum
/// with kopecks (`247-23`, `247,23`, `247.23`). Returns `None` for other text.
pub fn convert(text: &str) -> Option<String> {
    let text = text.trim();
    let (negative, body) = match text.strip_prefix(['-', '−']) {
        Some(rest) => (true, rest.trim_start()),
        None => (false, text),
    };
    let digits_only = |s: &str| -> Option<u64> {
        let cleaned: String = s
            .chars()
            .filter(|c| !matches!(c, ' ' | '\u{a0}' | '\u{202f}' | '\''))
            .collect();
        (!cleaned.is_empty() && cleaned.chars().all(|c| c.is_ascii_digit()))
            .then(|| cleaned.parse().ok())
            .flatten()
    };
    let result = if let Some((int, frac)) = body.rsplit_once(['-', ',', '.']) {
        let rubles = digits_only(int)?;
        if frac.len() != 2 || !frac.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        money_to_words(rubles, frac.parse().ok()?)
    } else {
        capitalize(&number_to_words(digits_only(body)?, Gender::Masculine))
    };
    Some(if negative {
        format!("Минус {}", lowercase_first(&result))
    } else {
        result
    })
}

fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|f| f.to_lowercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_examples() {
        assert_eq!(
            convert("458").as_deref(),
            Some("Четыреста пятьдесят восемь")
        );
        assert_eq!(
            convert("247-23").as_deref(),
            Some("Двести сорок семь рублей 23 копейки")
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(number_to_words(0, Gender::Masculine), "ноль");
        assert_eq!(number_to_words(11, Gender::Masculine), "одиннадцать");
        assert_eq!(number_to_words(21, Gender::Feminine), "двадцать одна");
        assert_eq!(number_to_words(1002, Gender::Masculine), "одна тысяча два");
        assert_eq!(number_to_words(2_000, Gender::Masculine), "две тысячи");
        assert_eq!(number_to_words(5_000, Gender::Masculine), "пять тысяч");
        assert_eq!(
            number_to_words(11_000, Gender::Masculine),
            "одиннадцать тысяч"
        );
        assert_eq!(
            number_to_words(1_234_567, Gender::Masculine),
            "один миллион двести тридцать четыре тысячи пятьсот шестьдесят семь"
        );
        assert_eq!(
            number_to_words(3_000_000_001, Gender::Masculine),
            "три миллиарда один"
        );
        assert_eq!(
            number_to_words(u64::MAX, Gender::Masculine),
            u64::MAX.to_string()
        );
    }

    #[test]
    fn money_and_forms() {
        assert_eq!(money_to_words(1, 1), "Один рубль 01 копейка");
        assert_eq!(money_to_words(2, 2), "Два рубля 02 копейки");
        assert_eq!(money_to_words(11, 11), "Одиннадцать рублей 11 копеек");
        assert_eq!(money_to_words(0, 50), "Ноль рублей 50 копеек");
        assert_eq!(
            convert("1 000,00").as_deref(),
            Some("Одна тысяча рублей 00 копеек")
        );
        assert_eq!(convert("-5").as_deref(), Some("Минус пять"));
        assert_eq!(convert("12.5"), None);
        assert_eq!(convert("abc"), None);
        assert_eq!(convert(""), None);
    }
}
