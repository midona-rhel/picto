//! The catalogs and the message rules, and the number and date formats
//! against what Node's ICU 78 (the ICU family Chromium ships) writes.
use super::format::{self, Unit};
use super::*;
use chrono::NaiveDate;

/// The `{name}` tokens of a text, in order.
fn tokens(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        let name = &after[..close];
        if name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            found.push(name);
        }
        rest = &after[close + 1..];
    }
    found.sort_unstable();
    found
}

#[test]
fn every_catalog_parses_and_holds_every_english_key() {
    let english = catalog(Locale::En);
    assert!(english.len() > 1500, "{} keys", english.len());
    for (key, text) in english {
        assert_eq!(key, text, "en.json is keyed by its own text");
    }
    for locale in Locale::ALL {
        let catalog = catalog(locale);
        let missing: Vec<_> = english
            .keys()
            .filter(|key| !catalog.contains_key(*key))
            .collect();
        let extra: Vec<_> = catalog
            .keys()
            .filter(|key| !english.contains_key(*key))
            .collect();
        assert!(missing.is_empty(), "{}: missing {missing:?}", locale.code());
        assert!(
            extra.is_empty(),
            "{}: not in en.json {extra:?}",
            locale.code()
        );
    }
}

#[test]
fn every_translation_has_its_keys_placeholders() {
    let mut wrong = Vec::new();
    for locale in Locale::ALL {
        for (key, text) in catalog(locale) {
            if tokens(key) != tokens(text) {
                wrong.push(format!("{}: {key:?} => {text:?}", locale.code()));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn a_message_is_looked_up_by_its_english_text() {
    assert_eq!(translate(Locale::De, "Settings"), "Einstellungen");
    assert_eq!(translate(Locale::Es, "Rename"), "Renombrar");
    assert_eq!(translate(Locale::Es, "Rename Group…"), "Renombrar grupo…");
    assert_eq!(translate(Locale::En, "Settings"), "Settings");
}

#[test]
fn a_missing_message_shows_in_english() {
    assert_eq!(
        translate(Locale::De, "Uncatalogued test message"),
        "Uncatalogued test message"
    );
    assert_eq!(translate(Locale::Ja, ""), "");
}

#[test]
fn a_label_kept_from_another_language_is_shown_again() {
    // `translateMessage` recovers the English key of a translation.
    assert_eq!(translate(Locale::En, "Einstellungen"), "Settings");
    assert_eq!(
        translate(Locale::Es, "Einstellungen"),
        translate(Locale::Es, "Settings")
    );
}

#[test]
fn every_placeholder_is_filled_in_order() {
    let text = translate_with(Locale::Es, "Review item {value0}", &[&4]);
    assert!(text.contains('4') && !text.contains("{value0}"), "{text}");
    for locale in Locale::ALL {
        for (key, _) in catalog(Locale::En) {
            let count = tokens(key).len();
            let values: Vec<String> = (0..count).map(|index| format!("<{index}>")).collect();
            let values: Vec<&dyn std::fmt::Display> = values
                .iter()
                .map(|value| value as &dyn std::fmt::Display)
                .collect();
            let filled = translate_with(locale, key, &values);
            assert!(
                !filled.contains("{value"),
                "{}: {key:?} => {filled:?}",
                locale.code()
            );
            for index in 0..count {
                assert!(
                    filled.contains(&format!("<{index}>")),
                    "{}: {key:?}",
                    locale.code()
                );
            }
        }
    }
}

#[test]
fn filling_leaves_other_tokens_and_missing_values() {
    assert_eq!(fill("{value0} of {value1}", &[&1]), "1 of {value1}");
    assert_eq!(fill("{name} {value0}", &[&"x"]), "{name} x");
    assert_eq!(fill("{{value0}}", &[&7]), "{7}");
    assert_eq!(fill("a { b", &[]), "a { b");
    assert_eq!(
        fill("{value0} subscription{value1}", &[&2, &plural_s(2)]),
        "2 subscriptions"
    );
    assert_eq!(
        fill("{value0} subscription{value1}", &[&1, &plural_s(1)]),
        "1 subscription"
    );
}

#[test]
fn the_language_is_the_choice_then_the_systems_then_english() {
    assert_eq!(resolve("de", Some("ja-JP")), Locale::De);
    assert_eq!(resolve("", Some("ja-JP")), Locale::Ja);
    assert_eq!(resolve("", Some("zh-Hant-TW")), Locale::ZhCn);
    assert_eq!(resolve("", Some("pt-PT")), Locale::Pt);
    assert_eq!(resolve("unknown", Some("ru-RU")), Locale::En);
    assert_eq!(resolve("", None), Locale::En);
    assert_eq!(resolve("FI", None), Locale::Fi);
    for locale in Locale::ALL {
        assert_eq!(Locale::normalize(locale.code()), Some(locale));
    }
}

#[test]
fn counts_are_grouped_as_each_language_groups_them() {
    for (locale, count, expected) in [
        (Locale::En, 0, "0"),
        (Locale::En, 999, "999"),
        (Locale::En, 1000, "1,000"),
        (Locale::En, 9999, "9,999"),
        (Locale::En, 10000, "10,000"),
        (Locale::En, 1234567, "1,234,567"),
        (Locale::En, -1234, "-1,234"),
        (Locale::De, 0, "0"),
        (Locale::De, 999, "999"),
        (Locale::De, 1000, "1.000"),
        (Locale::De, 9999, "9.999"),
        (Locale::De, 10000, "10.000"),
        (Locale::De, 1234567, "1.234.567"),
        (Locale::De, -1234, "-1.234"),
        (Locale::Es, 0, "0"),
        (Locale::Es, 999, "999"),
        (Locale::Es, 1000, "1000"),
        (Locale::Es, 9999, "9999"),
        (Locale::Es, 10000, "10.000"),
        (Locale::Es, 1234567, "1.234.567"),
        (Locale::Es, -1234, "-1234"),
        (Locale::Pt, 0, "0"),
        (Locale::Pt, 999, "999"),
        (Locale::Pt, 1000, "1.000"),
        (Locale::Pt, 9999, "9.999"),
        (Locale::Pt, 10000, "10.000"),
        (Locale::Pt, 1234567, "1.234.567"),
        (Locale::Pt, -1234, "-1.234"),
        (Locale::Fr, 0, "0"),
        (Locale::Fr, 999, "999"),
        (Locale::Fr, 1000, "1\u{202f}000"),
        (Locale::Fr, 9999, "9\u{202f}999"),
        (Locale::Fr, 10000, "10\u{202f}000"),
        (Locale::Fr, 1234567, "1\u{202f}234\u{202f}567"),
        (Locale::Fr, -1234, "-1\u{202f}234"),
        (Locale::Ja, 0, "0"),
        (Locale::Ja, 999, "999"),
        (Locale::Ja, 1000, "1,000"),
        (Locale::Ja, 9999, "9,999"),
        (Locale::Ja, 10000, "10,000"),
        (Locale::Ja, 1234567, "1,234,567"),
        (Locale::Ja, -1234, "-1,234"),
        (Locale::ZhCn, 0, "0"),
        (Locale::ZhCn, 999, "999"),
        (Locale::ZhCn, 1000, "1,000"),
        (Locale::ZhCn, 9999, "9,999"),
        (Locale::ZhCn, 10000, "10,000"),
        (Locale::ZhCn, 1234567, "1,234,567"),
        (Locale::ZhCn, -1234, "-1,234"),
        (Locale::Fi, 0, "0"),
        (Locale::Fi, 999, "999"),
        (Locale::Fi, 1000, "1\u{a0}000"),
        (Locale::Fi, 9999, "9\u{a0}999"),
        (Locale::Fi, 10000, "10\u{a0}000"),
        (Locale::Fi, 1234567, "1\u{a0}234\u{a0}567"),
        (Locale::Fi, -1234, "\u{2212}1\u{a0}234"),
    ] {
        assert_eq!(
            format::grouped_in(locale, count),
            expected,
            "{locale:?} {count}"
        );
    }
}

#[test]
fn dates_are_written_as_each_language_writes_them() {
    for (locale, (y, m, d, h, mi, s), [date, date_time, short_month, medium, month_year]) in [
        (
            Locale::En,
            (2026, 1, 4, 15, 4, 5),
            [
                "1/4/2026",
                "1/4/2026, 3:04:05 PM",
                "Jan 4, 2026",
                "Jan 4, 2026, 3:04 PM",
                "January 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 1, 14, 9, 4, 5),
            [
                "1/14/2026",
                "1/14/2026, 9:04:05 AM",
                "Jan 14, 2026",
                "Jan 14, 2026, 9:04 AM",
                "January 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 2, 4, 15, 4, 5),
            [
                "2/4/2026",
                "2/4/2026, 3:04:05 PM",
                "Feb 4, 2026",
                "Feb 4, 2026, 3:04 PM",
                "February 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 2, 14, 9, 4, 5),
            [
                "2/14/2026",
                "2/14/2026, 9:04:05 AM",
                "Feb 14, 2026",
                "Feb 14, 2026, 9:04 AM",
                "February 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 3, 4, 15, 4, 5),
            [
                "3/4/2026",
                "3/4/2026, 3:04:05 PM",
                "Mar 4, 2026",
                "Mar 4, 2026, 3:04 PM",
                "March 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 3, 14, 9, 4, 5),
            [
                "3/14/2026",
                "3/14/2026, 9:04:05 AM",
                "Mar 14, 2026",
                "Mar 14, 2026, 9:04 AM",
                "March 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 4, 4, 15, 4, 5),
            [
                "4/4/2026",
                "4/4/2026, 3:04:05 PM",
                "Apr 4, 2026",
                "Apr 4, 2026, 3:04 PM",
                "April 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 4, 14, 9, 4, 5),
            [
                "4/14/2026",
                "4/14/2026, 9:04:05 AM",
                "Apr 14, 2026",
                "Apr 14, 2026, 9:04 AM",
                "April 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 5, 4, 15, 4, 5),
            [
                "5/4/2026",
                "5/4/2026, 3:04:05 PM",
                "May 4, 2026",
                "May 4, 2026, 3:04 PM",
                "May 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 5, 14, 9, 4, 5),
            [
                "5/14/2026",
                "5/14/2026, 9:04:05 AM",
                "May 14, 2026",
                "May 14, 2026, 9:04 AM",
                "May 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 6, 4, 15, 4, 5),
            [
                "6/4/2026",
                "6/4/2026, 3:04:05 PM",
                "Jun 4, 2026",
                "Jun 4, 2026, 3:04 PM",
                "June 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 6, 14, 9, 4, 5),
            [
                "6/14/2026",
                "6/14/2026, 9:04:05 AM",
                "Jun 14, 2026",
                "Jun 14, 2026, 9:04 AM",
                "June 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 7, 4, 15, 4, 5),
            [
                "7/4/2026",
                "7/4/2026, 3:04:05 PM",
                "Jul 4, 2026",
                "Jul 4, 2026, 3:04 PM",
                "July 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 7, 14, 9, 4, 5),
            [
                "7/14/2026",
                "7/14/2026, 9:04:05 AM",
                "Jul 14, 2026",
                "Jul 14, 2026, 9:04 AM",
                "July 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 8, 4, 15, 4, 5),
            [
                "8/4/2026",
                "8/4/2026, 3:04:05 PM",
                "Aug 4, 2026",
                "Aug 4, 2026, 3:04 PM",
                "August 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 8, 14, 9, 4, 5),
            [
                "8/14/2026",
                "8/14/2026, 9:04:05 AM",
                "Aug 14, 2026",
                "Aug 14, 2026, 9:04 AM",
                "August 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 9, 4, 15, 4, 5),
            [
                "9/4/2026",
                "9/4/2026, 3:04:05 PM",
                "Sep 4, 2026",
                "Sep 4, 2026, 3:04 PM",
                "September 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 9, 14, 9, 4, 5),
            [
                "9/14/2026",
                "9/14/2026, 9:04:05 AM",
                "Sep 14, 2026",
                "Sep 14, 2026, 9:04 AM",
                "September 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 10, 4, 15, 4, 5),
            [
                "10/4/2026",
                "10/4/2026, 3:04:05 PM",
                "Oct 4, 2026",
                "Oct 4, 2026, 3:04 PM",
                "October 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 10, 14, 9, 4, 5),
            [
                "10/14/2026",
                "10/14/2026, 9:04:05 AM",
                "Oct 14, 2026",
                "Oct 14, 2026, 9:04 AM",
                "October 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 11, 4, 15, 4, 5),
            [
                "11/4/2026",
                "11/4/2026, 3:04:05 PM",
                "Nov 4, 2026",
                "Nov 4, 2026, 3:04 PM",
                "November 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 11, 14, 9, 4, 5),
            [
                "11/14/2026",
                "11/14/2026, 9:04:05 AM",
                "Nov 14, 2026",
                "Nov 14, 2026, 9:04 AM",
                "November 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 12, 4, 15, 4, 5),
            [
                "12/4/2026",
                "12/4/2026, 3:04:05 PM",
                "Dec 4, 2026",
                "Dec 4, 2026, 3:04 PM",
                "December 2026",
            ],
        ),
        (
            Locale::En,
            (2026, 12, 14, 9, 4, 5),
            [
                "12/14/2026",
                "12/14/2026, 9:04:05 AM",
                "Dec 14, 2026",
                "Dec 14, 2026, 9:04 AM",
                "December 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 1, 4, 15, 4, 5),
            [
                "4.1.2026",
                "4.1.2026, 15:04:05",
                "4. Jan. 2026",
                "04.01.2026, 15:04",
                "Januar 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 1, 14, 9, 4, 5),
            [
                "14.1.2026",
                "14.1.2026, 09:04:05",
                "14. Jan. 2026",
                "14.01.2026, 09:04",
                "Januar 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 2, 4, 15, 4, 5),
            [
                "4.2.2026",
                "4.2.2026, 15:04:05",
                "4. Feb. 2026",
                "04.02.2026, 15:04",
                "Februar 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 2, 14, 9, 4, 5),
            [
                "14.2.2026",
                "14.2.2026, 09:04:05",
                "14. Feb. 2026",
                "14.02.2026, 09:04",
                "Februar 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 3, 4, 15, 4, 5),
            [
                "4.3.2026",
                "4.3.2026, 15:04:05",
                "4. März 2026",
                "04.03.2026, 15:04",
                "März 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 3, 14, 9, 4, 5),
            [
                "14.3.2026",
                "14.3.2026, 09:04:05",
                "14. März 2026",
                "14.03.2026, 09:04",
                "März 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 4, 4, 15, 4, 5),
            [
                "4.4.2026",
                "4.4.2026, 15:04:05",
                "4. Apr. 2026",
                "04.04.2026, 15:04",
                "April 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 4, 14, 9, 4, 5),
            [
                "14.4.2026",
                "14.4.2026, 09:04:05",
                "14. Apr. 2026",
                "14.04.2026, 09:04",
                "April 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 5, 4, 15, 4, 5),
            [
                "4.5.2026",
                "4.5.2026, 15:04:05",
                "4. Mai 2026",
                "04.05.2026, 15:04",
                "Mai 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 5, 14, 9, 4, 5),
            [
                "14.5.2026",
                "14.5.2026, 09:04:05",
                "14. Mai 2026",
                "14.05.2026, 09:04",
                "Mai 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 6, 4, 15, 4, 5),
            [
                "4.6.2026",
                "4.6.2026, 15:04:05",
                "4. Juni 2026",
                "04.06.2026, 15:04",
                "Juni 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 6, 14, 9, 4, 5),
            [
                "14.6.2026",
                "14.6.2026, 09:04:05",
                "14. Juni 2026",
                "14.06.2026, 09:04",
                "Juni 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 7, 4, 15, 4, 5),
            [
                "4.7.2026",
                "4.7.2026, 15:04:05",
                "4. Juli 2026",
                "04.07.2026, 15:04",
                "Juli 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 7, 14, 9, 4, 5),
            [
                "14.7.2026",
                "14.7.2026, 09:04:05",
                "14. Juli 2026",
                "14.07.2026, 09:04",
                "Juli 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 8, 4, 15, 4, 5),
            [
                "4.8.2026",
                "4.8.2026, 15:04:05",
                "4. Aug. 2026",
                "04.08.2026, 15:04",
                "August 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 8, 14, 9, 4, 5),
            [
                "14.8.2026",
                "14.8.2026, 09:04:05",
                "14. Aug. 2026",
                "14.08.2026, 09:04",
                "August 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 9, 4, 15, 4, 5),
            [
                "4.9.2026",
                "4.9.2026, 15:04:05",
                "4. Sept. 2026",
                "04.09.2026, 15:04",
                "September 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 9, 14, 9, 4, 5),
            [
                "14.9.2026",
                "14.9.2026, 09:04:05",
                "14. Sept. 2026",
                "14.09.2026, 09:04",
                "September 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 10, 4, 15, 4, 5),
            [
                "4.10.2026",
                "4.10.2026, 15:04:05",
                "4. Okt. 2026",
                "04.10.2026, 15:04",
                "Oktober 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 10, 14, 9, 4, 5),
            [
                "14.10.2026",
                "14.10.2026, 09:04:05",
                "14. Okt. 2026",
                "14.10.2026, 09:04",
                "Oktober 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 11, 4, 15, 4, 5),
            [
                "4.11.2026",
                "4.11.2026, 15:04:05",
                "4. Nov. 2026",
                "04.11.2026, 15:04",
                "November 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 11, 14, 9, 4, 5),
            [
                "14.11.2026",
                "14.11.2026, 09:04:05",
                "14. Nov. 2026",
                "14.11.2026, 09:04",
                "November 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 12, 4, 15, 4, 5),
            [
                "4.12.2026",
                "4.12.2026, 15:04:05",
                "4. Dez. 2026",
                "04.12.2026, 15:04",
                "Dezember 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 12, 14, 9, 4, 5),
            [
                "14.12.2026",
                "14.12.2026, 09:04:05",
                "14. Dez. 2026",
                "14.12.2026, 09:04",
                "Dezember 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 1, 4, 15, 4, 5),
            [
                "4/1/2026",
                "4/1/2026, 15:04:05",
                "4 ene 2026",
                "4 ene 2026, 15:04",
                "enero de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 1, 14, 9, 4, 5),
            [
                "14/1/2026",
                "14/1/2026, 9:04:05",
                "14 ene 2026",
                "14 ene 2026, 9:04",
                "enero de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 2, 4, 15, 4, 5),
            [
                "4/2/2026",
                "4/2/2026, 15:04:05",
                "4 feb 2026",
                "4 feb 2026, 15:04",
                "febrero de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 2, 14, 9, 4, 5),
            [
                "14/2/2026",
                "14/2/2026, 9:04:05",
                "14 feb 2026",
                "14 feb 2026, 9:04",
                "febrero de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 3, 4, 15, 4, 5),
            [
                "4/3/2026",
                "4/3/2026, 15:04:05",
                "4 mar 2026",
                "4 mar 2026, 15:04",
                "marzo de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 3, 14, 9, 4, 5),
            [
                "14/3/2026",
                "14/3/2026, 9:04:05",
                "14 mar 2026",
                "14 mar 2026, 9:04",
                "marzo de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 4, 4, 15, 4, 5),
            [
                "4/4/2026",
                "4/4/2026, 15:04:05",
                "4 abr 2026",
                "4 abr 2026, 15:04",
                "abril de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 4, 14, 9, 4, 5),
            [
                "14/4/2026",
                "14/4/2026, 9:04:05",
                "14 abr 2026",
                "14 abr 2026, 9:04",
                "abril de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 5, 4, 15, 4, 5),
            [
                "4/5/2026",
                "4/5/2026, 15:04:05",
                "4 may 2026",
                "4 may 2026, 15:04",
                "mayo de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 5, 14, 9, 4, 5),
            [
                "14/5/2026",
                "14/5/2026, 9:04:05",
                "14 may 2026",
                "14 may 2026, 9:04",
                "mayo de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 6, 4, 15, 4, 5),
            [
                "4/6/2026",
                "4/6/2026, 15:04:05",
                "4 jun 2026",
                "4 jun 2026, 15:04",
                "junio de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 6, 14, 9, 4, 5),
            [
                "14/6/2026",
                "14/6/2026, 9:04:05",
                "14 jun 2026",
                "14 jun 2026, 9:04",
                "junio de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 7, 4, 15, 4, 5),
            [
                "4/7/2026",
                "4/7/2026, 15:04:05",
                "4 jul 2026",
                "4 jul 2026, 15:04",
                "julio de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 7, 14, 9, 4, 5),
            [
                "14/7/2026",
                "14/7/2026, 9:04:05",
                "14 jul 2026",
                "14 jul 2026, 9:04",
                "julio de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 8, 4, 15, 4, 5),
            [
                "4/8/2026",
                "4/8/2026, 15:04:05",
                "4 ago 2026",
                "4 ago 2026, 15:04",
                "agosto de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 8, 14, 9, 4, 5),
            [
                "14/8/2026",
                "14/8/2026, 9:04:05",
                "14 ago 2026",
                "14 ago 2026, 9:04",
                "agosto de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 9, 4, 15, 4, 5),
            [
                "4/9/2026",
                "4/9/2026, 15:04:05",
                "4 sept 2026",
                "4 sept 2026, 15:04",
                "septiembre de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 9, 14, 9, 4, 5),
            [
                "14/9/2026",
                "14/9/2026, 9:04:05",
                "14 sept 2026",
                "14 sept 2026, 9:04",
                "septiembre de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 10, 4, 15, 4, 5),
            [
                "4/10/2026",
                "4/10/2026, 15:04:05",
                "4 oct 2026",
                "4 oct 2026, 15:04",
                "octubre de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 10, 14, 9, 4, 5),
            [
                "14/10/2026",
                "14/10/2026, 9:04:05",
                "14 oct 2026",
                "14 oct 2026, 9:04",
                "octubre de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 11, 4, 15, 4, 5),
            [
                "4/11/2026",
                "4/11/2026, 15:04:05",
                "4 nov 2026",
                "4 nov 2026, 15:04",
                "noviembre de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 11, 14, 9, 4, 5),
            [
                "14/11/2026",
                "14/11/2026, 9:04:05",
                "14 nov 2026",
                "14 nov 2026, 9:04",
                "noviembre de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 12, 4, 15, 4, 5),
            [
                "4/12/2026",
                "4/12/2026, 15:04:05",
                "4 dic 2026",
                "4 dic 2026, 15:04",
                "diciembre de 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 12, 14, 9, 4, 5),
            [
                "14/12/2026",
                "14/12/2026, 9:04:05",
                "14 dic 2026",
                "14 dic 2026, 9:04",
                "diciembre de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 1, 4, 15, 4, 5),
            [
                "04/01/2026",
                "04/01/2026, 15:04:05",
                "4 de jan. de 2026",
                "4 de jan. de 2026, 15:04",
                "janeiro de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 1, 14, 9, 4, 5),
            [
                "14/01/2026",
                "14/01/2026, 09:04:05",
                "14 de jan. de 2026",
                "14 de jan. de 2026, 09:04",
                "janeiro de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 2, 4, 15, 4, 5),
            [
                "04/02/2026",
                "04/02/2026, 15:04:05",
                "4 de fev. de 2026",
                "4 de fev. de 2026, 15:04",
                "fevereiro de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 2, 14, 9, 4, 5),
            [
                "14/02/2026",
                "14/02/2026, 09:04:05",
                "14 de fev. de 2026",
                "14 de fev. de 2026, 09:04",
                "fevereiro de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 3, 4, 15, 4, 5),
            [
                "04/03/2026",
                "04/03/2026, 15:04:05",
                "4 de mar. de 2026",
                "4 de mar. de 2026, 15:04",
                "março de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 3, 14, 9, 4, 5),
            [
                "14/03/2026",
                "14/03/2026, 09:04:05",
                "14 de mar. de 2026",
                "14 de mar. de 2026, 09:04",
                "março de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 4, 4, 15, 4, 5),
            [
                "04/04/2026",
                "04/04/2026, 15:04:05",
                "4 de abr. de 2026",
                "4 de abr. de 2026, 15:04",
                "abril de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 4, 14, 9, 4, 5),
            [
                "14/04/2026",
                "14/04/2026, 09:04:05",
                "14 de abr. de 2026",
                "14 de abr. de 2026, 09:04",
                "abril de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 5, 4, 15, 4, 5),
            [
                "04/05/2026",
                "04/05/2026, 15:04:05",
                "4 de mai. de 2026",
                "4 de mai. de 2026, 15:04",
                "maio de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 5, 14, 9, 4, 5),
            [
                "14/05/2026",
                "14/05/2026, 09:04:05",
                "14 de mai. de 2026",
                "14 de mai. de 2026, 09:04",
                "maio de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 6, 4, 15, 4, 5),
            [
                "04/06/2026",
                "04/06/2026, 15:04:05",
                "4 de jun. de 2026",
                "4 de jun. de 2026, 15:04",
                "junho de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 6, 14, 9, 4, 5),
            [
                "14/06/2026",
                "14/06/2026, 09:04:05",
                "14 de jun. de 2026",
                "14 de jun. de 2026, 09:04",
                "junho de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 7, 4, 15, 4, 5),
            [
                "04/07/2026",
                "04/07/2026, 15:04:05",
                "4 de jul. de 2026",
                "4 de jul. de 2026, 15:04",
                "julho de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 7, 14, 9, 4, 5),
            [
                "14/07/2026",
                "14/07/2026, 09:04:05",
                "14 de jul. de 2026",
                "14 de jul. de 2026, 09:04",
                "julho de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 8, 4, 15, 4, 5),
            [
                "04/08/2026",
                "04/08/2026, 15:04:05",
                "4 de ago. de 2026",
                "4 de ago. de 2026, 15:04",
                "agosto de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 8, 14, 9, 4, 5),
            [
                "14/08/2026",
                "14/08/2026, 09:04:05",
                "14 de ago. de 2026",
                "14 de ago. de 2026, 09:04",
                "agosto de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 9, 4, 15, 4, 5),
            [
                "04/09/2026",
                "04/09/2026, 15:04:05",
                "4 de set. de 2026",
                "4 de set. de 2026, 15:04",
                "setembro de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 9, 14, 9, 4, 5),
            [
                "14/09/2026",
                "14/09/2026, 09:04:05",
                "14 de set. de 2026",
                "14 de set. de 2026, 09:04",
                "setembro de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 10, 4, 15, 4, 5),
            [
                "04/10/2026",
                "04/10/2026, 15:04:05",
                "4 de out. de 2026",
                "4 de out. de 2026, 15:04",
                "outubro de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 10, 14, 9, 4, 5),
            [
                "14/10/2026",
                "14/10/2026, 09:04:05",
                "14 de out. de 2026",
                "14 de out. de 2026, 09:04",
                "outubro de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 11, 4, 15, 4, 5),
            [
                "04/11/2026",
                "04/11/2026, 15:04:05",
                "4 de nov. de 2026",
                "4 de nov. de 2026, 15:04",
                "novembro de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 11, 14, 9, 4, 5),
            [
                "14/11/2026",
                "14/11/2026, 09:04:05",
                "14 de nov. de 2026",
                "14 de nov. de 2026, 09:04",
                "novembro de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 12, 4, 15, 4, 5),
            [
                "04/12/2026",
                "04/12/2026, 15:04:05",
                "4 de dez. de 2026",
                "4 de dez. de 2026, 15:04",
                "dezembro de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 12, 14, 9, 4, 5),
            [
                "14/12/2026",
                "14/12/2026, 09:04:05",
                "14 de dez. de 2026",
                "14 de dez. de 2026, 09:04",
                "dezembro de 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 1, 4, 15, 4, 5),
            [
                "04/01/2026",
                "04/01/2026 15:04:05",
                "4 janv. 2026",
                "4 janv. 2026, 15:04",
                "janvier 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 1, 14, 9, 4, 5),
            [
                "14/01/2026",
                "14/01/2026 09:04:05",
                "14 janv. 2026",
                "14 janv. 2026, 09:04",
                "janvier 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 2, 4, 15, 4, 5),
            [
                "04/02/2026",
                "04/02/2026 15:04:05",
                "4 févr. 2026",
                "4 févr. 2026, 15:04",
                "février 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 2, 14, 9, 4, 5),
            [
                "14/02/2026",
                "14/02/2026 09:04:05",
                "14 févr. 2026",
                "14 févr. 2026, 09:04",
                "février 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 3, 4, 15, 4, 5),
            [
                "04/03/2026",
                "04/03/2026 15:04:05",
                "4 mars 2026",
                "4 mars 2026, 15:04",
                "mars 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 3, 14, 9, 4, 5),
            [
                "14/03/2026",
                "14/03/2026 09:04:05",
                "14 mars 2026",
                "14 mars 2026, 09:04",
                "mars 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 4, 4, 15, 4, 5),
            [
                "04/04/2026",
                "04/04/2026 15:04:05",
                "4 avr. 2026",
                "4 avr. 2026, 15:04",
                "avril 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 4, 14, 9, 4, 5),
            [
                "14/04/2026",
                "14/04/2026 09:04:05",
                "14 avr. 2026",
                "14 avr. 2026, 09:04",
                "avril 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 5, 4, 15, 4, 5),
            [
                "04/05/2026",
                "04/05/2026 15:04:05",
                "4 mai 2026",
                "4 mai 2026, 15:04",
                "mai 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 5, 14, 9, 4, 5),
            [
                "14/05/2026",
                "14/05/2026 09:04:05",
                "14 mai 2026",
                "14 mai 2026, 09:04",
                "mai 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 6, 4, 15, 4, 5),
            [
                "04/06/2026",
                "04/06/2026 15:04:05",
                "4 juin 2026",
                "4 juin 2026, 15:04",
                "juin 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 6, 14, 9, 4, 5),
            [
                "14/06/2026",
                "14/06/2026 09:04:05",
                "14 juin 2026",
                "14 juin 2026, 09:04",
                "juin 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 7, 4, 15, 4, 5),
            [
                "04/07/2026",
                "04/07/2026 15:04:05",
                "4 juil. 2026",
                "4 juil. 2026, 15:04",
                "juillet 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 7, 14, 9, 4, 5),
            [
                "14/07/2026",
                "14/07/2026 09:04:05",
                "14 juil. 2026",
                "14 juil. 2026, 09:04",
                "juillet 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 8, 4, 15, 4, 5),
            [
                "04/08/2026",
                "04/08/2026 15:04:05",
                "4 août 2026",
                "4 août 2026, 15:04",
                "août 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 8, 14, 9, 4, 5),
            [
                "14/08/2026",
                "14/08/2026 09:04:05",
                "14 août 2026",
                "14 août 2026, 09:04",
                "août 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 9, 4, 15, 4, 5),
            [
                "04/09/2026",
                "04/09/2026 15:04:05",
                "4 sept. 2026",
                "4 sept. 2026, 15:04",
                "septembre 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 9, 14, 9, 4, 5),
            [
                "14/09/2026",
                "14/09/2026 09:04:05",
                "14 sept. 2026",
                "14 sept. 2026, 09:04",
                "septembre 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 10, 4, 15, 4, 5),
            [
                "04/10/2026",
                "04/10/2026 15:04:05",
                "4 oct. 2026",
                "4 oct. 2026, 15:04",
                "octobre 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 10, 14, 9, 4, 5),
            [
                "14/10/2026",
                "14/10/2026 09:04:05",
                "14 oct. 2026",
                "14 oct. 2026, 09:04",
                "octobre 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 11, 4, 15, 4, 5),
            [
                "04/11/2026",
                "04/11/2026 15:04:05",
                "4 nov. 2026",
                "4 nov. 2026, 15:04",
                "novembre 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 11, 14, 9, 4, 5),
            [
                "14/11/2026",
                "14/11/2026 09:04:05",
                "14 nov. 2026",
                "14 nov. 2026, 09:04",
                "novembre 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 12, 4, 15, 4, 5),
            [
                "04/12/2026",
                "04/12/2026 15:04:05",
                "4 déc. 2026",
                "4 déc. 2026, 15:04",
                "décembre 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 12, 14, 9, 4, 5),
            [
                "14/12/2026",
                "14/12/2026 09:04:05",
                "14 déc. 2026",
                "14 déc. 2026, 09:04",
                "décembre 2026",
            ],
        ),
        (
            Locale::Ja,
            (2026, 1, 4, 15, 4, 5),
            [
                "2026/1/4",
                "2026/1/4 15:04:05",
                "2026年1月4日",
                "2026/01/04 15:04",
                "2026年1月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 1, 14, 9, 4, 5),
            [
                "2026/1/14",
                "2026/1/14 9:04:05",
                "2026年1月14日",
                "2026/01/14 9:04",
                "2026年1月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 2, 4, 15, 4, 5),
            [
                "2026/2/4",
                "2026/2/4 15:04:05",
                "2026年2月4日",
                "2026/02/04 15:04",
                "2026年2月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 2, 14, 9, 4, 5),
            [
                "2026/2/14",
                "2026/2/14 9:04:05",
                "2026年2月14日",
                "2026/02/14 9:04",
                "2026年2月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 3, 4, 15, 4, 5),
            [
                "2026/3/4",
                "2026/3/4 15:04:05",
                "2026年3月4日",
                "2026/03/04 15:04",
                "2026年3月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 3, 14, 9, 4, 5),
            [
                "2026/3/14",
                "2026/3/14 9:04:05",
                "2026年3月14日",
                "2026/03/14 9:04",
                "2026年3月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 4, 4, 15, 4, 5),
            [
                "2026/4/4",
                "2026/4/4 15:04:05",
                "2026年4月4日",
                "2026/04/04 15:04",
                "2026年4月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 4, 14, 9, 4, 5),
            [
                "2026/4/14",
                "2026/4/14 9:04:05",
                "2026年4月14日",
                "2026/04/14 9:04",
                "2026年4月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 5, 4, 15, 4, 5),
            [
                "2026/5/4",
                "2026/5/4 15:04:05",
                "2026年5月4日",
                "2026/05/04 15:04",
                "2026年5月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 5, 14, 9, 4, 5),
            [
                "2026/5/14",
                "2026/5/14 9:04:05",
                "2026年5月14日",
                "2026/05/14 9:04",
                "2026年5月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 6, 4, 15, 4, 5),
            [
                "2026/6/4",
                "2026/6/4 15:04:05",
                "2026年6月4日",
                "2026/06/04 15:04",
                "2026年6月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 6, 14, 9, 4, 5),
            [
                "2026/6/14",
                "2026/6/14 9:04:05",
                "2026年6月14日",
                "2026/06/14 9:04",
                "2026年6月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 7, 4, 15, 4, 5),
            [
                "2026/7/4",
                "2026/7/4 15:04:05",
                "2026年7月4日",
                "2026/07/04 15:04",
                "2026年7月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 7, 14, 9, 4, 5),
            [
                "2026/7/14",
                "2026/7/14 9:04:05",
                "2026年7月14日",
                "2026/07/14 9:04",
                "2026年7月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 8, 4, 15, 4, 5),
            [
                "2026/8/4",
                "2026/8/4 15:04:05",
                "2026年8月4日",
                "2026/08/04 15:04",
                "2026年8月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 8, 14, 9, 4, 5),
            [
                "2026/8/14",
                "2026/8/14 9:04:05",
                "2026年8月14日",
                "2026/08/14 9:04",
                "2026年8月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 9, 4, 15, 4, 5),
            [
                "2026/9/4",
                "2026/9/4 15:04:05",
                "2026年9月4日",
                "2026/09/04 15:04",
                "2026年9月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 9, 14, 9, 4, 5),
            [
                "2026/9/14",
                "2026/9/14 9:04:05",
                "2026年9月14日",
                "2026/09/14 9:04",
                "2026年9月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 10, 4, 15, 4, 5),
            [
                "2026/10/4",
                "2026/10/4 15:04:05",
                "2026年10月4日",
                "2026/10/04 15:04",
                "2026年10月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 10, 14, 9, 4, 5),
            [
                "2026/10/14",
                "2026/10/14 9:04:05",
                "2026年10月14日",
                "2026/10/14 9:04",
                "2026年10月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 11, 4, 15, 4, 5),
            [
                "2026/11/4",
                "2026/11/4 15:04:05",
                "2026年11月4日",
                "2026/11/04 15:04",
                "2026年11月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 11, 14, 9, 4, 5),
            [
                "2026/11/14",
                "2026/11/14 9:04:05",
                "2026年11月14日",
                "2026/11/14 9:04",
                "2026年11月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 12, 4, 15, 4, 5),
            [
                "2026/12/4",
                "2026/12/4 15:04:05",
                "2026年12月4日",
                "2026/12/04 15:04",
                "2026年12月",
            ],
        ),
        (
            Locale::Ja,
            (2026, 12, 14, 9, 4, 5),
            [
                "2026/12/14",
                "2026/12/14 9:04:05",
                "2026年12月14日",
                "2026/12/14 9:04",
                "2026年12月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 1, 4, 15, 4, 5),
            [
                "2026/1/4",
                "2026/1/4 15:04:05",
                "2026年1月4日",
                "2026年1月4日 15:04",
                "2026年1月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 1, 14, 9, 4, 5),
            [
                "2026/1/14",
                "2026/1/14 09:04:05",
                "2026年1月14日",
                "2026年1月14日 09:04",
                "2026年1月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 2, 4, 15, 4, 5),
            [
                "2026/2/4",
                "2026/2/4 15:04:05",
                "2026年2月4日",
                "2026年2月4日 15:04",
                "2026年2月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 2, 14, 9, 4, 5),
            [
                "2026/2/14",
                "2026/2/14 09:04:05",
                "2026年2月14日",
                "2026年2月14日 09:04",
                "2026年2月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 3, 4, 15, 4, 5),
            [
                "2026/3/4",
                "2026/3/4 15:04:05",
                "2026年3月4日",
                "2026年3月4日 15:04",
                "2026年3月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 3, 14, 9, 4, 5),
            [
                "2026/3/14",
                "2026/3/14 09:04:05",
                "2026年3月14日",
                "2026年3月14日 09:04",
                "2026年3月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 4, 4, 15, 4, 5),
            [
                "2026/4/4",
                "2026/4/4 15:04:05",
                "2026年4月4日",
                "2026年4月4日 15:04",
                "2026年4月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 4, 14, 9, 4, 5),
            [
                "2026/4/14",
                "2026/4/14 09:04:05",
                "2026年4月14日",
                "2026年4月14日 09:04",
                "2026年4月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 5, 4, 15, 4, 5),
            [
                "2026/5/4",
                "2026/5/4 15:04:05",
                "2026年5月4日",
                "2026年5月4日 15:04",
                "2026年5月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 5, 14, 9, 4, 5),
            [
                "2026/5/14",
                "2026/5/14 09:04:05",
                "2026年5月14日",
                "2026年5月14日 09:04",
                "2026年5月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 6, 4, 15, 4, 5),
            [
                "2026/6/4",
                "2026/6/4 15:04:05",
                "2026年6月4日",
                "2026年6月4日 15:04",
                "2026年6月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 6, 14, 9, 4, 5),
            [
                "2026/6/14",
                "2026/6/14 09:04:05",
                "2026年6月14日",
                "2026年6月14日 09:04",
                "2026年6月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 7, 4, 15, 4, 5),
            [
                "2026/7/4",
                "2026/7/4 15:04:05",
                "2026年7月4日",
                "2026年7月4日 15:04",
                "2026年7月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 7, 14, 9, 4, 5),
            [
                "2026/7/14",
                "2026/7/14 09:04:05",
                "2026年7月14日",
                "2026年7月14日 09:04",
                "2026年7月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 8, 4, 15, 4, 5),
            [
                "2026/8/4",
                "2026/8/4 15:04:05",
                "2026年8月4日",
                "2026年8月4日 15:04",
                "2026年8月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 8, 14, 9, 4, 5),
            [
                "2026/8/14",
                "2026/8/14 09:04:05",
                "2026年8月14日",
                "2026年8月14日 09:04",
                "2026年8月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 9, 4, 15, 4, 5),
            [
                "2026/9/4",
                "2026/9/4 15:04:05",
                "2026年9月4日",
                "2026年9月4日 15:04",
                "2026年9月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 9, 14, 9, 4, 5),
            [
                "2026/9/14",
                "2026/9/14 09:04:05",
                "2026年9月14日",
                "2026年9月14日 09:04",
                "2026年9月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 10, 4, 15, 4, 5),
            [
                "2026/10/4",
                "2026/10/4 15:04:05",
                "2026年10月4日",
                "2026年10月4日 15:04",
                "2026年10月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 10, 14, 9, 4, 5),
            [
                "2026/10/14",
                "2026/10/14 09:04:05",
                "2026年10月14日",
                "2026年10月14日 09:04",
                "2026年10月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 11, 4, 15, 4, 5),
            [
                "2026/11/4",
                "2026/11/4 15:04:05",
                "2026年11月4日",
                "2026年11月4日 15:04",
                "2026年11月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 11, 14, 9, 4, 5),
            [
                "2026/11/14",
                "2026/11/14 09:04:05",
                "2026年11月14日",
                "2026年11月14日 09:04",
                "2026年11月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 12, 4, 15, 4, 5),
            [
                "2026/12/4",
                "2026/12/4 15:04:05",
                "2026年12月4日",
                "2026年12月4日 15:04",
                "2026年12月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 12, 14, 9, 4, 5),
            [
                "2026/12/14",
                "2026/12/14 09:04:05",
                "2026年12月14日",
                "2026年12月14日 09:04",
                "2026年12月",
            ],
        ),
        (
            Locale::Fi,
            (2026, 1, 4, 15, 4, 5),
            [
                "4.1.2026",
                "4.1.2026 klo 15.04.05",
                "4.1.2026",
                "4.1.2026 klo 15.04",
                "tammikuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 1, 14, 9, 4, 5),
            [
                "14.1.2026",
                "14.1.2026 klo 9.04.05",
                "14.1.2026",
                "14.1.2026 klo 9.04",
                "tammikuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 2, 4, 15, 4, 5),
            [
                "4.2.2026",
                "4.2.2026 klo 15.04.05",
                "4.2.2026",
                "4.2.2026 klo 15.04",
                "helmikuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 2, 14, 9, 4, 5),
            [
                "14.2.2026",
                "14.2.2026 klo 9.04.05",
                "14.2.2026",
                "14.2.2026 klo 9.04",
                "helmikuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 3, 4, 15, 4, 5),
            [
                "4.3.2026",
                "4.3.2026 klo 15.04.05",
                "4.3.2026",
                "4.3.2026 klo 15.04",
                "maaliskuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 3, 14, 9, 4, 5),
            [
                "14.3.2026",
                "14.3.2026 klo 9.04.05",
                "14.3.2026",
                "14.3.2026 klo 9.04",
                "maaliskuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 4, 4, 15, 4, 5),
            [
                "4.4.2026",
                "4.4.2026 klo 15.04.05",
                "4.4.2026",
                "4.4.2026 klo 15.04",
                "huhtikuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 4, 14, 9, 4, 5),
            [
                "14.4.2026",
                "14.4.2026 klo 9.04.05",
                "14.4.2026",
                "14.4.2026 klo 9.04",
                "huhtikuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 5, 4, 15, 4, 5),
            [
                "4.5.2026",
                "4.5.2026 klo 15.04.05",
                "4.5.2026",
                "4.5.2026 klo 15.04",
                "toukokuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 5, 14, 9, 4, 5),
            [
                "14.5.2026",
                "14.5.2026 klo 9.04.05",
                "14.5.2026",
                "14.5.2026 klo 9.04",
                "toukokuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 6, 4, 15, 4, 5),
            [
                "4.6.2026",
                "4.6.2026 klo 15.04.05",
                "4.6.2026",
                "4.6.2026 klo 15.04",
                "kesäkuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 6, 14, 9, 4, 5),
            [
                "14.6.2026",
                "14.6.2026 klo 9.04.05",
                "14.6.2026",
                "14.6.2026 klo 9.04",
                "kesäkuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 7, 4, 15, 4, 5),
            [
                "4.7.2026",
                "4.7.2026 klo 15.04.05",
                "4.7.2026",
                "4.7.2026 klo 15.04",
                "heinäkuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 7, 14, 9, 4, 5),
            [
                "14.7.2026",
                "14.7.2026 klo 9.04.05",
                "14.7.2026",
                "14.7.2026 klo 9.04",
                "heinäkuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 8, 4, 15, 4, 5),
            [
                "4.8.2026",
                "4.8.2026 klo 15.04.05",
                "4.8.2026",
                "4.8.2026 klo 15.04",
                "elokuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 8, 14, 9, 4, 5),
            [
                "14.8.2026",
                "14.8.2026 klo 9.04.05",
                "14.8.2026",
                "14.8.2026 klo 9.04",
                "elokuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 9, 4, 15, 4, 5),
            [
                "4.9.2026",
                "4.9.2026 klo 15.04.05",
                "4.9.2026",
                "4.9.2026 klo 15.04",
                "syyskuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 9, 14, 9, 4, 5),
            [
                "14.9.2026",
                "14.9.2026 klo 9.04.05",
                "14.9.2026",
                "14.9.2026 klo 9.04",
                "syyskuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 10, 4, 15, 4, 5),
            [
                "4.10.2026",
                "4.10.2026 klo 15.04.05",
                "4.10.2026",
                "4.10.2026 klo 15.04",
                "lokakuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 10, 14, 9, 4, 5),
            [
                "14.10.2026",
                "14.10.2026 klo 9.04.05",
                "14.10.2026",
                "14.10.2026 klo 9.04",
                "lokakuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 11, 4, 15, 4, 5),
            [
                "4.11.2026",
                "4.11.2026 klo 15.04.05",
                "4.11.2026",
                "4.11.2026 klo 15.04",
                "marraskuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 11, 14, 9, 4, 5),
            [
                "14.11.2026",
                "14.11.2026 klo 9.04.05",
                "14.11.2026",
                "14.11.2026 klo 9.04",
                "marraskuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 12, 4, 15, 4, 5),
            [
                "4.12.2026",
                "4.12.2026 klo 15.04.05",
                "4.12.2026",
                "4.12.2026 klo 15.04",
                "joulukuu 2026",
            ],
        ),
        (
            Locale::Fi,
            (2026, 12, 14, 9, 4, 5),
            [
                "14.12.2026",
                "14.12.2026 klo 9.04.05",
                "14.12.2026",
                "14.12.2026 klo 9.04",
                "joulukuu 2026",
            ],
        ),
    ] {
        let at = NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, mi, s)
            .unwrap();
        let day = at.date();
        assert_eq!(
            super::format::numeric_date_for_test(locale, day),
            date,
            "{locale:?}"
        );
        assert_eq!(format::date_time_in(locale, at), date_time, "{locale:?}");
        assert_eq!(
            format::short_month_date_in(locale, day),
            short_month,
            "{locale:?}"
        );
        assert_eq!(
            format::medium_date_time_in(locale, at),
            medium,
            "{locale:?}"
        );
        assert_eq!(format::month_year_in(locale, day), month_year, "{locale:?}");
    }
}

#[test]
fn relative_times_are_written_as_each_language_writes_them() {
    for (locale, amount, unit, expected) in [
        (Locale::En, -45, Unit::Minute, "45 minutes ago"),
        (Locale::En, -5, Unit::Minute, "5 minutes ago"),
        (Locale::En, -2, Unit::Minute, "2 minutes ago"),
        (Locale::En, -1, Unit::Minute, "1 minute ago"),
        (Locale::En, 0, Unit::Minute, "this minute"),
        (Locale::En, 1, Unit::Minute, "in 1 minute"),
        (Locale::En, 2, Unit::Minute, "in 2 minutes"),
        (Locale::En, 5, Unit::Minute, "in 5 minutes"),
        (Locale::En, 45, Unit::Minute, "in 45 minutes"),
        (Locale::En, -45, Unit::Hour, "45 hours ago"),
        (Locale::En, -5, Unit::Hour, "5 hours ago"),
        (Locale::En, -2, Unit::Hour, "2 hours ago"),
        (Locale::En, -1, Unit::Hour, "1 hour ago"),
        (Locale::En, 0, Unit::Hour, "this hour"),
        (Locale::En, 1, Unit::Hour, "in 1 hour"),
        (Locale::En, 2, Unit::Hour, "in 2 hours"),
        (Locale::En, 5, Unit::Hour, "in 5 hours"),
        (Locale::En, 45, Unit::Hour, "in 45 hours"),
        (Locale::En, -45, Unit::Day, "45 days ago"),
        (Locale::En, -5, Unit::Day, "5 days ago"),
        (Locale::En, -2, Unit::Day, "2 days ago"),
        (Locale::En, -1, Unit::Day, "yesterday"),
        (Locale::En, 0, Unit::Day, "today"),
        (Locale::En, 1, Unit::Day, "tomorrow"),
        (Locale::En, 2, Unit::Day, "in 2 days"),
        (Locale::En, 5, Unit::Day, "in 5 days"),
        (Locale::En, 45, Unit::Day, "in 45 days"),
        (Locale::En, -45, Unit::Month, "45 months ago"),
        (Locale::En, -5, Unit::Month, "5 months ago"),
        (Locale::En, -2, Unit::Month, "2 months ago"),
        (Locale::En, -1, Unit::Month, "last month"),
        (Locale::En, 0, Unit::Month, "this month"),
        (Locale::En, 1, Unit::Month, "next month"),
        (Locale::En, 2, Unit::Month, "in 2 months"),
        (Locale::En, 5, Unit::Month, "in 5 months"),
        (Locale::En, 45, Unit::Month, "in 45 months"),
        (Locale::De, -45, Unit::Minute, "vor 45 Minuten"),
        (Locale::De, -5, Unit::Minute, "vor 5 Minuten"),
        (Locale::De, -2, Unit::Minute, "vor 2 Minuten"),
        (Locale::De, -1, Unit::Minute, "vor 1 Minute"),
        (Locale::De, 0, Unit::Minute, "in dieser Minute"),
        (Locale::De, 1, Unit::Minute, "in 1 Minute"),
        (Locale::De, 2, Unit::Minute, "in 2 Minuten"),
        (Locale::De, 5, Unit::Minute, "in 5 Minuten"),
        (Locale::De, 45, Unit::Minute, "in 45 Minuten"),
        (Locale::De, -45, Unit::Hour, "vor 45 Stunden"),
        (Locale::De, -5, Unit::Hour, "vor 5 Stunden"),
        (Locale::De, -2, Unit::Hour, "vor 2 Stunden"),
        (Locale::De, -1, Unit::Hour, "vor 1 Stunde"),
        (Locale::De, 0, Unit::Hour, "in dieser Stunde"),
        (Locale::De, 1, Unit::Hour, "in 1 Stunde"),
        (Locale::De, 2, Unit::Hour, "in 2 Stunden"),
        (Locale::De, 5, Unit::Hour, "in 5 Stunden"),
        (Locale::De, 45, Unit::Hour, "in 45 Stunden"),
        (Locale::De, -45, Unit::Day, "vor 45 Tagen"),
        (Locale::De, -5, Unit::Day, "vor 5 Tagen"),
        (Locale::De, -2, Unit::Day, "vorgestern"),
        (Locale::De, -1, Unit::Day, "gestern"),
        (Locale::De, 0, Unit::Day, "heute"),
        (Locale::De, 1, Unit::Day, "morgen"),
        (Locale::De, 2, Unit::Day, "übermorgen"),
        (Locale::De, 5, Unit::Day, "in 5 Tagen"),
        (Locale::De, 45, Unit::Day, "in 45 Tagen"),
        (Locale::De, -45, Unit::Month, "vor 45 Monaten"),
        (Locale::De, -5, Unit::Month, "vor 5 Monaten"),
        (Locale::De, -2, Unit::Month, "vor 2 Monaten"),
        (Locale::De, -1, Unit::Month, "letzten Monat"),
        (Locale::De, 0, Unit::Month, "diesen Monat"),
        (Locale::De, 1, Unit::Month, "nächsten Monat"),
        (Locale::De, 2, Unit::Month, "in 2 Monaten"),
        (Locale::De, 5, Unit::Month, "in 5 Monaten"),
        (Locale::De, 45, Unit::Month, "in 45 Monaten"),
        (Locale::Es, -45, Unit::Minute, "hace 45 minutos"),
        (Locale::Es, -5, Unit::Minute, "hace 5 minutos"),
        (Locale::Es, -2, Unit::Minute, "hace 2 minutos"),
        (Locale::Es, -1, Unit::Minute, "hace 1 minuto"),
        (Locale::Es, 0, Unit::Minute, "este minuto"),
        (Locale::Es, 1, Unit::Minute, "dentro de 1 minuto"),
        (Locale::Es, 2, Unit::Minute, "dentro de 2 minutos"),
        (Locale::Es, 5, Unit::Minute, "dentro de 5 minutos"),
        (Locale::Es, 45, Unit::Minute, "dentro de 45 minutos"),
        (Locale::Es, -45, Unit::Hour, "hace 45 horas"),
        (Locale::Es, -5, Unit::Hour, "hace 5 horas"),
        (Locale::Es, -2, Unit::Hour, "hace 2 horas"),
        (Locale::Es, -1, Unit::Hour, "hace 1 hora"),
        (Locale::Es, 0, Unit::Hour, "esta hora"),
        (Locale::Es, 1, Unit::Hour, "dentro de 1 hora"),
        (Locale::Es, 2, Unit::Hour, "dentro de 2 horas"),
        (Locale::Es, 5, Unit::Hour, "dentro de 5 horas"),
        (Locale::Es, 45, Unit::Hour, "dentro de 45 horas"),
        (Locale::Es, -45, Unit::Day, "hace 45 días"),
        (Locale::Es, -5, Unit::Day, "hace 5 días"),
        (Locale::Es, -2, Unit::Day, "anteayer"),
        (Locale::Es, -1, Unit::Day, "ayer"),
        (Locale::Es, 0, Unit::Day, "hoy"),
        (Locale::Es, 1, Unit::Day, "mañana"),
        (Locale::Es, 2, Unit::Day, "pasado mañana"),
        (Locale::Es, 5, Unit::Day, "dentro de 5 días"),
        (Locale::Es, 45, Unit::Day, "dentro de 45 días"),
        (Locale::Es, -45, Unit::Month, "hace 45 meses"),
        (Locale::Es, -5, Unit::Month, "hace 5 meses"),
        (Locale::Es, -2, Unit::Month, "hace 2 meses"),
        (Locale::Es, -1, Unit::Month, "el mes pasado"),
        (Locale::Es, 0, Unit::Month, "este mes"),
        (Locale::Es, 1, Unit::Month, "el próximo mes"),
        (Locale::Es, 2, Unit::Month, "dentro de 2 meses"),
        (Locale::Es, 5, Unit::Month, "dentro de 5 meses"),
        (Locale::Es, 45, Unit::Month, "dentro de 45 meses"),
        (Locale::Pt, -45, Unit::Minute, "há 45 minutos"),
        (Locale::Pt, -5, Unit::Minute, "há 5 minutos"),
        (Locale::Pt, -2, Unit::Minute, "há 2 minutos"),
        (Locale::Pt, -1, Unit::Minute, "há 1 minuto"),
        (Locale::Pt, 0, Unit::Minute, "este minuto"),
        (Locale::Pt, 1, Unit::Minute, "em 1 minuto"),
        (Locale::Pt, 2, Unit::Minute, "em 2 minutos"),
        (Locale::Pt, 5, Unit::Minute, "em 5 minutos"),
        (Locale::Pt, 45, Unit::Minute, "em 45 minutos"),
        (Locale::Pt, -45, Unit::Hour, "há 45 horas"),
        (Locale::Pt, -5, Unit::Hour, "há 5 horas"),
        (Locale::Pt, -2, Unit::Hour, "há 2 horas"),
        (Locale::Pt, -1, Unit::Hour, "há 1 hora"),
        (Locale::Pt, 0, Unit::Hour, "esta hora"),
        (Locale::Pt, 1, Unit::Hour, "em 1 hora"),
        (Locale::Pt, 2, Unit::Hour, "em 2 horas"),
        (Locale::Pt, 5, Unit::Hour, "em 5 horas"),
        (Locale::Pt, 45, Unit::Hour, "em 45 horas"),
        (Locale::Pt, -45, Unit::Day, "há 45 dias"),
        (Locale::Pt, -5, Unit::Day, "há 5 dias"),
        (Locale::Pt, -2, Unit::Day, "anteontem"),
        (Locale::Pt, -1, Unit::Day, "ontem"),
        (Locale::Pt, 0, Unit::Day, "hoje"),
        (Locale::Pt, 1, Unit::Day, "amanhã"),
        (Locale::Pt, 2, Unit::Day, "depois de amanhã"),
        (Locale::Pt, 5, Unit::Day, "em 5 dias"),
        (Locale::Pt, 45, Unit::Day, "em 45 dias"),
        (Locale::Pt, -45, Unit::Month, "há 45 meses"),
        (Locale::Pt, -5, Unit::Month, "há 5 meses"),
        (Locale::Pt, -2, Unit::Month, "há 2 meses"),
        (Locale::Pt, -1, Unit::Month, "mês passado"),
        (Locale::Pt, 0, Unit::Month, "este mês"),
        (Locale::Pt, 1, Unit::Month, "próximo mês"),
        (Locale::Pt, 2, Unit::Month, "em 2 meses"),
        (Locale::Pt, 5, Unit::Month, "em 5 meses"),
        (Locale::Pt, 45, Unit::Month, "em 45 meses"),
        (Locale::Fr, -45, Unit::Minute, "il y a 45 minutes"),
        (Locale::Fr, -5, Unit::Minute, "il y a 5 minutes"),
        (Locale::Fr, -2, Unit::Minute, "il y a 2 minutes"),
        (Locale::Fr, -1, Unit::Minute, "il y a 1 minute"),
        (Locale::Fr, 0, Unit::Minute, "cette minute-ci"),
        (Locale::Fr, 1, Unit::Minute, "dans 1 minute"),
        (Locale::Fr, 2, Unit::Minute, "dans 2 minutes"),
        (Locale::Fr, 5, Unit::Minute, "dans 5 minutes"),
        (Locale::Fr, 45, Unit::Minute, "dans 45 minutes"),
        (Locale::Fr, -45, Unit::Hour, "il y a 45 heures"),
        (Locale::Fr, -5, Unit::Hour, "il y a 5 heures"),
        (Locale::Fr, -2, Unit::Hour, "il y a 2 heures"),
        (Locale::Fr, -1, Unit::Hour, "il y a 1 heure"),
        (Locale::Fr, 0, Unit::Hour, "cette heure-ci"),
        (Locale::Fr, 1, Unit::Hour, "dans 1 heure"),
        (Locale::Fr, 2, Unit::Hour, "dans 2 heures"),
        (Locale::Fr, 5, Unit::Hour, "dans 5 heures"),
        (Locale::Fr, 45, Unit::Hour, "dans 45 heures"),
        (Locale::Fr, -45, Unit::Day, "il y a 45 jours"),
        (Locale::Fr, -5, Unit::Day, "il y a 5 jours"),
        (Locale::Fr, -2, Unit::Day, "avant-hier"),
        (Locale::Fr, -1, Unit::Day, "hier"),
        (Locale::Fr, 0, Unit::Day, "aujourd’hui"),
        (Locale::Fr, 1, Unit::Day, "demain"),
        (Locale::Fr, 2, Unit::Day, "après-demain"),
        (Locale::Fr, 5, Unit::Day, "dans 5 jours"),
        (Locale::Fr, 45, Unit::Day, "dans 45 jours"),
        (Locale::Fr, -45, Unit::Month, "il y a 45 mois"),
        (Locale::Fr, -5, Unit::Month, "il y a 5 mois"),
        (Locale::Fr, -2, Unit::Month, "il y a 2 mois"),
        (Locale::Fr, -1, Unit::Month, "le mois dernier"),
        (Locale::Fr, 0, Unit::Month, "ce mois-ci"),
        (Locale::Fr, 1, Unit::Month, "le mois prochain"),
        (Locale::Fr, 2, Unit::Month, "dans 2 mois"),
        (Locale::Fr, 5, Unit::Month, "dans 5 mois"),
        (Locale::Fr, 45, Unit::Month, "dans 45 mois"),
        (Locale::Ja, -45, Unit::Minute, "45 分前"),
        (Locale::Ja, -5, Unit::Minute, "5 分前"),
        (Locale::Ja, -2, Unit::Minute, "2 分前"),
        (Locale::Ja, -1, Unit::Minute, "1 分前"),
        (Locale::Ja, 0, Unit::Minute, "1 分以内"),
        (Locale::Ja, 1, Unit::Minute, "1 分後"),
        (Locale::Ja, 2, Unit::Minute, "2 分後"),
        (Locale::Ja, 5, Unit::Minute, "5 分後"),
        (Locale::Ja, 45, Unit::Minute, "45 分後"),
        (Locale::Ja, -45, Unit::Hour, "45 時間前"),
        (Locale::Ja, -5, Unit::Hour, "5 時間前"),
        (Locale::Ja, -2, Unit::Hour, "2 時間前"),
        (Locale::Ja, -1, Unit::Hour, "1 時間前"),
        (Locale::Ja, 0, Unit::Hour, "1 時間以内"),
        (Locale::Ja, 1, Unit::Hour, "1 時間後"),
        (Locale::Ja, 2, Unit::Hour, "2 時間後"),
        (Locale::Ja, 5, Unit::Hour, "5 時間後"),
        (Locale::Ja, 45, Unit::Hour, "45 時間後"),
        (Locale::Ja, -45, Unit::Day, "45 日前"),
        (Locale::Ja, -5, Unit::Day, "5 日前"),
        (Locale::Ja, -2, Unit::Day, "一昨日"),
        (Locale::Ja, -1, Unit::Day, "昨日"),
        (Locale::Ja, 0, Unit::Day, "今日"),
        (Locale::Ja, 1, Unit::Day, "明日"),
        (Locale::Ja, 2, Unit::Day, "明後日"),
        (Locale::Ja, 5, Unit::Day, "5 日後"),
        (Locale::Ja, 45, Unit::Day, "45 日後"),
        (Locale::Ja, -45, Unit::Month, "45 か月前"),
        (Locale::Ja, -5, Unit::Month, "5 か月前"),
        (Locale::Ja, -2, Unit::Month, "2 か月前"),
        (Locale::Ja, -1, Unit::Month, "先月"),
        (Locale::Ja, 0, Unit::Month, "今月"),
        (Locale::Ja, 1, Unit::Month, "来月"),
        (Locale::Ja, 2, Unit::Month, "2 か月後"),
        (Locale::Ja, 5, Unit::Month, "5 か月後"),
        (Locale::Ja, 45, Unit::Month, "45 か月後"),
        (Locale::ZhCn, -45, Unit::Minute, "45分钟前"),
        (Locale::ZhCn, -5, Unit::Minute, "5分钟前"),
        (Locale::ZhCn, -2, Unit::Minute, "2分钟前"),
        (Locale::ZhCn, -1, Unit::Minute, "1分钟前"),
        (Locale::ZhCn, 0, Unit::Minute, "此刻"),
        (Locale::ZhCn, 1, Unit::Minute, "1分钟后"),
        (Locale::ZhCn, 2, Unit::Minute, "2分钟后"),
        (Locale::ZhCn, 5, Unit::Minute, "5分钟后"),
        (Locale::ZhCn, 45, Unit::Minute, "45分钟后"),
        (Locale::ZhCn, -45, Unit::Hour, "45小时前"),
        (Locale::ZhCn, -5, Unit::Hour, "5小时前"),
        (Locale::ZhCn, -2, Unit::Hour, "2小时前"),
        (Locale::ZhCn, -1, Unit::Hour, "1小时前"),
        (Locale::ZhCn, 0, Unit::Hour, "这一时间 / 此时"),
        (Locale::ZhCn, 1, Unit::Hour, "1小时后"),
        (Locale::ZhCn, 2, Unit::Hour, "2小时后"),
        (Locale::ZhCn, 5, Unit::Hour, "5小时后"),
        (Locale::ZhCn, 45, Unit::Hour, "45小时后"),
        (Locale::ZhCn, -45, Unit::Day, "45天前"),
        (Locale::ZhCn, -5, Unit::Day, "5天前"),
        (Locale::ZhCn, -2, Unit::Day, "前天"),
        (Locale::ZhCn, -1, Unit::Day, "昨天"),
        (Locale::ZhCn, 0, Unit::Day, "今天"),
        (Locale::ZhCn, 1, Unit::Day, "明天"),
        (Locale::ZhCn, 2, Unit::Day, "后天"),
        (Locale::ZhCn, 5, Unit::Day, "5天后"),
        (Locale::ZhCn, 45, Unit::Day, "45天后"),
        (Locale::ZhCn, -45, Unit::Month, "45个月前"),
        (Locale::ZhCn, -5, Unit::Month, "5个月前"),
        (Locale::ZhCn, -2, Unit::Month, "2个月前"),
        (Locale::ZhCn, -1, Unit::Month, "上个月"),
        (Locale::ZhCn, 0, Unit::Month, "本月"),
        (Locale::ZhCn, 1, Unit::Month, "下个月"),
        (Locale::ZhCn, 2, Unit::Month, "2个月后"),
        (Locale::ZhCn, 5, Unit::Month, "5个月后"),
        (Locale::ZhCn, 45, Unit::Month, "45个月后"),
        (Locale::Fi, -45, Unit::Minute, "45 minuuttia sitten"),
        (Locale::Fi, -5, Unit::Minute, "5 minuuttia sitten"),
        (Locale::Fi, -2, Unit::Minute, "2 minuuttia sitten"),
        (Locale::Fi, -1, Unit::Minute, "1 minuutti sitten"),
        (Locale::Fi, 0, Unit::Minute, "tämän minuutin aikana"),
        (Locale::Fi, 1, Unit::Minute, "1 minuutin päästä"),
        (Locale::Fi, 2, Unit::Minute, "2 minuutin päästä"),
        (Locale::Fi, 5, Unit::Minute, "5 minuutin päästä"),
        (Locale::Fi, 45, Unit::Minute, "45 minuutin päästä"),
        (Locale::Fi, -45, Unit::Hour, "45 tuntia sitten"),
        (Locale::Fi, -5, Unit::Hour, "5 tuntia sitten"),
        (Locale::Fi, -2, Unit::Hour, "2 tuntia sitten"),
        (Locale::Fi, -1, Unit::Hour, "1 tunti sitten"),
        (Locale::Fi, 0, Unit::Hour, "tämän tunnin aikana"),
        (Locale::Fi, 1, Unit::Hour, "1 tunnin päästä"),
        (Locale::Fi, 2, Unit::Hour, "2 tunnin päästä"),
        (Locale::Fi, 5, Unit::Hour, "5 tunnin päästä"),
        (Locale::Fi, 45, Unit::Hour, "45 tunnin päästä"),
        (Locale::Fi, -45, Unit::Day, "45 päivää sitten"),
        (Locale::Fi, -5, Unit::Day, "5 päivää sitten"),
        (Locale::Fi, -2, Unit::Day, "toissa päivänä"),
        (Locale::Fi, -1, Unit::Day, "eilen"),
        (Locale::Fi, 0, Unit::Day, "tänään"),
        (Locale::Fi, 1, Unit::Day, "huomenna"),
        (Locale::Fi, 2, Unit::Day, "ylihuomenna"),
        (Locale::Fi, 5, Unit::Day, "5 päivän päästä"),
        (Locale::Fi, 45, Unit::Day, "45 päivän päästä"),
        (Locale::Fi, -45, Unit::Month, "45 kuukautta sitten"),
        (Locale::Fi, -5, Unit::Month, "5 kuukautta sitten"),
        (Locale::Fi, -2, Unit::Month, "2 kuukautta sitten"),
        (Locale::Fi, -1, Unit::Month, "viime kuussa"),
        (Locale::Fi, 0, Unit::Month, "tässä kuussa"),
        (Locale::Fi, 1, Unit::Month, "ensi kuussa"),
        (Locale::Fi, 2, Unit::Month, "2 kuukauden päästä"),
        (Locale::Fi, 5, Unit::Month, "5 kuukauden päästä"),
        (Locale::Fi, 45, Unit::Month, "45 kuukauden päästä"),
    ] {
        assert_eq!(
            format::relative_in(locale, amount, unit),
            expected,
            "{locale:?} {amount} {unit:?}"
        );
    }
}
