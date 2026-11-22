//! The parts that the sections of the Settings window are built from:
//! cards, rows, labels, values, help texts, grids of columns and theme
//! swatches. The file also has the functions that make a control's handler
//! write the control's value into the draft.
use crate::settings::{Settings, SettingsModel};
use crate::theme::Text as _;
use crate::ui;
use crate::{motion, theme};
use gpui::*;

/// Returns the container of one card of a section. It is as wide as the
/// section and has 12px of space below it.
pub(super) fn block() -> Div {
    div().w_full().mb(px(12.)).flex().flex_col()
}

/// Returns a card: a box with 12px of padding, rounded corners, a border,
/// and the theme's contrast colour at 3% as its background. The rows of a
/// section are laid out in cards.
pub(super) fn card(theme: theme::Theme) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .p(px(12.))
        .rounded(px(theme::RADIUS_LG))
        .border_1()
        .border_color(theme.border_secondary())
        .bg(theme.contrast(3.))
}

/// Returns the title of a card: `text` in semibold, in a line 26px high.
/// Assistive technology is told that it is a heading of level 3.
pub(super) fn title(theme: theme::Theme, text: &'static str) -> Stateful<Div> {
    ui::a11y::heading(
        div().id(ElementId::Name(format!("title-{text}").into())),
        crate::i18n::t(text),
        3,
    )
    .flex()
    .items_center()
    .gap(px(8.))
    .text(theme::TEXT_MD)
    .line_height(px(26.))
    .font_weight(FontWeight::SEMIBOLD)
    .text_color(theme.text())
    .child(text)
}

/// Returns the box for the label of a row: at least 100px wide, with text
/// in the secondary text colour.
pub(super) fn label(theme: theme::Theme) -> Div {
    div()
        .min_w(px(100.))
        .flex_none()
        .pt(px(2.))
        .text(theme::TEXT_MD)
        .line_height(px(theme::TEXT_MD * 1.3))
        .text_color(theme.text_secondary())
}

/// Returns the box for the control of a row. It is at the right end of the
/// row, and its children are 6px apart.
pub(super) fn control() -> Div {
    div()
        .ml_auto()
        .min_w_0()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(6.))
}

/// Returns one row of a card: the label `text` on the left and the control
/// `child` at the right end.
pub(super) fn row(
    theme: theme::Theme,
    text: impl Into<SharedString>,
    child: impl IntoElement,
) -> Div {
    // Assistive technology reads the label as text, before the control.
    let text = text.into();
    div()
        .flex()
        .items_center()
        .min_h(px(26.))
        .min_w_0()
        .child(ui::a11y::text(
            label(theme)
                .id(ElementId::Name(format!("label-{text}").into()))
                .child(text.clone()),
            text,
        ))
        .child(control().child(child))
}

/// The line between two rows of a card: 1px thick, with 12px of space above
/// it and below it.
pub(super) use crate::ui::form::separator;

/// Returns the help text below a row: small text in the secondary text
/// colour, in lines 20px high, 8px below the row.
pub(super) fn hint(theme: theme::Theme, text: &'static str) -> Div {
    ui::form::help_text(theme, text)
        .mt(px(8.))
        .line_height(px(20.))
}

/// Returns a value that the user cannot change, shown at the right end of a
/// row.
pub(super) fn value(theme: theme::Theme, text: impl Into<SharedString>) -> Div {
    div()
        .ml_auto()
        .min_h(px(26.))
        .flex()
        .items_center()
        .justify_end()
        .text(theme::TEXT_MD)
        .text_color(theme.text())
        .child(text.into())
}

/// Returns the small monospaced text that shows the current value of a
/// slider, 8px to the right of the slider.
pub(super) fn value_label(theme: theme::Theme, text: String) -> Div {
    div()
        .ml(px(8.))
        .font_family(theme::FONT_INSPECTOR_MONO)
        .text(theme::TEXT_SM)
        .text_color(theme.text_secondary())
        .child(text)
}

/// Returns a grid of two equal columns for checkboxes, with 4px between its
/// rows. The grid has 12px of padding at its right end, so the second
/// column starts 6px before the middle of the card.
pub(super) fn two_columns() -> Div {
    div().w_full().grid().grid_cols(2).gap_y(px(4.)).pr(px(12.))
}

/// Returns a grid of three equal columns, with 4px between its cells.
pub(super) fn three_columns() -> Div {
    div().w_full().grid().grid_cols(3).gap(px(4.))
}

/// Returns a handler for a control. The handler applies `change` to the
/// draft.
pub(super) fn edit(
    model: &Entity<SettingsModel>,
    change: impl Fn(&mut Settings) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let model = model.clone();
    move |_, cx| model.update(cx, |model, cx| model.edit(&change, cx))
}

/// Returns a handler for a select. The handler applies `change` to the
/// draft with the value of the chosen choice.
pub(super) fn pick(
    model: &Entity<SettingsModel>,
    change: impl Fn(&mut Settings, &'static str) + 'static,
) -> impl Fn(&'static str, &mut Window, &mut App) + 'static {
    let model = model.clone();
    move |value, _, cx| model.update(cx, |model, cx| model.edit(|draft| change(draft, value), cx))
}

/// Returns a handler for a number field or a slider. The handler applies
/// `change` to the draft with the new number.
pub(super) fn number<T: 'static>(
    model: &Entity<SettingsModel>,
    change: impl Fn(&mut Settings, T) + 'static,
) -> impl Fn(T, &mut App) + 'static {
    let model = model.clone();
    move |value, cx| model.update(cx, |model, cx| model.edit(|draft| change(draft, value), cx))
}

/// Draws the swatch of one theme: a 20px square in `color`, or half light
/// and half dark when `color` is `None`. The swatch has an outline around
/// it. The outline is in the text colour when the theme is the chosen one
/// (`active`). Otherwise it is invisible, and it fades to the secondary
/// text colour over 100 ms while the pointer is on the swatch. The theme's
/// name is shown above the swatch as soon as the pointer is on it, without
/// the delay that other tooltips have.
pub(super) fn swatch(
    index: usize,
    name: &'static str,
    color: Option<u32>,
    active: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let theme = theme::theme(cx);
    let id = ElementId::Name(format!("theme-swatch-{index}").into());
    let (lit, set_hover) =
        motion::hover_fade(id.clone(), motion::ROW_HOVER, motion::EASE, window, cx);
    let tip = window.use_keyed_state(
        ElementId::Name(format!("theme-swatch-tip-{index}").into()),
        cx,
        |_, _| ui::tooltip::Hover::default(),
    );
    if tip.read(cx).animating() {
        window.request_animation_frame();
    }
    let outline = if active {
        theme.text()
    } else {
        theme::blend(
            gpui::transparent_black().into(),
            theme.text_secondary(),
            lit,
        )
    };
    let fill = match color {
        Some(color) => div().bg(rgb(color)),
        // The swatch of Auto is divided along its diagonal into a light
        // half and a dark half (`theme::THEME_SWATCH_AUTO`).
        None => div().bg(linear_gradient(
            135.,
            linear_color_stop(rgb(theme::THEME_SWATCH_AUTO.0), 0.5),
            linear_color_stop(rgb(theme::THEME_SWATCH_AUTO.1), 0.5001),
        )),
    };
    // The outline is 1.5px thick, and there is 1.5px between it and the
    // edge of the swatch.
    let mut ring = div()
        .absolute()
        .top(px(-3.))
        .left(px(-3.))
        .size(px(26.))
        .rounded(px(9.))
        .border_color(outline);
    ring.style().border_widths = EdgesRefinement {
        top: Some(px(1.5).into()),
        right: Some(px(1.5).into()),
        bottom: Some(px(1.5).into()),
        left: Some(px(1.5).into()),
    };
    // The swatch can have the keyboard focus, and Enter and Space choose
    // its theme then. Its outline does not change when it has the keyboard
    // focus.
    let focus = ui::focus::control(&id, window, cx);
    let on_click = std::rc::Rc::new(on_click);
    let pressed = on_click.clone();
    let bubble = tip.read(cx).layer(&ui::tip(name).plain(), 20., cx);
    let mut cell = div()
        .id(id)
        .role(Role::Button)
        // Assistive technology is told the theme's name followed by
        // "theme".
        .aria_label(crate::tf!("{value0} theme", name))
        .aria_toggled(ui::a11y::toggled(active))
        .relative()
        .size(px(20.))
        .flex_none()
        .cursor_pointer()
        .child(
            fill.absolute()
                .inset_0()
                .rounded(px(theme::RADIUS_MD))
                .border_1()
                .border_color(theme.border_secondary()),
        )
        .child(ring)
        .on_hover(move |hovered, window, cx| {
            set_hover(hovered, window, cx);
            tip.update(cx, |tip, cx| tip.set_now(*hovered, cx));
        })
        .on_click(move |_, window, cx| on_click(window, cx))
        .children(bubble);
    cell = focus.attach(cell, ui::focus::Kind::Button, move |_, window, cx| {
        pressed(window, cx)
    });
    cell
}
