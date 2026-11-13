use crate::theme;
use crate::theme::Text as _;
use gpui::*;

/// A label and its value on one 20px line (`PropertyRow.tsx`): the label in
/// an 88px column, the value right-aligned after a 12px gap, in a monospace
/// face with tabular figures at 80%, ending in an ellipsis when too long.
#[derive(IntoElement)]
pub struct PropertyRow {
    label: SharedString,
    value: SharedString,
    mono: &'static str,
}

/// The value is set in `--inspector-mono-font-family` (Fira Mono), as it is
/// inside the inspector; [`PropertyRow::mono`] picks another face.
pub fn property_row(label: impl Into<SharedString>, value: impl Into<SharedString>) -> PropertyRow {
    PropertyRow {
        label: label.into(),
        value: value.into(),
        mono: theme::FONT_INSPECTOR_MONO,
    }
}

impl PropertyRow {
    /// The value's face, e.g. `--font-family-mono` outside the inspector.
    pub fn mono(mut self, family: &'static str) -> Self {
        self.mono = family;
        self
    }
}

impl RenderOnce for PropertyRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .h(px(20.))
            .flex()
            .items_center()
            .gap(px(12.))
            .whitespace_nowrap()
            .text(theme::TEXT_CAPTION)
            .child(div().w(px(88.)).flex_none().child(self.label))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_right()
                    .truncate()
                    .font_family(self.mono)
                    .font_features(theme::tabular_figures())
                    .text_color(theme::theme(cx).contrast(80.))
                    .child(self.value),
            )
    }
}
