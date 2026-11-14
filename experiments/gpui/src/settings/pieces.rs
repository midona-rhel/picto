//! The pieces the Settings window's sections are built from
//! (`Settings.module.css`): cards, rows, labels, values, hints, columns,
//! fields, selects and theme swatches.
use crate::settings::{Settings, SettingsModel};
use crate::theme::Text as _;
use crate::ui;
use crate::{motion, theme};
use gpui::*;

/// `.settingsBlock`
pub(super) fn block() -> Div {
    div().w_full().mb(px(12.)).flex().flex_col()
}

/// `.blockContent`
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

/// `.blockTitle`
pub(super) fn title(theme: theme::Theme, text: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .text(theme::TEXT_MD)
        .line_height(px(26.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text())
        .child(text)
}

/// `.settingLabel`
pub(super) fn label(theme: theme::Theme) -> Div {
    div()
        .min_w(px(100.))
        .flex_none()
        .pt(px(2.))
        .text(theme::TEXT_MD)
        .line_height(px(theme::TEXT_MD * 1.3))
        .text_color(theme.text_secondary())
}

/// `.settingControl`
pub(super) fn control() -> Div {
    div()
        .ml_auto()
        .min_w_0()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(6.))
}

/// `Row`: a label and its control.
pub(super) fn row(
    theme: theme::Theme,
    text: impl Into<SharedString>,
    child: impl IntoElement,
) -> Div {
    div()
        .flex()
        .items_center()
        .min_h(px(26.))
        .min_w_0()
        .child(label(theme).child(text.into()))
        .child(control().child(child))
}

/// `.rowSep`
pub(super) fn separator(theme: theme::Theme) -> Div {
    div()
        .h(px(1.))
        .my(px(12.))
        .flex_none()
        .bg(theme.border_secondary())
}

/// `.settingHint`
pub(super) fn hint(theme: theme::Theme, text: &'static str) -> Div {
    ui::form::help_text(theme, text)
        .mt(px(8.))
        .line_height(px(20.))
}

/// `.staticValue`
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

/// `.valueLabel`
pub(super) fn value_label(theme: theme::Theme, text: String) -> Div {
    div()
        .ml(px(8.))
        .font_family(theme::FONT_INSPECTOR_MONO)
        .text(theme::TEXT_SM)
        .text_color(theme.text_secondary())
        .child(text)
}

/// `.checkboxGrid`: items 4px short of half wide, wrapping 4px apart, each
/// pulled 6px left by its own margin, so the second column starts 6px short
/// of half. Equal grid columns put it there with no gap and 12px spare at
/// the end (an item is then 2px narrower than production's).
pub(super) fn two_columns() -> Div {
    div().w_full().grid().grid_cols(2).gap_y(px(4.)).pr(px(12.))
}

/// `.sidebarSettingsGrid`: three equal columns 4px apart.
pub(super) fn three_columns() -> Div {
    div().w_full().grid().grid_cols(3).gap(px(4.))
}

/// A change to the draft, for a control's handler.
pub(super) fn edit(
    model: &Entity<SettingsModel>,
    change: impl Fn(&mut Settings) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let model = model.clone();
    move |_, cx| model.update(cx, |model, cx| model.edit(&change, cx))
}

/// A choice from a select, written into the draft.
pub(super) fn pick(
    model: &Entity<SettingsModel>,
    change: impl Fn(&mut Settings, &'static str) + 'static,
) -> impl Fn(&'static str, &mut Window, &mut App) + 'static {
    let model = model.clone();
    move |value, _, cx| model.update(cx, |model, cx| model.edit(|draft| change(draft, value), cx))
}

/// A number from a field or slider, written into the draft.
pub(super) fn number<T: 'static>(
    model: &Entity<SettingsModel>,
    change: impl Fn(&mut Settings, T) + 'static,
) -> impl Fn(T, &mut App) + 'static {
    let model = model.clone();
    move |value, cx| model.update(cx, |model, cx| model.edit(|draft| change(draft, value), cx))
}

/// `.themeSwatch`: a 20px swatch whose outline lightens under the pointer
/// and is full when chosen, over 100 ms, with its name above it while the
/// pointer is on it (a CSS tooltip, without the usual delay).
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
        // `linear-gradient(135deg, #f0f0f0 50%, #1a1a1e 50%)`
        None => div().bg(linear_gradient(
            135.,
            linear_color_stop(rgb(theme::THEME_SWATCH_AUTO.0), 0.5),
            linear_color_stop(rgb(theme::THEME_SWATCH_AUTO.1), 0.5001),
        )),
    };
    // The 1.5px outline sits 1.5px out from the border.
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
    // A `<button>`: a stop that Enter and Space choose; its outline does not
    // change with the keyboard.
    let focus = ui::focus::control(&id, window, cx);
    let on_click = std::rc::Rc::new(on_click);
    let pressed = on_click.clone();
    let bubble = tip.read(cx).layer(&ui::tip(name).plain(), 20., cx);
    let mut cell = div()
        .id(id)
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
