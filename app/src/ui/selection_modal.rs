//! The parts that the Add Tags dialog and the Folders dialog share: the search
//! bar under the title, the rail of views on the left with their counts, the
//! 28px rows of the list, the text that is shown when the list is empty, and
//! the footer's summary of what Apply will do.
use super::overlay_shell;
use super::text_input::{self, TextInput};
use crate::i18n::t;
use crate::theme;
use crate::theme::Text as _;
use gpui::*;
use std::rc::Rc;

use super::keys::{self, Down, NextView, Up};

/// Binds the keys that the dialogs handle while their search field has the
/// keyboard focus: Up, Down and Tab. Enter and Escape are bound by the field
/// itself, and the search bar handles them before the field does.
pub fn bind(cx: &mut App) {
    keys::bind(
        cx,
        "SelectionModal",
        &[keys::Key::Up, keys::Key::Down, keys::Key::Tab],
    );
}

/// The height of a rail item and of a row of the list.
pub const ROW: f32 = theme::CONTROL_MD;
/// The height of the search bar.
const SEARCH: f32 = 38.;

/// A key that was pressed while the search field had the keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    /// The Enter key.
    Confirm,
    /// The Tab key.
    Next,
    /// The Escape key.
    Dismiss,
}

type KeyHandler = Rc<dyn Fn(Key, &mut Window, &mut App) + 'static>;

/// Returns the search bar: a search icon and the field `input`. `on_key` is
/// called with each key of [`Key`] that is pressed while the field has the
/// keyboard focus. With `views`, Tab is passed to `on_key` as `Key::Next`,
/// which the Add Tags dialog uses to show the next view of the rail. Without
/// `views`, Tab moves the keyboard focus to the dialog's next control.
pub fn search_bar(
    input: &Entity<TextInput>,
    views: bool,
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
        .on_action(move |_: &NextView, window, cx| {
            if views {
                next(window, cx);
            } else {
                cx.propagate();
            }
        })
        // Enter and Escape are handled here, before the field handles them,
        // and are passed to the dialog.
        .capture_action(move |_: &text_input::Commit, window, cx| {
            cx.stop_propagation();
            confirm(window, cx);
        })
        .capture_action(move |_: &text_input::Cancel, window, cx| {
            cx.stop_propagation();
            dismiss(window, cx);
        })
        // The search icon is in the tertiary text colour at 60% of that
        // colour's opacity.
        .child(
            super::icon(
                "library-search",
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

/// Returns the rail: `width` wide, with a border on its right and 4px of
/// padding.
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

/// Returns the element for a count, in the monospaced font at the caption
/// size. It is drawn at half strength until the pointer is over the row whose
/// hover group is `group`. With `strong` it is always at full strength.
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

/// Returns the 1px line that separates two groups of rail items.
pub fn rail_separator(cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .h(px(1.))
        .flex_none()
        .mx(px(10.))
        .my(px(4.))
        .bg(theme.border_secondary())
}

/// Returns a row of the list, 4px in from the list's edges. Its background is
/// the subtle overlay under the pointer, and while `focused` is true, which is
/// when the arrow keys have moved to the row. The row is the hover group
/// `group`, so that its children can change while the pointer is over the row.
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

/// Returns the text that is shown in place of an empty list.
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

/// Returns the footer's summary: how many tags or folders Apply adds and
/// removes, and how many files it applies to. While nothing is waiting to be
/// applied, it is the number of selected files.
pub fn summary(added: usize, removed: usize, selected: usize) -> String {
    // i18n: the summary is English in every language
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

/// Returns the left part of the footer: the summary, in the monospaced font at
/// the caption size. When the summary is empty and `hints` is true, it shows
/// the keys that the list handles.
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
            .child(format!(" {}", t("Select")))
            .child(overlay_shell::tab_key(cx))
            .child(format!(" {}", t("Switch"))),
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
