//! What the Add Tags and Folders dialogs share
//! (`SelectionModalShell.module.css`): the search bar under the title, the
//! rail of views on the left with their counts, the 28px rows of the list,
//! the empty state and the footer's summary of what Apply will do.
use super::overlay_shell;
use super::text_input::{self, TextInput};
use crate::theme;
use crate::theme::Text as _;
use gpui::*;
use std::rc::Rc;

actions!(selection_modal, [Up, Down, NextView]);

/// The keys the dialogs take from their search field. Enter and Escape are
/// the field's own, which the search bar catches before it.
pub fn bind(cx: &mut App) {
    let context = Some("SelectionModal");
    cx.bind_keys([
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("tab", NextView, context),
    ]);
}

/// `--control-height-md`: a rail item and a row of the list.
pub const ROW: f32 = theme::CONTROL_MD;
/// `.searchBar`
const SEARCH: f32 = 38.;

/// A key typed in the search field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    /// Enter.
    Confirm,
    /// Tab.
    Next,
    /// Escape.
    Dismiss,
}

type KeyHandler = Rc<dyn Fn(Key, &mut Window, &mut App) + 'static>;

/// `.searchBar` with its `IconSearch` and the field, which takes the
/// keyboard. `on_key` hears the keys typed in the field.
pub fn search_bar(
    input: &Entity<TextInput>,
    on_key: impl Fn(Key, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let theme = theme::theme(cx);
    let on_key: KeyHandler = Rc::new(on_key);
    let send = |key: Key| {
        let on_key = on_key.clone();
        move |window: &mut Window, cx: &mut App| on_key(key, window, cx)
    };
    let (up, down, next, confirm, dismiss) = (
        send(Key::Up),
        send(Key::Down),
        send(Key::Next),
        send(Key::Confirm),
        send(Key::Dismiss),
    );
    div()
        .h(px(SEARCH))
        .flex_none()
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(6.))
        .border_b_1()
        .border_color(theme.border_secondary())
        .key_context("SelectionModal")
        .on_action(move |_: &Up, window, cx| up(window, cx))
        .on_action(move |_: &Down, window, cx| down(window, cx))
        .on_action(move |_: &NextView, window, cx| next(window, cx))
        // Enter and Escape reach the dialog, not the field.
        .capture_action(move |_: &text_input::Commit, window, cx| {
            cx.stop_propagation();
            confirm(window, cx);
        })
        .capture_action(move |_: &text_input::Cancel, window, cx| {
            cx.stop_propagation();
            dismiss(window, cx);
        })
        // `.searchIcon`: the tertiary colour at 60%.
        .child(
            super::icon(
                "selection-search",
                14.,
                Rgba {
                    a: theme.text_tertiary().a * 0.6,
                    ..theme.text_tertiary()
                },
            )
            .flex_none(),
        )
        .child(div().flex_1().min_w_0().h(px(SEARCH)).child(input.clone()))
}

/// `.sidebar`: the rail, `width` wide with its right border, padded 4px.
pub fn rail(width: f32, cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .w(px(width))
        .flex_none()
        .h_full()
        .overflow_hidden()
        .p(px(4.))
        .pr(px(4. + super::scrollbar::GUTTER))
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(theme.border_secondary())
}

/// `.sidebarItem`: a view of the rail, with an optional group dot, its name
/// and its count. The chosen one is filled and ringed; the count and dot
/// brighten under the pointer.
pub fn rail_item(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    count: impl Into<SharedString>,
    active: bool,
    dot: Option<u32>,
    name_width: Option<f32>,
    cx: &App,
) -> Stateful<Div> {
    let theme = theme::theme(cx);
    let id = id.into();
    let group = SharedString::from(format!("selection-rail-{id}"));
    let mut name = div().flex_1().min_w_0().truncate().child(label.into());
    if let Some(width) = name_width {
        name = name.max_w(px(width));
    }
    let mut row = div()
        .id(id)
        .group(group.clone())
        .relative()
        .h(px(ROW))
        .flex_none()
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(theme::RADIUS_MD))
        .cursor_pointer()
        .text(theme::TEXT_MD)
        .text_color(theme.text())
        .whitespace_nowrap();
    if active {
        row = row.bg(theme.surface_active()).child(
            // `outline: 1px solid var(--color-contrast-25)`, inside.
            div()
                .absolute()
                .inset_0()
                .rounded(px(theme::RADIUS_MD))
                .border_1()
                .border_color(theme.contrast(25.)),
        );
    } else {
        row = row.hover(move |style| style.bg(theme.subtle_overlay()));
    }
    if let Some(color) = dot {
        row = row.child(
            div()
                .size(px(10.))
                .flex_none()
                .rounded_full()
                .bg(rgb(color))
                .opacity(if active { 1. } else { 0.6 })
                .group_hover(group.clone(), |style| style.opacity(1.)),
        );
    }
    row.child(name)
        .child(badge(group, active, cx).ml_auto().child(count.into()))
}

/// `.sidebarBadge` and `.itemBadge`: a count in the monospaced caption, at
/// half strength until the row is under the pointer or chosen.
pub fn badge(group: SharedString, strong: bool, cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .flex_none()
        .font_family(theme::FONT_MONO)
        .text(theme::TEXT_CAPTION)
        .line_height(px(theme::LINE_HEIGHT_MD))
        .text_color(theme.text_tertiary())
        .opacity(if strong { 1. } else { 0.5 })
        .group_hover(group, |style| style.opacity(1.))
}

/// `.sidebarSep`
pub fn rail_separator(cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .h(px(1.))
        .flex_none()
        .mx(px(10.))
        .my(px(4.))
        .bg(theme.border_secondary())
}

/// `.itemRow`: a row of the list, inset 4px, lit under the pointer or while
/// the keyboard is on it. Its children brighten through `group`.
pub fn item_row(
    id: impl Into<ElementId>,
    group: SharedString,
    focused: bool,
    cx: &App,
) -> Stateful<Div> {
    let theme = theme::theme(cx);
    let row = div()
        .id(id)
        .group(group)
        .h(px(ROW))
        .flex_none()
        .mx(px(4.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(theme::RADIUS_MD))
        .cursor_pointer()
        .text(theme::TEXT_MD)
        .text_color(theme.text())
        .whitespace_nowrap();
    if focused {
        row.bg(theme.subtle_overlay())
    } else {
        row.hover(move |style| style.bg(theme.subtle_overlay()))
    }
}

/// `.emptyState`
pub fn empty_state(text: &'static str, cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .h(px(80.))
        .flex()
        .items_center()
        .justify_center()
        .text(theme::TEXT_SM)
        .text_color(theme.text_tertiary())
        .child(text)
}

/// The footer's summary: what Apply adds and removes and how many files it
/// applies to, or how many are selected while nothing is pending.
pub fn summary(added: usize, removed: usize, selected: usize) -> String {
    let files = |count: usize| format!("{count} file{}", if count == 1 { "" } else { "s" });
    let mut parts = Vec::new();
    if added > 0 {
        parts.push(format!("+{added}"));
    }
    if removed > 0 {
        parts.push(format!("\u{2212}{removed}"));
    }
    if !parts.is_empty() {
        let mut text = parts.join(", ");
        if selected > 0 {
            text.push_str(&format!(" · {}", files(selected)));
        }
        text
    } else if selected > 0 {
        format!("{} selected", files(selected))
    } else {
        String::new()
    }
}

/// `.footerLeft`: the summary in the monospaced caption, or, with `hints`
/// and nothing to summarise, the keys the list takes.
pub fn footer_left(summary: String, hints: bool, cx: &App) -> Div {
    let theme = theme::theme(cx);
    let left = div().flex_1().flex().items_center().gap(px(6.));
    if !summary.is_empty() {
        return left.child(
            div()
                .font_family(theme::FONT_MONO)
                .text(theme::TEXT_CAPTION)
                .line_height(px(theme::LINE_HEIGHT_MD))
                .text_color(theme.text_secondary())
                .child(summary),
        );
    }
    if !hints {
        return left;
    }
    left.child(
        overlay_shell::hint(cx)
            .child(overlay_shell::kbd("↑↓", cx))
            .child(overlay_shell::kbd("↩", cx))
            .child(" Select")
            .child(overlay_shell::tab_key(cx))
            .child(" Switch"),
    )
}

#[cfg(test)]
mod tests {
    use super::summary;

    #[test]
    fn the_summary_counts_what_apply_changes_and_the_files() {
        assert_eq!(summary(0, 0, 1), "1 file selected");
        assert_eq!(summary(0, 0, 2673), "2673 files selected");
        assert_eq!(summary(2, 0, 1), "+2 · 1 file");
        assert_eq!(summary(1, 3, 5), "+1, \u{2212}3 · 5 files");
        assert_eq!(summary(0, 1, 0), "\u{2212}1");
        assert_eq!(summary(0, 0, 0), "");
    }
}
