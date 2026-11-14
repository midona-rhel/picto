//! The editors the filter bar's menus hold (`GridFilterMenu.tsx`): a
//! numeric range with its unit, the resolution's two ranges, a date range,
//! and the field that names a saved filter. Each is a view a menu row
//! draws; each tells the bar what was committed.
use super::filters;
use crate::i18n::t;
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    field::Look,
    text_input::{Kind, TextInput},
};
use gpui::*;
use std::time::Duration;

/// `useDeferredFilterCommit`: typing settles for 100 ms before it filters.
const SETTLE: Duration = Duration::from_millis(100);

/// `.rangeEditor`: two fields with a dash between and the unit after, at
/// least 36px tall.
pub const RANGE_HEIGHT: f32 = 36.;
/// `.resolutionEditor`: two labelled ranges 6px apart, 4px above and below.
pub const RESOLUTION_HEIGHT: f32 = 4. + RANGE_HEIGHT + 6. + RANGE_HEIGHT + 4.;
/// `.dateEditor`: a caption line, 3px, the 26px button, 4px above and below.
pub const DATE_HEIGHT: f32 = 4. + 13. + 3. + theme::CONTROL_SM + 4.;
/// `.saveFilterEditor`: a 28px row inside 4px of padding.
pub const SAVE_HEIGHT: f32 = 4. + theme::CONTROL_MD + 4.;

/// A range committed, in the base unit.
pub struct RangeCommitted(pub Option<u64>, pub Option<u64>);

/// `NumericRangeEditor`: Min and Max in the unit chosen.
pub struct RangeEditor {
    min: Entity<TextInput>,
    max: Entity<TextInput>,
    /// The units offered, each with how many base units it is. One unit is
    /// shown as a label; more as a select.
    units: Vec<(&'static str, u64)>,
    unit: usize,
    /// The range last scheduled, so a repaint of a field schedules nothing.
    scheduled: (Option<u64>, Option<u64>),
    settle: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RangeCommitted> for RangeEditor {}

impl RangeEditor {
    pub fn new(
        minimum: Option<u64>,
        maximum: Option<u64>,
        units: Vec<(&'static str, u64)>,
        unit: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        let multiplier = units.get(unit).map_or(1, |(_, multiplier)| *multiplier);
        let field = |value: Option<u64>, placeholder: &'static str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut input = TextInput::new(Kind::Compact, cx).placeholder(placeholder);
                if let Some(value) = value {
                    input.set_text(filters::scaled_value(value, multiplier), cx);
                }
                input
            })
        };
        let min = field(minimum, t("Min"), cx);
        let max = field(maximum, t("Max"), cx);
        let subscriptions = vec![
            cx.observe(&min, |this, _, cx| this.schedule(cx)),
            cx.observe(&max, |this, _, cx| this.schedule(cx)),
        ];
        Self {
            min,
            max,
            units,
            unit,
            // What the fields show, which may be rounded in this unit.
            scheduled: (
                minimum.and_then(|value| {
                    filters::amount(&filters::scaled_value(value, multiplier), multiplier)
                }),
                maximum.and_then(|value| {
                    filters::amount(&filters::scaled_value(value, multiplier), multiplier)
                }),
            ),
            settle: None,
            _subscriptions: subscriptions,
        }
    }

    fn values(&self, cx: &App) -> (Option<u64>, Option<u64>) {
        let multiplier = self.units.get(self.unit).map_or(1, |(_, m)| *m);
        (
            filters::amount(&self.min.read(cx).value(), multiplier),
            filters::amount(&self.max.read(cx).value(), multiplier),
        )
    }

    fn schedule(&mut self, cx: &mut Context<Self>) {
        let (min, max) = self.values(cx);
        if (min, max) == self.scheduled {
            return;
        }
        self.scheduled = (min, max);
        self.settle = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let _ = this.update(cx, |this, cx| {
                this.settle = None;
                cx.emit(RangeCommitted(min, max));
            });
        }));
    }
}

impl Render for RangeEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let unit: AnyElement = if self.units.len() > 1 {
            let choices = self
                .units
                .iter()
                .map(|(label, _)| ui::choice(label, *label))
                .collect();
            let current = self.units[self.unit].0;
            let editor = cx.entity().downgrade();
            ui::select("range-unit", current, choices)
                .on_change(move |value, _, cx| {
                    let _ = editor.update(cx, |this, cx| {
                        if let Some(at) = this.units.iter().position(|(label, _)| *label == value) {
                            this.unit = at;
                            this.schedule(cx);
                            cx.notify();
                        }
                    });
                })
                .into_any_element()
        } else {
            div()
                .flex_none()
                .text(theme::TEXT_SM)
                .text_color(theme.text_secondary())
                .child(self.units.first().map_or("", |(label, _)| *label))
                .into_any_element()
        };
        div()
            .min_h(px(RANGE_HEIGHT))
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(ui::field("range-min", &self.min, Look::Range)),
            )
            .child(div().flex_none().child("–"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(ui::field("range-max", &self.max, Look::Range)),
            )
            .child(unit)
    }
}

/// `ResolutionEditor`: Width and Height, each a range in pixels.
pub struct ResolutionEditor {
    pub width: Entity<RangeEditor>,
    pub height: Entity<RangeEditor>,
}

impl Render for ResolutionEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let row = |label: &'static str, editor: &Entity<RangeEditor>| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .text(theme::TEXT_SM)
                .text_color(theme.text_secondary())
                .child(div().w(px(42.)).flex_none().child(label))
                .child(div().flex_1().min_w_0().child(editor.clone()))
        };
        div()
            .py(px(4.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(row(t("Width"), &self.width))
            .child(row(t("Height"), &self.height))
    }
}

/// A date range committed: the first moment included and the first left
/// out.
pub struct DatesCommitted(pub Option<i64>, pub Option<i64>);

/// `DateRangeEditor`: From and To, each a day.
pub struct DateRangeEditor {
    first: String,
    last: String,
}

impl EventEmitter<DatesCommitted> for DateRangeEditor {}

impl DateRangeEditor {
    pub fn new(after: Option<i64>, before: Option<i64>) -> Self {
        let (first, last) = filters::range_days(after, before);
        Self { first, last }
    }

    /// Shows a range chosen elsewhere, such as a preset.
    pub fn set_range(&mut self, after: Option<i64>, before: Option<i64>, cx: &mut Context<Self>) {
        let (first, last) = filters::range_days(after, before);
        if (first.as_str(), last.as_str()) != (self.first.as_str(), self.last.as_str()) {
            self.first = first;
            self.last = last;
            cx.notify();
        }
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        let (after, before) = filters::range_from_days(&self.first, &self.last);
        cx.emit(DatesCommitted(after, before));
        cx.notify();
    }
}

impl Render for DateRangeEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let column = |label: &'static str, picker: ui::date_picker::DatePicker| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .text(theme::TEXT_CAPTION)
                .text_color(theme.text_secondary())
                .child(label)
                .child(picker)
        };
        let (from, to) = (cx.entity().downgrade(), cx.entity().downgrade());
        div()
            .py(px(4.))
            .flex()
            .gap(px(8.))
            .child(column(
                t("From"),
                ui::date_picker::date_picker("date-from", self.first.clone()).on_change(
                    move |day, _, cx| {
                        let _ = from.update(cx, |this, cx| {
                            this.first = day;
                            this.commit(cx);
                        });
                    },
                ),
            ))
            .child(column(
                t("To"),
                ui::date_picker::date_picker("date-to", self.last.clone()).on_change(
                    move |day, _, cx| {
                        let _ = to.update(cx, |this, cx| {
                            this.last = day;
                            this.commit(cx);
                        });
                    },
                ),
            ))
    }
}

/// A name to save the current filters under.
pub struct SaveRequested(pub String);

/// `.saveFilterEditor`: the name and Save, which is offered once there is
/// a name and something to save.
pub struct SaveFilterEditor {
    name: Entity<TextInput>,
    /// Whether any filter is set.
    pub savable: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SaveRequested> for SaveFilterEditor {}

impl SaveFilterEditor {
    pub fn new(savable: bool, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| {
            TextInput::new(Kind::Form, cx)
                .placeholder(t("Filter name"))
                .submits()
        });
        let subscriptions = vec![
            cx.observe(&name, |_, _, cx| cx.notify()),
            cx.subscribe(&name, |this, _, _: &ui::text_input::Submitted, cx| {
                this.save(cx)
            }),
        ];
        Self {
            name,
            savable,
            _subscriptions: subscriptions,
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() || !self.savable {
            return;
        }
        self.name.update(cx, |input, cx| input.clear(cx));
        cx.emit(SaveRequested(name));
    }
}

impl Render for SaveFilterEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let enabled = self.savable && !self.name.read(cx).value().trim().is_empty();
        let mut button = div()
            .id("save-filter")
            .h(px(theme::CONTROL_MD))
            .px(px(10.))
            .flex()
            .flex_none()
            .items_center()
            .rounded(px(theme::RADIUS_MD))
            .bg(theme.subtle_overlay())
            .text(theme::TEXT_SM)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.text())
            .child(t("Save"));
        button = if enabled {
            button
                .cursor_pointer()
                // `--color-active-overlay`.
                .hover(move |style| style.bg(theme.contrast(14.)))
                .on_click(cx.listener(|this, _, _, cx| this.save(cx)))
        } else {
            button.opacity(0.3)
        };
        div()
            .p(px(4.))
            .flex()
            .gap(px(6.))
            .child(div().flex_1().min_w_0().child(ui::field(
                "save-filter-name",
                &self.name,
                Look::SavedFilter,
            )))
            .child(button)
    }
}
