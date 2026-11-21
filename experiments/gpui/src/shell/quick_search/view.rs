//! Draws the Quick Search panel. From top to bottom the panel has:
//!
//! - a header 48 px high, with the search field and the close button;
//! - a row 36 px high with the five tabs, each with the number of its
//!   matches;
//! - the list, whose rows are 26 px high. A row has an icon, the name with
//!   the matched letters in bold and underlined, and a short text at the
//!   right. An item's row is 92 px high and has an 80 px thumbnail, the name
//!   and a line of facts;
//! - a footer 40 px high that names the keys.
//!
//! The panel is 480 px wide and is drawn in the middle of the main window
//! over the dimmed backdrop that the dialogs use. These measurements are
//! those of the Eagle app's quick-search panel. The parts are the app's own
//! controls: the dialog's backdrop and glass panel (`ui::modal`), the
//! dialog search field (`ui::field` with `Look::ModalSearch`), the tabs of
//! the batch rename dialog (`ui::segmented`), the row of the Add Tags
//! dialog's list (`ui::selection_modal::item_row`) and the key hints of the
//! tag picker (`ui::overlay_shell`).
use super::sources::{Entry, Picture};
use super::{Items, Kind, QuickSearch, Row};
use crate::i18n::{en, t};
use crate::theme::Text as _;
use crate::ui::keys::{Dismiss, Down, NextView, Up};
use crate::ui::{self, overlay_shell, selection_modal, text_input};
use crate::{keymap, motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::ops::Range;
use std::time::Duration;

/// The names under which the panel pins the thumbnails of the item rows
/// that are in view (`Thumbnails::pin`). A pinned thumbnail stays in the
/// thumbnail cache while the panel draws it. Twelve names are enough,
/// because the list never shows more than six item rows at once.
pub const PINS: [&str; 12] = [
    "quick-search-0",
    "quick-search-1",
    "quick-search-2",
    "quick-search-3",
    "quick-search-4",
    "quick-search-5",
    "quick-search-6",
    "quick-search-7",
    "quick-search-8",
    "quick-search-9",
    "quick-search-10",
    "quick-search-11",
];

/// The width of the panel. In a narrow window the panel keeps 20 px free at
/// each side.
const WIDTH: f32 = 480.;
const HEADER: f32 = 48.;
const TABS: f32 = 36.;
const FOOTER: f32 = 40.;
/// The height of a row of an action, a place or a tag.
const ROW: f32 = 26.;
/// The height of an item's row, and the side of its thumbnail.
const ITEM_ROW: f32 = 92.;
const THUMBNAIL: f32 = 80.;
/// The height of the name of a group of rows.
const GROUP: f32 = 24.;
/// The space between two rows, and above the first and below the last.
const GAP: f32 = 2.;
/// The space above the first line of the list and below the last.
const LIST_PADDING: f32 = 6.;
/// The panel fades in over this time.
const OPEN: Duration = Duration::from_millis(70);

/// Returns the largest height of the list in a window `viewport` pixels
/// high. It is 16 rows where the window has room, and never less than 8
/// rows.
fn largest_list_height(viewport: f32) -> f32 {
    let rows = |count: f32| count * (ROW + GAP) + 2. * LIST_PADDING;
    (viewport - 260.).clamp(rows(8.), rows(16.))
}

/// Returns the height of the list for these lines in a window `viewport`
/// pixels high. The list is as high as its lines, but never lower than 8
/// rows and never higher than [`largest_list_height`]. A list with few
/// lines therefore leaves no large empty area in the panel.
fn list_height(viewport: f32, rows: &[Row]) -> f32 {
    let lines: f32 = rows.iter().map(line_height).sum();
    let smallest = 8. * (ROW + GAP) + 2. * LIST_PADDING;
    (lines + 2. * LIST_PADDING).clamp(smallest, largest_list_height(viewport))
}

/// Returns the height that the line `row` takes in the list, including the
/// space below it.
fn line_height(row: &Row) -> f32 {
    match row {
        Row::Header(_) => GROUP,
        Row::Entry { entry, .. } if entry.picture.is_some() => ITEM_ROW + GAP,
        Row::Entry { .. } => ROW + GAP,
    }
}

/// Returns `label` with the letters in `ranges` in bold and underlined. This
/// is how the tag picker marks the match of its search
/// (`overlay_shell::search_match`), for several pieces of the name.
fn marked(label: &str, ranges: &[Range<usize>], cx: &App) -> StyledText {
    let theme = theme::theme(cx);
    let style = HighlightStyle {
        font_weight: Some(FontWeight::BOLD),
        color: Some(theme.text().into()),
        underline: Some(UnderlineStyle {
            color: Some(theme.contrast(20.).into()),
            thickness: px(1.),
            wavy: false,
        }),
        ..Default::default()
    };
    StyledText::new(SharedString::from(label.to_owned()))
        .with_highlights(ranges.iter().cloned().map(|range| (range, style)))
}

/// Splits a keystroke that `keymap::keystroke` wrote, such as `cmd-shift-t`,
/// into its modifiers in alphabetical order and its key. Two keystrokes
/// with the same modifiers written in a different order then compare equal.
fn split_keystroke(written: &str) -> (Vec<&str>, &str) {
    // The key can itself be `-`, as in `cmd--`.
    let (modifiers, key) = match written.strip_suffix('-') {
        Some(modifiers) => (modifiers, "-"),
        None => written.rsplit_once('-').unwrap_or(("", written)),
    };
    let mut modifiers: Vec<&str> = modifiers
        .split('-')
        .filter(|modifier| !modifier.is_empty())
        .collect();
    modifiers.sort_unstable();
    (modifiers, key)
}

/// Returns whether `event` is a press of one of the keys of the registry
/// shortcut `id`, with the keyboard preset and the user's overrides applied.
fn pressed(id: &str, event: &KeyDownEvent, cx: &App) -> bool {
    let (preset, overrides) = crate::settings::keyboard(cx);
    let Some(shortcut) = crate::settings::registry::shortcut(id, preset, &overrides) else {
        return false;
    };
    let modifiers = event.keystroke.modifiers;
    let Some(typed) = crate::settings::registry::from_keystroke(
        &event.keystroke.key,
        modifiers.platform,
        modifiers.control,
        modifiers.alt,
        modifiers.shift,
    )
    .and_then(|keys| keymap::keystroke(&keys)) else {
        return false;
    };
    [shortcut.keys, shortcut.keys2].iter().any(|keys| {
        keymap::keystroke(keys)
            .is_some_and(|bound| split_keystroke(&bound) == split_keystroke(&typed))
    })
}

impl QuickSearch {
    /// Draws the header: the search field and the close button.
    ///
    /// The header takes the keys of the list from the search field. Up and
    /// Down move the highlight, Tab and Shift+Tab change the tab, Enter
    /// chooses the highlighted row and Escape closes the panel. The first
    /// three are the keys that the search field of the Add Tags dialog
    /// takes, in that dialog's key context (`SelectionModal`).
    fn header(&self, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let icon = theme::blend(gpui::transparent_black().into(), theme.text_tertiary(), 0.6);
        div()
            .h(px(HEADER))
            .flex_none()
            .pl(px(4.))
            .pr(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .key_context("SelectionModal")
            .on_action(cx.listener(|this, _: &Up, _, cx| this.step(false, cx)))
            .on_action(cx.listener(|this, _: &Down, _, cx| this.step(true, cx)))
            .on_action(cx.listener(|this, _: &NextView, window, cx| {
                let backward = window.modifiers().shift;
                this.step_kind(!backward, cx);
            }))
            .capture_action(cx.listener(|this, _: &text_input::Commit, window, cx| {
                cx.stop_propagation();
                this.confirm(window, cx);
            }))
            .capture_action(cx.listener(|this, _: &text_input::Cancel, window, cx| {
                cx.stop_propagation();
                this.close(window, cx);
            }))
            .child(
                div().flex_1().min_w_0().child(
                    ui::field(
                        "quick-search-field",
                        &self.input,
                        ui::field::Look::ModalSearch,
                    )
                    .icon("library-search", 14., 10., icon),
                ),
            )
            .child(ui::window_chrome::close_button(
                "quick-search-close",
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            ))
    }

    /// Draws the row of tabs. A tab shows its name and, after it, the number
    /// of rows of its kind that match the text. A click on a tab gives the
    /// keyboard focus back to the search field, so that the user can go on
    /// typing.
    fn tabs(&self, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let Some(open) = &self.open else {
            return div();
        };
        let segments = Kind::TABS
            .iter()
            .map(|kind| {
                let label = match open.count(*kind) {
                    Some(count) => {
                        format!("{} {}", kind.label(), crate::i18n::format::grouped(count))
                    }
                    None => kind.label().to_owned(),
                };
                ui::segmented::tab(label)
            })
            .collect();
        let selected = Kind::TABS.iter().position(|kind| *kind == open.kind);
        let this = cx.entity().downgrade();
        div()
            .h(px(TABS))
            .flex_none()
            .px(px(8.))
            .pb(px(4.))
            .flex()
            .flex_col()
            .justify_center()
            .border_b_1()
            .border_color(theme.border_secondary())
            .child(
                ui::segmented::segmented(
                    "quick-search-tabs",
                    ui::segmented::Look::FilledTabs,
                    segments,
                    selected,
                )
                .name(en("Quick Search"))
                .on_select(move |index, window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.show_kind(Kind::TABS[index], cx);
                        window.focus(&this.input.focus_handle(cx), cx);
                    });
                }),
            )
    }

    /// Draws one row of an action, a place or a tag: the icon, the name and
    /// the text at the right. For an action the text at the right is the
    /// shortcut, drawn as a menu row draws its shortcut.
    fn plain_row(&self, entry: &Entry, ranges: &[Range<usize>], cx: &App) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let tint = entry.tint.map(rgb).unwrap_or(theme.text_secondary());
        let mut parts = vec![
            div()
                .w(px(18.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(ui::icon(entry.icon.clone(), 16., tint))
                .into_any_element(),
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(marked(&entry.label, ranges, cx))
                .into_any_element(),
        ];
        if !entry.detail.is_empty() {
            let detail = div()
                .flex_none()
                .max_w(relative(0.4))
                .truncate()
                .text(theme::TEXT_CAPTION)
                .child(entry.detail.clone());
            parts.push(if entry.kind == Kind::Actions {
                detail.opacity(0.5).into_any_element()
            } else {
                detail.text_color(theme.text_tertiary()).into_any_element()
            });
        }
        parts
    }

    /// Draws one row of an item: the thumbnail from the thumbnail cache, the
    /// name on at most two lines, and the line of facts. A name that needs
    /// more than two lines ends with an ellipsis. A thumbnail that
    /// cannot be loaded is replaced by the icon of a missing picture.
    fn item_row(
        &self,
        entry: &Entry,
        picture: &Picture,
        ranges: &[Range<usize>],
        cx: &App,
    ) -> Vec<AnyElement> {
        let theme = theme::theme(cx);
        let path = self.session.read(cx).thumbnail(&picture.hash);
        let resource = Resource::Path(path.clone().into());
        let failed = picture.hash.is_empty() || self.thumbnails.read(cx).is_failed(&resource);
        let thumbnail = div()
            .w(px(THUMBNAIL))
            .h(px(THUMBNAIL))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_SM))
            .overflow_hidden();
        let thumbnail = if failed {
            thumbnail.bg(theme.subtle_overlay()).child(ui::icon(
                "empty-photo",
                28.,
                theme.text_tertiary(),
            ))
        } else {
            thumbnail.child(
                img(path)
                    .image_cache(&self.thumbnails)
                    .size_full()
                    .object_fit(ObjectFit::Contain),
            )
        };
        vec![
            thumbnail.into_any_element(),
            div()
                .flex_1()
                .min_w_0()
                .ml(px(6.))
                .pr(px(6.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .whitespace_normal()
                        .line_height(px(16.))
                        .line_clamp(2)
                        .text_ellipsis()
                        .child(marked(&entry.label, ranges, cx)),
                )
                .child(
                    div()
                        .truncate()
                        .text(theme::TEXT_CAPTION)
                        .text_color(theme.text_tertiary())
                        .child(entry.detail.clone()),
                )
                .into_any_element(),
        ]
    }

    /// Draws the list, or the text that stands in its place when it has no
    /// rows. It also pins the thumbnails of the item rows that are in view.
    fn list(&self, height: f32, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let Some(open) = &self.open else {
            return div();
        };
        let frame = div().relative().h(px(height)).flex_none();
        if open.rows.is_empty() {
            let text: SharedString = match (&open.items, open.kind) {
                (Items::TooShort, Kind::Items) => {
                    en("Type at least 3 characters to search items.").into()
                }
                (Items::Waiting, Kind::Items) => t("Searching...").into(),
                (Items::Failed(error), Kind::Items) => error.clone().into(),
                _ => t("No results").into(),
            };
            return frame.child(
                ui::a11y::text(div().id("quick-search-empty"), text.clone())
                    .size_full()
                    .px(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_center()
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child(text),
            );
        }
        let chosen = open
            .rows
            .iter()
            .filter(|row| matches!(row, Row::Entry { .. }))
            .count();
        // The part of the list that is in view, to pin the thumbnails drawn
        // there. The scroll offset is zero or negative.
        let top = -f32::from(self.scroll.offset().y);
        let mut y = LIST_PADDING;
        let mut in_view: Vec<Resource> = Vec::new();
        let mut list = div()
            .id("quick-search-list")
            .role(Role::ListBox)
            .aria_label(open.kind.label())
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .py(px(LIST_PADDING))
            .pr(px(ui::scrollbar::GUTTER - 4.))
            .flex()
            .flex_col();
        let mut position = 0;
        for (index, row) in open.rows.iter().enumerate() {
            let line = line_height(row);
            let visible = y + line > top && y < top + height;
            y += line;
            match row {
                Row::Header(label) => {
                    list = list.child(
                        div()
                            .h(px(GROUP))
                            .flex_none()
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .text(theme::TEXT_CAPTION)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_tertiary())
                            .child(label.clone()),
                    );
                }
                Row::Entry { entry, ranges } => {
                    position += 1;
                    let highlighted = open.highlighted == Some(index);
                    // The Add Tags dialog's row, in its lit state. That
                    // state has no highlight of its own under the pointer:
                    // the panel moves its one highlight to the row under the
                    // pointer instead. A row that is not highlighted gets a
                    // clear background.
                    let mut line = selection_modal::item_row(
                        ElementId::Name(format!("quick-search-row-{}", entry.key).into()),
                        SharedString::from(format!("quick-search-{}", entry.key)),
                        true,
                        cx,
                    )
                    .role(Role::ListBoxOption)
                    .aria_label(entry.label.clone())
                    .aria_selected(highlighted)
                    .aria_position_in_set(position)
                    .aria_size_of_set(chosen)
                    .mb(px(GAP))
                    .when(!highlighted, |line| line.bg(gpui::transparent_black()))
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        this.point_at(index, event.position, cx)
                    }))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.choose(index, window, cx)),
                    );
                    line = match &entry.picture {
                        Some(picture) => {
                            if visible && in_view.len() < PINS.len() && !picture.hash.is_empty() {
                                let path = self.session.read(cx).thumbnail(&picture.hash);
                                in_view.push(Resource::Path(path.into()));
                            }
                            line.h(px(ITEM_ROW))
                                .px(px(6.))
                                .children(self.item_row(entry, picture, ranges, cx))
                        }
                        None => line.h(px(ROW)).children(self.plain_row(entry, ranges, cx)),
                    };
                    list = list.child(line);
                }
            }
        }
        self.thumbnails.update(cx, |thumbnails, _| {
            let mut in_view = in_view.into_iter();
            for holder in PINS {
                thumbnails.pin(holder, in_view.next());
            }
        });
        frame.child(list).child(ui::scrollbar(
            "quick-search-scrollbar",
            &self.scroll,
            window,
            cx,
        ))
    }

    /// Draws the footer, which names the keys: Tab changes the tab, the
    /// arrows move the highlight, Enter chooses and Escape closes.
    fn footer(&self, cx: &App) -> Div {
        let theme = theme::theme(cx);
        let hint = |keys: Vec<Div>, label: &str| {
            overlay_shell::hint(cx)
                .children(keys)
                .child(label.to_owned())
        };
        div()
            .h(px(FOOTER))
            .flex_none()
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(12.))
            .bg(theme.footer_surface())
            .border_t_1()
            .border_color(theme.border_secondary())
            .child(hint(vec![overlay_shell::tab_key(cx)], t("Switch")))
            .child(hint(vec![overlay_shell::kbd("↑↓", cx)], t("Move")))
            .child(hint(vec![overlay_shell::kbd("↩", cx)], t("Select")))
            .child(div().flex_1())
            .child(hint(vec![overlay_shell::kbd(t("Esc"), cx)], t("Close")))
    }
}

impl Render for QuickSearch {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(open) = &mut self.open else {
            return gpui::Empty.into_any_element();
        };
        // The search field takes the keyboard focus when the panel is first
        // drawn, as a dialog's first field does (`ui::modal::Host`).
        if open.focus_field {
            open.focus_field = false;
            window.focus(&self.input.focus_handle(cx), cx);
        }
        let shown = motion::EASE_SCALE_IN.ease(motion::progress(open.opened.elapsed(), OPEN));
        if shown < 1. {
            window.request_animation_frame();
        }
        let theme = theme::theme(cx);
        let viewport = window.viewport_size();
        let width = WIDTH.min(f32::from(viewport.width) - 40.);
        let viewport_height = f32::from(viewport.height);
        let height = self
            .open
            .as_ref()
            .map_or(0., |open| list_height(viewport_height, &open.rows));
        let largest = HEADER + TABS + largest_list_height(viewport_height) + FOOTER;
        let panel =
            ui::modal::dialog_panel("quick-search", theme::RADIUS_LG, en("Quick Search"), cx)
                .a11y_focus(&self.focus)
                .w(px(width))
                .flex_none()
                .overflow_hidden()
                .opacity(shown)
                .flex()
                .flex_col()
                .font_family(theme::FONT_UI)
                .text(theme::TEXT_MD)
                .text_color(theme.text())
                .child(self.header(cx))
                .child(self.tabs(cx))
                .child(self.list(height, window, cx))
                .child(self.footer(cx));
        let layer = ui::modal::backdrop("quick-search-backdrop", shown, Some(&self.focus), window)
            // The key that opens the panel closes it, and the key that opens
            // it on the Places tab shows that tab. The keymap does not run
            // either shortcut here, because the keyboard focus is in a text
            // field inside a dialog.
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if pressed("nav.commandPalette", event, cx) {
                    cx.stop_propagation();
                    this.close(window, cx);
                } else if pressed("nav.goToFolder", event, cx) {
                    cx.stop_propagation();
                    this.show_kind(Kind::Places, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| this.close(window, cx)))
            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx)))
            // The panel is the first child of a box that has the panel's
            // largest height, and the backdrop centres that box. The top of
            // the panel therefore stays where it is when the list becomes
            // shorter or longer, and only the bottom moves.
            .child(
                div()
                    .h(px(largest))
                    .flex()
                    .flex_col()
                    .justify_start()
                    .child(panel),
            );
        ui::modal::over_window(layer, ui::layer::DIALOG).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{GAP, LIST_PADDING, ROW, Row, largest_list_height, list_height, split_keystroke};

    #[test]
    fn keystrokes_compare_equal_whatever_the_order_of_their_modifiers() {
        assert_eq!(
            split_keystroke("cmd-shift-alt-n"),
            split_keystroke("cmd-alt-shift-n")
        );
        assert_eq!(split_keystroke("cmd-t"), (vec!["cmd"], "t"));
        assert_eq!(split_keystroke("t"), (vec![], "t"));
        assert_eq!(split_keystroke("cmd--"), (vec!["cmd"], "-"));
        assert_eq!(split_keystroke("-"), (vec![], "-"));
        assert_ne!(split_keystroke("cmd-t"), split_keystroke("ctrl-t"));
    }

    #[test]
    fn the_list_is_as_high_as_its_lines_between_eight_and_sixteen_rows() {
        let rows = |count: f32| count * (ROW + GAP) + 2. * LIST_PADDING;
        assert_eq!(largest_list_height(865.), rows(16.));
        assert_eq!(largest_list_height(2000.), rows(16.));
        // In a window 600 px high the list is at most 340 px high.
        assert_eq!(largest_list_height(600.), 340.);
        assert_eq!(largest_list_height(300.), rows(8.));
        // A group name is 24 px high.
        let lines = |count: usize| -> Vec<Row> {
            (0..count).map(|_| Row::Header("Group".into())).collect()
        };
        assert_eq!(list_height(865., &[]), rows(8.));
        assert_eq!(list_height(865., &lines(12)), 12. * 24. + 2. * LIST_PADDING);
        assert_eq!(list_height(865., &lines(40)), rows(16.));
    }
}
