// Adapted from gpui 0.2.2 examples/input.rs (Apache-2.0).
//! The platform's text input: what an input method composes (marked text)
//! and asks about, in UTF-16 offsets.
use std::ops::Range;

use super::editing::{self, Typing};
use super::element::{caret_in, rows_of};
use super::*;
use gpui::{Bounds, EntityInputHandler, Pixels, UTF16Selection, point};
impl TextInput {
    fn offset_from_utf16(&self, offset: usize) -> usize {
        editing::utf8_offset(&self.content, offset)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        // Typed text continues the run of typing; text an input method was
        // composing joins the step its composition began.
        if self.marked_range.is_none() {
            self.history.record(self.snapshot(), Some(Typing::Insert));
        }
        self.splice(range, new_text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        if self.marked_range.is_none() {
            self.history.record(self.snapshot(), Some(Typing::Insert));
        }

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        // The selection the input method gives is within the new text.
        self.selected_range = new_selected_range_utf16
            .map(|selected| {
                range.start + editing::utf8_offset(new_text, selected.start)
                    ..range.start + editing::utf8_offset(new_text, selected.end)
            })
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;
        self.upstream = false;
        self.goal_x = None;
        self.blink = std::time::Instant::now();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        if let Some(lines) = self.last_lines.as_ref() {
            let line_height = px(self.kind.line_height());
            let rows = rows_of(lines, line_height);
            let (start_x, top) = caret_in(lines, &rows, range.start, false)?;
            let (end_x, end_top) = caret_in(lines, &rows, range.end, true)?;
            let end_x = if end_top == top { end_x } else { start_x };
            return Some(Bounds::from_corners(
                point(bounds.left() + start_x, bounds.top() + top),
                point(bounds.left() + end_x, bounds.top() + top + line_height),
            ));
        }
        let last_layout = self.last_layout.as_ref()?;
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start) - self.scroll,
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end) - self.scroll,
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        bounds.localize(&point)?;
        if self.last_lines.is_some() {
            let (index, _) = self.hit(point);
            return Some(self.offset_to_utf16(index));
        }
        let last_layout = self.last_layout.as_ref()?;
        // The line drawn may be the clipped or placeholder text, so only an
        // index inside the content counts.
        let utf8_index = last_layout
            .index_for_x(point.x - bounds.left() + self.scroll)
            .filter(|index| *index <= self.content.len())?;
        Some(self.offset_to_utf16(utf8_index))
    }
}
