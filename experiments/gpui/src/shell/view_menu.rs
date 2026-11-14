//! The view menu the titlebar's "View options" button opens
//! (`GridViewMenu.tsx`): the layout and the sort, then the display toggles
//! and the two rails. Each choice changes the destination's view at once
//! and saves it to its `view_pref` row.
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

/// A rail's toggle in the menu was used.
pub enum RailToggled {
    Sidebar,
    Inspector,
}

/// One of the view panels (`ViewPanel`, `DisplayPanel`).
pub struct ViewOptions {
    session: Entity<Session>,
    part: Panel,
    pub sidebar_shown: bool,
    pub inspector_shown: bool,
    _subscription: Subscription,
}

impl EventEmitter<RailToggled> for ViewOptions {}

/// The menu's usual width.
const WIDTH: f32 = 270.;
/// `.panel`: 4px above and below, rows 6px apart.
const PADDING: f32 = 4.;
const GAP: f32 = 6.;
/// `.toggleRow`
const TOGGLE_ROW: f32 = 28.;
/// `.sep`: one pixel with 2px above and below.
const SEPARATOR: f32 = 5.;

fn is_folder(scope: &ItemScope) -> bool {
    matches!(
        scope,
        ItemScope::Folder { .. } | ItemScope::FolderTree { .. }
    )
}

/// The display panel's toggles for a destination, and whether the folder's
/// subfolder toggle is among them.
fn toggle_count(folder: bool) -> usize {
    7 + folder as usize + 2
}

/// `.toggleRow`'s 24px gap and the 32px switch after the label, and the
/// 6px (`.customItem`) and 4px (the menu) either side of a panel.
const TOGGLE_GAP: f32 = 24.;
const SWITCH: f32 = 32.;
const PANEL_INSET: f32 = 2. * 6. + 2. * 4.;

/// The display panel's labels for a destination, in order.
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

/// The display panel's width where it sets its own (the "Display"
/// submenu, `width: auto`): its widest label, the gap and the switch, inside
/// the panel's and the menu's padding.
fn display_width(widest_label: f32) -> f32 {
    (PANEL_INSET + widest_label + TOGGLE_GAP + SWITCH).ceil()
}

/// `.dirPill`: 24px buttons inside 2px of padding, taller than the select
/// beside it.
const DIRECTION_PILL: f32 = 28.;

/// A panel's height in its menu row. The sort row is as tall as the
/// direction pill, which a random sort does not show.
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
            // The toggles and the separator, each 6px from the next.
            2. * PADDING + toggles * TOGGLE_ROW + SEPARATOR + toggles * GAP
        }
    }
}

/// `LAYOUT_OPTIONS` as a select's choices, each with its layout's glyph.
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

/// The two view panels for the current destination, which the titlebar's
/// view menu shows one above the other and the grid's context menu shows
/// inline and inside "Display".
pub struct Panels {
    pub layout: Entity<ViewOptions>,
    pub display: Entity<ViewOptions>,
    scope: ItemScope,
    random: bool,
    /// The display panel's own width, for the "Display" submenu.
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

    /// A panel as a menu entry, at its height for the destination.
    pub fn entry(&self, part: Panel) -> menu::Entry {
        // The menu's own width holds the panels at its top level; a
        // submenu takes the width the display panel sets.
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

/// The view menu (`buildViewMenuEntries`) for the current destination,
/// with its two panels.
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
    // A pop-up menu like the others: its selects open lists of their own.
    let menu = cx.new(|cx| Menu::new(entries, position, cx).search(false));
    (menu, panels)
}

/// A toggle label's width in the menu's text.
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
        // A label longer than the room left (German, Finnish) is cut with an
        // ellipsis rather than pushing the controls out of the menu.
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
        // Label, value, the patch that flips it (or a rail), whether it
        // fetches again, and whether it is disabled.
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
            // A disabled row is dimmed as a whole (`.toggleRowDisabled`);
            // its switch is not dimmed again.
            let mut toggle = ui::toggle(("view-toggle", index), on);
            if !disabled {
                toggle = toggle.on_toggle(move |_, cx| switch_act(cx));
            }
            let mut element = div()
                .id(("view-toggle-row", index))
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
        // `width: auto`: 103.33px of label, the 24px gap, the 32px switch,
        // 6px and 4px either side.
        assert_eq!(display_width(103.33), 180.);
        assert_eq!(display_labels(false).len(), 9);
        assert!(display_labels(true).contains(&"Show Subfolder Content"));
    }

    #[test]
    fn panel_heights_follow_their_rows() {
        // Layout, then the sort beside the 28px direction pill, 6px apart,
        // 4px above and below; a random sort has no pill.
        let all = ItemScope::All;
        assert_eq!(height(Panel::Layout, &all, false), 4. + 26. + 6. + 28. + 4.);
        assert_eq!(height(Panel::Layout, &all, true), 4. + 26. + 6. + 26. + 4.);
        // A folder has no sort, and one more toggle.
        let folder = ItemScope::FolderTree {
            folder_id: FolderId(1),
        };
        assert_eq!(height(Panel::Layout, &folder, false), 4. + 26. + 4.);
        // Seven toggles, the separator and the two rails.
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
