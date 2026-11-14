use crate::theme;
use crate::theme::Text as _;
use gpui::*;
use std::time::{Duration, Instant};

/// `property-spinner`: one turn of the loading spinner.
const SPIN: Duration = Duration::from_millis(650);

/// A label and its value on one 20px line (`PropertyRow.tsx`): the label in
/// an 88px column, the value right-aligned after a 12px gap, in a monospace
/// face with tabular figures at 80%, ending in an ellipsis when too long;
/// while the value loads, a spinner in its place.
#[derive(IntoElement)]
pub struct PropertyRow {
    label: SharedString,
    value: SharedString,
    mono: &'static str,
    /// Loading since then: the spinner shows instead of the value.
    loading: Option<Instant>,
}

/// The value is set in `--inspector-mono-font-family` (Fira Mono), as it is
/// inside the inspector; [`PropertyRow::mono`] picks another face.
pub fn property_row(label: impl Into<SharedString>, value: impl Into<SharedString>) -> PropertyRow {
    PropertyRow {
        label: label.into(),
        value: value.into(),
        mono: theme::FONT_INSPECTOR_MONO,
        loading: None,
    }
}

/// `.spinner`: a 10px ring open on its right, turning once every 650 ms
/// from `since`, so spinners started together turn together.
pub fn spinner(since: Instant, window: &mut Window, cx: &App) -> Svg {
    window.request_animation_frame();
    let turn = since.elapsed().as_secs_f32() / SPIN.as_secs_f32();
    let mut color = theme::theme(cx).text_secondary();
    color.a *= 0.65;
    super::icon("inspector-spinner", 10., color).with_transformation(Transformation::rotate(
        radians(turn.fract() * std::f32::consts::TAU),
    ))
}

impl PropertyRow {
    /// The value's face, e.g. `--font-family-mono` outside the inspector.
    pub fn mono(mut self, family: &'static str) -> Self {
        self.mono = family;
        self
    }

    /// `loading`: the value is on its way, loading since `since`.
    pub fn loading(mut self, since: Instant) -> Self {
        self.loading = Some(since);
        self
    }
}

impl RenderOnce for PropertyRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let value = match self.loading {
            Some(since) => div()
                .flex_1()
                .min_w_0()
                .flex()
                .justify_end()
                .child(spinner(since, window, cx)),
            None => div()
                .flex_1()
                .min_w_0()
                .text_right()
                .truncate()
                .font_family(self.mono)
                .font_features(theme::tabular_figures())
                .text_color(theme::theme(cx).contrast(80.))
                .child(self.value.clone()),
        };
        // Read as one line of text, its label then its value; while the
        // value loads, `Loading …` as the spinner's `aria-label` says.
        let read = match self.loading {
            Some(_) => crate::tf!("Loading {value0}", self.label),
            None => format!("{} {}", self.label, self.value),
        };
        super::a11y::text(
            div().id(ElementId::Name(format!("property-{}", self.label).into())),
            read,
        )
        .aria_busy(self.loading.is_some())
        .h(px(20.))
        .flex()
        .items_center()
        .gap(px(12.))
        .whitespace_nowrap()
        .text(theme::TEXT_CAPTION)
        .child(div().w(px(88.)).flex_none().child(self.label))
        .child(value)
    }
}
