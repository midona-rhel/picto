//! The interface's languages and the translation of its texts.
//!
//! The translations are the JSON files in `locales/` beside this file, one
//! for each language. They are included in the app when it is built. Each
//! file is a flat map from an English text to its translation, with
//! `{value0}`, `{value1}` … where values are filled in. A message is looked
//! up by its English text, so the English text written in the code must be
//! exactly a key of `locales/en.json`. A message that no catalog holds is
//! shown in English, as it is written in the code.
//!
//! The language shown is the one chosen in Settings. When none was chosen,
//! it is the system's language if that is one of the eight, and English
//! otherwise. The choice is stored for the machine, in the app's state
//! (`crate::state`), and not in a library. Choosing a language in Settings
//! shows it at once in every window.
//!
//! Numbers and dates are not written in the chosen language. They are
//! written in the system's language (`format.rs`).
pub mod format;
#[cfg(test)]
mod guard;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// The interface languages, in the order in which Settings lists them.
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

    /// Returns the language's code. It is the value stored as the chosen
    /// language, and the name of the language's file in `locales/`.
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

    /// Returns the language's name in that language, as Settings lists it.
    /// The name is never translated.
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

    /// Returns the language that `value` names. `value` is a stored code or
    /// a system language tag (`de-AT`, `pt_BR`). Only its first two letters
    /// are compared, ignoring case. Returns `None` when they are the letters
    /// of none of the eight languages.
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

    /// Returns the text of this language's file in `locales/`.
    fn source(self) -> &'static str {
        match self {
            Locale::En => include_str!("locales/en.json"),
            Locale::De => include_str!("locales/de.json"),
            Locale::Es => include_str!("locales/es.json"),
            Locale::Pt => include_str!("locales/pt.json"),
            Locale::Fr => include_str!("locales/fr.json"),
            Locale::ZhCn => include_str!("locales/zh-CN.json"),
            Locale::Ja => include_str!("locales/ja.json"),
            Locale::Fi => include_str!("locales/fi.json"),
        }
    }
}

type Catalog = HashMap<String, String>;

/// Every language's catalog, in the order of `Locale::ALL`. The catalogs are
/// parsed the first time a message is looked up. A file that cannot be
/// parsed causes a panic then.
static CATALOGS: LazyLock<Vec<Catalog>> = LazyLock::new(|| {
    Locale::ALL
        .iter()
        .map(|locale| {
            serde_json::from_str(locale.source())
                .unwrap_or_else(|error| panic!("{}.json: {error}", locale.code()))
        })
        .collect()
});

/// The English key of each translation, for every language. When two keys
/// have the same translation, the map holds the key that was read first.
static KEY_BY_TRANSLATION: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    // The catalogs are read in the order of `Locale::ALL` and the keys of
    // each catalog in sorted order, so a translation that two keys share
    // always maps to the same key.
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

/// Returns whether `key` is a key of the English catalog
/// (`locales/en.json`).
pub fn is_key(key: &str) -> bool {
    catalog(Locale::En).contains_key(key)
}

/// The language shown, as its index in `Locale::ALL`. The process has one
/// language for all its windows, and every window reads it while it draws.
static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn current() -> Locale {
    Locale::ALL
        .get(CURRENT.load(Ordering::Relaxed) as usize)
        .copied()
        .unwrap_or(Locale::En)
}

/// Returns the language to show: the stored choice `stored` when it names
/// one of the eight languages, otherwise the system's language `system` when
/// it names one, otherwise English.
pub fn resolve(stored: &str, system: Option<&str>) -> Locale {
    Locale::normalize(stored)
        .or_else(|| system.and_then(Locale::normalize))
        .unwrap_or(Locale::En)
}

/// Sets the language that the app starts in. `stored` is the stored choice,
/// which is empty when no language was chosen.
pub fn init(stored: &str) {
    let system = sys_locale::get_locale();
    let locale = resolve(stored, system.as_deref());
    CURRENT.store(locale.index() as u8, Ordering::Relaxed);
}

/// Shows `locale` in every window at once, unless it is the language already
/// shown. It does not store the choice; the caller does.
pub fn show(locale: Locale, cx: &mut gpui::App) {
    if current() != locale {
        CURRENT.store(locale.index() as u8, Ordering::Relaxed);
        // A cached view is painted again only when its window is refreshed.
        cx.refresh_windows();
    }
}

/// Returns `message` in `locale`. When `message` is a key of `locale`'s
/// catalog, the result is its text there. Otherwise, when `message` is not
/// an English key but is a translation in some language, the result is the
/// text of that translation's key in `locale`, or the key itself when
/// `locale`'s catalog lacks it. Any other `message` is returned as it is.
pub fn translate(locale: Locale, message: &str) -> &str {
    if let Some(text) = catalog(locale).get(message) {
        return text;
    }
    // A label that was translated while another language was shown is mapped
    // back to its English key here, so that it is shown in `locale`.
    match KEY_BY_TRANSLATION.get(message) {
        Some(key) if !is_key(message) => catalog(locale).get(*key).map_or(key, |text| text),
        _ => message,
    }
}

/// Returns `message`, an English key, in the language shown.
pub fn t(message: &str) -> &str {
    translate(current(), message)
}

/// Returns a label that a shared control was given (a menu row, a select's
/// choice, a text field's placeholder) in the language shown. A label that
/// has no translation is returned as it came.
pub fn t_shared(label: gpui::SharedString) -> gpui::SharedString {
    let shown = t(&label);
    if shown == label.as_ref() {
        label
    } else {
        gpui::SharedString::from(shown.to_owned())
    }
}

/// Replaces each `{value0}`, `{value1}` … in `template` with the value at
/// that index in `values`. Any other text in braces, and a token whose index
/// has no value, is left as it is.
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

/// Returns `message` in `locale`, with `values` filled in by [`fill`].
pub fn translate_with(locale: Locale, message: &str, values: &[&dyn std::fmt::Display]) -> String {
    fill(translate(locale, message), values)
}

/// Returns `message` in the language shown, with `values` filled in by
/// [`fill`]. The macro [`tf!`](crate::tf) calls it.
pub fn t_with(message: &str, values: &[&dyn std::fmt::Display]) -> String {
    translate_with(current(), message, values)
}

/// `tf!("Delete {value0} items", count)` returns the message in the language
/// shown, with the arguments after the message filled in as `{value0}`,
/// `{value1}` … in order.
#[macro_export]
macro_rules! tf {
    ($message:literal $(, $value:expr)* $(,)?) => {
        $crate::i18n::t_with($message, &[$(&$value as &dyn ::std::fmt::Display),*])
    };
}

/// Marks a text that has no key in the catalogs and is therefore shown in
/// English in every language. It returns `text` unchanged. Every such text
/// is listed in `gpui_only.txt`, and the guard (`guard.rs`) checks the list.
pub const fn en(text: &'static str) -> &'static str {
    text
}

/// Returns the English plural suffix: an empty text when `count` is 1, and
/// `s` otherwise. A message such as `{value0} subscription{value1}` takes it
/// as a value. The suffix is the same whichever language is shown.
pub fn plural_s(count: impl Into<i128>) -> &'static str {
    if count.into() == 1 { "" } else { "s" }
}
