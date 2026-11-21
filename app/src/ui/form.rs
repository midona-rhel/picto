//! The parts that dialogs and Settings lay their forms out with: a field with
//! its label, a row of controls, a column, a separator, and the help, error
//! and warning texts. Each caller adds its own spacing around them.
use crate::theme;
use crate::theme::Text as _;
use gpui::*;

/// Returns a 1px line with 12px above it and below it. The export dialogs and
/// Settings use it.
pub fn separator(theme: theme::Theme) -> Div {
    div()
        .h(px(1.))
        .my(px(12.))
        .flex_none()
        .bg(theme.border_secondary())
}

/// Returns a help text of a dialog: small text in the secondary text colour.
/// Each caller adds its own spacing.
pub fn help_text(theme: theme::Theme, text: impl Into<SharedString>) -> Div {
    div()
        .text(theme::TEXT_SM)
        .text_color(theme.text_secondary())
        .child(text.into())
}

/// Returns a field's label: 13px text with a 20px line height, in the primary
/// text colour and the regular weight.
pub fn label(theme: &theme::Theme, text: impl Into<SharedString>) -> Div {
    div()
        .text(theme::TEXT_MD)
        .line_height(px(theme::LINE_HEIGHT_MD))
        .font_weight(FontWeight::NORMAL)
        .text_color(theme.text())
        .child(text.into())
}

/// Returns a field: a label and, 6px below it, its control.
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

/// Returns a row for controls, which are 8px apart.
pub fn row() -> Div {
    div().flex().items_center().gap(px(8.))
}

/// Returns a column whose children are 16px apart.
pub fn stack() -> Div {
    div().flex().flex_col().gap(px(16.))
}

/// Returns the box that tells the user that an edit or a search failed. A
/// dialog or a screen shows it above its content. Each caller adds its own
/// margins.
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

/// Returns a box for a warning in a dialog. Its background and border are
/// faintly tinted with the warning colour.
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
