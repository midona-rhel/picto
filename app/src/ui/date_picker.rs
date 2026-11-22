//! The date picker: a button that shows a day and opens a calendar of one
//! month below it, or above it when there is no room below. The calendar has
//! the buttons Today and Clear. A day is a text of the form `YYYY-MM-DD` in
//! local time. An empty text means no day.
use super::popover::{Dismissal, Flip, Placement, Side, floating, place};
use crate::i18n::{en, t};
use crate::theme;
use crate::theme::Text as _;
use chrono::{Datelike, Duration as Days, Local, NaiveDate};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::{cell::Cell, rc::Rc};

type ChangeHandler = Rc<dyn Fn(String, &mut Window, &mut App) + 'static>;

/// The calendar's width, and the height that is used to place it.
const WIDTH: f32 = 278.;
const HEIGHT: f32 = 340.;
/// Where the calendar opens: 5px below the button, with its left edge at the
/// button's left edge, and at least 8px inside the window. It opens above the
/// button when it does not fit below.
const PLACEMENT: Placement = Placement::new(Side::Below, 5., 8.).flip(Flip::WhenNoRoom);

struct State {
    open: bool,
    /// The first day of the month that the calendar shows.
    month: NaiveDate,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

#[derive(IntoElement)]
pub struct DatePicker {
    id: ElementId,
    value: SharedString,
    /// The name that assistive technology gives the button and the calendar.
    name: Option<SharedString>,
    on_change: Option<ChangeHandler>,
}

pub fn date_picker(id: impl Into<ElementId>, value: impl Into<SharedString>) -> DatePicker {
    DatePicker {
        id: id.into(),
        value: value.into(),
        name: None,
        on_change: None,
    }
}

impl DatePicker {
    /// Sets the name that assistive technology gives the picker, for example
    /// "From" or "To".
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the handler that is called with the day the user chose, or with an
    /// empty text when the user pressed Clear.
    pub fn on_change(mut self, handler: impl Fn(String, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

/// Returns the day that `value` names, if `value` is a real day written as
/// `YYYY-MM-DD`.
fn parse_day(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, crate::text::date_format::DAY)
        .ok()
        .filter(|date| date.format(crate::text::date_format::DAY).to_string() == value)
}

/// Returns the 42 days that the calendar shows for `month`: six weeks,
/// starting on the Sunday on or before the first day of the month.
fn calendar_days(month: NaiveDate) -> Vec<NaiveDate> {
    let first = month.with_day(1).unwrap_or(month);
    let start = first - Days::days(first.weekday().num_days_from_sunday() as i64);
    (0..42).map(|offset| start + Days::days(offset)).collect()
}

fn add_months(month: NaiveDate, delta: i32) -> NaiveDate {
    let index = month.year() * 12 + month.month0() as i32 + delta;
    NaiveDate::from_ymd_opt(index.div_euclid(12), index.rem_euclid(12) as u32 + 1, 1)
        .unwrap_or(month)
}

/// Opens the calendar, or closes it when it is open. It opens at the month of
/// the chosen day, or at the current month when no day is chosen. The keyboard
/// focus stays where it was, so Escape reaches the dialog or the menu that the
/// picker is in.
fn toggle_open(state: &Entity<State>, selected: Option<NaiveDate>, cx: &mut App) {
    state.update(cx, |state, cx| {
        state.open = !state.open;
        let shown = selected.unwrap_or_else(|| Local::now().date_naive());
        state.month = shown.with_day(1).unwrap_or(shown);
        cx.notify();
    });
}

fn close(state: &Entity<State>, cx: &mut App) {
    state.update(cx, |state, cx| {
        state.open = false;
        cx.notify();
    });
}

/// Returns a button of the calendar's header or footer, without its contents:
/// 26px high, with a faint border and a faint fill. The caller makes the
/// border and the fill stronger under the pointer.
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

/// Returns the focus ring of the picker's button and of the calendar's
/// buttons: 1px wide, in the primary text colour, 1px outside the button.
fn ring(theme: &theme::Theme) -> super::focus::Ring {
    super::focus::Ring {
        width: 1.,
        offset: 1.,
        color: theme.text(),
    }
}

/// Makes `button` a button of the calendar: a Tab stop that Enter and Space
/// press, named `name` for assistive technology, with its focus ring. `border`
/// is the width of the button's own border.
#[allow(clippy::too_many_arguments)]
fn calendar_button(
    button: Stateful<Div>,
    id: ElementId,
    border: f32,
    name: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
    press: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let theme = theme::theme(cx);
    let focus = super::focus::control(&id, window, cx);
    let button = button
        .role(Role::Button)
        .aria_label(name)
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
            .map(crate::i18n::format::short_month_date)
            .unwrap_or_else(|| en("Choose date").into());
        let toggle = state.clone();
        let mut trigger = div()
            .id(self.id.clone())
            .role(Role::Button)
            .aria_expanded(open)
            .aria_value(display.clone())
            .when_some(self.name.clone(), |trigger, name| trigger.aria_label(name))
            .occlude()
            .relative()
            .w_full()
            .min_w(px(138.))
            .h(px(theme::CONTROL_SM))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(7.))
            // The digits are tabular figures, which all have the same width.
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
        // The button clips what is inside it, so its focus ring is not its
        // child. The ring is drawn by the element that wraps the button.
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
        let choose = {
            let state = state.clone();
            let handler = handler.clone();
            Rc::new(
                move |day: Option<NaiveDate>, window: &mut Window, cx: &mut App| {
                    close(&state, cx);
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
        let mut step = |delta: i32, label: &'static str, icon: &'static str, name: &str| {
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
            calendar_button(
                button,
                named(label),
                1.,
                name.to_owned(),
                window,
                cx,
                move |_, cx| pressed(cx),
            )
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .mb(px(7.))
            .child(step(
                -1,
                "date-previous-month",
                "date-previous",
                t("Previous month"),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .justify_center()
                    .truncate()
                    .text(theme::TEXT_MD)
                    .font_weight(FontWeight::MEDIUM)
                    .child(crate::i18n::format::month_year(month)),
            )
            .child(step(1, "date-next-month", "date-next", t("Next month")));
        let weekdays = div()
            .flex()
            .gap(px(2.))
            .mb(px(3.))
            .text(theme::TEXT_XS)
            .line_height(px(22.))
            .text_color(theme.text_tertiary())
            .children(
                // The initials of the weekdays are English in every language.
                [
                    en("S"),
                    en("M"),
                    en("T"),
                    en("W"),
                    en("T"),
                    en("F"),
                    en("S"),
                ]
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
                    .aria_toggled(super::a11y::toggled(is_selected))
                    .relative()
                    .flex_1()
                    // A cell is as high as it is wide: a seventh of the 260px
                    // inside the calendar's padding, less the six gaps of 2px
                    // between the cells.
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
                    // Today has a 1px line under its number, 4px from the
                    // bottom of the cell.
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
                    crate::i18n::format::date(day),
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
                .child(t("Today"))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    today_pick(Some(today), window, cx)
                }),
            named("today"),
            1.,
            t("Today"),
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
            .child(t("Clear"));
        // With no day chosen, Clear is disabled and is not a Tab stop.
        if selected.is_none() {
            clear_button = clear_button
                .role(Role::Button)
                .aria_label(t("Clear"))
                .aria_disabled(true);
        } else {
            clear_button = calendar_button(
                clear_button,
                named("clear"),
                1.,
                t("Clear"),
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
        let corner = place(
            bounds.get(),
            size(WIDTH, HEIGHT),
            window.viewport_size(),
            PLACEMENT,
        );
        let popover = super::glass_panel(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "calendar".into()),
            theme::RADIUS_LG,
            cx,
        )
        .role(Role::Dialog)
        .when_some(self.name.clone(), |popover, name| popover.aria_label(name))
        .w(px(WIDTH))
        // The calendar has 9px of padding inside its 1px glass border. The
        // border is drawn over the contents, so the padding here is 10px.
        .p(px(10.))
        .font_family(theme::FONT_UI)
        .text(theme::TEXT_SM)
        .text_color(theme.text())
        .child(header)
        .child(weekdays)
        .child(grid)
        .child(footer);
        // A press anywhere outside the calendar closes it.
        div().w_full().child(trigger).child(
            floating(
                ElementId::NamedChild(std::sync::Arc::new(self.id), "floating".into()),
                super::layer::POPOVER,
            )
            .at(corner)
            .dismiss(Dismissal::Backdrop { right: false }, move |_, cx| {
                close(&state, cx)
            })
            .child(popover),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{add_months, calendar_days, parse_day};
    use chrono::NaiveDate;

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
}
