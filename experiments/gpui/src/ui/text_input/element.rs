// Adapted from gpui 0.2.2 examples/input.rs (Apache-2.0).
//! The element that lays out, wraps and paints a field's text, and hit
//! testing against what it last drew.
use std::ops::Range;

use super::editing::hard_lines;
use super::*;
use gpui::{
    Bounds, DispatchPhase, ElementId, ElementInputHandler, Entity, GlobalElementId, LayoutId,
    MouseMoveEvent, PaintQuad, Pixels, Point, Style, TextRun, UnderlineStyle, fill, point,
    relative, size,
};
use unicode_segmentation::*;
/// One row of wrapped text as drawn: its bytes in the whole text, its top
/// edge, where it starts along its unwrapped line, which line that is, and
/// whether the row ends that line (rather than at a wrap).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) top: Pixels,
    pub(super) x0: Pixels,
    pub(super) line: usize,
    pub(super) last: bool,
}

/// The rows of wrapped lines, top to bottom.
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

/// The row a caret at `offset` is drawn on. At a wrap the offset is both
/// the end of the upper row and the start of the lower; it is on the lower
/// unless `upstream`.
pub(super) fn row_index(rows: &[Row], offset: usize, upstream: bool) -> usize {
    rows.iter()
        .position(|row| {
            row.start <= offset
                && (offset < row.end || (offset == row.end && (row.last || upstream)))
        })
        .unwrap_or(rows.len().saturating_sub(1))
}

/// How far from the left of its row `offset` is drawn.
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

/// The caret position in a row nearest to `x` from its left, and whether
/// it is the row's end at a wrap.
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

/// Where `offset` is drawn in wrapped lines, from the text's top left.
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
    /// Where a press lands in the text: the nearest caret position, and
    /// whether it is after the upper line at a wrap.
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
        (self.clamp(line.closest_index_for_x(x)), false)
    }

    /// The character under a point, which a double-click selects the word
    /// of: the one whose box holds the point, not the nearest edge.
    pub(super) fn character_under(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds else {
            return 0;
        };
        // Above or below the text, as a press there places the caret.
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
        self.clamp(line.index_for_x(x).unwrap_or(self.content.len()))
    }
}

pub(super) struct TextElement {
    pub(super) input: Entity<TextInput>,
}

pub(super) enum PrepaintState {
    /// One line, scrolled sideways to keep the caret in view.
    Single {
        line: Option<ShapedLine>,
        cursor: Option<PaintQuad>,
        selection: Option<PaintQuad>,
        scroll: Pixels,
    },
    /// An expanded field's text, wrapped over as many lines as it needs.
    Wrapped {
        lines: Vec<(usize, gpui::WrappedLine, Pixels)>,
        cursor: Option<PaintQuad>,
        selection: Vec<PaintQuad>,
        /// Where the selection's moving end is, which the expanded area
        /// keeps in view.
        focus: Option<Bounds<Pixels>>,
    },
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// The runs for `text`, with the input method's marked range underlined.
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

/// `text` wrapped to `width`: each line's first byte, its layout and its top
/// edge, and the height of them all.
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

impl TextElement {
    /// Whether the input is drawn wrapped this frame.
    fn wrapped(&self, window: &Window, cx: &App) -> bool {
        let input = self.input.read(cx);
        input.multiline
            && ((input.text_area() && !input.content.is_empty())
                || (input.overflowing && input.focus_handle.is_focused(window)))
    }

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
            // Chromium's selection: each row from where the selection
            // starts in it to where it ends, and a space's width more where
            // it runs on through a line break.
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
        // A notification made while a frame is drawn schedules no frame of
        // its own, so the owner's expansion is asked for explicitly.
        let changed = self.input.update(cx, |input, cx| {
            let changed = input.overflowing != overflowing;
            if changed {
                input.overflowing = overflowing;
                cx.notify();
            }
            changed
        });
        if changed {
            window.request_animation_frame();
        }
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
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        if !self.wrapped(window, cx) {
            style.size.height = window.line_height().into();
            return (window.request_layout(style, [], cx), ());
        }
        // Expanded: as tall as the text wraps at the width it is given.
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
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let focused = input.focus_handle.is_focused(window);
        let marked_range = input.marked_range.clone();
        // Shown in the language of the moment, as production's
        // `translateMessage` recovers a placeholder made in another one.
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
            // This element shapes one line, and a line break panics the shaper.
            // A space is the same byte length, so cursor offsets stay valid and
            // the stored value keeps its real line breaks.
            (
                SharedString::from(content.replace(['\n', '\r'], " ")),
                style.color,
            )
        } else {
            (content.clone(), style.color)
        };
        let mut line = shape(display_text.clone(), text_color, window);
        // An expanding field notes when its text needs more than one line
        // (`measureOverflow`), so its owner can expand it while focused.
        let overflowing = !content.is_empty()
            && (content.contains(['\n', '\r']) || line.width > bounds.size.width);
        if multiline && input.overflowing != overflowing {
            self.input.update(cx, |input, cx| {
                input.overflowing = overflowing;
                cx.notify();
            });
            // Made while a frame is drawn: ask for the next one.
            window.request_animation_frame();
        }

        // A field that is not being edited shows its start and an ellipsis,
        // as `text-overflow: ellipsis` does; text edited in its own face
        // (`contentEditable`) is cut off instead.
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

        // While editing, the text scrolls just far enough to show the caret.
        let cursor_x = line.x_for_index(cursor.min(line.len()));
        let caret_room = bounds.size.width - px(1.);
        let right_aligned = kind == Kind::Number;
        let scroll = if right_aligned && line.width <= caret_room {
            // Text set against the right edge: a negative scroll moves it
            // there, and the caret, selection and hit testing follow.
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

        // Chromium's caret: one pixel wide and as tall as the glyphs.
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
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let line_height = window.line_height();
        // The caret is on for the first half of each second since it moved.
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
                window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                    if let Some(selection) = selection.take() {
                        window.paint_quad(selection)
                    }
                    // A line the text system cannot draw is left out of this
                    // frame rather than ending the app.
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

        // A press in the field goes on selecting wherever the pointer goes,
        // in the window or past it, while the button is held.
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
            input.last_bounds = Some(bounds);
            match (single, wrapped) {
                (Some(line), _) => {
                    input.last_layout = Some(line);
                    input.last_lines = None;
                }
                (_, Some((lines, caret))) => {
                    input.last_layout = None;
                    input.last_lines = Some(lines);
                    // The expanded area scrolls to keep a moved caret in view.
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
                            input.area.set_offset(scroll);
                            cx.notify();
                            // Made while a frame is drawn: ask for the
                            // next one, or the caret stays out of view
                            // until something else redraws the field.
                            window.request_animation_frame();
                        }
                    }
                }
                _ => {}
            }
            // Repaint at the caret's next change for as long as it shows.
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
