// Adapted from gpui 0.2.2 examples/input.rs (Apache-2.0).
//! The element that draws the text of a text field (`TextInput`).
//! `TextElement` measures the text, draws it on one line or wrapped over
//! several rows, and draws the caret and the selection. In each frame it
//! stores in the `TextInput` where it drew the text (`last_bounds`,
//! `last_layout`, `last_lines`). `TextInput::hit` and
//! `TextInput::character_under`, which are also in this file, use what was
//! stored to find the place in the text that is under a point in the window.
//!
//! The code for wrapped text uses two words with a fixed meaning. A line
//! (`lines`) is the text between two line breaks (`\n`). A row (`rows`) is
//! one row of text as it is drawn. A line that is wider than the field is
//! drawn as several rows.
use std::ops::Range;

use super::editing::hard_lines;
use super::*;
use gpui::{
    Bounds, DispatchPhase, ElementId, ElementInputHandler, Entity, GlobalElementId, LayoutId,
    MouseMoveEvent, PaintQuad, Pixels, Point, Style, TextRun, UnderlineStyle, fill, point,
    relative, size,
};
use unicode_segmentation::*;
/// One row of wrapped text as it is drawn. A line that is wrapped at n
/// places is drawn as n + 1 rows.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    /// The byte offset in the whole text of the row's first byte.
    pub(super) start: usize,
    /// The byte offset in the whole text of the end of the row. When the
    /// row ends at a wrap, it is the offset of the first byte of the next
    /// row. When the row is the last row of its line, it is the offset of
    /// the line break after the line, or the length of the text for the
    /// last line.
    pub(super) end: usize,
    /// The row's top edge, in pixels below the top of the text.
    pub(super) top: Pixels,
    /// The distance in pixels from the start of the row's line to the start
    /// of this row, in the layout of the line without wrapping. That layout
    /// measures every position from the start of the line, so this value is
    /// subtracted from such a position to get the distance from the row's
    /// left edge.
    pub(super) x0: Pixels,
    /// The index of the row's line in the list of lines that `wrap`
    /// returns.
    pub(super) line: usize,
    /// True when this is the last row of its line, so the row ends at a
    /// line break or at the end of the text. False when the row ends at a
    /// wrap.
    pub(super) last: bool,
}

/// Returns the rows that `lines` are drawn as, from top to bottom. `lines`
/// is the list that `wrap` returns: for each line, the byte offset of its
/// first byte in the whole text, its layout, and its top edge in pixels
/// below the top of the text. Each row of a line is `line_height` below the
/// row before it.
pub(super) fn rows_of(
    lines: &[(usize, gpui::WrappedLine, Pixels)],
    line_height: Pixels,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for (index, (start, line, top)) in lines.iter().enumerate() {
        let layout = &line.unwrapped_layout;
        let wraps: Vec<usize> = line
            .wrap_boundaries()
            .iter()
            .map(|wrap| layout.runs[wrap.run_ix].glyphs[wrap.glyph_ix].index)
            .collect();
        let mut row_start = 0;
        for (number, row_end) in wraps.iter().copied().chain([line.len()]).enumerate() {
            rows.push(Row {
                start: start + row_start,
                end: start + row_end,
                top: *top + line_height * number as f32,
                x0: layout.x_for_index(row_start),
                line: index,
                last: number == wraps.len(),
            });
            row_start = row_end;
        }
    }
    rows
}

/// Returns the index in `rows` of the row that a caret at the byte offset
/// `offset` is drawn on. Where a line wraps, the same offset is both the end
/// of the upper row and the start of the lower row. The caret is then drawn
/// on the upper row when `upstream` is true, and on the lower row when it
/// is false. An offset that is on no row gives the index of the last row.
pub(super) fn row_index(rows: &[Row], offset: usize, upstream: bool) -> usize {
    rows.iter()
        .position(|row| {
            row.start <= offset
                && (offset < row.end || (offset == row.end && (row.last || upstream)))
        })
        .unwrap_or(rows.len().saturating_sub(1))
}

/// Returns the distance in pixels from the left edge of `row` to the place
/// where the byte offset `offset` of the whole text is drawn. `lines` is the
/// list of lines that `row` was made from.
pub(super) fn x_in(
    lines: &[(usize, gpui::WrappedLine, Pixels)],
    row: &Row,
    offset: usize,
) -> Pixels {
    let (start, line, _) = &lines[row.line];
    line.unwrapped_layout
        .x_for_index(offset.saturating_sub(*start))
        - row.x0
}

/// Returns the caret position in `row` that is nearest to a point `x` pixels
/// from the row's left edge, as a byte offset in the whole text. A negative
/// `x` counts as 0. The second value is true when that position is the end
/// of the row and the row ends at a wrap. The caller stores it as
/// `TextInput::upstream`, so that the caret is drawn at the end of this row
/// and not at the start of the next row.
pub(super) fn hit_row(
    lines: &[(usize, gpui::WrappedLine, Pixels)],
    row: &Row,
    x: Pixels,
) -> (usize, bool) {
    let (start, line, _) = &lines[row.line];
    let index = start
        + line
            .unwrapped_layout
            .closest_index_for_x(x.max(px(0.)) + row.x0);
    let offset = index.clamp(row.start, row.end);
    (offset, offset == row.end && !row.last)
}

/// Returns where a caret at the byte offset `offset` is drawn in wrapped
/// text: its distance in pixels from the left edge of the text, and the top
/// edge of its row in pixels below the top of the text. `upstream` chooses
/// the row where a line wraps, as in `row_index`. Returns `None` when `rows`
/// is empty.
pub(super) fn caret_in(
    lines: &[(usize, gpui::WrappedLine, Pixels)],
    rows: &[Row],
    offset: usize,
    upstream: bool,
) -> Option<(Pixels, Pixels)> {
    let row = rows.get(row_index(rows, offset, upstream))?;
    Some((x_in(lines, row, offset), row.top))
}

impl TextInput {
    /// Returns the caret position that is nearest to `position`, a point in
    /// window coordinates, as a byte offset in the text. It uses what the
    /// element drew in the last frame. A press of the pointer puts the caret
    /// at this position, and a drag moves the end of the selection to it.
    /// The second value is true when the position is the end of a row that
    /// ends at a wrap (see `hit_row`).
    ///
    /// The result is the start of the text when the text is empty, when the
    /// field has not been drawn yet, and when the point is above the text.
    /// It is the end of the text when the point is below the text.
    pub(super) fn hit(&self, position: Point<Pixels>) -> (usize, bool) {
        if self.content.is_empty() {
            return (0, false);
        }
        let Some(bounds) = self.last_bounds else {
            return (0, false);
        };
        if let Some(lines) = self.last_lines.as_ref() {
            let line_height = px(self.line_height());
            let rows = rows_of(lines, line_height);
            let y = position.y - bounds.top();
            let bottom = rows.last().map_or(line_height, |row| row.top + line_height);
            if y < px(0.) {
                return (0, false);
            }
            if y >= bottom {
                return (self.content.len(), false);
            }
            let row = rows.iter().rposition(|row| row.top <= y).unwrap_or(0);
            let (offset, upstream) = hit_row(lines, &rows[row], position.x - bounds.left());
            return (self.clamp(offset), upstream);
        }
        let Some(line) = self.last_layout.as_ref() else {
            return (0, false);
        };
        if position.y < bounds.top() {
            return (0, false);
        }
        if position.y > bounds.bottom() {
            return (self.content.len(), false);
        }
        let x = position.x - bounds.left() + self.scroll;
        (
            self.clamp(self.content_offset(line.closest_index_for_x(x))),
            false,
        )
    }

    /// Returns the byte offset of the character that is drawn at
    /// `position`, a point in window coordinates. A double-click selects the
    /// word that contains this character, and a triple-click selects its
    /// line (`TextInput::unit_at` in `mouse.rs`).
    ///
    /// This is not the same as `hit`, which returns the nearest edge
    /// between two characters. For a point on the right half of a
    /// character, this function returns the offset of that character, and
    /// `hit` returns the offset of the character after it. A point to the
    /// right of the end of a row gives the end of that row.
    pub(super) fn character_under(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds else {
            return 0;
        };
        // A point above the text gives the start of the text and a point
        // below the text gives its end. `hit` puts the caret at the same
        // two places for a press at such a point.
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        if let Some(lines) = self.last_lines.as_ref() {
            let line_height = px(self.line_height());
            let rows = rows_of(lines, line_height);
            let y = position.y - bounds.top();
            let row = &rows[rows.iter().rposition(|row| row.top <= y).unwrap_or(0)];
            let (start, line, _) = &lines[row.line];
            let x = position.x - bounds.left() + row.x0;
            let index = line
                .unwrapped_layout
                .index_for_x(x)
                .map_or(row.end, |index| start + index);
            return self.clamp(index.clamp(row.start, row.end));
        }
        let Some(line) = self.last_layout.as_ref() else {
            return 0;
        };
        let x = position.x - bounds.left() + self.scroll;
        self.clamp(
            line.index_for_x(x)
                .map_or(self.content.len(), |index| self.content_offset(index)),
        )
    }
}

/// The element that draws the text, the caret and the selection of `input`.
/// `TextInput::render` puts one inside the field's root element.
pub(super) struct TextElement {
    pub(super) input: Entity<TextInput>,
}

/// What `prepaint` computes in a frame for `paint` to draw.
pub(super) enum PrepaintState {
    /// The text is drawn on one line. `line` is the shaped line: the text,
    /// the placeholder when the text is empty, or the start of the text
    /// followed by an ellipsis. `cursor` is the caret's rectangle and
    /// `selection` is the selection's rectangle. Both are `None` when the
    /// field does not have the keyboard focus. When it has, one of the two
    /// is set: the selection's rectangle when text is selected, and the
    /// caret's rectangle otherwise. `scroll` is the number of pixels the
    /// line is drawn to the left of the element's left edge. It is negative
    /// when the line of a number field is moved to the right (see
    /// `prepaint`).
    Single {
        line: Option<ShapedLine>,
        cursor: Option<PaintQuad>,
        selection: Option<PaintQuad>,
        scroll: Pixels,
    },
    /// The text is drawn wrapped, over as many rows as it needs. `lines` is
    /// the list that `wrap` returns. `cursor` is the caret's rectangle, and
    /// `selection` has one rectangle for each row on which a part of the
    /// selection is drawn. When the field does not have the keyboard focus,
    /// `cursor` is `None` and `selection` is empty.
    Wrapped {
        lines: Vec<(usize, gpui::WrappedLine, Pixels)>,
        cursor: Option<PaintQuad>,
        selection: Vec<PaintQuad>,
        /// A rectangle 1 px wide and one row tall at the caret, in window
        /// coordinates. When text is selected, the caret is at the end of
        /// the selection that moves when the selection is extended. The
        /// value is `None` when the field does not have the keyboard focus.
        /// `paint` scrolls the area of the expanded field so that this
        /// rectangle is visible.
        focus: Option<Bounds<Pixels>>,
    },
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Returns the text runs with which `text` is shaped in `font` and `color`.
/// `marked_range` is the byte range of the text that an input method is
/// composing. When it is given and ends inside `text`, the text in that
/// range gets a 1 px underline in `color`. Otherwise the whole text is one
/// run without an underline.
fn runs_for(
    text: &str,
    font: gpui::Font,
    color: gpui::Hsla,
    marked_range: Option<&Range<usize>>,
) -> Vec<TextRun> {
    let run = TextRun {
        len: text.len(),
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    match marked_range {
        Some(marked) if marked.end <= text.len() => vec![
            TextRun {
                len: marked.start,
                ..run.clone()
            },
            TextRun {
                len: marked.end - marked.start,
                underline: Some(UnderlineStyle {
                    color: Some(run.color),
                    thickness: px(1.0),
                    wavy: false,
                }),
                ..run.clone()
            },
            TextRun {
                len: text.len() - marked.end,
                ..run
            },
        ]
        .into_iter()
        .filter(|run| run.len > 0)
        .collect(),
        _ => vec![run],
    }
}

/// Shapes `text` and wraps it at `width` pixels. When `width` is `None`, no
/// line is wrapped. The first value is a list with one entry for each line
/// of the text: the byte offset of the line's first byte in `text`, the
/// line's layout, and the line's top edge in pixels below the top of the
/// text. The second value is the height of all the lines together in
/// pixels, and at least `line_height`. When the text system cannot shape
/// the text, the list is empty.
fn wrap(
    text: &SharedString,
    font: gpui::Font,
    font_size: Pixels,
    color: gpui::Hsla,
    marked_range: Option<&Range<usize>>,
    width: Option<Pixels>,
    line_height: Pixels,
    window: &mut Window,
) -> (Vec<(usize, gpui::WrappedLine, Pixels)>, Pixels) {
    let runs = runs_for(text, font, color, marked_range);
    let shaped = window
        .text_system()
        .shape_text(text.clone(), font_size, &runs, width, None)
        .unwrap_or_default();
    let mut lines = Vec::with_capacity(shaped.len());
    let mut top = px(0.);
    for (range, line) in hard_lines(text).into_iter().zip(shaped) {
        let height = line.size(line_height).height.max(line_height);
        lines.push((range.start, line, top));
        top += height;
    }
    (lines, top.max(line_height))
}

/// Asks for `input` to be drawn again in the next frame. The text element
/// calls this while a frame is being drawn. GPUI does not act on a
/// notification that is made at that time, so the notification is made
/// when GPUI has finished the frame (`App::defer`).
fn draw_again_after_this_frame(input: gpui::WeakEntity<TextInput>, cx: &mut App) {
    cx.defer(move |cx| {
        let _ = input.update(cx, |_, cx| cx.notify());
    });
}

/// Records in `input` whether its text needs more than one line
/// (`overflowing`), which the text element finds out while it lays the text
/// out. A field is drawn expanded only while it has the keyboard and its
/// text needs more than one line (`TextInput::expanded`). A change therefore
/// alters what is drawn only when the field has the keyboard (`focused`),
/// and only then is the field drawn again, in the next frame.
fn note_overflow(input: &Entity<TextInput>, overflowing: bool, focused: bool, cx: &mut App) {
    let changed = input.update(cx, |input, _| {
        std::mem::replace(&mut input.overflowing, overflowing) != overflowing
    });
    if changed && focused {
        draw_again_after_this_frame(input.downgrade(), cx);
    }
}

impl TextElement {
    /// Returns true when the text is drawn wrapped in this frame, which is
    /// over as many rows as it needs, and false when all of it is drawn on
    /// one row. Only a field that takes line breaks (`multiline`) is drawn
    /// wrapped. A dialog's text area (`Kind::Area`) is drawn wrapped
    /// whenever it has text. Any other such field is drawn wrapped while it
    /// has the keyboard focus and its text needs more than one row
    /// (`overflowing`).
    fn wrapped(&self, window: &Window, cx: &App) -> bool {
        let input = self.input.read(cx);
        input.multiline
            && ((input.text_area() && !input.content.is_empty())
                || (input.overflowing && input.focus_handle.is_focused(window)))
    }

    /// Computes what `paint` draws when the text is wrapped. `bounds` is
    /// the element's position and size in window coordinates. The text is
    /// wrapped at the element's width. When the field has the keyboard
    /// focus, the result has the caret's rectangle, or the selection's
    /// rectangles when text is selected. The function also records whether
    /// the text needs more than one row (`note_overflow`).
    fn prepaint_wrapped(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let marked_range = input.marked_range.clone();
        let style = window.text_style();
        let theme = crate::theme::theme(cx);
        let caret: gpui::Hsla = if input.caret_in_text_color() {
            style.color
        } else {
            theme.text().into()
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let (lines, _) = wrap(
            &content,
            style.font(),
            font_size,
            style.color,
            marked_range.as_ref(),
            Some(bounds.size.width),
            line_height,
            window,
        );
        let rows = rows_of(&lines, line_height);
        let overflowing = rows.len() > 1;
        let focused = input.focus_handle.is_focused(window);
        let upstream = input.upstream;
        // The height of the caret: 1.2 times the font size. The caret is
        // 1 px wide and is centred in the height of its row.
        let glyphs = font_size * 1.2;
        let focus = caret_in(&lines, &rows, cursor, upstream)
            .filter(|_| focused)
            .map(|(x, y)| {
                Bounds::new(
                    point(bounds.left() + x, bounds.top() + y),
                    size(px(1.), line_height),
                )
            });
        let (cursor, selection) = if !focused {
            (None, Vec::new())
        } else if selected_range.is_empty() {
            let quad = caret_in(&lines, &rows, cursor, upstream).map(|(x, y)| {
                fill(
                    Bounds::new(
                        point(
                            bounds.left() + x,
                            bounds.top() + y + (line_height - glyphs) / 2.,
                        ),
                        size(px(1.), glyphs),
                    ),
                    caret,
                )
            });
            (quad, Vec::new())
        } else {
            // The selection is drawn as Chromium draws it. Each row that
            // has a part of the selection gets a rectangle from the place
            // where the selection starts on that row to the place where it
            // ends on that row, as tall as the row. When the selection
            // goes on past the line break at the end of a row, the
            // rectangle is wider by the width of one space, which shows
            // that the line break is selected.
            let space = window
                .text_system()
                .shape_line(
                    " ".into(),
                    font_size,
                    &runs_for(" ", style.font(), style.color, None),
                    None,
                )
                .width;
            let quads = rows
                .iter()
                .filter(|row| row.end >= selected_range.start && row.start <= selected_range.end)
                .filter_map(|row| {
                    let from = selected_range.start.max(row.start);
                    let to = selected_range.end.min(row.end);
                    let left = x_in(&lines, row, from);
                    let mut right = x_in(&lines, row, to);
                    if row.last && selected_range.end > row.end {
                        right += space;
                    }
                    (right > left).then(|| {
                        fill(
                            Bounds::from_corners(
                                point(bounds.left() + left, bounds.top() + row.top),
                                point(bounds.left() + right, bounds.top() + row.top + line_height),
                            ),
                            theme.selection(),
                        )
                    })
                })
                .collect();
            (None, quads)
        };
        note_overflow(&self.input, overflowing, focused, cx);
        PrepaintState::Wrapped {
            lines,
            cursor,
            selection,
            focus,
        }
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // The element is as wide as its parent. When the text is drawn on
        // one line, the element is one line height tall.
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        if !self.wrapped(window, cx) {
            style.size.height = window.line_height().into();
            return (window.request_layout(style, [], cx), ());
        }
        // When the text is drawn wrapped, the element is as tall as the
        // text is after it has been wrapped at the element's width, and at
        // least one line height tall. GPUI calls the closure below while it
        // lays the element out. The closure wraps the text at the width
        // that is known for the element, or else at the width that is
        // available to it, and returns the height of the wrapped text.
        let input = self.input.read(cx);
        let text = input.content.clone();
        let marked = input.marked_range.clone();
        let text_style = window.text_style();
        let font = text_style.font();
        let color = text_style.color;
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let layout = window.request_measured_layout(style, move |known, available, window, _| {
            let width = known.width.or(match available.width {
                gpui::AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            let (_, height) = wrap(
                &text,
                font.clone(),
                font_size,
                color,
                marked.as_ref(),
                width,
                line_height,
                window,
            );
            size(width.unwrap_or_default(), height)
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if self.wrapped(window, cx) {
            return self.prepaint_wrapped(bounds, window, cx);
        }
        let input = self.input.read(cx);
        // `drawn` returns the text as it is drawn. For a password field that
        // is one bullet (•) for each character of the text, and
        // `drawn_offset` converts a byte offset in the text to the byte
        // offset of the same character in the bullets. For every other
        // field, `drawn` returns the text and `drawn_offset` returns the
        // offset it was given. The selection, the caret and the range that
        // an input method is composing are converted, so that they are
        // drawn at the right bullets.
        let content = input.drawn();
        let selected_range =
            input.drawn_offset(input.selected_range.start)..input.drawn_offset(input.selected_range.end);
        let cursor = input.drawn_offset(input.cursor_offset());
        let focused = input.focus_handle.is_focused(window);
        let marked_range = input
            .marked_range
            .clone()
            .map(|range| input.drawn_offset(range.start)..input.drawn_offset(range.end));
        // The placeholder is translated here, each time the field is drawn,
        // so it is always shown in the language the app is set to. A
        // placeholder that was stored as its translation into another
        // language is also shown in the present language: `t_shared` finds
        // the English text that the translation belongs to and translates
        // that, as production's `translateMessage` does.
        let placeholder = crate::i18n::t_shared(input.placeholder.clone());
        let kind = input.kind;
        let multiline = input.multiline;
        let previous_scroll = input.scroll;
        let caret_in_text_color = input.caret_in_text_color();
        let style = window.text_style();
        let theme = crate::theme::theme(cx);
        let font_size = style.font_size.to_pixels(window.rem_size());
        let shape = |text: SharedString, color: gpui::Hsla, window: &mut Window| {
            let runs = runs_for(&text, style.font(), color, marked_range.as_ref());
            window
                .text_system()
                .shape_line(text, font_size, &runs, None)
        };

        let (display_text, text_color) = if content.is_empty() {
            (placeholder, kind.placeholder(theme))
        } else if content.contains(['\n', '\r']) {
            // The text has a line break but is drawn on one line here, for
            // example the text of an expanding field that does not have the
            // keyboard focus. `shape_line` shapes one line, and in a debug
            // build it panics when its text contains `\n`. Each `\n` and
            // `\r` is therefore replaced by a space in the text that is
            // drawn. A space is one byte long, as `\n` and `\r` are, so
            // every byte offset in the text is the same offset in the text
            // that is drawn. The field's text itself is not changed and
            // keeps its line breaks.
            (
                SharedString::from(content.replace(['\n', '\r'], " ")),
                style.color,
            )
        } else {
            (content.clone(), style.color)
        };
        let mut line = shape(display_text.clone(), text_color, window);
        // The text needs more than one line when it has a line break or is
        // wider than the element. A field that takes line breaks
        // (`multiline`) records this in `TextInput::overflowing`
        // (production's `measureOverflow`). While that is true and the
        // field has the keyboard focus, the text is drawn wrapped, and
        // `ui::field` draws an inspector field expanded over what is below
        // it.
        let overflowing = !content.is_empty()
            && (content.contains(['\n', '\r']) || line.width > bounds.size.width);
        if multiline {
            note_overflow(&self.input, overflowing, focused, cx);
        }

        // A field without the keyboard focus whose text is wider than the
        // element shows the start of the text followed by an ellipsis (…),
        // as the CSS `text-overflow: ellipsis` does. The text is cut at the
        // last boundary between two characters at which the text before
        // the boundary and the ellipsis together fit in the element. A field
        // that draws its text in a face of its own (`TextInput::set_face`,
        // for text that production edits in a `contentEditable` element)
        // gets no ellipsis. Its text is clipped at the element's right
        // edge.
        if !focused && !caret_in_text_color && line.width > bounds.size.width && !content.is_empty()
        {
            let ellipsis = shape("…".into(), text_color, window).width;
            let fits = display_text
                .grapheme_indices(true)
                .map(|(index, _)| index)
                .take_while(|index| line.x_for_index(*index) + ellipsis <= bounds.size.width)
                .last()
                .unwrap_or(0);
            let clipped = SharedString::from(format!("{}…", &display_text[..fits]));
            line = shape(clipped, text_color, window);
        }

        // `scroll` is the number of pixels the line is drawn to the left of
        // the element's left edge. `caret_room` is the element's width less
        // 1 px. That pixel is kept free at the right edge for a caret after
        // the last character. `scroll` is 0 when the field does not have
        // the keyboard focus, and when the line is not wider than
        // `caret_room`. A number field whose line is not wider than
        // `caret_room` is the exception, and is described below.
        //
        // When the field has the keyboard focus and the line is wider than
        // `caret_room`, the line is scrolled. `scroll` keeps the value it
        // had in the last frame, unless the caret would then be drawn to
        // the left of the element's left edge, or more than `caret_room`
        // to the right of it. In that case `scroll` changes by just as much
        // as puts the caret on that limit. The line is never scrolled so
        // far that its end is less than `caret_room` from the element's
        // left edge.
        let cursor_x = line.x_for_index(cursor.min(line.len()));
        let caret_room = bounds.size.width - px(1.);
        let right_aligned = kind == Kind::Number;
        let scroll = if right_aligned && line.width <= caret_room {
            // A number field (`Kind::Number`) draws its text against the
            // right edge. `scroll` is negative here, which moves the line
            // to the right, so that the line ends 1 px before the element's
            // right edge. The caret, the selection and `TextInput::hit` all
            // use `scroll`, so they agree with where the text is drawn.
            line.width - caret_room
        } else if !focused || line.width <= caret_room {
            px(0.)
        } else {
            previous_scroll
                .min(cursor_x)
                .max(cursor_x - caret_room)
                .min(line.width - caret_room)
                .max(px(0.))
        };
        if scroll != previous_scroll {
            self.input.update(cx, |input, _| input.scroll = scroll);
        }

        // The caret is drawn as Chromium draws it: 1 px wide and as tall as
        // the font's ascent plus its descent, centred in the element's
        // height. It is drawn only when no text is selected. When text is
        // selected, the selection is drawn as a rectangle from the start of
        // the first selected character to the end of the last one, as tall
        // as the element.
        let glyphs = line.ascent + line.descent;
        let caret_top = bounds.top() + (bounds.size.height - glyphs) / 2.;
        let left = bounds.left() - scroll;
        let (selection, cursor) = if !focused {
            (None, None)
        } else if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(point(left + cursor_x, caret_top), size(px(1.), glyphs)),
                    if caret_in_text_color {
                        style.color
                    } else {
                        theme.text().into()
                    },
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(left + line.x_for_index(selected_range.start), bounds.top()),
                        point(left + line.x_for_index(selected_range.end), bounds.bottom()),
                    ),
                    theme.selection(),
                )),
                None,
            )
        };
        PrepaintState::Single {
            line: Some(line),
            cursor,
            selection,
            scroll,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // When the field has the keyboard focus, this makes the field the
        // receiver of typed text and of the input method's requests, until
        // the next frame is drawn.
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let line_height = window.line_height();
        // The caret blinks: it is drawn for 500 ms (`motion::CARET_BLINK`),
        // then not drawn for 500 ms, and so on. The time is counted from
        // the moment the caret last moved or the text last changed
        // (`TextInput::blink`), so the caret is drawn right after either.
        let since_move = self.input.read(cx).blink.elapsed();
        let caret_on = (since_move.as_millis() / crate::motion::CARET_BLINK.as_millis()) % 2 == 0;
        let (single, wrapped) = match prepaint {
            PrepaintState::Single {
                line,
                cursor,
                selection,
                scroll,
            } => {
                let line = line.take().unwrap();
                let origin = point(bounds.left() - *scroll, bounds.top());
                // Nothing is drawn outside the element's bounds, so the
                // part of a scrolled line that is outside them is not
                // visible.
                window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                    if let Some(selection) = selection.take() {
                        window.paint_quad(selection)
                    }
                    // When the text system returns an error for the line,
                    // the line is not drawn in this frame and the error is
                    // written to standard error. The app keeps running.
                    if let Err(error) =
                        line.paint(origin, line_height, gpui::TextAlign::Left, None, window, cx)
                    {
                        eprintln!("text input: a line could not be painted: {error}");
                    }
                    if let Some(cursor) = cursor.take().filter(|_| caret_on) {
                        window.paint_quad(cursor);
                    }
                });
                (Some(line), None)
            }
            PrepaintState::Wrapped {
                lines,
                cursor,
                selection,
                focus,
            } => {
                for quad in selection.drain(..) {
                    window.paint_quad(quad);
                }
                for (_, line, top) in lines.iter() {
                    let origin = point(bounds.left(), bounds.top() + *top);
                    if let Err(error) = line.paint(
                        origin,
                        line_height,
                        gpui::TextAlign::Left,
                        Some(bounds),
                        window,
                        cx,
                    ) {
                        eprintln!("text input: a line could not be painted: {error}");
                    }
                }
                if let Some(cursor) = cursor.take().filter(|_| caret_on) {
                    window.paint_quad(cursor);
                }
                (None, Some((std::mem::take(lines), focus.take())))
            }
        };

        // This listener is called for every move of the pointer until the
        // next frame is drawn, also when the pointer is not over the field.
        // It does nothing unless a press that started in the field is still
        // held (`is_selecting`). While one is held, a move with the left
        // button down extends the selection to the pointer
        // (`TextInput::drag`). A move without the left button down ends the
        // drag (`TextInput::end_drag`), so that the selection stops
        // following the pointer when the field did not receive the release
        // of the button.
        let input = self.input.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase != DispatchPhase::Bubble || !input.read(cx).is_selecting {
                return;
            }
            input.update(cx, |input, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    input.drag(event.position, cx);
                } else {
                    input.end_drag();
                }
            });
        });

        let focused = focus_handle.is_focused(window);
        self.input.update(cx, |input, cx| {
            // The element's bounds and the layout of the text are stored in
            // the `TextInput`. Code that runs between frames uses them, for
            // example `hit`, `character_under`, the commands that move the
            // caret up or down a row, and the answers to the input method.
            // `last_layout` is set when the text was drawn on one line, and
            // `last_lines` when it was drawn wrapped. The other one of the
            // two is set to `None`.
            input.last_bounds = Some(bounds);
            match (single, wrapped) {
                (Some(line), _) => {
                    input.last_layout = Some(line);
                    input.last_lines = None;
                }
                (_, Some((lines, caret))) => {
                    input.last_layout = None;
                    input.last_lines = Some(lines);
                    // `area` is the scrolling box that `ui::field` draws
                    // around the text of an expanded field. The box has
                    // 6 px of padding above and below the text. When the
                    // field has the keyboard focus and the caret is at
                    // another offset than the one the box was last scrolled
                    // for (`revealed`), the box is scrolled so that the
                    // caret's row and the 6 px above and below it are
                    // visible. If the point 6 px below the row is below the
                    // box's bottom edge, the box scrolls down by the
                    // difference. Otherwise, if the point 6 px above the
                    // row is above the box's top edge, the box scrolls up
                    // by the difference.
                    let offset = input.cursor_offset();
                    if let Some(caret) = caret
                        && input.revealed != Some(offset)
                    {
                        input.revealed = Some(offset);
                        let area = input.area.bounds();
                        let top = caret.top() - px(6.);
                        let bottom = caret.bottom() + px(6.);
                        let mut scroll = input.area.offset();
                        if bottom > area.bottom() {
                            scroll.y -= bottom - area.bottom();
                        } else if top < area.top() {
                            scroll.y += area.top() - top;
                        }
                        if scroll != input.area.offset() {
                            // The area was laid out for this frame before
                            // its text was, so this frame still shows the
                            // old scroll position. The field is drawn again
                            // in the next frame, with the caret in view.
                            input.area.set_offset(scroll);
                            draw_again_after_this_frame(cx.weak_entity(), cx);
                        }
                    }
                }
                _ => {}
            }
            // While the field has the keyboard focus, a timer makes the
            // caret blink. The timer ends 1 ms after the moment at which the
            // caret next changes between drawn and not drawn, and then asks
            // for the field to be drawn again. Only one timer runs at a
            // time (`blink_task`). The frame that a timer causes starts the
            // next timer.
            if focused && input.blink_task.is_none() {
                let wait = crate::motion::CARET_BLINK.as_millis()
                    - since_move.as_millis() % crate::motion::CARET_BLINK.as_millis();
                input.blink_task = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(wait as u64 + 1))
                        .await;
                    let _ = this.update(cx, |this: &mut TextInput, cx| {
                        this.blink_task = None;
                        cx.notify();
                    });
                }));
            }
        });
    }
}
