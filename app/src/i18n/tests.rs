//! Tests of the catalogs, of the lookup and filling of messages, and of the
//! number and date formats. The numbers and dates that the tests expect are
//! what ICU 78 writes. They were taken from the ICU that Node.js includes.
use super::format::{self, Unit};
use super::*;
use chrono::NaiveDate;

/// Returns the `{name}` tokens of `text`, in order.
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
    // `translate` finds the English key of the German text and returns that
    // key's text in the language asked for.
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
        (Locale::En, 1000, "1,000"),
        (Locale::En, 10000, "10,000"),
        (Locale::En, 1234567, "1,234,567"),
        (Locale::En, -1234, "-1,234"),
        (Locale::De, 1000, "1.000"),
        (Locale::De, 10000, "10.000"),
        (Locale::De, 1234567, "1.234.567"),
        (Locale::De, -1234, "-1.234"),
        (Locale::Es, 1000, "1000"),
        (Locale::Es, 10000, "10.000"),
        (Locale::Es, 1234567, "1.234.567"),
        (Locale::Es, -1234, "-1234"),
        (Locale::Pt, 1000, "1.000"),
        (Locale::Pt, 10000, "10.000"),
        (Locale::Pt, 1234567, "1.234.567"),
        (Locale::Pt, -1234, "-1.234"),
        (Locale::Fr, 1000, "1\u{202f}000"),
        (Locale::Fr, 10000, "10\u{202f}000"),
        (Locale::Fr, 1234567, "1\u{202f}234\u{202f}567"),
        (Locale::Fr, -1234, "-1\u{202f}234"),
        (Locale::Ja, 1000, "1,000"),
        (Locale::Ja, 10000, "10,000"),
        (Locale::Ja, 1234567, "1,234,567"),
        (Locale::Ja, -1234, "-1,234"),
        (Locale::ZhCn, 1000, "1,000"),
        (Locale::ZhCn, 10000, "10,000"),
        (Locale::ZhCn, 1234567, "1,234,567"),
        (Locale::ZhCn, -1234, "-1,234"),
        (Locale::Fi, 1000, "1\u{a0}000"),
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
    for (locale, (y, m, d, h, mi, s), [date, short_month, medium, month_year]) in [
        (
            Locale::En,
            (2026, 1, 4, 15, 4, 5),
            [
                "1/4/2026",
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
                "Jan 14, 2026",
                "Jan 14, 2026, 9:04 AM",
                "January 2026",
            ],
        ),
        (
            Locale::De,
            (2026, 1, 4, 15, 4, 5),
            [
                "4.1.2026",
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
                "14. Jan. 2026",
                "14.01.2026, 09:04",
                "Januar 2026",
            ],
        ),
        (
            Locale::Es,
            (2026, 1, 4, 15, 4, 5),
            [
                "4/1/2026",
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
                "14 ene 2026",
                "14 ene 2026, 9:04",
                "enero de 2026",
            ],
        ),
        (
            Locale::Pt,
            (2026, 1, 4, 15, 4, 5),
            [
                "04/01/2026",
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
                "14 de jan. de 2026",
                "14 de jan. de 2026, 09:04",
                "janeiro de 2026",
            ],
        ),
        (
            Locale::Fr,
            (2026, 1, 4, 15, 4, 5),
            [
                "04/01/2026",
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
                "14 janv. 2026",
                "14 janv. 2026, 09:04",
                "janvier 2026",
            ],
        ),
        (
            Locale::Ja,
            (2026, 1, 4, 15, 4, 5),
            [
                "2026/1/4",
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
                "2026年1月14日",
                "2026/01/14 9:04",
                "2026年1月",
            ],
        ),
        (
            Locale::ZhCn,
            (2026, 1, 4, 15, 4, 5),
            [
                "2026/1/4",
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
                "2026年1月14日",
                "2026年1月14日 09:04",
                "2026年1月",
            ],
        ),
        (
            Locale::Fi,
            (2026, 1, 4, 15, 4, 5),
            [
                "4.1.2026",
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
                "14.1.2026",
                "14.1.2026 klo 9.04",
                "tammikuu 2026",
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
        (Locale::En, -1, Unit::Minute, "1 minute ago"),
        (Locale::En, 0, Unit::Minute, "this minute"),
        (Locale::En, 1, Unit::Minute, "in 1 minute"),
        (Locale::En, 2, Unit::Minute, "in 2 minutes"),
        (Locale::En, 5, Unit::Minute, "in 5 minutes"),
        (Locale::En, -1, Unit::Hour, "1 hour ago"),
        (Locale::En, 0, Unit::Hour, "this hour"),
        (Locale::En, -2, Unit::Day, "2 days ago"),
        (Locale::En, -1, Unit::Day, "yesterday"),
        (Locale::En, 0, Unit::Day, "today"),
        (Locale::En, 1, Unit::Day, "tomorrow"),
        (Locale::En, 2, Unit::Day, "in 2 days"),
        (Locale::En, -1, Unit::Month, "last month"),
        (Locale::En, 0, Unit::Month, "this month"),
        (Locale::En, 1, Unit::Month, "next month"),
        (Locale::De, -1, Unit::Minute, "vor 1 Minute"),
        (Locale::De, 0, Unit::Minute, "in dieser Minute"),
        (Locale::De, 1, Unit::Minute, "in 1 Minute"),
        (Locale::De, 2, Unit::Minute, "in 2 Minuten"),
        (Locale::De, 5, Unit::Minute, "in 5 Minuten"),
        (Locale::De, -1, Unit::Hour, "vor 1 Stunde"),
        (Locale::De, 0, Unit::Hour, "in dieser Stunde"),
        (Locale::De, -2, Unit::Day, "vorgestern"),
        (Locale::De, -1, Unit::Day, "gestern"),
        (Locale::De, 0, Unit::Day, "heute"),
        (Locale::De, 1, Unit::Day, "morgen"),
        (Locale::De, 2, Unit::Day, "übermorgen"),
        (Locale::De, -1, Unit::Month, "letzten Monat"),
        (Locale::De, 0, Unit::Month, "diesen Monat"),
        (Locale::De, 1, Unit::Month, "nächsten Monat"),
        (Locale::Es, -1, Unit::Minute, "hace 1 minuto"),
        (Locale::Es, 0, Unit::Minute, "este minuto"),
        (Locale::Es, 1, Unit::Minute, "dentro de 1 minuto"),
        (Locale::Es, 2, Unit::Minute, "dentro de 2 minutos"),
        (Locale::Es, 5, Unit::Minute, "dentro de 5 minutos"),
        (Locale::Es, -1, Unit::Hour, "hace 1 hora"),
        (Locale::Es, 0, Unit::Hour, "esta hora"),
        (Locale::Es, -2, Unit::Day, "anteayer"),
        (Locale::Es, -1, Unit::Day, "ayer"),
        (Locale::Es, 0, Unit::Day, "hoy"),
        (Locale::Es, 1, Unit::Day, "mañana"),
        (Locale::Es, 2, Unit::Day, "pasado mañana"),
        (Locale::Es, -1, Unit::Month, "el mes pasado"),
        (Locale::Es, 0, Unit::Month, "este mes"),
        (Locale::Es, 1, Unit::Month, "el próximo mes"),
        (Locale::Pt, -1, Unit::Minute, "há 1 minuto"),
        (Locale::Pt, 0, Unit::Minute, "este minuto"),
        (Locale::Pt, 1, Unit::Minute, "em 1 minuto"),
        (Locale::Pt, 2, Unit::Minute, "em 2 minutos"),
        (Locale::Pt, 5, Unit::Minute, "em 5 minutos"),
        (Locale::Pt, -1, Unit::Hour, "há 1 hora"),
        (Locale::Pt, 0, Unit::Hour, "esta hora"),
        (Locale::Pt, -2, Unit::Day, "anteontem"),
        (Locale::Pt, -1, Unit::Day, "ontem"),
        (Locale::Pt, 0, Unit::Day, "hoje"),
        (Locale::Pt, 1, Unit::Day, "amanhã"),
        (Locale::Pt, 2, Unit::Day, "depois de amanhã"),
        (Locale::Pt, -1, Unit::Month, "mês passado"),
        (Locale::Pt, 0, Unit::Month, "este mês"),
        (Locale::Pt, 1, Unit::Month, "próximo mês"),
        (Locale::Fr, -1, Unit::Minute, "il y a 1 minute"),
        (Locale::Fr, 0, Unit::Minute, "cette minute-ci"),
        (Locale::Fr, 1, Unit::Minute, "dans 1 minute"),
        (Locale::Fr, 2, Unit::Minute, "dans 2 minutes"),
        (Locale::Fr, 5, Unit::Minute, "dans 5 minutes"),
        (Locale::Fr, -1, Unit::Hour, "il y a 1 heure"),
        (Locale::Fr, 0, Unit::Hour, "cette heure-ci"),
        (Locale::Fr, -2, Unit::Day, "avant-hier"),
        (Locale::Fr, -1, Unit::Day, "hier"),
        (Locale::Fr, 0, Unit::Day, "aujourd’hui"),
        (Locale::Fr, 1, Unit::Day, "demain"),
        (Locale::Fr, 2, Unit::Day, "après-demain"),
        (Locale::Fr, -1, Unit::Month, "le mois dernier"),
        (Locale::Fr, 0, Unit::Month, "ce mois-ci"),
        (Locale::Fr, 1, Unit::Month, "le mois prochain"),
        (Locale::Ja, -1, Unit::Minute, "1 分前"),
        (Locale::Ja, 0, Unit::Minute, "1 分以内"),
        (Locale::Ja, 1, Unit::Minute, "1 分後"),
        (Locale::Ja, 2, Unit::Minute, "2 分後"),
        (Locale::Ja, 5, Unit::Minute, "5 分後"),
        (Locale::Ja, -1, Unit::Hour, "1 時間前"),
        (Locale::Ja, 0, Unit::Hour, "1 時間以内"),
        (Locale::Ja, -2, Unit::Day, "一昨日"),
        (Locale::Ja, -1, Unit::Day, "昨日"),
        (Locale::Ja, 0, Unit::Day, "今日"),
        (Locale::Ja, 1, Unit::Day, "明日"),
        (Locale::Ja, 2, Unit::Day, "明後日"),
        (Locale::Ja, -1, Unit::Month, "先月"),
        (Locale::Ja, 0, Unit::Month, "今月"),
        (Locale::Ja, 1, Unit::Month, "来月"),
        (Locale::ZhCn, -1, Unit::Minute, "1分钟前"),
        (Locale::ZhCn, 0, Unit::Minute, "此刻"),
        (Locale::ZhCn, 1, Unit::Minute, "1分钟后"),
        (Locale::ZhCn, 2, Unit::Minute, "2分钟后"),
        (Locale::ZhCn, 5, Unit::Minute, "5分钟后"),
        (Locale::ZhCn, -1, Unit::Hour, "1小时前"),
        (Locale::ZhCn, 0, Unit::Hour, "这一时间 / 此时"),
        (Locale::ZhCn, -2, Unit::Day, "前天"),
        (Locale::ZhCn, -1, Unit::Day, "昨天"),
        (Locale::ZhCn, 0, Unit::Day, "今天"),
        (Locale::ZhCn, 1, Unit::Day, "明天"),
        (Locale::ZhCn, 2, Unit::Day, "后天"),
        (Locale::ZhCn, -1, Unit::Month, "上个月"),
        (Locale::ZhCn, 0, Unit::Month, "本月"),
        (Locale::ZhCn, 1, Unit::Month, "下个月"),
        (Locale::Fi, -1, Unit::Minute, "1 minuutti sitten"),
        (Locale::Fi, 0, Unit::Minute, "tämän minuutin aikana"),
        (Locale::Fi, 1, Unit::Minute, "1 minuutin päästä"),
        (Locale::Fi, 2, Unit::Minute, "2 minuutin päästä"),
        (Locale::Fi, 5, Unit::Minute, "5 minuutin päästä"),
        (Locale::Fi, -1, Unit::Hour, "1 tunti sitten"),
        (Locale::Fi, 0, Unit::Hour, "tämän tunnin aikana"),
        (Locale::Fi, -2, Unit::Day, "toissa päivänä"),
        (Locale::Fi, -1, Unit::Day, "eilen"),
        (Locale::Fi, 0, Unit::Day, "tänään"),
        (Locale::Fi, 1, Unit::Day, "huomenna"),
        (Locale::Fi, 2, Unit::Day, "ylihuomenna"),
        (Locale::Fi, -1, Unit::Month, "viime kuussa"),
        (Locale::Fi, 0, Unit::Month, "tässä kuussa"),
        (Locale::Fi, 1, Unit::Month, "ensi kuussa"),
    ] {
        assert_eq!(
            format::relative_in(locale, amount, unit),
            expected,
            "{locale:?} {amount} {unit:?}"
        );
    }
}
