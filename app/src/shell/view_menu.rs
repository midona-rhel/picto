//! The view menu, which the titlebar's "View options" button opens. It has
//! two panels. The layout panel holds the layout select and the sort
//! controls. The display panel holds the switches that say what a tile
//! shows, and the two switches that show or hide the sidebar and the
//! inspector. Each choice changes the view of the open destination at once
//! and is saved to that destination's `view_pref` row.
use crate::grid::layout::ViewMode;
use crate::grid::menu::Panel;
use crate::grid::prefs::{self, fixed_order};
use crate::i18n::{en, t};
use crate::session::{Change, Session};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    menu::{self, Menu},
    segmented::{self, segment, segmented},
};
use gpui::*;
use picto_library::predicate::{SortDirection, SortField};
use picto_library::query::ItemScope;
use serde_json::json;

/// The event a panel emits when the user uses its "Show Sidebar" or its
/// "Show Inspector" switch.
pub enum RailToggled {
    Sidebar,
    Inspector,
}

/// One of the two view panels: the layout panel or the display panel, as
/// `part` says.
pub struct ViewOptions {
    session: Entity<Session>,
    part: Panel,
    pub sidebar_shown: bool,
    pub inspector_shown: bool,
    _subscription: Subscription,
}

impl EventEmitter<RailToggled> for ViewOptions {}

/// The width of the layout panel's menu entry, in pixels.
const WIDTH: f32 = 270.;
/// A panel has 4px of padding above and below its rows (`PADDING`), and its
/// rows are 6px apart (`GAP`).
const PADDING: f32 = 4.;
const GAP: f32 = 6.;
/// The height of a row that has a switch, in pixels.
const TOGGLE_ROW: f32 = 28.;
/// The height of the separator between the display switches and the two
/// switches of the rails: a 1px line with 2px of margin above and below it.
const SEPARATOR: f32 = 5.;

fn is_folder(scope: &ItemScope) -> bool {
    matches!(
        scope,
        ItemScope::Folder { .. } | ItemScope::FolderTree { .. }
    )
}

/// Returns the number of switches in the display panel: seven that every
/// destination has, "Show Subfolder Content" when the destination is a
/// folder, and the two that show or hide the sidebar and the inspector.
fn toggle_count(folder: bool) -> usize {
    7 + folder as usize + 2
}

/// `TOGGLE_GAP` is the gap between the label of a switch's row and the
/// switch, and `SWITCH` is the width of the switch. `PANEL_INSET` is the
/// padding at the left and right of a panel together: on each side 6px from
/// the panel's menu entry and 4px from the menu.
const TOGGLE_GAP: f32 = 24.;
const SWITCH: f32 = 32.;
const PANEL_INSET: f32 = 2. * 6. + 2. * 4.;

/// Returns the labels of the display panel's switches for a destination, in
/// the order of the rows.
fn display_labels(folder: bool) -> Vec<&'static str> {
    let mut labels = vec![
        en("Show Name"),
        en("Show Resolution"),
        en("Show Extension"),
        en("Show File Type"),
        en("Show Item Count"),
        en("Compact"),
        en("Fit Thumbnails"),
    ];
    if folder {
        labels.push(en("Show Subfolder Content"));
    }
    labels.extend([en("Show Sidebar"), en("Show Inspector")]);
    labels
}

/// Returns the width of the display panel's menu entry, which is the width
/// of the "Display" submenu of the grid's context menu. It is the width of
/// the widest label, the gap and the switch, plus the padding of the panel
/// and of the menu, rounded up to a whole pixel.
fn display_width(widest_label: f32) -> f32 {
    (PANEL_INSET + widest_label + TOGGLE_GAP + SWITCH).ceil()
}

/// The height of the sort direction control, in pixels: 24px buttons inside
/// 2px of padding. It is taller than the select beside it.
const DIRECTION_PILL: f32 = 28.;

/// Returns the height of a panel's menu entry. The sort row is as tall as
/// the direction control. With a random sort the direction control is not
/// shown, and the sort row is as tall as its select.
fn height(part: Panel, scope: &ItemScope, random: bool) -> f32 {
    match part {
        Panel::Layout if fixed_order(scope) => 2. * PADDING + theme::CONTROL_SM,
        Panel::Layout => {
            let sort = if random {
                theme::CONTROL_SM
            } else {
                DIRECTION_PILL
            };
            2. * PADDING + theme::CONTROL_SM + GAP + sort
        }
        Panel::Display => {
            let toggles = toggle_count(is_folder(scope)) as f32;
            // The rows are the switches and the separator, with a 6px gap
            // between each row and the next.
            2. * PADDING + toggles * TOGGLE_ROW + SEPARATOR + toggles * GAP
        }
    }
}

/// Returns the layouts (`prefs::LAYOUTS`) as the choices of a select, each
/// with the icon of its layout.
pub fn layout_choices() -> Vec<ui::select::Choice> {
    prefs::LAYOUTS
        .iter()
        .map(|(mode, value, label)| {
            let choice = ui::choice(value, *label);
            match mode {
                ViewMode::Grid => choice.icon("select-border-all"),
                ViewMode::Justified => choice.rotated_icon("select-layout-board"),
                ViewMode::Waterfall => choice.icon("select-layout-board"),
            }
        })
        .collect()
}

/// The two view panels for the current destination. The titlebar's view
/// menu shows one above the other. The grid's context menu shows the layout
/// panel among its rows and the display panel inside its "Display" submenu.
pub struct Panels {
    pub layout: Entity<ViewOptions>,
    pub display: Entity<ViewOptions>,
    scope: ItemScope,
    random: bool,
    /// The width of the display panel's menu entry (`display_width`).
    display_width: f32,
}

impl Panels {
    pub fn new(
        session: &Entity<Session>,
        sidebar_shown: bool,
        inspector_shown: bool,
        window: &Window,
        cx: &mut App,
    ) -> Self {
        let (scope, random) = {
            let session = session.read(cx);
            (
                session
                    .nav
                    .get(session.scope)
                    .map_or(ItemScope::All, |entry| entry.scope.clone()),
                session.prefs.sort.field == SortField::Random,
            )
        };
        let panel = |part: Panel, cx: &mut App| {
            cx.new(|cx| ViewOptions {
                session: session.clone(),
                part,
                sidebar_shown,
                inspector_shown,
                _subscription: cx.subscribe(session, |_, _, change: &Change, cx| {
                    if matches!(change, Change::Results | Change::Navigation) {
                        cx.notify();
                    }
                }),
            })
        };
        let widest = display_labels(is_folder(&scope))
            .into_iter()
            .map(|label| label_width(label, window))
            .fold(0f32, f32::max);
        Self {
            layout: panel(Panel::Layout, cx),
            display: panel(Panel::Display, cx),
            scope,
            random,
            display_width: display_width(widest),
        }
    }

    /// Returns the panel `part` as a menu entry, with the height it has for
    /// the destination.
    pub fn entry(&self, part: Panel) -> menu::Entry {
        // The layout panel's entry is `WIDTH` wide. The display panel's
        // entry is as wide as its rows need, so that the "Display" submenu,
        // which holds only that entry, is no wider than that.
        let (view, width) = match part {
            Panel::Layout => (self.layout.clone(), WIDTH),
            Panel::Display => (self.display.clone(), self.display_width),
        };
        menu::view(view, width, height(part, &self.scope, self.random))
            .padded()
            .into()
    }

    pub fn both(&self) -> [Entity<ViewOptions>; 2] {
        [self.layout.clone(), self.display.clone()]
    }
}

/// Builds the view menu for the current destination at `position`, and
/// returns it together with its two panels.
pub fn build(
    session: &Entity<Session>,
    position: Point<Pixels>,
    sidebar_shown: bool,
    inspector_shown: bool,
    window: &Window,
    cx: &mut App,
) -> (Entity<Menu>, Panels) {
    let panels = Panels::new(session, sidebar_shown, inspector_shown, window, cx);
    let entries = vec![
        panels.entry(Panel::Layout),
        menu::Entry::Separator,
        panels.entry(Panel::Display),
    ];
    // The menu has no search row. The selects in its panels open their own
    // lists of choices.
    let menu = cx.new(|cx| Menu::new(entries, position, cx).search(false));
    (menu, panels)
}

/// Returns the width of a switch's label in the menu's font and text size.
fn label_width(label: &'static str, window: &Window) -> f32 {
    crate::ui::text_width(label, font(theme::FONT_UI), theme::TEXT_MD, window)
}

impl ViewOptions {
    fn change(&self, patch: serde_json::Value, refetch: bool, cx: &mut Context<Self>) {
        self.session
            .update(cx, |session, cx| session.update_view(patch, refetch, cx));
    }

    fn layout_panel(&self, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let (prefs, scope) = {
            let session = self.session.read(cx);
            (
                session.prefs.clone(),
                session
                    .nav
                    .get(session.scope)
                    .map_or(ItemScope::All, |entry| entry.scope.clone()),
            )
        };
        // A label that is longer than the room beside the controls, as some
        // German and Finnish labels are, is cut off with an ellipsis.
        // Otherwise it would push the controls out of the menu.
        let label = |text: &'static str| {
            div()
                .min_w_0()
                .truncate()
                .text(theme::TEXT_SM)
                .text_color(theme.text_tertiary())
                .child(text)
        };
        let mode = prefs::mode_value(prefs.mode);
        let choices = layout_choices();
        let this = cx.entity().downgrade();
        let mut panel = div().py(px(PADDING)).flex().flex_col().gap(px(GAP)).child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(label(t("Layout")))
                .child(
                    ui::select("view-layout", mode, choices)
                        .width(130.)
                        .on_change(move |value, _, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.change(json!({ "view_mode": value }), false, cx)
                            });
                        }),
                ),
        );
        if !fixed_order(&scope) {
            let field = prefs::sort_value(prefs.sort.field);
            let direction = prefs.sort.direction;
            let order = prefs::direction_value;
            let sort_handle = cx.entity().downgrade();
            let select = ui::select(
                "view-sort",
                field,
                prefs::SORTS
                    .iter()
                    .map(|(_, value, label)| ui::choice(value, *label))
                    .collect(),
            )
            .width(130.)
            .on_change(move |value, _, cx| {
                let _ = sort_handle.update(cx, |this, cx| {
                    this.change(
                        json!({ "sort_field": value, "sort_order": order(direction) }),
                        true,
                        cx,
                    )
                });
            });
            let mut controls = div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(select);
            if prefs.sort.field != SortField::Random {
                let directions = [SortDirection::Ascending, SortDirection::Descending];
                let pill_handle = cx.entity().downgrade();
                controls = controls.child(
                    segmented(
                        "view-direction",
                        segmented::Look::Direction,
                        vec![
                            segment("select-sort-ascending", t("Ascending")),
                            segment("select-sort-descending", t("Descending")),
                        ],
                        directions.iter().position(|value| *value == direction),
                    )
                    .on_select(move |index, _, cx| {
                        let _ = pill_handle.update(cx, |this, cx| {
                            this.change(
                                json!({ "sort_field": field, "sort_order": order(directions[index]) }),
                                true,
                                cx,
                            )
                        });
                    }),
                );
            }
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label(t("Sort by")))
                    .child(controls),
            );
        }
        panel
    }

    fn display_panel(&self, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let (prefs, scope) = {
            let session = self.session.read(cx);
            (
                session.prefs.clone(),
                session
                    .nav
                    .get(session.scope)
                    .map_or(ItemScope::All, |entry| entry.scope.clone()),
            )
        };
        // Each entry of `toggles` is one switch: its label, whether it is
        // on, what using it does, and whether it is disabled. `Flip::View`
        // holds the patch of the view preferences and whether the results
        // are read again. `Flip::Rail` holds the event that asks the shell
        // to show or hide a rail.
        enum Flip {
            View(serde_json::Value, bool),
            Rail(fn() -> RailToggled),
        }
        let mut toggles: Vec<(&'static str, bool, Flip, bool)> = vec![
            (
                en("Show Name"),
                prefs.show_name,
                Flip::View(json!({ "show_name": !prefs.show_name }), false),
                false,
            ),
            (
                en("Show Resolution"),
                prefs.show_resolution,
                Flip::View(json!({ "show_resolution": !prefs.show_resolution }), false),
                false,
            ),
            (
                en("Show Extension"),
                prefs.show_extension,
                Flip::View(json!({ "show_extension": !prefs.show_extension }), false),
                false,
            ),
            (
                en("Show File Type"),
                prefs.show_extension_label,
                Flip::View(json!({ "show_label": !prefs.show_extension_label }), false),
                false,
            ),
            (
                en("Show Item Count"),
                prefs.show_item_count,
                Flip::View(json!({ "show_item_count": !prefs.show_item_count }), false),
                false,
            ),
            (
                en("Compact"),
                prefs.gap == 8.,
                Flip::View(
                    json!({ "spacing": if prefs.gap == 8. { "wide" } else { "tight" } }),
                    false,
                ),
                false,
            ),
            (
                en("Fit Thumbnails"),
                prefs.fit_cover,
                Flip::View(
                    json!({ "thumbnail_fit": if prefs.fit_cover { "contain" } else { "cover" } }),
                    false,
                ),
                prefs.mode != ViewMode::Grid,
            ),
        ];
        if is_folder(&scope) {
            toggles.push((
                en("Show Subfolder Content"),
                prefs.show_subfolders,
                Flip::View(json!({ "show_subfolders": !prefs.show_subfolders }), true),
                false,
            ));
        }
        let rails = [
            (
                en("Show Sidebar"),
                self.sidebar_shown,
                Flip::Rail(|| RailToggled::Sidebar),
            ),
            (
                en("Show Inspector"),
                self.inspector_shown,
                Flip::Rail(|| RailToggled::Inspector),
            ),
        ];
        let row = |index: usize,
                   label: &'static str,
                   on: bool,
                   flip: Flip,
                   disabled: bool,
                   cx: &mut Context<Self>| {
            let this = cx.entity().downgrade();
            let act = std::rc::Rc::new(move |cx: &mut App| {
                let _ = this.update(cx, |this, cx| match &flip {
                    Flip::View(patch, refetch) => this.change(patch.clone(), *refetch, cx),
                    Flip::Rail(event) => cx.emit(event()),
                });
            });
            let (row_act, switch_act) = (act.clone(), act);
            // A disabled row is drawn at 35% opacity as a whole. Its switch
            // is therefore not marked as disabled, which would dim it a
            // second time. It only gets no handler.
            let mut toggle = ui::toggle(("view-toggle", index), on).name(label.to_owned());
            if !disabled {
                toggle = toggle.on_toggle(move |_, cx| switch_act(cx));
            }
            // A click anywhere on the row does what the switch does. For
            // assistive technology the row is presentational, and the switch
            // carries the row's name.
            let mut element = div()
                .id(("view-toggle-row", index))
                .a11y_presentational()
                .h(px(TOGGLE_ROW))
                .flex()
                .items_center()
                .gap(px(24.))
                .child(
                    div()
                        .flex_1()
                        .text(theme::TEXT_MD)
                        .text_color(theme.text())
                        .whitespace_nowrap()
                        .child(label),
                )
                .child(div().ml_auto().flex().items_center().child(toggle));
            if disabled {
                element = element.opacity(0.35);
            } else {
                element = element
                    .cursor_pointer()
                    .on_click(move |_, _, cx| row_act(cx));
            }
            element
        };
        let mut panel = div().py(px(PADDING)).flex().flex_col().gap(px(GAP));
        let count = toggles.len();
        for (index, (label, on, flip, disabled)) in toggles.into_iter().enumerate() {
            panel = panel.child(row(index, label, on, flip, disabled, cx));
        }
        panel = panel.child(
            div()
                .h(px(1.))
                .my(px(2.))
                .flex_none()
                .bg(theme.border_secondary()),
        );
        for (offset, (label, on, flip)) in rails.into_iter().enumerate() {
            panel = panel.child(row(count + offset, label, on, flip, false, cx));
        }
        panel
    }
}

impl Render for ViewOptions {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.part {
            Panel::Layout => self.layout_panel(cx),
            Panel::Display => self.display_panel(cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{display_labels, display_width, height};
    use crate::grid::menu::Panel;
    use picto_library::FolderId;
    use picto_library::query::ItemScope;

    #[test]
    fn the_display_submenu_is_as_wide_as_its_widest_row() {
        // The width is 103.33px of label, the 24px gap, the 32px switch, and
        // 6px and 4px on each side, rounded up.
        assert_eq!(display_width(103.33), 180.);
        assert_eq!(display_labels(false).len(), 9);
        assert!(display_labels(true).contains(&"Show Subfolder Content"));
    }

    #[test]
    fn panel_heights_follow_their_rows() {
        // The layout row, then the sort row, which is as tall as the 28px
        // direction control, with 6px between the rows and 4px above and
        // below them. With a random sort there is no direction control.
        let all = ItemScope::All;
        assert_eq!(height(Panel::Layout, &all, false), 4. + 26. + 6. + 28. + 4.);
        assert_eq!(height(Panel::Layout, &all, true), 4. + 26. + 6. + 26. + 4.);
        // A folder has no sort row, and one more switch.
        let folder = ItemScope::FolderTree {
            folder_id: FolderId(1),
        };
        assert_eq!(height(Panel::Layout, &folder, false), 4. + 26. + 4.);
        // Seven switches, the separator and the two switches of the rails.
        assert_eq!(
            height(Panel::Display, &all, false),
            8. + 9. * 28. + 5. + 9. * 6.
        );
        assert_eq!(
            height(Panel::Display, &folder, false) - height(Panel::Display, &all, false),
            28. + 6.
        );
    }
}
