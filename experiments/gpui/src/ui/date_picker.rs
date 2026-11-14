//! `DatePickerButton.tsx`: a button showing a day, which opens a month's
//! calendar below it (above it without room), with Today and Clear. Days
//! are local `YYYY-MM-DD` keys; an empty key is no day.
use crate::theme;
use crate::theme::Text as _;
use chrono::{Datelike, Duration as Days, Local, NaiveDate};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::{cell::Cell, rc::Rc};

type ChangeHandler = Rc<dyn Fn(String, &mut Window, &mut App) + 'static>;

/// `.popover`'s width and the height it is placed by.
const WIDTH: f32 = 278.;
const HEIGHT: f32 = 340.;
const MARGIN: f32 = 8.;

struct State {
    open: bool,
    /// The first day of the month shown.
    month: NaiveDate,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

#[derive(IntoElement)]
pub struct DatePicker {
    id: ElementId,
    value: SharedString,
    on_change: Option<ChangeHandler>,
    layer: usize,
}

pub fn date_picker(id: impl Into<ElementId>, value: impl Into<SharedString>) -> DatePicker {
    DatePicker {
        id: id.into(),
        value: value.into(),
        on_change: None,
        layer: 3,
    }
}

impl DatePicker {
    /// The calendar's drawing order, above a dialog the picker sits in.
    pub fn layer(mut self, priority: usize) -> Self {
        self.layer = priority;
        self
    }

    /// Called with the day chosen, or an empty key for Clear.
    pub fn on_change(mut self, handler: impl Fn(String, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

/// `parseDate`: a valid `YYYY-MM-DD` day.
pub fn parse_day(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, crate::text::date_format::DAY)
        .ok()
        .filter(|date| date.format(crate::text::date_format::DAY).to_string() == value)
}

/// `calendarDays`: six weeks from the Sunday on or before the month's first.
pub fn calendar_days(month: NaiveDate) -> Vec<NaiveDate> {
    let first = month.with_day(1).unwrap_or(month);
    let start = first - Days::days(first.weekday().num_days_from_sunday() as i64);
    (0..42).map(|offset| start + Days::days(offset)).collect()
}

fn add_months(month: NaiveDate, delta: i32) -> NaiveDate {
    let index = month.year() * 12 + month.month0() as i32 + delta;
    NaiveDate::from_ymd_opt(index.div_euclid(12), index.rem_euclid(12) as u32 + 1, 1)
        .unwrap_or(month)
}

/// `.popover`'s place: under the button's left edge, kept 8px inside the
/// window, or above the button when it does not fit below.
pub fn place(button: Bounds<Pixels>, viewport: Size<Pixels>) -> (f32, f32) {
    place_sized(button, viewport, WIDTH, HEIGHT)
}

/// [`place`] for a popover of another size, as the smart-folder rules'
/// colour popover is placed the same way.
pub fn place_sized(
    button: Bounds<Pixels>,
    viewport: Size<Pixels>,
    popover_width: f32,
    popover_height: f32,
) -> (f32, f32) {
    let (left, top, bottom) = (
        f32::from(button.left()),
        f32::from(button.top()),
        f32::from(button.bottom()),
    );
    let (width, height) = (f32::from(viewport.width), f32::from(viewport.height));
    let x = left.min(width - popover_width - MARGIN).max(MARGIN);
    let y = if bottom + popover_height <= height - MARGIN {
        bottom + 5.
    } else {
        (top - popover_height - 5.).max(MARGIN)
    };
    (x, y)
}

/// `.navButton` and `.footerButton`: 26px, a faint border and fill that
/// deepen under the pointer.
/// Opens or closes the calendar, at the chosen day's month (today's
/// without one).
fn toggle_open(state: &Entity<State>, selected: Option<NaiveDate>, cx: &mut App) {
    state.update(cx, |state, cx| {
        state.open = !state.open;
        let shown = selected.unwrap_or_else(|| Local::now().date_naive());
        state.month = shown.with_day(1).unwrap_or(shown);
        cx.notify();
    });
}

fn frame_button(id: impl Into<ElementId>, theme: theme::Theme) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(theme::CONTROL_SM))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_MD))
        .border_1()
        .border_color(theme.contrast(14.))
        .bg(theme.contrast(5.))
        .cursor_pointer()
}

/// `.trigger`, `.navButton`, `.day` and `.footerButton` `:focus-visible`:
/// 1px of the primary text colour, 1px out.
fn ring(theme: &theme::Theme) -> super::focus::Ring {
    super::focus::Ring {
        width: 1.,
        offset: 1.,
        color: theme.text(),
    }
}

/// A button of the calendar: a stop that Enter and Space press, with its
/// ring. `border` is how thick the button's own border is.
fn calendar_button(
    button: Stateful<Div>,
    id: ElementId,
    border: f32,
    window: &mut Window,
    cx: &mut App,
    press: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let theme = theme::theme(cx);
    let focus = super::focus::control(&id, window, cx);
    let button = button
        .relative()
        .children(focus.ring(ring(&theme), theme::RADIUS_MD, border));
    focus.attach(button, super::focus::Kind::Button, move |_, window, cx| {
        press(window, cx)
    })
}

impl RenderOnce for DatePicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let selected = parse_day(&self.value);
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| State {
            open: false,
            month: Local::now().date_naive(),
            bounds: Rc::new(Cell::new(Bounds::default())),
        });
        let (open, month, bounds) = {
            let state = state.read(cx);
            (state.open, state.month, state.bounds.clone())
        };
        let display = selected
            .map(|day| {
                day.format(crate::text::date_format::MEDIUM_DATE)
                    .to_string()
            })
            .unwrap_or_else(|| "Choose date".into());
        let toggle = state.clone();
        let mut trigger = div()
            .id(self.id.clone())
            .occlude()
            .relative()
            .w_full()
            .min_w(px(138.))
            .h(px(theme::CONTROL_SM))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(7.))
            // `font-variant-numeric: tabular-nums`
            .font_features(theme::tabular_figures())
            .overflow_hidden()
            .rounded(px(theme::RADIUS_MD))
            .border_1()
            .border_color(theme.contrast(if open { 24. } else { 14. }))
            .bg(if open {
                theme.surface_hover()
            } else {
                theme.control_surface()
            })
            .hover(move |style| {
                style
                    .border_color(theme.contrast(24.))
                    .bg(theme.surface_hover())
            })
            .text_color(if selected.is_some() {
                theme.text()
            } else {
                theme.text_tertiary()
            })
            .cursor_pointer()
            .child(super::icon("date-calendar", 14., theme.text()).flex_none())
            .child(div().min_w_0().truncate().child(display))
            .child({
                let bounds = bounds.clone();
                canvas(move |painted, _, _| bounds.set(painted), |_, _, _, _| {})
                    .absolute()
                    .inset_0()
            })
            .on_click({
                let toggle = toggle.clone();
                move |_, _, cx| {
                    cx.stop_propagation();
                    toggle_open(&toggle, selected, cx);
                }
            });
        if selected.is_none() {
            trigger = trigger.text_color(theme.text_tertiary());
        }
        // The trigger clips its label, so its ring is drawn beside it.
        let focus = super::focus::control(&self.id, window, cx);
        let trigger_ring = focus.ring(ring(&theme), theme::RADIUS_MD, 0.);
        let trigger = focus.attach(trigger, super::focus::Kind::Button, move |_, _, cx| {
            toggle_open(&toggle, selected, cx)
        });
        let trigger = div()
            .relative()
            .w_full()
            .child(trigger)
            .children(trigger_ring);
        if !open {
            return trigger;
        }

        let today = Local::now().date_naive();
        let handler = self.on_change;
        let close = {
            let state = state.clone();
            move |cx: &mut App| {
                state.update(cx, |state, cx| {
                    state.open = false;
                    cx.notify();
                })
            }
        };
        let choose = {
            let close = close.clone();
            let handler = handler.clone();
            Rc::new(
                move |day: Option<NaiveDate>, window: &mut Window, cx: &mut App| {
                    close(cx);
                    if let Some(handler) = &handler {
                        handler(
                            day.map(|day| day.format(crate::text::date_format::DAY).to_string())
                                .unwrap_or_default(),
                            window,
                            cx,
                        );
                    }
                },
            )
        };
        let named = |name: &str| {
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), name.to_owned().into())
        };
        let mut step = |delta: i32, label: &'static str, icon: &'static str| {
            let state = state.clone();
            let turn = move |cx: &mut App| {
                state.update(cx, |state, cx| {
                    state.month = add_months(state.month, delta);
                    cx.notify();
                })
            };
            let pressed = turn.clone();
            let button = frame_button(label, theme)
                .w(px(theme::CONTROL_SM))
                .hover(move |style| {
                    style
                        .border_color(theme.contrast(22.))
                        .bg(theme.contrast(11.))
                })
                .child(super::icon(icon, 15., theme.text()))
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    turn(cx);
                });
            calendar_button(button, named(label), 1., window, cx, move |_, cx| {
                pressed(cx)
            })
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .mb(px(7.))
            .child(step(-1, "date-previous-month", "date-previous"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .justify_center()
                    .truncate()
                    .text(theme::TEXT_MD)
                    .font_weight(FontWeight::MEDIUM)
                    .child(
                        month
                            .format(crate::text::date_format::MONTH_YEAR)
                            .to_string(),
                    ),
            )
            .child(step(1, "date-next-month", "date-next"));
        let weekdays = div()
            .flex()
            .gap(px(2.))
            .mb(px(3.))
            .text(theme::TEXT_XS)
            .line_height(px(22.))
            .text_color(theme.text_tertiary())
            .children(
                ["S", "M", "T", "W", "T", "F", "S"]
                    .into_iter()
                    .map(|day| div().flex_1().flex().justify_center().child(day)),
            );
        let mut outside = theme.text_tertiary();
        outside.a *= 0.48;
        let mut grid = div().flex().flex_col().gap(px(2.));
        for week in calendar_days(month).chunks(7) {
            let mut row = div().flex().gap(px(2.));
            for day in week {
                let day = *day;
                let in_month = day.month() == month.month();
                let is_selected = selected == Some(day);
                let is_today = day == today;
                let pick = choose.clone();
                let pressed = choose.clone();
                let mut cell = div()
                    .id(SharedString::from(format!("date-{day}")))
                    .relative()
                    .flex_1()
                    // `aspect-ratio: 1` on a seventh of the 260px grid.
                    .h(px((WIDTH - 18. - 12.) / 7.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(theme::RADIUS_MD))
                    .font_features(theme::tabular_figures())
                    .cursor_pointer()
                    .text_color(if in_month { theme.text() } else { outside })
                    .child(day.day().to_string())
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        pick(Some(day), window, cx)
                    });
                cell = if is_selected {
                    cell.bg(theme.text())
                        .text_color(theme.background())
                        .shadow(vec![
                            BoxShadow::new(
                                px(0.),
                                px(1.),
                                rgba(theme::DATE_SELECTED_HIGHLIGHT).into(),
                            )
                            .inset(),
                        ])
                } else {
                    cell.hover(move |style| style.bg(theme.contrast(10.)))
                };
                if is_today {
                    // `.today::after`: a 1px line 4px from the bottom.
                    cell = cell.child(
                        div()
                            .absolute()
                            .left(px(7.))
                            .right(px(7.))
                            .bottom(px(4.))
                            .h(px(1.))
                            .rounded(px(1.))
                            .bg(if is_selected {
                                theme.background()
                            } else if in_month {
                                theme.text()
                            } else {
                                outside
                            }),
                    );
                }
                let cell = calendar_button(
                    cell,
                    named(&format!("day-{day}")),
                    0.,
                    window,
                    cx,
                    move |window, cx| pressed(Some(day), window, cx),
                );
                row = row.child(cell);
            }
            grid = grid.child(row);
        }
        let today_pick = choose.clone();
        let today_press = choose.clone();
        let clear_press = choose.clone();
        let clear_pick = choose;
        let today_button = calendar_button(
            frame_button("date-today", theme)
                .min_w(px(54.))
                .px(px(9.))
                .hover(move |style| {
                    style
                        .border_color(theme.contrast(22.))
                        .bg(theme.contrast(11.))
                })
                .child("Today")
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    today_pick(Some(today), window, cx)
                }),
            named("today"),
            1.,
            window,
            cx,
            move |window, cx| today_press(Some(today), window, cx),
        );
        let mut clear_button = frame_button("date-clear", theme)
            .min_w(px(54.))
            .px(px(9.))
            .when(selected.is_none(), |button| {
                button.opacity(0.35).cursor_default()
            })
            .when(selected.is_some(), |button| {
                button
                    .hover(move |style| {
                        style
                            .border_color(theme.contrast(22.))
                            .bg(theme.contrast(11.))
                    })
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        clear_pick(None, window, cx)
                    })
            })
            .child("Clear");
        // Disabled with no day, and then no stop.
        if selected.is_some() {
            clear_button = calendar_button(
                clear_button,
                named("clear"),
                1.,
                window,
                cx,
                move |window, cx| clear_press(None, window, cx),
            );
        }
        let footer = div()
            .flex()
            .justify_end()
            .gap(px(6.))
            .mt(px(8.))
            .pt(px(8.))
            .border_t_1()
            .border_color(theme.border_secondary())
            .child(today_button)
            .child(clear_button);
        let (x, y) = place(bounds.get(), window.viewport_size());
        let popover = super::glass_panel(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "calendar".into()),
            theme::RADIUS_LG,
            cx,
        )
        .occlude()
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(WIDTH))
        // `.popover`'s 9px padding inside its 1px glass border.
        .p(px(10.))
        .font_family(theme::FONT_UI)
        .text(theme::TEXT_SM)
        .text_color(theme.text())
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(header)
        .child(weekdays)
        .child(grid)
        .child(footer);
        // `.backdrop`: a press anywhere else closes the calendar.
        let backdrop = div()
            .id(ElementId::NamedChild(
                std::sync::Arc::new(self.id.clone()),
                "backdrop".into(),
            ))
            .occlude()
            .absolute()
            .inset_0()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                close(cx);
            });
        let viewport = window.viewport_size();
        div().w_full().child(trigger).child(
            deferred(
                anchored()
                    .position_mode(AnchoredPositionMode::Window)
                    .position(point(px(0.), px(0.)))
                    .child(
                        div()
                            .w(viewport.width)
                            .h(viewport.height)
                            .child(backdrop)
                            .child(popover),
                    ),
            )
            .with_priority(self.layer),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{add_months, calendar_days, parse_day, place, place_sized};
    use chrono::NaiveDate;
    use gpui::{Bounds, point, px, size};

    #[test]
    fn days_parse_only_when_real() {
        assert_eq!(
            parse_day("2025-02-28"),
            NaiveDate::from_ymd_opt(2025, 2, 28)
        );
        assert_eq!(parse_day("2025-02-30"), None);
        assert_eq!(parse_day("2025-2-3"), None);
        assert_eq!(parse_day(""), None);
    }

    #[test]
    fn the_calendar_starts_on_a_sunday_and_shows_six_weeks() {
        // November 2025 starts on a Saturday.
        let days = calendar_days(NaiveDate::from_ymd_opt(2025, 11, 1).unwrap());
        assert_eq!(days.len(), 42);
        assert_eq!(days[0], NaiveDate::from_ymd_opt(2025, 10, 26).unwrap());
        assert_eq!(days[6], NaiveDate::from_ymd_opt(2025, 11, 1).unwrap());
        assert_eq!(
            add_months(NaiveDate::from_ymd_opt(2025, 12, 1).unwrap(), 1),
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
        );
        assert_eq!(
            add_months(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(), -1),
            NaiveDate::from_ymd_opt(2024, 12, 1).unwrap()
        );
    }

    #[test]
    fn the_calendar_opens_below_unless_it_does_not_fit() {
        let viewport = size(px(1000.), px(800.));
        let button = Bounds::new(point(px(900.), px(100.)), size(px(140.), px(26.)));
        // Kept 8px inside the right edge, 5px below the button.
        assert_eq!(place(button, viewport), (1000. - 278. - 8., 131.));
        let low = Bounds::new(point(px(20.), px(700.)), size(px(140.), px(26.)));
        assert_eq!(place(low, viewport), (20., 700. - 340. - 5.));
        // A smaller popover (the rules' colour, 230×286) fits below there.
        let middle = Bounds::new(point(px(20.), px(480.)), size(px(140.), px(26.)));
        assert_eq!(place_sized(middle, viewport, 230., 286.), (20., 511.));
        assert_eq!(place(middle, viewport), (20., 480. - 340. - 5.));
    }
}
