// Adapted from gpui 0.2.2 examples/input.rs (Apache-2.0).
//! Pressing, dragging and secondary clicks in a field.
use std::ops::Range;

use super::editing;
use super::*;
use gpui::{MouseDownEvent, MouseUpEvent, Pixels, Point};
/// What a press selects as the pointer drags: characters, or the whole
/// words or lines a double- or triple-click started with.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Granularity {
    Character,
    Word,
    Line,
}

impl TextInput {
    /// What a double- or triple-click selects around a character.
    fn unit_at(&self, granularity: Granularity, at: usize) -> Range<usize> {
        match granularity {
            Granularity::Character => at..at,
            Granularity::Word => editing::word_at(&self.content, at, !cfg!(target_os = "macos")),
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

    /// A press on the field's frame around the text (its padding), which
    /// in production is part of the field: it places the caret as a press
    /// on the nearest text would.
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

    /// The pointer moved with the button held after a press in the field:
    /// the selection follows it by character, or by whole words or lines
    /// after a double- or triple-click.
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

    /// While the pointer is held past the field's edge, the selection goes
    /// on following it, and the text scrolls, every 50 ms.
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

    /// Whether the held pointer is past the edge the text scrolls along.
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

    /// A secondary click takes focus and, on a Mac, selects the word under
    /// the pointer unless it is inside the selection; elsewhere it moves
    /// the caret there. Production shows no menu of its own in a field, so
    /// the click goes on to whatever holds the field.
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
