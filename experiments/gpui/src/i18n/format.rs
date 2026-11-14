//! Numbers and dates as production writes them: `toLocaleString()`,
//! `toLocaleDateString()`, `Intl.DateTimeFormat` and
//! `Intl.RelativeTimeFormat`, all without a locale, so in the language
//! Chromium takes from the system (`navigator.language`), whatever the
//! interface's language. The patterns are ICU's for the eight interface
//! languages (checked against Node's ICU 78 in the tests); a system language
//! outside them is written as American English, and regions are not told
//! apart (British English writes as American, Portugal's Portuguese as
//! Brazil's).
use super::Locale;
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use std::sync::LazyLock;

/// The language numbers and dates are written in.
static SYSTEM: LazyLock<Locale> = LazyLock::new(|| {
    sys_locale::get_locale()
        .as_deref()
        .and_then(Locale::normalize)
        .unwrap_or(Locale::En)
});

pub fn system() -> Locale {
    *SYSTEM
}

/// `count.toLocaleString()`.
pub fn grouped(count: i128) -> String {
    grouped_in(system(), count)
}

pub fn grouped_in(locale: Locale, count: i128) -> String {
    let separator = match locale {
        Locale::En | Locale::Ja | Locale::ZhCn => ",",
        Locale::De | Locale::Es | Locale::Pt => ".",
        Locale::Fr => "\u{202f}",
        Locale::Fi => "\u{a0}",
    };
    let digits = count.unsigned_abs().to_string();
    // Spanish groups only from five digits on.
    let groups = !(locale == Locale::Es && digits.len() < 5);
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 * 3 + 3);
    if count < 0 {
        out.push(if locale == Locale::Fi {
            '\u{2212}'
        } else {
            '-'
        });
    }
    for (index, digit) in digits.chars().enumerate() {
        if groups && index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push_str(separator);
        }
        out.push(digit);
    }
    out
}

/// `{ month: 'short' }` within a date, January first.
fn short_month(locale: Locale, month: u32) -> &'static str {
    const EN: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const DE: [&str; 12] = [
        "Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.",
        "Dez.",
    ];
    const ES: [&str; 12] = [
        "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic",
    ];
    const PT: [&str; 12] = [
        "jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.",
        "dez.",
    ];
    const FR: [&str; 12] = [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
        "déc.",
    ];
    let names = match locale {
        Locale::De => &DE,
        Locale::Es => &ES,
        Locale::Pt => &PT,
        Locale::Fr => &FR,
        _ => &EN,
    };
    names[(month as usize).saturating_sub(1).min(11)]
}

/// `{ month: 'long' }` beside a year, January first.
fn long_month(locale: Locale, month: u32) -> &'static str {
    const EN: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    const DE: [&str; 12] = [
        "Januar",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ];
    const ES: [&str; 12] = [
        "enero",
        "febrero",
        "marzo",
        "abril",
        "mayo",
        "junio",
        "julio",
        "agosto",
        "septiembre",
        "octubre",
        "noviembre",
        "diciembre",
    ];
    const PT: [&str; 12] = [
        "janeiro",
        "fevereiro",
        "março",
        "abril",
        "maio",
        "junho",
        "julho",
        "agosto",
        "setembro",
        "outubro",
        "novembro",
        "dezembro",
    ];
    const FR: [&str; 12] = [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ];
    const FI: [&str; 12] = [
        "tammikuu",
        "helmikuu",
        "maaliskuu",
        "huhtikuu",
        "toukokuu",
        "kesäkuu",
        "heinäkuu",
        "elokuu",
        "syyskuu",
        "lokakuu",
        "marraskuu",
        "joulukuu",
    ];
    let names = match locale {
        Locale::De => &DE,
        Locale::Es => &ES,
        Locale::Pt => &PT,
        Locale::Fr => &FR,
        Locale::Fi => &FI,
        _ => &EN,
    };
    names[(month as usize).saturating_sub(1).min(11)]
}

/// The numeric date `toLocaleDateString()` and `toLocaleString()` write.
fn numeric_date(locale: Locale, date: NaiveDate) -> String {
    let (y, m, d) = (date.year(), date.month(), date.day());
    match locale {
        Locale::En => format!("{m}/{d}/{y}"),
        Locale::De | Locale::Fi => format!("{d}.{m}.{y}"),
        Locale::Es => format!("{d}/{m}/{y}"),
        Locale::Pt | Locale::Fr => format!("{d:02}/{m:02}/{y}"),
        Locale::Ja | Locale::ZhCn => format!("{y}/{m}/{d}"),
    }
}

/// A time of day, with or without seconds.
fn time(locale: Locale, time: NaiveDateTime, seconds: bool) -> String {
    let (h, mi, s) = (time.hour(), time.minute(), time.second());
    let tail = |separator: char| {
        if seconds {
            format!("{separator}{mi:02}{separator}{s:02}")
        } else {
            format!("{separator}{mi:02}")
        }
    };
    match locale {
        Locale::En => {
            let hour = if h % 12 == 0 { 12 } else { h % 12 };
            let half = if h < 12 { "AM" } else { "PM" };
            format!("{hour}{} {half}", tail(':'))
        }
        Locale::Es | Locale::Ja => format!("{h}{}", tail(':')),
        Locale::Fi => format!("{h}{}", tail('.')),
        Locale::De | Locale::Pt | Locale::Fr | Locale::ZhCn => format!("{h:02}{}", tail(':')),
    }
}

/// What joins a date and a time.
fn joiner(locale: Locale) -> &'static str {
    match locale {
        Locale::En | Locale::De | Locale::Es | Locale::Pt => ", ",
        Locale::Fr | Locale::Ja | Locale::ZhCn => " ",
        Locale::Fi => " klo ",
    }
}

/// `date.toLocaleDateString()`.
pub fn date(date: NaiveDate) -> String {
    numeric_date(system(), date)
}

/// `date.toLocaleString()`: the date, then the time with seconds.
pub fn date_time(value: NaiveDateTime) -> String {
    date_time_in(system(), value)
}

pub fn date_time_in(locale: Locale, value: NaiveDateTime) -> String {
    format!(
        "{}{}{}",
        numeric_date(locale, value.date()),
        joiner(locale),
        time(locale, value, true)
    )
}

/// `toLocaleDateString(undefined, { year: 'numeric', month: 'short', day:
/// 'numeric' })`.
pub fn short_month_date(date: NaiveDate) -> String {
    short_month_date_in(system(), date)
}

pub fn short_month_date_in(locale: Locale, date: NaiveDate) -> String {
    let (y, m, d) = (date.year(), date.month(), date.day());
    let month = short_month(locale, m);
    match locale {
        Locale::En => format!("{month} {d}, {y}"),
        Locale::De => format!("{d}. {month} {y}"),
        Locale::Es | Locale::Fr => format!("{d} {month} {y}"),
        Locale::Pt => format!("{d} de {month} de {y}"),
        Locale::Ja | Locale::ZhCn => format!("{y}年{m}月{d}日"),
        Locale::Fi => format!("{d}.{m}.{y}"),
    }
}

/// `Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle:
/// 'short' })`.
pub fn medium_date_time(value: NaiveDateTime) -> String {
    medium_date_time_in(system(), value)
}

pub fn medium_date_time_in(locale: Locale, value: NaiveDateTime) -> String {
    let (y, m, d) = (value.year(), value.month(), value.day());
    let date = match locale {
        Locale::De => format!("{d:02}.{m:02}.{y}"),
        Locale::Ja => format!("{y}/{m:02}/{d:02}"),
        Locale::Fi => format!("{d}.{m}.{y}"),
        _ => short_month_date_in(locale, value.date()),
    };
    let join = match locale {
        Locale::Fr => ", ",
        other => joiner(other),
    };
    format!("{date}{join}{}", time(locale, value, false))
}

/// `toLocaleDateString(undefined, { month: 'long', year: 'numeric' })`.
pub fn month_year(date: NaiveDate) -> String {
    month_year_in(system(), date)
}

pub fn month_year_in(locale: Locale, date: NaiveDate) -> String {
    let (y, m) = (date.year(), date.month());
    let month = long_month(locale, m);
    match locale {
        Locale::Es | Locale::Pt => format!("{month} de {y}"),
        Locale::Ja | Locale::ZhCn => format!("{y}年{m}月"),
        _ => format!("{month} {y}"),
    }
}

/// A unit `Intl.RelativeTimeFormat` is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Minute,
    Hour,
    Day,
    Month,
}

/// `new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' }).format(amount,
/// unit)` for a whole amount.
pub fn relative(amount: i64, unit: Unit) -> String {
    relative_in(system(), amount, unit)
}

pub fn relative_in(locale: Locale, amount: i64, unit: Unit) -> String {
    if let Some(word) = relative_word(locale, amount, unit) {
        return word.to_owned();
    }
    let one = amount.abs() == 1;
    let n = grouped_in(locale, amount.abs() as i128);
    let past = amount < 0;
    use Unit::*;
    match locale {
        Locale::En => {
            let name = match unit {
                Minute => "minute",
                Hour => "hour",
                Day => "day",
                Month => "month",
            };
            let s = if one { "" } else { "s" };
            if past {
                format!("{n} {name}{s} ago")
            } else {
                format!("in {n} {name}{s}")
            }
        }
        Locale::De => {
            let name = match (unit, one, past) {
                (Minute, true, _) => "Minute",
                (Minute, false, _) => "Minuten",
                (Hour, true, _) => "Stunde",
                (Hour, false, _) => "Stunden",
                (Day, true, _) => "Tag",
                (Day, false, true) => "Tagen",
                (Day, false, false) => "Tagen",
                (Month, true, _) => "Monat",
                (Month, false, _) => "Monaten",
            };
            if past {
                format!("vor {n} {name}")
            } else {
                format!("in {n} {name}")
            }
        }
        Locale::Es => {
            let name = match (unit, one) {
                (Minute, true) => "minuto",
                (Minute, false) => "minutos",
                (Hour, true) => "hora",
                (Hour, false) => "horas",
                (Day, true) => "día",
                (Day, false) => "días",
                (Month, true) => "mes",
                (Month, false) => "meses",
            };
            if past {
                format!("hace {n} {name}")
            } else {
                format!("dentro de {n} {name}")
            }
        }
        Locale::Pt => {
            let name = match (unit, one) {
                (Minute, true) => "minuto",
                (Minute, false) => "minutos",
                (Hour, true) => "hora",
                (Hour, false) => "horas",
                (Day, true) => "dia",
                (Day, false) => "dias",
                (Month, true) => "mês",
                (Month, false) => "meses",
            };
            if past {
                format!("há {n} {name}")
            } else {
                format!("em {n} {name}")
            }
        }
        Locale::Fr => {
            let name = match (unit, one) {
                (Minute, true) => "minute",
                (Minute, false) => "minutes",
                (Hour, true) => "heure",
                (Hour, false) => "heures",
                (Day, true) => "jour",
                (Day, false) => "jours",
                (Month, _) => "mois",
            };
            if past {
                format!("il y a {n} {name}")
            } else {
                format!("dans {n} {name}")
            }
        }
        Locale::Ja => {
            let name = match unit {
                Minute => "分",
                Hour => "時間",
                Day => "日",
                Month => "か月",
            };
            format!("{n} {name}{}", if past { "前" } else { "後" })
        }
        Locale::ZhCn => {
            let name = match unit {
                Minute => "分钟",
                Hour => "小时",
                Day => "天",
                Month => "个月",
            };
            format!("{n}{name}{}", if past { "前" } else { "后" })
        }
        Locale::Fi => {
            if past {
                let name = match (unit, one) {
                    (Minute, true) => "minuutti",
                    (Minute, false) => "minuuttia",
                    (Hour, true) => "tunti",
                    (Hour, false) => "tuntia",
                    (Day, true) => "päivä",
                    (Day, false) => "päivää",
                    (Month, true) => "kuukausi",
                    (Month, false) => "kuukautta",
                };
                format!("{n} {name} sitten")
            } else {
                let name = match unit {
                    Minute => "minuutin",
                    Hour => "tunnin",
                    Day => "päivän",
                    Month => "kuukauden",
                };
                format!("{n} {name} päästä")
            }
        }
    }
}

/// The words `numeric: 'auto'` uses instead of a number.
fn relative_word(locale: Locale, amount: i64, unit: Unit) -> Option<&'static str> {
    use Unit::*;
    let words: &[(i64, Unit, &str)] = match locale {
        Locale::En => &[
            (0, Minute, "this minute"),
            (0, Hour, "this hour"),
            (-1, Day, "yesterday"),
            (0, Day, "today"),
            (1, Day, "tomorrow"),
            (-1, Month, "last month"),
            (0, Month, "this month"),
            (1, Month, "next month"),
        ],
        Locale::De => &[
            (0, Minute, "in dieser Minute"),
            (0, Hour, "in dieser Stunde"),
            (-2, Day, "vorgestern"),
            (-1, Day, "gestern"),
            (0, Day, "heute"),
            (1, Day, "morgen"),
            (2, Day, "übermorgen"),
            (-1, Month, "letzten Monat"),
            (0, Month, "diesen Monat"),
            (1, Month, "nächsten Monat"),
        ],
        Locale::Es => &[
            (0, Minute, "este minuto"),
            (0, Hour, "esta hora"),
            (-2, Day, "anteayer"),
            (-1, Day, "ayer"),
            (0, Day, "hoy"),
            (1, Day, "mañana"),
            (2, Day, "pasado mañana"),
            (-1, Month, "el mes pasado"),
            (0, Month, "este mes"),
            (1, Month, "el próximo mes"),
        ],
        Locale::Pt => &[
            (0, Minute, "este minuto"),
            (0, Hour, "esta hora"),
            (-2, Day, "anteontem"),
            (-1, Day, "ontem"),
            (0, Day, "hoje"),
            (1, Day, "amanhã"),
            (2, Day, "depois de amanhã"),
            (-1, Month, "mês passado"),
            (0, Month, "este mês"),
            (1, Month, "próximo mês"),
        ],
        Locale::Fr => &[
            (0, Minute, "cette minute-ci"),
            (0, Hour, "cette heure-ci"),
            (-2, Day, "avant-hier"),
            (-1, Day, "hier"),
            (0, Day, "aujourd’hui"),
            (1, Day, "demain"),
            (2, Day, "après-demain"),
            (-1, Month, "le mois dernier"),
            (0, Month, "ce mois-ci"),
            (1, Month, "le mois prochain"),
        ],
        Locale::Ja => &[
            (0, Minute, "1 分以内"),
            (0, Hour, "1 時間以内"),
            (-2, Day, "一昨日"),
            (-1, Day, "昨日"),
            (0, Day, "今日"),
            (1, Day, "明日"),
            (2, Day, "明後日"),
            (-1, Month, "先月"),
            (0, Month, "今月"),
            (1, Month, "来月"),
        ],
        Locale::ZhCn => &[
            (0, Minute, "此刻"),
            (0, Hour, "这一时间 / 此时"),
            (-2, Day, "前天"),
            (-1, Day, "昨天"),
            (0, Day, "今天"),
            (1, Day, "明天"),
            (2, Day, "后天"),
            (-1, Month, "上个月"),
            (0, Month, "本月"),
            (1, Month, "下个月"),
        ],
        Locale::Fi => &[
            (0, Minute, "tämän minuutin aikana"),
            (0, Hour, "tämän tunnin aikana"),
            (-2, Day, "toissa päivänä"),
            (-1, Day, "eilen"),
            (0, Day, "tänään"),
            (1, Day, "huomenna"),
            (2, Day, "ylihuomenna"),
            (-1, Month, "viime kuussa"),
            (0, Month, "tässä kuussa"),
            (1, Month, "ensi kuussa"),
        ],
    };
    words
        .iter()
        .find(|(at, of, _)| *at == amount && *of == unit)
        .map(|(_, _, word)| *word)
}

#[cfg(test)]
pub(super) fn numeric_date_for_test(locale: Locale, date: NaiveDate) -> String {
    numeric_date(locale, date)
}
