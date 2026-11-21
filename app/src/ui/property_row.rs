use crate::theme;
use crate::theme::Text as _;
use gpui::*;
use std::time::{Duration, Instant};

/// The time in which the loading spinner makes one full turn.
const SPIN: Duration = Duration::from_millis(650);

/// One row of the inspector's property list. The row is 20 px high. The label
/// is on the left in a column at least 88 px wide. The value is right-aligned
/// after a 12 px gap, in the Fira Mono font with tabular figures, and is cut
/// off with an ellipsis when it does not fit. While the value is loading, a
/// spinner is drawn where the value would be.
#[derive(IntoElement)]
pub struct PropertyRow {
    label: SharedString,
    value: SharedString,
    mono: &'static str,
    /// The time at which the value started loading. While this is set, the
    /// spinner is drawn instead of the value.
    loading: Option<Instant>,
    color: RowColor,
}

/// The text colour of a property row's label and value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum RowColor {
    /// The colours the inspector uses: the label takes the text colour of
    /// the element that contains the row, and the value is drawn in the
    /// contrast colour at 80% opacity.
    #[default]
    Inspector,
    /// The label and the value are both drawn in the secondary text colour.
    /// The Duplicates screen uses this for a row whose value is the same
    /// for both files.
    Secondary,
    /// The label and the value are both drawn in the primary text colour.
    /// The Duplicates screen uses this for a row whose value differs
    /// between the two files.
    Primary,
}

/// Makes a property row with the inspector's colours. The value is drawn in
/// Fira Mono.
pub fn property_row(label: impl Into<SharedString>, value: impl Into<SharedString>) -> PropertyRow {
    PropertyRow {
        label: label.into(),
        value: value.into(),
        mono: theme::FONT_INSPECTOR_MONO,
        loading: None,
        color: RowColor::Inspector,
    }
}

/// Returns the loading spinner: a ring 10 px across with a gap on its right
/// side. Its angle is computed from the time since `since`, so two spinners
/// that started at the same time are always at the same angle. It asks for a
/// new frame every [`crate::motion::LOOP_FRAME`] instead of at the display's
/// refresh rate.
pub fn spinner(since: Instant, window: &mut Window, cx: &mut App) -> Svg {
    crate::motion::loop_frame(window, cx);
    let turn = since.elapsed().as_secs_f32() / SPIN.as_secs_f32();
    let mut color = theme::theme(cx).text_secondary();
    color.a *= 0.65;
    super::icon("inspector-spinner", 10., color).with_transformation(Transformation::rotate(
        radians(turn.fract() * std::f32::consts::TAU),
    ))
}

impl PropertyRow {
    /// Draws the spinner instead of the value. `since` is the time at which
    /// the value started loading.
    pub fn loading(mut self, since: Instant) -> Self {
        self.loading = Some(since);
        self
    }

    /// Sets the text colour of the label and the value.
    pub fn color(mut self, color: RowColor) -> Self {
        self.color = color;
        self
    }
}

impl RenderOnce for PropertyRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (label_color, value_color) = match self.color {
            RowColor::Inspector => (None, theme.contrast(80.)),
            RowColor::Secondary => (Some(theme.text_secondary()), theme.text_secondary()),
            RowColor::Primary => (Some(theme.text()), theme.text()),
        };
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
                .text_color(value_color)
                .child(self.value.clone()),
        };
        let mut label = div().min_w(px(88.)).flex_none();
        if let Some(color) = label_color {
            label = label.text_color(color);
        }
        let label = label.child(self.label.clone());
        // Assistive technology reads the row as one line of text: the label
        // followed by the value. While the value is loading it reads "Loading"
        // followed by the label.
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
        .child(label)
        .child(value)
    }
}
