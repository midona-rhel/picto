//! The interface's languages, as production's `src/i18n/index.ts` has them.
//!
//! The catalogs are production's own files (`src/i18n/locales/*.json`),
//! included when the app is built: flat maps keyed by the English text, with
//! `{value0}`, `{value1}` … for what is filled in. A message is looked up by
//! its English text, so the English written in the code must be exactly
//! production's key; a message no catalog holds shows in English.
//!
//! The language is the one chosen in Settings (kept per machine as
//! production's `picto:locale`), else the system's language when it is one
//! of the eight, else English. Choosing a language in Settings applies it
//! at once in every window, as production's `setLocale` does.
//!
//! Numbers and dates are not written in the chosen language: production
//! writes them with `toLocaleString()` and `Intl` without a locale, so in
//! the system's (`format.rs`).
pub mod format;
#[cfg(test)]
mod guard;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// The interface languages, in Settings' order (`LOCALES`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Locale {
    En,
    De,
    Es,
    Pt,
    Fr,
    ZhCn,
    Ja,
    Fi,
}

impl Locale {
    pub const ALL: [Locale; 8] = [
        Locale::En,
        Locale::De,
        Locale::Es,
        Locale::Pt,
        Locale::Fr,
        Locale::ZhCn,
        Locale::Ja,
        Locale::Fi,
    ];

    /// The value production stores (`picto:locale`).
    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::De => "de",
            Locale::Es => "es",
            Locale::Pt => "pt",
            Locale::Fr => "fr",
            Locale::ZhCn => "zh-CN",
            Locale::Ja => "ja",
            Locale::Fi => "fi",
        }
    }

    /// The language's own name, as Settings lists it (`LOCALE_OPTIONS`);
    /// never translated.
    pub fn label(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::De => "Deutsch",
            Locale::Es => "Español",
            Locale::Pt => "Português",
            Locale::Fr => "Français",
            Locale::ZhCn => "简体中文",
            Locale::Ja => "日本語",
            Locale::Fi => "Suomi",
        }
    }

    /// `normalizeLocale`: a stored value or a system language tag by its
    /// first letters; `None` for any other language.
    pub fn normalize(value: &str) -> Option<Locale> {
        let value = value.to_lowercase();
        [
            ("de", Locale::De),
            ("es", Locale::Es),
            ("pt", Locale::Pt),
            ("fr", Locale::Fr),
            ("zh", Locale::ZhCn),
            ("ja", Locale::Ja),
            ("fi", Locale::Fi),
            ("en", Locale::En),
        ]
        .into_iter()
        .find_map(|(prefix, locale)| value.starts_with(prefix).then_some(locale))
    }

    fn index(self) -> usize {
        Locale::ALL
            .iter()
            .position(|locale| *locale == self)
            .unwrap_or(0)
    }

    /// The source text of this language's catalog.
    fn source(self) -> &'static str {
        match self {
            Locale::En => include_str!("../../../../src/i18n/locales/en.json"),
            Locale::De => include_str!("../../../../src/i18n/locales/de.json"),
            Locale::Es => include_str!("../../../../src/i18n/locales/es.json"),
            Locale::Pt => include_str!("../../../../src/i18n/locales/pt.json"),
            Locale::Fr => include_str!("../../../../src/i18n/locales/fr.json"),
            Locale::ZhCn => include_str!("../../../../src/i18n/locales/zh-CN.json"),
            Locale::Ja => include_str!("../../../../src/i18n/locales/ja.json"),
            Locale::Fi => include_str!("../../../../src/i18n/locales/fi.json"),
        }
    }
}

type Catalog = HashMap<String, String>;

/// Every catalog, parsed the first time a message is looked up.
static CATALOGS: LazyLock<Vec<Catalog>> = LazyLock::new(|| {
    Locale::ALL
        .iter()
        .map(|locale| {
            serde_json::from_str(locale.source())
                .unwrap_or_else(|error| panic!("{}.json: {error}", locale.code()))
        })
        .collect()
});

/// `messageKeyByTranslation`: the English key of each translation, the
/// first catalog's when two share a text.
static KEY_BY_TRANSLATION: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    // In a fixed order (catalogs in `Locale::ALL`'s, keys sorted), so a
    // translation two keys share is always told back to the same one.
    let mut keys = HashMap::new();
    for catalog in CATALOGS.iter() {
        let mut entries: Vec<_> = catalog.iter().collect();
        entries.sort_unstable();
        for (key, translation) in entries {
            keys.entry(translation.as_str()).or_insert(key.as_str());
        }
    }
    keys
});

fn catalog(locale: Locale) -> &'static Catalog {
    &CATALOGS[locale.index()]
}

/// Whether `key` is one of production's messages.
pub fn is_key(key: &str) -> bool {
    catalog(Locale::En).contains_key(key)
}

/// The shown language. One per process, as the theme is: every window
/// reads it while it draws.
static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn current() -> Locale {
    Locale::ALL
        .get(CURRENT.load(Ordering::Relaxed) as usize)
        .copied()
        .unwrap_or(Locale::En)
}

/// `getLocale`: the stored choice, else the system's language, else English.
pub fn resolve(stored: &str, system: Option<&str>) -> Locale {
    Locale::normalize(stored)
        .or_else(|| system.and_then(Locale::normalize))
        .unwrap_or(Locale::En)
}

/// Sets the language the app starts in from the kept choice (empty when
/// none was made).
pub fn init(stored: &str) {
    let system = sys_locale::get_locale();
    let locale = resolve(stored, system.as_deref());
    CURRENT.store(locale.index() as u8, Ordering::Relaxed);
}

/// `setLocale`: shows `locale` in every window at once, if it is not the
/// one shown. The caller keeps the choice.
pub fn show(locale: Locale, cx: &mut gpui::App) {
    if current() != locale {
        CURRENT.store(locale.index() as u8, Ordering::Relaxed);
        // Cached views paint again only when the window refreshes.
        cx.refresh_windows();
    }
}

/// `translateMessage` in `locale`: the catalog's text for the English
/// `message`, else the English, else the message itself.
pub fn translate(locale: Locale, message: &str) -> &str {
    if let Some(text) = catalog(locale).get(message) {
        return text;
    }
    // A label kept from while another language was shown is told back to
    // its English key, so it is shown again in this one.
    match KEY_BY_TRANSLATION.get(message) {
        Some(key) if !is_key(message) => catalog(locale).get(*key).map_or(key, |text| text),
        _ => message,
    }
}

/// `t(message)`: `message`, production's English text, in the shown
/// language.
pub fn t(message: &str) -> &str {
    translate(current(), message)
}

/// `translateMessage` for a label a shared control is handed (a menu row,
/// a select's choice): the key's text, or the label as it came.
pub fn t_shared(label: gpui::SharedString) -> gpui::SharedString {
    let shown = t(&label);
    if shown == label.as_ref() {
        label
    } else {
        gpui::SharedString::from(shown.to_owned())
    }
}

/// Fills `{value0}`, `{value1}` … with `values` in order; any other token,
/// or one with no value, is left as it is (production's `replace`).
pub fn fill(template: &str, values: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let filled = after.find('}').and_then(|close| {
            let name = &after[..close];
            let index: usize = name.strip_prefix("value")?.parse().ok()?;
            let value = values.get(index)?;
            Some((close, value.to_string()))
        });
        match filled {
            Some((close, value)) => {
                out.push_str(&value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `t(message, { value0, value1, … })` in `locale`.
pub fn translate_with(locale: Locale, message: &str, values: &[&dyn std::fmt::Display]) -> String {
    fill(translate(locale, message), values)
}

/// `t(message, { value0, value1, … })` in the shown language; the macro
/// [`tf!`](crate::tf) writes it.
pub fn t_with(message: &str, values: &[&dyn std::fmt::Display]) -> String {
    translate_with(current(), message, values)
}

/// `tf!("Delete {value0} items", count)`: `t` with its values in order.
#[macro_export]
macro_rules! tf {
    ($message:literal $(, $value:expr)* $(,)?) => {
        $crate::i18n::t_with($message, &[$(&$value as &dyn ::std::fmt::Display),*])
    };
}

/// A text only this app shows, with no key in production's catalogs: it
/// stays English in every language. Each is listed in `gpui_only.txt`.
pub const fn en(text: &'static str) -> &'static str {
    text
}

/// The English plural suffix production passes as a message's value
/// (`count === 1 ? '' : 's'`), in every language.
pub fn plural_s(count: impl Into<i128>) -> &'static str {
    if count.into() == 1 { "" } else { "s" }
}
