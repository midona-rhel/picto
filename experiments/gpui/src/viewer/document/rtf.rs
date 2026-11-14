//! RTF documents as `TextDocumentViewer.tsx` shows them: the visible text,
//! one paragraph per `\par`, without character formatting.
use super::rich::{Block, Inline};

/// An RTF document's text, one paragraph per `\par`. Character formatting
/// is not kept.
pub fn rtf(source: &str) -> Vec<Block> {
    rtf_text(source)
        .split('\n')
        .map(|paragraph| Block::Paragraph(Inline::plain(paragraph)))
        .collect()
}

/// The visible text of an RTF document: control words dropped, the
/// destinations that hold no body text skipped, escapes decoded.
pub fn rtf_text(source: &str) -> String {
    const SKIPPED: [&str; 10] = [
        "fonttbl",
        "colortbl",
        "stylesheet",
        "info",
        "pict",
        "header",
        "footer",
        "listtable",
        "listoverridetable",
        "generator",
    ];
    let bytes = source.as_bytes();
    let mut out = String::new();
    // For each open group, whether its text is skipped.
    let mut groups: Vec<bool> = vec![false];
    // Characters to drop after a `\uN` escape (`\ucN`, default 1).
    let mut unicode_skip = 1usize;
    let mut pending_skip = 0usize;
    let mut index = 0;
    let skipping = |groups: &Vec<bool>| groups.last().copied().unwrap_or(false);
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            b'{' => {
                let parent = skipping(&groups);
                groups.push(parent);
                index += 1;
            }
            b'}' => {
                groups.pop();
                if groups.is_empty() {
                    groups.push(false);
                }
                index += 1;
            }
            b'\\' => {
                index += 1;
                let Some(&next) = bytes.get(index) else { break };
                if next.is_ascii_alphabetic() {
                    let start = index;
                    while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
                        index += 1;
                    }
                    let word = &source[start..index];
                    let number_start = index;
                    if index < bytes.len()
                        && (bytes[index] == b'-' || bytes[index].is_ascii_digit())
                    {
                        index += 1;
                        while index < bytes.len() && bytes[index].is_ascii_digit() {
                            index += 1;
                        }
                    }
                    let number: Option<i32> = source[number_start..index].parse().ok();
                    if index < bytes.len() && bytes[index] == b' ' {
                        index += 1;
                    }
                    if SKIPPED.contains(&word)
                        && let Some(last) = groups.last_mut()
                    {
                        *last = true;
                    }
                    if skipping(&groups) {
                        continue;
                    }
                    match word {
                        "par" | "line" | "sect" | "page" => out.push('\n'),
                        "tab" => out.push('\t'),
                        "uc" => unicode_skip = number.unwrap_or(1).max(0) as usize,
                        "u" => {
                            if let Some(code) = number {
                                let code = if code < 0 { code + 65536 } else { code } as u32;
                                if let Some(character) = char::from_u32(code) {
                                    out.push(character);
                                }
                                pending_skip = unicode_skip;
                            }
                        }
                        "emdash" => out.push('—'),
                        "endash" => out.push('–'),
                        "lquote" => out.push('‘'),
                        "rquote" => out.push('’'),
                        "ldblquote" => out.push('“'),
                        "rdblquote" => out.push('”'),
                        "bullet" => out.push('•'),
                        _ => {}
                    }
                } else {
                    // A control symbol is one character, which need not be
                    // one byte: the index stays on a character boundary.
                    index += char_len(source, index);
                    match next {
                        b'*' => {
                            if let Some(last) = groups.last_mut() {
                                *last = true;
                            }
                        }
                        b'\'' => {
                            // Two hex digits are two bytes; anything else
                            // after `\'` is read as text.
                            let hex = source.get(index..index + 2).unwrap_or("");
                            if hex.len() == 2 {
                                index += 2;
                            }
                            if pending_skip > 0 {
                                pending_skip -= 1;
                            } else if !skipping(&groups)
                                && let Ok(code) = u8::from_str_radix(hex, 16)
                            {
                                out.push(windows_1252(code));
                            }
                        }
                        b'~' if !skipping(&groups) => out.push('\u{a0}'),
                        b'\n' | b'\r' if !skipping(&groups) => out.push('\n'),
                        b'\\' | b'{' | b'}' if !skipping(&groups) => out.push(next as char),
                        _ => {}
                    }
                }
            }
            b'\r' | b'\n' => index += 1,
            _ => {
                let character = source.get(index..).and_then(|rest| rest.chars().next());
                let Some(character) = character else { break };
                index += character.len_utf8();
                if pending_skip > 0 {
                    pending_skip -= 1;
                } else if !skipping(&groups) {
                    out.push(character);
                }
            }
        }
    }
    out.trim_end().to_owned()
}

/// A byte of the Windows-1252 code page RTF's `\'hh` escapes use.
fn windows_1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9f => HIGH[(byte - 0x80) as usize],
        other => other as char,
    }
}

/// The length of the character at byte `index`, at least 1, so a scan
/// moves on whatever is there.
fn char_len(source: &str, index: usize) -> usize {
    source
        .get(index..)
        .and_then(|rest| rest.chars().next())
        .map_or(1, char::len_utf8)
}

#[cfg(test)]
mod tests {
    use super::rtf_text;

    #[test]
    fn rtf_keeps_the_body_text() {
        let source = r"{\rtf1\ansi{\fonttbl{\f0 Helvetica;}}{\colortbl;\red0\green0\blue0;}\f0\fs24 Hello \b bold\b0  world\par Caf\'e9 \u8364? end\par{\*\generator Picto}}";
        assert_eq!(rtf_text(source), "Hello bold world\nCafé € end");
    }

    #[test]
    fn a_control_symbol_that_is_not_ascii_does_not_split_a_character() {
        // A backslash before a letter of several bytes is a control symbol
        // of one character, dropped; the text around it stays.
        assert_eq!(rtf_text("{\\rtf1 a\\\u{e9}b}"), "ab");
        assert_eq!(rtf_text("{\\rtf1 x\\'\u{20ac}y}"), "x\u{20ac}y");
        assert_eq!(rtf_text("{\\rtf1 \\'e"), "e");
        assert_eq!(rtf_text("\\\u{1f600}"), "");
        // Every way a file can end mid-escape.
        for source in ["\\", "\\'", "\\'4", "{\\u", "{\\u-", "\\uc99999999999 x"] {
            rtf_text(source);
        }
    }
}
