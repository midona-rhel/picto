// Adapted from gpui 0.2.2 examples/input.rs (Apache-2.0).
//! This file handles presses of the pointer, drags and secondary clicks in
//! a text field.
use std::ops::Range;

use super::editing::{self, Platform};
use super::*;
use gpui::{MouseDownEvent, MouseUpEvent, Pixels, Point};
/// The unit by which a press selects while the pointer is dragged: single
/// characters after a single click, whole words after a double-click, and
/// whole lines after a triple-click.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Granularity {
    Character,
    Word,
    Line,
}

impl TextInput {
    /// Returns the range that a click of `granularity` selects around the
    /// character at the byte offset `at`. A single click selects nothing and
    /// a double-click selects the word. A triple-click selects the paragraph
    /// in a field that takes line breaks, and the whole text in any other
    /// field.
    fn unit_at(&self, granularity: Granularity, at: usize) -> Range<usize> {
        match granularity {
            Granularity::Character => at..at,
            // The words of a password field cannot be seen, so the whole
            // text is selected.
            Granularity::Word if self.masked => 0..self.content.len(),
            Granularity::Word => editing::word_at(
                &self.content,
                at,
                Platform::CURRENT.selects_trailing_space(),
            ),
            Granularity::Line if self.multiline => editing::paragraph_at(&self.content, at),
            Granularity::Line => 0..self.content.len(),
        }
    }

    pub(super) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        self.is_selecting = true;
        let (offset, upstream) = self.hit(event.position);
        self.granularity = match event.click_count {
            0 | 1 => Granularity::Character,
            2 => Granularity::Word,
            _ => Granularity::Line,
        };
        if self.granularity == Granularity::Character {
            self.go(offset, upstream, event.modifiers.shift, cx);
            self.drag_origin = self.anchor()..self.anchor();
            return;
        }
        let unit = self.unit_at(self.granularity, self.character_under(event.position));
        self.move_to(unit.start, cx);
        self.select_to(unit.end, cx);
        self.drag_origin = unit;
    }

    /// Handles a press on the frame that `ui::field` draws around the text,
    /// which is the field's padding. The press places the caret as a press
    /// on the nearest part of the text would. A press inside the bounds of
    /// the text is ignored here, because the text handles it itself.
    pub fn press_outside(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .last_bounds
            .is_some_and(|bounds| bounds.contains(&event.position))
        {
            return;
        }
        self.on_mouse_down(event, window, cx);
    }

    /// Extends the selection to `position`. It is called when the pointer
    /// moves with its button held after a press in the field. The selection
    /// grows by characters, or by whole words or lines after a double-click
    /// or a triple-click.
    pub(super) fn drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.drag_point = Some(position);
        let (offset, upstream) = self.hit(position);
        let origin = self.drag_origin.clone();
        match self.granularity {
            Granularity::Character => {
                self.select_to(offset, cx);
                self.upstream = upstream;
            }
            granularity if offset < origin.start => {
                let unit = self.unit_at(granularity, offset);
                self.move_to(origin.end, cx);
                self.select_to(unit.start.min(origin.start), cx);
            }
            granularity => {
                let unit = self.unit_at(
                    granularity,
                    self.previous_boundary(offset).max(origin.start),
                );
                self.move_to(origin.start, cx);
                self.select_to(unit.end.max(origin.end), cx);
            }
        }
        self.autoscroll(cx);
    }

    /// Starts a timer that runs while the pointer is held past the field's
    /// edge. Every 50 ms it extends the selection to the pointer again,
    /// which scrolls the text. The timer stops when the pointer is inside
    /// the field again or its button is released.
    fn autoscroll(&mut self, cx: &mut Context<Self>) {
        if !self.outside() {
            self.autoscroll = None;
            return;
        }
        if self.autoscroll.is_some() {
            return;
        }
        self.autoscroll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
                let going = this.update(cx, |this: &mut TextInput, cx| {
                    let point = this.drag_point.filter(|_| this.is_selecting);
                    match point {
                        Some(point) if this.outside() => {
                            this.drag(point, cx);
                            true
                        }
                        _ => {
                            this.autoscroll = None;
                            false
                        }
                    }
                });
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        }));
    }

    /// Returns whether the pointer, with its button held, is past an edge
    /// of the field in the direction in which the text scrolls. That is
    /// above or below the field when the text is drawn wrapped, and left or
    /// right of it when the text is drawn on one line.
    fn outside(&self) -> bool {
        let (Some(point), Some(bounds)) = (self.drag_point, self.last_bounds) else {
            return false;
        };
        if self.last_lines.is_some() {
            let area = self.area.bounds();
            let area = if area.size.height > px(0.) {
                area
            } else {
                bounds
            };
            point.y < area.top() || point.y > area.bottom()
        } else {
            point.x < bounds.left() || point.x > bounds.right()
        }
    }

    pub(super) fn on_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.end_drag();
    }

    pub(super) fn end_drag(&mut self) {
        self.is_selecting = false;
        self.drag_point = None;
        self.autoscroll = None;
    }

    /// Handles a secondary click. The field takes the keyboard focus. A
    /// click inside the selection changes nothing else. Otherwise, on a Mac
    /// the word under the pointer is selected, and on Windows and Linux the
    /// caret is moved to the pointer. The field shows no menu of its own,
    /// and the click is passed on to the view that holds the field.
    pub(super) fn on_secondary_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        let (offset, upstream) = self.hit(event.position);
        let range = &self.selected_range;
        if !range.is_empty() && range.start <= offset && offset <= range.end {
            return;
        }
        if cfg!(target_os = "macos") {
            let word = self.unit_at(Granularity::Word, self.character_under(event.position));
            self.move_to(word.start, cx);
            self.select_to(word.end, cx);
        } else {
            self.go(offset, upstream, false, cx);
        }
    }
}
