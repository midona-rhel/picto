use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn previous_grapheme(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .rev()
        .map(|(index, _)| index)
        .find(|index| *index < offset)
        .unwrap_or(0)
}

pub(super) fn next_grapheme(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .find(|index| *index > offset)
        .unwrap_or(text.len())
}

/// The text cut into words, runs of space and marks, as Chromium cuts
/// it: Unicode's word rules with the dictionaries for Chinese, Japanese
/// and Thai, except that a full stop parts letters (`example.com`) and
/// joins only digits (`3.14`), and a colon parts everything.
fn segments(text: &str) -> Vec<Range<usize>> {
    let mut bounds: Vec<usize> = icu_segmenter::WordSegmenter::new_dictionary(Default::default())
        .segment_str(text)
        .collect();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let digit = |at: Option<&(usize, char)>| at.is_some_and(|(_, c)| c.is_numeric());
    for (n, &(index, c)) in chars.iter().enumerate() {
        let stop = matches!(c, '.' | '\u{FE52}' | '\u{FF0E}');
        let colon = matches!(c, ':' | '\u{FE55}' | '\u{FF1A}');
        let between_digits = n > 0 && digit(chars.get(n - 1)) && digit(chars.get(n + 1));
        if colon || (stop && !between_digits) {
            bounds.push(index);
            bounds.push(index + c.len_utf8());
        }
    }
    bounds.push(0);
    bounds.push(text.len());
    bounds.sort_unstable();
    bounds.dedup();
    bounds.windows(2).map(|pair| pair[0]..pair[1]).collect()
}

fn is_space(text: &str, range: &Range<usize>) -> bool {
    text[range.clone()].chars().all(char::is_whitespace)
}

/// Option-Left (Control-Left elsewhere): the start of the word before
/// the caret, or of the text.
pub(super) fn word_start_before(text: &str, offset: usize) -> usize {
    segments(text)
        .into_iter()
        .rev()
        .find(|segment| segment.start < offset && !is_space(text, segment))
        .map_or(0, |segment| segment.start)
}

/// Option-Right on a Mac: the end of the word after the caret, or of
/// the text.
fn word_end_after(text: &str, offset: usize) -> usize {
    segments(text)
        .into_iter()
        .find(|segment| segment.end > offset && !is_space(text, segment))
        .map_or(text.len(), |segment| segment.end)
}

/// Control-Right on Windows and Linux: past the word the caret is in
/// and the space after it, to the start of the next word.
fn next_word_start(text: &str, offset: usize) -> usize {
    let segments = segments(text);
    let mut rest = segments
        .iter()
        .filter(|segment| segment.end > offset)
        .peekable();
    let mut position = offset;
    if let Some(word) = rest.next_if(|segment| !is_space(text, segment)) {
        position = word.end;
    }
    while let Some(space) = rest.next_if(|segment| is_space(text, segment)) {
        position = space.end;
    }
    position
}

/// The word Option-Right (Control-Right elsewhere) and Option-Delete go
/// to on this platform.
pub(super) fn word_right(text: &str, offset: usize) -> usize {
    if cfg!(target_os = "macos") {
        word_end_after(text, offset)
    } else {
        next_word_start(text, offset)
    }
}

/// What a double-click on the character at `offset` selects: its word,
/// run of space or mark; with `trailing_space` (Windows), a word with
/// the space after it.
pub(super) fn word_at(text: &str, offset: usize, trailing_space: bool) -> Range<usize> {
    let segments = segments(text);
    let Some(index) = segments
        .iter()
        .position(|segment| segment.contains(&offset))
        .or(segments.len().checked_sub(1))
    else {
        return offset..offset;
    };
    let mut range = segments[index].clone();
    if trailing_space
        && !is_space(text, &range)
        && let Some(next) = segments.get(index + 1)
        && is_space(text, next)
        && !text[next.clone()].contains('\n')
    {
        range.end = next.end;
    }
    range
}

/// The start of the paragraph `offset` is in: just after the line
/// break before it.
pub(super) fn paragraph_start(text: &str, offset: usize) -> usize {
    text[..offset].rfind('\n').map_or(0, |index| index + 1)
}

/// The end of the paragraph `offset` is in: at the line break after it.
pub(super) fn paragraph_end(text: &str, offset: usize) -> usize {
    text[offset..]
        .find('\n')
        .map_or(text.len(), |index| offset + index)
}

/// What a triple-click in text of several lines selects: the
/// paragraph with its line break.
pub(super) fn paragraph_at(text: &str, offset: usize) -> Range<usize> {
    let end = paragraph_end(text, offset);
    let end = if end < text.len() { end + 1 } else { end };
    paragraph_start(text, offset)..end
}

/// Control-T: the range to replace, what replaces it and where the
/// caret goes. The characters either side of the caret swap and the
/// caret moves past them; at the end of a line, the two before it swap.
pub(super) fn transpose(text: &str, offset: usize) -> Option<(Range<usize>, String, usize)> {
    if offset == 0 {
        return None;
    }
    let at_end = offset == text.len() || text[offset..].starts_with('\n');
    let (first, second) = if at_end {
        let middle = previous_grapheme(text, offset);
        (previous_grapheme(text, middle)..middle, middle..offset)
    } else {
        let start = previous_grapheme(text, offset);
        (start..offset, offset..next_grapheme(text, offset))
    };
    if first.is_empty() || second.is_empty() {
        return None;
    }
    let (a, b) = (&text[first.clone()], &text[second.clone()]);
    if a.contains('\n') || b.contains('\n') {
        return None;
    }
    Some((first.start..second.end, format!("{b}{a}"), second.end))
}

/// Control-Delete on a Mac: where to delete back to so that only the
/// last code point of the character before the caret goes (an accent
/// typed as its own mark, say).
pub(super) fn decomposed_backward(text: &str, offset: usize) -> usize {
    let start = previous_grapheme(text, offset);
    text[start..offset]
        .char_indices()
        .next_back()
        .map_or(start, |(index, _)| start + index)
}

/// What a one-line field (`<input>`) keeps of pasted text: line breaks
/// at the end dropped, the others made spaces.
pub(super) fn one_line(text: &str) -> String {
    text.trim_end_matches(['\r', '\n'])
        .replace("\r\n", " ")
        .replace(['\r', '\n'], " ")
}

/// Pasted text for a field of several lines, its line breaks all `\n`.
pub(super) fn normalize_breaks(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// The byte offset of a UTF-16 offset into `text`.
pub(super) fn utf8_offset(text: &str, utf16: usize) -> usize {
    let mut utf8 = 0;
    let mut counted = 0;
    for c in text.chars() {
        if counted >= utf16 {
            break;
        }
        counted += c.len_utf16();
        utf8 += c.len_utf8();
    }
    utf8
}

/// A field's text and selection at one moment.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Snapshot {
    pub(super) text: String,
    pub(super) selection: Range<usize>,
}

/// How many steps a field can take back.
const DEPTH: usize = 200;

/// An edit made by typing: characters put in, or deleted one at a time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Typing {
    Insert,
    Delete,
}

/// A field's Undo and Redo, as Chromium's editing keeps them: a run of
/// typed characters is one step, and so is a run of characters deleted
/// one at a time with whatever is typed straight after it, until the
/// caret is moved or another kind of edit comes; every other edit (a
/// paste, a cut, deleting a word or a line) is a step of its own.
/// Taking a step back selects the text it brings back.
#[derive(Default, Debug)]
pub(super) struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    typing: Option<Typing>,
}

impl History {
    /// Notes the state before an edit, and how it was made.
    pub(super) fn record(&mut self, before: Snapshot, typing: Option<Typing>) {
        let continues = match (self.typing, typing) {
            (Some(_), Some(Typing::Insert)) => true,
            (Some(Typing::Delete), Some(Typing::Delete)) => true,
            _ => false,
        };
        if !continues {
            self.undo.push(before);
            if self.undo.len() > DEPTH {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.typing = typing;
    }

    /// Ends the run of typing: what is typed next is a new step.
    pub(super) fn close(&mut self) {
        self.typing = None;
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    #[cfg(test)]
    pub(super) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// The state before the last step, with what it brings back
    /// selected, given the state now (which Redo returns to).
    pub(super) fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let before = self.undo.pop()?;
        self.typing = None;
        let selection = changed(&current.text, &before.text, current.selection.end);
        self.redo.push(current);
        Some(Snapshot {
            text: before.text,
            selection,
        })
    }

    /// The state the last Undo left, with the caret after what it
    /// changes.
    pub(super) fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let after = self.redo.pop()?;
        self.typing = None;
        let end = changed(&current.text, &after.text, current.selection.start).end;
        self.undo.push(current);
        Some(Snapshot {
            text: after.text,
            selection: end..end,
        })
    }
}

/// The part of `to` that `from` does not have: what lies between their
/// common start and their common end. The common start goes no further
/// than `at`, where the caret in `from` shows the edit was, so that of
/// two equal characters the one edited is the one found.
fn changed(from: &str, to: &str, at: usize) -> Range<usize> {
    let mut prefix = 0;
    for (a, b) in from.chars().zip(to.chars()) {
        if a != b || prefix + a.len_utf8() > at {
            break;
        }
        prefix += a.len_utf8();
    }
    let suffix: usize = from[prefix..]
        .chars()
        .rev()
        .zip(to[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    prefix..to.len() - suffix
}

/// The byte range of each line of `text`, without its line break.
pub(super) fn hard_lines(text: &str) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, _) in text.match_indices('\n') {
        lines.push(start..index);
        start = index + 1;
    }
    lines.push(start..text.len());
    lines
}

#[cfg(test)]
mod tests {
    use super::{
        History, Snapshot, Typing, changed, decomposed_backward, hard_lines, next_grapheme,
        next_word_start, normalize_breaks, one_line, paragraph_at, paragraph_end, paragraph_start,
        previous_grapheme, transpose, utf8_offset, word_at, word_end_after, word_start_before,
    };

    #[test]
    fn line_breaks_split_the_text_into_lines_by_byte() {
        assert_eq!(hard_lines("one"), [0..3]);
        assert_eq!(hard_lines("a\nbc\n"), [0..1, 2..4, 5..5]);
        assert_eq!(hard_lines(""), [0..0]);
        assert_eq!(hard_lines("é\nx"), [0..2, 3..4]);
    }

    /// Byte offsets of the stops reached by pressing a key over and over,
    /// in UTF-16 units as the browser reports them.
    fn walk(text: &str, from: usize, step: fn(&str, usize) -> usize) -> Vec<usize> {
        let mut stops = Vec::new();
        let mut at = from;
        loop {
            let next = step(text, at);
            if next == at {
                break;
            }
            at = next;
            stops.push(text[..at].encode_utf16().count());
        }
        stops
    }

    // The stops below were read from the production app (Chromium on
    // macOS) pressing Option-Right and Option-Left in its search field.

    #[test]
    fn option_right_stops_where_chromium_does() {
        let text = "foo.bar-baz_qux it's  a/b c:d 3.14 x";
        assert_eq!(
            walk(text, 0, word_end_after),
            [3, 4, 7, 8, 15, 20, 23, 24, 25, 27, 28, 29, 34, 36]
        );
        let text = "example.com e.g. U.S.A. file_name.jpg 1,000.5 don't";
        assert_eq!(
            walk(text, 0, word_end_after),
            [
                7, 8, 11, 13, 14, 15, 16, 18, 19, 20, 21, 22, 23, 33, 34, 37, 45, 51
            ]
        );
        let text = "  lead  trail  ";
        assert_eq!(walk(text, 0, word_end_after), [6, 13, 15]);
    }

    #[test]
    fn option_left_stops_where_chromium_does() {
        let text = "foo.bar-baz_qux it's  a/b c:d 3.14 x";
        assert_eq!(
            walk(text, text.len(), word_start_before),
            [35, 30, 28, 27, 26, 24, 23, 22, 16, 8, 7, 4, 3, 0]
        );
        let text =
            "a.1 1.a a:1 x;y x,y a·b a'b 'q' a’b 3:45 v1.2.3 IMG_0042.JPG #tag @me 50% $5 a&b";
        assert_eq!(
            walk(text, text.len(), word_start_before)[..30],
            [
                79, 78, 77, 75, 74, 72, 70, 67, 66, 62, 61, 57, 56, 48, 41, 38, 37, 36, 32, 30, 29,
                28, 24, 20, 18, 17, 16, 14, 13, 12
            ]
        );
        let text = "  lead  trail  ";
        assert_eq!(walk(text, text.len(), word_start_before), [8, 2, 0]);
    }

    #[test]
    fn emoji_accents_and_dashes_are_words_of_their_own() {
        let text = "naïve café — e\u{301}t\u{e9} 👍🏽 ok 👨‍👩‍👧 x";
        assert_eq!(
            walk(text, 0, word_end_after),
            [5, 10, 12, 17, 22, 25, 34, 36]
        );
        assert_eq!(
            walk(text, text.len(), word_start_before),
            [35, 26, 23, 18, 13, 11, 6, 0]
        );
    }

    #[test]
    fn chinese_japanese_and_thai_break_by_dictionary() {
        let text = "日本語のテキスト です ok";
        assert_eq!(walk(text, 0, word_end_after), [3, 4, 8, 11, 14]);
        assert_eq!(walk(text, text.len(), word_start_before), [12, 9, 4, 3, 0]);
        let text = "ทุกสองสัปดาห์ 中文分词测试 한국어 텍스트";
        assert_eq!(
            walk(text, 0, word_end_after),
            [3, 6, 13, 16, 18, 20, 24, 28]
        );
    }

    #[test]
    fn right_to_left_words_go_in_the_order_they_are_stored() {
        let text = "שלום עולם abc";
        assert_eq!(walk(text, 0, word_end_after), [4, 9, 13]);
        assert_eq!(walk(text, text.len(), word_start_before), [10, 5, 0]);
    }

    #[test]
    fn control_right_elsewhere_goes_to_the_next_word_start() {
        let text = "hello brave  new.world";
        assert_eq!(next_word_start(text, 0), 6);
        assert_eq!(next_word_start(text, 2), 6);
        assert_eq!(next_word_start(text, 5), 6);
        assert_eq!(next_word_start(text, 6), 13);
        assert_eq!(next_word_start(text, 13), 16);
        assert_eq!(next_word_start(text, 16), 17);
        assert_eq!(next_word_start(text, 17), text.len());
        assert_eq!(next_word_start(text, text.len()), text.len());
    }

    #[test]
    fn a_word_inside_or_at_its_edge_moves_to_that_edge() {
        let text = "hello brave new world";
        assert_eq!(word_start_before(text, 8), 6);
        assert_eq!(word_end_after(text, 8), 11);
        assert_eq!(word_start_before(text, 6), 0);
        assert_eq!(word_end_after(text, 11), 15);
        assert_eq!(word_start_before(text, 0), 0);
        assert_eq!(word_end_after(text, text.len()), text.len());
        assert_eq!(word_start_before("", 0), 0);
        assert_eq!(word_end_after("", 0), 0);
    }

    #[test]
    fn double_click_selects_the_word_space_or_mark_under_it() {
        let text = "visit example.com  now";
        assert_eq!(word_at(text, 7, false), 6..13);
        assert_eq!(word_at(text, 13, false), 13..14);
        assert_eq!(word_at(text, 17, false), 17..19);
        assert_eq!(word_at(text, 0, false), 0..5);
        assert_eq!(word_at(text, text.len(), false), 19..22);
        // Windows takes the space after a word with it.
        assert_eq!(word_at(text, 1, true), 0..6);
        assert_eq!(word_at(text, 15, true), 14..19);
        assert_eq!(word_at("", 0, false), 0..0);
        let emoji = "a 👍🏽 b";
        assert_eq!(word_at(emoji, 2, false), 2..10);
    }

    #[test]
    fn graphemes_keep_emoji_and_marks_whole() {
        let text = "e\u{301}👨‍👩‍👧x";
        assert_eq!(next_grapheme(text, 0), 3);
        assert_eq!(next_grapheme(text, 3), 3 + "👨‍👩‍👧".len());
        assert_eq!(previous_grapheme(text, text.len()), text.len() - 1);
        assert_eq!(previous_grapheme(text, text.len() - 1), 3);
        assert_eq!(previous_grapheme(text, 3), 0);
        assert_eq!(previous_grapheme(text, 0), 0);
        assert_eq!(next_grapheme(text, text.len()), text.len());
        // Control-Delete takes the accent off, not the letter.
        assert_eq!(decomposed_backward(text, 3), 1);
        assert_eq!(decomposed_backward("ab", 2), 1);
        assert_eq!(decomposed_backward("", 0), 0);
    }

    #[test]
    fn paragraphs_run_between_line_breaks() {
        let text = "one two\nthree\n\nfour";
        assert_eq!(paragraph_start(text, 5), 0);
        assert_eq!(paragraph_end(text, 5), 7);
        assert_eq!(paragraph_start(text, 10), 8);
        assert_eq!(paragraph_end(text, 8), 13);
        assert_eq!(paragraph_start(text, 14), 14);
        assert_eq!(paragraph_end(text, 14), 14);
        assert_eq!(paragraph_end(text, 16), text.len());
        assert_eq!(paragraph_at(text, 9), 8..14);
        assert_eq!(paragraph_at(text, 17), 15..19);
        assert_eq!(paragraph_at("single", 2), 0..6);
    }

    #[test]
    fn control_t_swaps_as_chromium_does() {
        // "hello br|ave" became "hello barve" with the caret after the pair.
        let text = "hello brave new world";
        assert_eq!(transpose(text, 8), Some((7..9, "ar".into(), 9)));
        assert_eq!(transpose(text, 5), Some((4..6, " o".into(), 6)));
        // At the end, the two before the caret.
        assert_eq!(transpose(text, 21), Some((19..21, "dl".into(), 21)));
        assert_eq!(transpose(text, 0), None);
        assert_eq!(transpose("a", 1), None);
        assert_eq!(transpose("ab\ncd", 3), None);
        assert_eq!(transpose("ab\ncd", 2), Some((0..2, "ba".into(), 2)));
        assert_eq!(transpose("é👍x", 2), Some((0..6, "👍é".into(), 6)));
    }

    #[test]
    fn pasting_into_one_line_does_what_input_does() {
        // Read from the production search field.
        assert_eq!(one_line("a\nb\r\nc\n\n"), "a b c");
        assert_eq!(one_line("\n\nfirst\nsecond"), "  first second");
        assert_eq!(one_line("plain"), "plain");
        assert_eq!(one_line("x\ry"), "x y");
        assert_eq!(normalize_breaks("a\r\nb\rc\n"), "a\nb\nc\n");
    }

    #[test]
    fn utf16_offsets_become_bytes() {
        let text = "a👍é";
        assert_eq!(utf8_offset(text, 0), 0);
        assert_eq!(utf8_offset(text, 1), 1);
        assert_eq!(utf8_offset(text, 3), 5);
        assert_eq!(utf8_offset(text, 4), 7);
        assert_eq!(utf8_offset(text, 9), 7);
    }

    fn state(text: &str, at: usize) -> Snapshot {
        Snapshot {
            text: text.into(),
            selection: at..at,
        }
    }

    fn type_into(history: &mut History, text: &mut String, typed: &str) {
        for c in typed.chars() {
            history.record(state(text, text.len()), Some(Typing::Insert));
            text.push(c);
        }
    }

    fn backspace(history: &mut History, text: &mut String, times: usize) {
        for _ in 0..times {
            history.record(state(text, text.len()), Some(Typing::Delete));
            text.pop();
        }
    }

    // The steps below were read from the production app's inspector
    // notes, a field the browser edits by itself.

    #[test]
    fn typing_is_one_step_and_deleting_then_typing_another() {
        let mut history = History::default();
        let mut text = String::new();
        type_into(&mut history, &mut text, "ab cd");
        backspace(&mut history, &mut text, 2);
        type_into(&mut history, &mut text, "xy");
        assert_eq!(text, "ab xy");
        let back = history.undo(state(&text, 5)).unwrap();
        assert_eq!(back.text, "ab cd");
        assert_eq!(back.selection, 3..5);
        let back = history.undo(back).unwrap();
        assert_eq!(back, state("", 0));
        assert!(!history.can_undo());
        let again = history.redo(back).unwrap();
        assert_eq!(again, state("ab cd", 5));
        let again = history.redo(again).unwrap();
        assert_eq!(again, state("ab xy", 5));
    }

    #[test]
    fn moving_the_caret_or_another_edit_starts_a_new_step() {
        // "ab xy", Left, "z", Option-Delete; then each Undo.
        let mut history = History::default();
        let mut text = String::new();
        type_into(&mut history, &mut text, "ab cd");
        backspace(&mut history, &mut text, 2);
        type_into(&mut history, &mut text, "xy");
        history.close();
        history.record(state("ab xy", 4), Some(Typing::Insert));
        text = "ab xzy".into();
        history.record(state(&text, 5), None);
        text = "ab y".into();

        let first = history.undo(state(&text, 3)).unwrap();
        assert_eq!(first.text, "ab xzy");
        assert_eq!(first.selection, 3..5);
        let second = history.undo(first).unwrap();
        assert_eq!(second, state("ab xy", 4));
        let third = history.undo(second).unwrap();
        assert_eq!(third.text, "ab cd");
        assert_eq!(third.selection, 3..5);
        let fourth = history.undo(third).unwrap();
        assert_eq!(fourth, state("", 0));

        let redone = history.redo(fourth).unwrap();
        assert_eq!(redone, state("ab cd", 5));
        let redone = history.redo(redone).unwrap();
        assert_eq!(redone, state("ab xy", 5));
        let redone = history.redo(redone).unwrap();
        assert_eq!(redone, state("ab xzy", 5));
        let redone = history.redo(redone).unwrap();
        assert_eq!(redone, state("ab y", 3));
        assert!(history.redo(redone).is_none());
    }

    #[test]
    fn deleting_after_typing_is_a_step_of_its_own() {
        let mut history = History::default();
        let mut text = String::new();
        type_into(&mut history, &mut text, "abc");
        backspace(&mut history, &mut text, 1);
        let back = history.undo(state(&text, 2)).unwrap();
        assert_eq!(back.text, "abc");
        assert_eq!(back.selection, 2..3);
    }

    #[test]
    fn an_edit_after_undo_drops_what_could_be_redone() {
        let mut history = History::default();
        history.record(state("", 0), None);
        let back = history.undo(state("pasted", 6)).unwrap();
        history.record(back, Some(Typing::Insert));
        assert!(history.redo(state("x", 1)).is_none());
        assert!(history.can_undo());
        history.clear();
        assert!(!history.can_undo());
        assert!(history.undo(state("x", 1)).is_none());
    }

    #[test]
    fn the_changed_part_leaves_out_what_both_share() {
        assert_eq!(changed("ab y", "ab xzy", 3), 3..5);
        assert_eq!(changed("ab xzy", "ab xy", 5), 4..4);
        assert_eq!(changed("ab xy", "ab cd", 5), 3..5);
        assert_eq!(changed("aaa", "aaaa", 3), 3..4);
        assert_eq!(changed("", "", 0), 0..0);
        assert_eq!(changed("é", "éé", 2), 2..4);
        assert_eq!(changed("x👍", "x", 5), 1..1);
        // Of two line breaks, the one deleted where the caret is.
        assert_eq!(changed("line\nend", "line\n\nend", 4), 4..5);
        assert_eq!(changed("aa", "aaa", 1), 1..2);
    }
}
