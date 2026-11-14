//! Keeps the interface translated: reads the crate's sources and fails on
//! text that would show in English whatever the language.
//!
//! What it checks, in the code outside tests:
//! - every literal given to `t(…)` or `tf!(…)` is one of production's keys
//!   (`en.json`), so the lookup cannot miss;
//! - every literal given to `en(…)`, the mark of a text only this app has,
//!   is not a key (it would be translatable) and is listed in
//!   `gpui_only.txt`, and every line there is still used;
//! - no literal that is one of production's keys is left outside `t`,
//!   unless a comment says why (`// i18n: <reason>` on its line or the line
//!   above, such as "not shown" or "production keeps it in English";
//!   `// i18n: translated where drawn` covers the lines after it up to a
//!   blank line, for a table whose labels go through `t` when drawn) or
//!   its file is listed below as one that shows none;
//! - no literal with letters is handed straight to a drawing call (`.child`,
//!   `.label`, `.tooltip`, `.placeholder`, a notification, a confirmation,
//!   …) without `t`, `tf!` or `en`.
//!
//! What it does not catch: a text built with `format!` and then drawn, a
//! literal kept in a variable or a table and drawn later (unless it is a
//! key), a drawing call it does not list, and a key used where production
//! itself leaves the text in English.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Files that draw no interface text of their own: the test tool, the
/// tests, the application menu, which production leaves in English
/// (`electron/windows/menu.mjs` is not localized), and the shortcut
/// registry, data whose labels, groups and descriptions Settings draws
/// through `t()` and whose key names stay English as production writes
/// them; and this module's own data (the languages' number and date
/// words, the English plural suffix).
const NOT_SHOWN: [&str; 7] = [
    "offscreen.rs",
    "i18n/tests.rs",
    "i18n/guard.rs",
    "i18n/format.rs",
    "i18n/mod.rs",
    "app_menu.rs",
    "settings/registry.rs",
];

/// Calls whose literal argument is drawn.
const SINKS: [&str; 14] = [
    "tip(",
    ".child(",
    ".label(",
    ".tooltip(",
    ".placeholder(",
    ".title(",
    ".message(",
    ".description(",
    "notify::error(cx, ",
    "notify::warning(cx, ",
    "notify::info(cx, ",
    "notify::success(cx, ",
    "Tooltip::text(",
    ".detail(",
];

/// A string literal and where it is.
struct Literal {
    text: String,
    line: usize,
    /// The code before it on its line and the line before, without
    /// comments or other literals.
    before: String,
    /// Marked `// i18n: <reason>` on its line or the line above.
    marked: bool,
    /// The code after it on its line.
    after: String,
    /// Where `after` starts in the file's code.
    code_end: usize,
}

/// The literals of a Rust file outside `#[cfg(test)]` items, and the lines
/// marked as showing no text.
fn literals(source: &str) -> Vec<Literal> {
    let chars: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut code = String::new();
    let mut line = 1;
    let mut index = 0;
    // `#[cfg(test)]` items are skipped: from the attribute to the end of
    // the item (its closing brace at the same depth, or its semicolon).
    let mut skip_from: Option<(usize, bool)> = None;
    let mut depth = 0usize;
    let mut markers = BTreeSet::new();
    // A block marker covers the lines after it up to the next blank line.
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
            // Keep a marker comment in the code so a literal can see it.
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
        // Raw strings: r"…", r#"…"#, br"…".
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
            // A byte string is never drawn.
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
                            // \u{…}
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
                            // A continued line drops the break and the
                            // indentation after it.
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
            // A character literal ('x', '\'', '\u{…}'); otherwise a lifetime.
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
    // The current line and the one before, so a call broken across lines
    // (`.child(\n "…"`) is seen.
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

/// How a literal is used: given to `t`/`tf!`, to `en`, or neither.
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
    // The shared menu's rows and a select's choices translate their labels
    // themselves, as production's `ContextMenu` and `CmSelect` do.
    let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    // `item(<action>, "…"`: the label after an action that is a literal or
    // a path (`Action::Rename`).
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
    }) || compact.ends_with("submenu(");
    if translating || called("t(") || called("tf!(") || called("t_with(") || called("i18n::t(") {
        Use::Translated
    } else if called("en(") || called("i18n::en(") {
        Use::GpuiOnly
    } else {
        Use::Plain
    }
}

/// A literal in a place where it names something rather than being shown:
/// a match pattern, one side of a comparison, an icon's name, a JSON value
/// sent to the library.
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

/// Files that are test modules: declared under `#[cfg(test)]` as `mod x;`.
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

/// What the guard finds, by kind.
#[derive(Default)]
struct Findings {
    /// Literals given to `t` that are not keys.
    unknown_keys: Vec<String>,
    /// Literals given to `en` that are keys or are not listed.
    unlisted: Vec<String>,
    /// Listed GPUI-only texts no longer used.
    stale: Vec<String>,
    /// Keys left in English, by file.
    untranslated: BTreeMap<String, Vec<String>>,
    /// Literals drawn without `t`, by file.
    drawn: BTreeMap<String, Vec<String>>,
}

fn has_letters(text: &str) -> bool {
    text.chars().any(|c| c.is_alphabetic())
}

fn scan() -> Findings {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = sources();
    let tests = test_modules(&files);
    let listed = gpui_only();
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
                    if !super::is_key(&literal.text) {
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
