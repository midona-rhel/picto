//! The pieces dialogs and settings lay their forms out with
//! (`GlassModal.module.css`'s `.field`, `.fieldRow`, `.stack`, and the help,
//! error and warning text that goes with them). Each place adds its own
//! spacing around them.
use crate::theme;
use crate::theme::Text as _;
use gpui::*;

/// A dialog's help text (`.pickerHint`, `.inlineLabel`, `.settingHint`):
/// small secondary text. Each place adds its own spacing.
pub fn help_text(theme: theme::Theme, text: impl Into<SharedString>) -> Div {
    div()
        .text(theme::TEXT_SM)
        .text_color(theme.text_secondary())
        .child(text.into())
}

/// `.fieldLabel`: 13px on 20px, the primary text colour, regular weight.
pub fn label(theme: &theme::Theme, text: impl Into<SharedString>) -> Div {
    div()
        .text(theme::TEXT_MD)
        .line_height(px(theme::LINE_HEIGHT_MD))
        .font_weight(FontWeight::NORMAL)
        .text_color(theme.text())
        .child(text.into())
}

/// `.field`: a label over its control, 6px apart.
pub fn field(
    theme: &theme::Theme,
    text: impl Into<SharedString>,
    control: impl IntoElement,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(label(theme, text))
        .child(control)
}

/// `.fieldRow` and `.row`: controls in a row, 8px apart.
pub fn row() -> Div {
    div().flex().items_center().gap(px(8.))
}

/// `.stack`: a column 16px apart.
pub fn stack() -> Div {
    div().flex().flex_col().gap(px(16.))
}

/// `.error`: an edit or search that failed, as a dialog or screen shows it
/// above its content. Each place adds its own margins.
pub fn error_banner(theme: theme::Theme, message: impl Into<SharedString>) -> Div {
    div()
        .px(px(12.))
        .py(px(9.))
        .rounded(px(theme::RADIUS_SM))
        .border_1()
        .border_color(theme.negative_at(35.))
        .bg(theme.negative_at(8.))
        .text_color(rgb(theme::ERROR_TEXT))
        .text(theme::TEXT_SM)
        .child(message.into())
}

/// `.warningBox`: a note in a dialog that something about its subject needs
/// attention, on a faint warning tint.
pub fn warning_box(theme: theme::Theme, message: impl Into<SharedString>) -> Div {
    let warning = rgb(theme::WARNING);
    div()
        .px(px(12.))
        .py(px(10.))
        .rounded(px(theme::RADIUS_MD))
        .border_1()
        .border_color(theme::blend(theme.border_secondary(), warning, 0.45))
        .bg(theme::blend(theme.control_surface(), warning, 0.09))
        .text_color(theme.text())
        .text(theme::TEXT_SM)
        .line_height(px(18.))
        .child(message.into())
}
