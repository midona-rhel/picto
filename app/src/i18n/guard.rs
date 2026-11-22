//! A test that reads the source files of the app and fails when the app
//! would show a text in English in every language without a stated reason.
//!
//! It checks these things in the code outside tests:
//! - Every literal given to `t(…)` or `tf!(…)`, or to a shared control that
//!   translates what it is given (a menu row, a select's choice, a text
//!   field's placeholder), is a key of `locales/en.json`. A literal that is
//!   not a key has no translation.
//! - Every literal given to `en(…)`, which marks a text that has no key, is
//!   really not a key of `locales/en.json` and is listed in `gpui_only.txt`.
//!   Every line of `gpui_only.txt` is still used by an `en(…)` call.
//! - No literal that is a key of `locales/en.json` is used without `t(…)`.
//!   A literal is exempt when a comment `// i18n: <reason>` is on its line
//!   or on the line above. The reason says why the text is not translated
//!   there, for example "not shown". A comment
//!   `// i18n: translated where drawn` exempts the lines after it up to the
//!   next blank line. It is for a table whose labels are given to `t(…)`
//!   where they are drawn. A literal is also exempt when it names something
//!   and is not shown ([`identifier`]), and when its file is in `NOT_SHOWN`.
//! - No literal with letters is given directly to a call that draws it
//!   (`.child`, `.label`, `.tooltip`, a notification and the other calls in
//!   `SINKS`) without `t`, `tf!` or `en`. The exemptions of the point above
//!   apply here too.
//! - No `format!` writes a message that has a key. A `format!` template that
//!   is equal to a key once the placeholders are ignored (`{name}` in the
//!   template, `{value0}` in the key) must be written with `tf!` instead.
//!
//! The test does not find these: a text without a key that is built with
//! `format!` and then drawn, a literal without a key that is kept in a
//! variable or a table and drawn later, a literal given to a drawing call
//! that `SINKS` does not list, and a literal whose `// i18n:` comment gives
//! a reason that is not true.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The files that the test does not read, because no text in them has to be
/// given to `t(…)` there:
/// - `offscreen.rs`, which plays a script into the app and saves pictures of
///   its window, and `net_check.rs`, which checks the app's network features
///   in debug builds. Neither is part of the interface.
/// - This module's tests and the test itself.
/// - `format.rs` and `mod.rs` of this module, which hold the number and date
///   words of each language and the English plural suffix.
/// - The files of the application menu, whose rows are shown in English in
///   every language.
/// - The shortcut registry. Settings gives its labels, groups and
///   descriptions to `t(…)` where it draws them, and the names of the keys
///   are English in every language.
const NOT_SHOWN: [&str; 10] = [
    "offscreen.rs",
    "net_check.rs",
    "i18n/tests.rs",
    "i18n/guard.rs",
    "i18n/format.rs",
    "i18n/mod.rs",
    "app_menu/mod.rs",
    "app_menu/model.rs",
    "app_menu/presentation.rs",
    "settings/registry.rs",
];

/// The calls that draw their literal argument, or that give it to assistive
/// technology as an accessible name, description, value or placeholder.
const SINKS: [&str; 17] = [
    "tip(",
    ".child(",
    ".label(",
    ".tooltip(",
    ".title(",
    ".message(",
    ".description(",
    "notify::error(cx, ",
    "notify::warning(cx, ",
    "notify::info(cx, ",
    "notify::success(cx, ",
    "Tooltip::text(",
    ".detail(",
    ".aria_label(",
    ".aria_description(",
    ".aria_placeholder(",
    ".aria_value(",
];

/// Holds one string literal of a source file and the code around it.
struct Literal {
    text: String,
    line: usize,
    /// The code before the literal on its line, and the line before that.
    /// Comments are left out of it, and every other literal is written as an
    /// empty one.
    before: String,
    /// Whether an `// i18n:` comment covers the literal: one on its line or
    /// on the line above, or an `// i18n: translated where drawn` comment
    /// further up.
    marked: bool,
    /// The code after the literal on its line.
    after: String,
    /// The position at which `after` starts in the file's code, where the
    /// code has no comments and every literal is written as an empty one.
    code_end: usize,
}

/// Returns the string literals of a Rust source file that are outside
/// `#[cfg(test)]` items. Each literal says whether an `// i18n:` comment
/// covers it.
fn literals(source: &str) -> Vec<Literal> {
    let chars: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut code = String::new();
    let mut line = 1;
    let mut index = 0;
    // A `#[cfg(test)]` item is skipped, from the attribute to the end of the
    // item. The item ends at its closing brace at the same depth, or at its
    // semicolon.
    let mut skip_from: Option<(usize, bool)> = None;
    let mut depth = 0usize;
    let mut markers = BTreeSet::new();
    // The line of the last `// i18n: translated where drawn` comment, while
    // the lines after it are still covered. The comment covers the lines up
    // to the next blank line.
    let mut block_from: Option<usize> = None;
    let starts = |at: usize, text: &str| {
        text.chars()
            .enumerate()
            .all(|(offset, expected)| chars.get(at + offset) == Some(&expected))
    };
    while index < chars.len() {
        let c = chars[index];
        if c == '\n' {
            if let Some(from) = block_from {
                markers.insert(line);
                let blank = chars[index + 1..]
                    .iter()
                    .take_while(|ch| **ch != '\n')
                    .all(|ch| ch.is_whitespace());
                if blank && line > from {
                    block_from = None;
                }
            }
            line += 1;
            code.push('\n');
            index += 1;
            continue;
        }
        if starts(index, "//") {
            // A comment is left out of `code`. The line of an `// i18n:`
            // comment is recorded in `markers`, so that the literals on that
            // line and on the next are marked.
            let start = index;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            let comment: String = chars[start..index].iter().collect();
            if comment.contains("i18n: translated where drawn") {
                block_from = Some(line);
            } else if comment.contains("i18n: ") {
                markers.insert(line);
            }
            continue;
        }
        if starts(index, "/*") {
            index += 2;
            while index < chars.len() && !starts(index, "*/") {
                if chars[index] == '\n' {
                    line += 1;
                    code.push('\n');
                }
                index += 1;
            }
            index += 2;
            continue;
        }
        if starts(index, "#[cfg(test)]") && skip_from.is_none() {
            skip_from = Some((depth, false));
            index += "#[cfg(test)]".len();
            continue;
        }
        // A raw string is `r"…"`, `r#"…"#` or `br"…"`. A raw byte string is
        // never drawn, so it is skipped and not recorded.
        let raw_start = if starts(index, "br") {
            2
        } else if c == 'r' {
            1
        } else {
            0
        };
        let previous_is_ident =
            index > 0 && (chars[index - 1].is_alphanumeric() || chars[index - 1] == '_');
        if raw_start > 0 && !previous_is_ident {
            let mut i = index + raw_start;
            let mut hashes = 0;
            while chars.get(i) == Some(&'#') {
                hashes += 1;
                i += 1;
            }
            if chars.get(i) == Some(&'"') {
                let start_line = line;
                let mut text = String::new();
                i += 1;
                loop {
                    let Some(&ch) = chars.get(i) else { break };
                    if ch == '"' && (0..hashes).all(|h| chars.get(i + 1 + h) == Some(&'#')) {
                        i += 1 + hashes;
                        break;
                    }
                    if ch == '\n' {
                        line += 1;
                    }
                    text.push(ch);
                    i += 1;
                }
                if skip_from.is_none() && raw_start == 1 {
                    found.push(literal(text, start_line, &code));
                }
                code.push_str("\"\"");
                if let Some(last) = found.last_mut()
                    && last.code_end == usize::MAX
                {
                    last.code_end = code.len();
                }
                index = i;
                continue;
            }
        }
        if c == 'b' && chars.get(index + 1) == Some(&'"') && !previous_is_ident {
            // A byte string is never drawn, so it is skipped and not
            // recorded.
            index += 1;
            let mut i = index + 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                if chars.get(i) == Some(&'\n') {
                    line += 1;
                }
                i += 1;
            }
            code.push_str("\"\"");
            index = i + 1;
            continue;
        }
        if c == '"' {
            let start_line = line;
            let mut text = String::new();
            let mut i = index + 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                    match chars.get(i) {
                        Some('n') => text.push('\n'),
                        Some('t') => text.push('\t'),
                        Some('u') => {
                            // The escape `\u{…}` stands for one character.
                            let close = (i..chars.len()).find(|&j| chars[j] == '}').unwrap_or(i);
                            let hex: String = chars[i + 2..close].iter().collect();
                            if let Some(ch) =
                                u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                            {
                                text.push(ch);
                            }
                            i = close;
                        }
                        Some('\n') => {
                            // A backslash at the end of a line removes the
                            // line break and the indentation of the next
                            // line from the text.
                            line += 1;
                            while chars.get(i + 1).is_some_and(|ch| ch.is_whitespace()) {
                                i += 1;
                                if chars[i] == '\n' {
                                    line += 1;
                                }
                            }
                        }
                        Some(&other) => text.push(other),
                        None => {}
                    }
                    i += 1;
                    continue;
                }
                if chars[i] == '\n' {
                    line += 1;
                }
                text.push(chars[i]);
                i += 1;
            }
            if skip_from.is_none() {
                found.push(literal(text, start_line, &code));
            }
            code.push_str("\"\"");
            if let Some(last) = found.last_mut()
                && last.code_end == usize::MAX
            {
                last.code_end = code.len();
            }
            index = i + 1;
            continue;
        }
        if c == '\'' {
            // A quote starts a character literal (`'x'`, `'\''`, `'\u{…}'`),
            // which is written to `code` as `' '`. Any other quote starts a
            // lifetime and is kept as code.
            if chars.get(index + 1) == Some(&'\\') {
                let mut i = index + 2;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
                code.push_str("' '");
                index = i + 1;
                continue;
            }
            if chars.get(index + 2) == Some(&'\'') {
                code.push_str("' '");
                index += 3;
                continue;
            }
        }
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if let Some((at, true)) = skip_from
                    && depth == at
                {
                    skip_from = None;
                }
            }
            ';' => {
                if let Some((at, false)) = skip_from
                    && depth == at
                {
                    skip_from = None;
                }
            }
            _ => {}
        }
        if c == '{'
            && let Some((at, false)) = skip_from
            && depth == at + 1
        {
            skip_from = Some((at, true));
        }
        code.push(c);
        index += 1;
    }
    for literal in &mut found {
        let rest = &code[literal.code_end.min(code.len())..];
        literal.after = rest.split('\n').next().unwrap_or("").to_owned();
        literal.marked =
            markers.contains(&literal.line) || markers.contains(&(literal.line.saturating_sub(1)));
    }
    found
}

fn literal(text: String, line: usize, code: &str) -> Literal {
    // `before` holds the current line and the line before it, so that a
    // call whose literal is on the next line (`.child(` and then `"…"`) is
    // recognised.
    let mut lines = code.rsplit('\n');
    let current = lines.next().unwrap_or("");
    let previous = lines.next().unwrap_or("");
    Literal {
        text,
        line,
        before: format!("{previous}\n{current}"),
        marked: false,
        after: String::new(),
        code_end: usize::MAX,
    }
}

/// Says how a literal is used. `Translated` means it is given to `t` or
/// `tf!`, or to a shared control that translates it. `GpuiOnly` means it is
/// given to `en`. `Plain` means neither.
#[derive(Debug, PartialEq)]
enum Use {
    Translated,
    GpuiOnly,
    Plain,
}

fn usage(before: &str) -> Use {
    let code = before.trim_end();
    let called = |name: &str| {
        code.strip_suffix(name).is_some_and(|head| {
            !head
                .chars()
                .last()
                .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
        })
    };
    // A row of the shared menu, a choice of a select and the placeholder of
    // a text field translate the label they are given, so a label given to
    // them counts as translated.
    let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    // In `item(<action>, "…")` and `choice(<value>, "…")` the label is the
    // second argument. It is recognised when the first argument is a literal
    // or a path such as `Action::Rename`, with no call and no comma in it.
    let translating = ["item(", "choice("].iter().any(|callee| {
        compact.rfind(callee).is_some_and(|at| {
            let head_ok = !compact[..at]
                .chars()
                .last()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            let action = &compact[at + callee.len()..];
            head_ok
                && action.ends_with(',')
                && action.len() > 1
                && !action[..action.len() - 1].contains([',', '(', ')'])
        })
    }) || compact.ends_with("submenu(")
        || compact.ends_with(".placeholder(")
        || compact.ends_with(".set_placeholder(");
    if translating || called("t(") || called("tf!(") || called("t_with(") || called("i18n::t(") {
        Use::Translated
    } else if called("en(") || called("i18n::en(") {
        Use::GpuiOnly
    } else {
        Use::Plain
    }
}

/// Returns whether the literal is in a place where it names something and
/// is not shown: a match pattern, one side of a comparison, an icon's name,
/// or a value on a line with a `json!` call.
fn identifier(literal: &Literal) -> bool {
    let before = literal.before.trim_end();
    let after = literal.after.trim_start();
    after.starts_with("=>")
        || (after.starts_with('|') && !after.starts_with("||"))
        || (before.ends_with('|') && !before.ends_with("||"))
        || after.starts_with("==")
        || after.starts_with("!=")
        || before.ends_with("==")
        || before.ends_with("!=")
        || before.ends_with("icon(")
        || before
            .rsplit('\n')
            .next()
            .is_some_and(|line| line.contains("json!("))
        || before.ends_with("Some(") && (after.starts_with(") =>") || after.starts_with(") |"))
}

fn sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out.sort();
    out
}

/// Returns the files that are test modules: those declared as `mod x;` on
/// the line after `#[cfg(test)]`.
fn test_modules(files: &[PathBuf]) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    for file in files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        let lines: Vec<&str> = source.lines().collect();
        for pair in lines.windows(2) {
            let declared = pair[1].trim();
            if pair[0].trim() == "#[cfg(test)]"
                && let Some(name) = declared
                    .strip_prefix("mod ")
                    .or_else(|| declared.strip_prefix("pub mod "))
                    .and_then(|rest| rest.strip_suffix(';'))
            {
                let dir = if file
                    .file_name()
                    .is_some_and(|n| n == "mod.rs" || n == "main.rs")
                {
                    file.parent().unwrap().to_path_buf()
                } else {
                    file.with_extension("")
                };
                out.insert(dir.join(format!("{name}.rs")));
                out.insert(dir.join(name).join("mod.rs"));
            }
        }
    }
    out
}

fn gpui_only() -> BTreeSet<String> {
    include_str!("gpui_only.txt")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.replace("\\n", "\n"))
        .collect()
}

/// Holds what the test found. Each field holds the findings of one kind.
#[derive(Default)]
struct Findings {
    /// The literals given to `t` that are not keys of `locales/en.json`.
    unknown_keys: Vec<String>,
    /// The literals given to `en` that are keys of `locales/en.json`, or
    /// that `gpui_only.txt` does not list.
    unlisted: Vec<String>,
    /// The lines of `gpui_only.txt` that no `en(…)` call uses.
    stale: Vec<String>,
    /// The keys of `locales/en.json` that are used without `t`, for each
    /// file.
    untranslated: BTreeMap<String, Vec<String>>,
    /// The literals without a key that are drawn without `t` or `en`, for
    /// each file.
    drawn: BTreeMap<String, Vec<String>>,
    /// The `format!` templates that write a message that has a key.
    formatted: Vec<String>,
}

fn has_letters(text: &str) -> bool {
    text.chars().any(|c| c.is_alphabetic())
}

/// Returns `text` with each placeholder replaced by `{}`. A `format!`
/// template (`{name}`, `{}`, `{0:>3}`) and a key (`{value0}`) that write the
/// same message are then equal. `{{` and `}}` are kept as they are.
fn template(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push_str("{{");
            }
            '{' => {
                for inside in chars.by_ref() {
                    if inside == '}' {
                        break;
                    }
                }
                out.push_str("{}");
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push_str("}}");
            }
            other => out.push(other),
        }
    }
    out
}

/// Returns whether `template` has two letters in a row outside its
/// placeholders. A template that only joins values, such as `{}{}`,
/// `{} · {}` or `{}x`, has none and is not a message.
fn worded(template: &str) -> bool {
    let text: Vec<char> = template.replace("{}", " ").chars().collect();
    text.windows(2)
        .any(|pair| pair[0].is_alphabetic() && pair[1].is_alphabetic())
}

/// Returns the keys of `locales/en.json` that have a placeholder and a word,
/// each in the form that [`template`] returns.
fn key_templates() -> BTreeSet<String> {
    super::catalog(super::Locale::En)
        .keys()
        .filter(|key| key.contains('{'))
        .map(|key| template(key))
        .filter(|template| worded(template))
        .collect()
}

fn scan() -> Findings {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = sources();
    let tests = test_modules(&files);
    let listed = gpui_only();
    let templates = key_templates();
    let mut used = BTreeSet::new();
    let mut findings = Findings::default();
    for file in &files {
        let name = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if tests.contains(file) || NOT_SHOWN.contains(&name.as_str()) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        for literal in literals(&source) {
            let at = format!("{name}:{} {:?}", literal.line, literal.text);
            match usage(&literal.before) {
                Use::Translated => {
                    if !super::is_key(&literal.text) && has_letters(&literal.text) {
                        findings.unknown_keys.push(at);
                    }
                }
                Use::GpuiOnly => {
                    used.insert(literal.text.clone());
                    if super::is_key(&literal.text) || !listed.contains(&literal.text) {
                        findings.unlisted.push(at);
                    }
                }
                Use::Plain => {
                    if literal.marked || identifier(&literal) {
                        continue;
                    }
                    if literal.before.trim_end().ends_with("format!(")
                        && has_letters(&literal.text)
                        && templates.contains(&template(&literal.text))
                    {
                        findings.formatted.push(at);
                        continue;
                    }
                    let current = literal.before.rsplit('\n').next().unwrap_or("");
                    if super::is_key(&literal.text) && has_letters(&literal.text) {
                        findings
                            .untranslated
                            .entry(name.clone())
                            .or_default()
                            .push(at);
                    } else if has_letters(&literal.text)
                        && SINKS.iter().any(|sink| {
                            current.trim_end().ends_with(sink.trim_end())
                                || literal.before.trim_end().ends_with(sink.trim_end())
                        })
                    {
                        findings.drawn.entry(name.clone()).or_default().push(at);
                    }
                }
            }
        }
    }
    findings.stale = listed.difference(&used).cloned().collect();
    findings
}

#[test]
fn every_shown_text_goes_through_t() {
    let findings = scan();
    let mut report = String::new();
    let mut section = |title: &str, items: &[String]| {
        if !items.is_empty() {
            report.push_str(&format!("\n{title} ({}):\n", items.len()));
            for item in items {
                report.push_str(&format!("  {item}\n"));
            }
        }
    };
    section(
        "t() given a text that is not a key of en.json",
        &findings.unknown_keys,
    );
    section(
        "en() given a key, or a text not in gpui_only.txt",
        &findings.unlisted,
    );
    section("gpui_only.txt lines no longer used", &findings.stale);
    section(
        "keys of en.json rebuilt with format! (use tf!)",
        &findings.formatted,
    );
    let by_file = |map: &BTreeMap<String, Vec<String>>| -> Vec<String> {
        map.values().flatten().cloned().collect()
    };
    section(
        "keys of en.json left outside t()",
        &by_file(&findings.untranslated),
    );
    section("texts drawn without t() or en()", &by_file(&findings.drawn));
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (file, items) in findings.untranslated.iter().chain(&findings.drawn) {
        *counts.entry(file).or_default() += items.len();
    }
    if !counts.is_empty() {
        report.push_str("\nby file:\n");
        for (file, count) in &counts {
            report.push_str(&format!("  {count:5} {file}\n"));
        }
    }
    assert!(report.is_empty(), "{report}");
}

#[test]
fn a_format_template_is_compared_with_the_keys_by_its_placeholders() {
    assert_eq!(
        template("Delete \"{name}\" and {0:>3} of {}"),
        "Delete \"{}\" and {} of {}"
    );
    assert_eq!(template("{{literal}} {value0}"), "{{literal}} {}");
    let templates = key_templates();
    assert!(templates.contains(&template("Add to \u{201c}{name}\u{201d}")));
    assert!(!templates.contains(&template("Move {count} to Trash, at once")));
    // A template that only joins values is not a message.
    assert!(!templates.contains(&template("{rate}x")));
    assert!(!templates.contains(&template("{progress} · {left}")));
}

#[test]
fn the_reader_finds_literals_and_skips_tests() {
    let source = r##"
fn shown() {
    div().child("Visible").child(t("Settings"));
    let raw = r#"Raw "quoted""#;
    let c = '"';
    // "comment"
    let x = "a\"b"; // i18n: not shown
}
#[cfg(test)]
mod tests {
    fn hidden() { let _ = "Hidden"; }
}
fn after() { let _ = "After"; }
"##;
    let found: Vec<(String, usize)> = literals(source)
        .into_iter()
        .map(|literal| (literal.text, literal.line))
        .collect();
    assert_eq!(
        found,
        vec![
            ("Visible".to_owned(), 3),
            ("Settings".to_owned(), 3),
            ("Raw \"quoted\"".to_owned(), 4),
            ("a\"b".to_owned(), 7),
            ("After".to_owned(), 13),
        ]
    );
    let used: Vec<Use> = literals(source)
        .iter()
        .map(|literal| usage(&literal.before))
        .collect();
    assert_eq!(used[0], Use::Plain);
    assert_eq!(used[1], Use::Translated);
}
