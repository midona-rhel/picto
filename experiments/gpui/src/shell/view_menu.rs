//! The view menu the titlebar's "View options" button opens
//! (`GridViewMenu.tsx`): the layout and the sort, then the display toggles
//! and the two rails. Each choice changes the destination's view at once
//! and saves it to its `view_pref` row.
use crate::grid::layout::ViewMode;
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

/// Which half of the menu a panel is.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    /// `ViewPanel`: layout and sort.
    Layout,
    /// `DisplayPanel`: the toggles.
    Display,
}

/// A rail's toggle in the menu was used.
pub enum RailToggled {
    Sidebar,
    Inspector,
}

pub struct ViewOptions {
    session: Entity<Session>,
    part: Part,
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

/// `LAYOUT_OPTIONS`
const LAYOUTS: [(&str, &str); 3] = [
    ("waterfall", "Waterfall"),
    ("grid", "Grid"),
    ("justified", "Justified"),
];

/// `SORT_OPTIONS`
const SORTS: [(&str, &str, SortField); 6] = [
    ("imported_at", "Date Added", SortField::ImportedAt),
    ("captured_at", "Date Created", SortField::CapturedAt),
    ("name", "Name", SortField::Name),
    ("total_size", "File Size", SortField::TotalSize),
    ("rating", "Rating", SortField::Rating),
    ("random", "Random", SortField::Random),
];

/// Whether the destination keeps its own order, which hides the sort
/// (`hasFixedOrder`): a folder, and Recently Viewed.
fn fixed_order(scope: &ItemScope) -> bool {
    matches!(
        scope,
        ItemScope::Folder { .. } | ItemScope::FolderTree { .. } | ItemScope::RecentlyViewed
    )
}

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

/// `.dirPill`: 24px buttons inside 2px of padding, taller than the select
/// beside it.
const DIRECTION_PILL: f32 = 28.;

/// A panel's height in its menu row. The sort row is as tall as the
/// direction pill, which a random sort does not show.
fn height(part: Part, scope: &ItemScope, random: bool) -> f32 {
    match part {
        Part::Layout if fixed_order(scope) => 2. * PADDING + theme::CONTROL_SM,
        Part::Layout => {
            let sort = if random {
                theme::CONTROL_SM
            } else {
                DIRECTION_PILL
            };
            2. * PADDING + theme::CONTROL_SM + GAP + sort
        }
        Part::Display => {
            let toggles = toggle_count(is_folder(scope)) as f32;
            // The toggles and the separator, each 6px from the next.
            2. * PADDING + toggles * TOGGLE_ROW + SEPARATOR + toggles * GAP
        }
    }
}

/// The view menu (`buildViewMenuEntries`) for the current destination,
/// with its two panels.
pub fn build(
    session: &Entity<Session>,
    position: Point<Pixels>,
    sidebar_shown: bool,
    inspector_shown: bool,
    cx: &mut App,
) -> (Entity<Menu>, [Entity<ViewOptions>; 2]) {
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
    let panel = |part: Part, cx: &mut App| {
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
    let layout = panel(Part::Layout, cx);
    let display = panel(Part::Display, cx);
    let entries = vec![
        menu::view(layout.clone(), WIDTH, height(Part::Layout, &scope, random))
            .padded()
            .into(),
        menu::Entry::Separator,
        menu::view(
            display.clone(),
            WIDTH,
            height(Part::Display, &scope, random),
        )
        .padded()
        .into(),
    ];
    // The selects open over the window, so the menu draws in it.
    let menu = cx.new(|cx| Menu::new(entries, position, cx).search(false).in_window());
    (menu, [layout, display])
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
        let label = |text: &'static str| {
            div()
                .text(theme::TEXT_SM)
                .text_color(theme.text_tertiary())
                .child(text)
        };
        let mode = match prefs.mode {
            ViewMode::Waterfall => "waterfall",
            ViewMode::Grid => "grid",
            ViewMode::Justified => "justified",
        };
        let choices = LAYOUTS
            .iter()
            .map(|(value, label)| {
                let choice = ui::choice(value, *label);
                match *value {
                    "grid" => choice.icon("select-border-all"),
                    "justified" => choice.rotated_icon("select-layout-board"),
                    _ => choice.icon("select-layout-board"),
                }
            })
            .collect();
        let this = cx.entity().downgrade();
        let mut panel = div().py(px(PADDING)).flex().flex_col().gap(px(GAP)).child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(label("Layout"))
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
            let field = SORTS
                .iter()
                .find(|(_, _, field)| *field == prefs.sort.field)
                .map_or("imported_at", |(value, _, _)| value);
            let direction = prefs.sort.direction;
            let order = |direction: SortDirection| match direction {
                SortDirection::Ascending => "ascending",
                SortDirection::Descending => "descending",
            };
            let sort_handle = cx.entity().downgrade();
            let select = ui::select(
                "view-sort",
                field,
                SORTS
                    .iter()
                    .map(|(value, label, _)| ui::choice(value, *label))
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
            let mut controls = div().flex().items_center().gap(px(4.)).child(select);
            if prefs.sort.field != SortField::Random {
                let directions = [SortDirection::Ascending, SortDirection::Descending];
                let pill_handle = cx.entity().downgrade();
                controls = controls.child(
                    segmented(
                        "view-direction",
                        segmented::Look::Direction,
                        vec![
                            segment("select-sort-ascending", "Ascending"),
                            segment("select-sort-descending", "Descending"),
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
                    .child(label("Sort by"))
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
                "Show Name",
                prefs.show_name,
                Flip::View(json!({ "show_name": !prefs.show_name }), false),
                false,
            ),
            (
                "Show Resolution",
                prefs.show_resolution,
                Flip::View(json!({ "show_resolution": !prefs.show_resolution }), false),
                false,
            ),
            (
                "Show Extension",
                prefs.show_extension,
                Flip::View(json!({ "show_extension": !prefs.show_extension }), false),
                false,
            ),
            (
                "Show File Type",
                prefs.show_extension_label,
                Flip::View(json!({ "show_label": !prefs.show_extension_label }), false),
                false,
            ),
            (
                "Show Item Count",
                prefs.show_item_count,
                Flip::View(json!({ "show_item_count": !prefs.show_item_count }), false),
                false,
            ),
            (
                "Compact",
                prefs.gap == 8.,
                Flip::View(
                    json!({ "spacing": if prefs.gap == 8. { "wide" } else { "tight" } }),
                    false,
                ),
                false,
            ),
            (
                "Fit Thumbnails",
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
                "Show Subfolder Content",
                prefs.show_subfolders,
                Flip::View(json!({ "show_subfolders": !prefs.show_subfolders }), true),
                false,
            ));
        }
        let rails = [
            (
                "Show Sidebar",
                self.sidebar_shown,
                Flip::Rail(|| RailToggled::Sidebar),
            ),
            (
                "Show Inspector",
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
            let mut toggle = ui::toggle(("view-toggle", index), on).disabled(disabled);
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
            Part::Layout => self.layout_panel(cx),
            Part::Display => self.display_panel(cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Part, height};
    use picto_library::FolderId;
    use picto_library::query::ItemScope;

    #[test]
    fn panel_heights_follow_their_rows() {
        // Layout, then the sort beside the 28px direction pill, 6px apart,
        // 4px above and below; a random sort has no pill.
        let all = ItemScope::All;
        assert_eq!(height(Part::Layout, &all, false), 4. + 26. + 6. + 28. + 4.);
        assert_eq!(height(Part::Layout, &all, true), 4. + 26. + 6. + 26. + 4.);
        // A folder has no sort, and one more toggle.
        let folder = ItemScope::FolderTree {
            folder_id: FolderId(1),
        };
        assert_eq!(height(Part::Layout, &folder, false), 4. + 26. + 4.);
        // Seven toggles, the separator and the two rails.
        assert_eq!(
            height(Part::Display, &all, false),
            8. + 9. * 28. + 5. + 9. * 6.
        );
        assert_eq!(
            height(Part::Display, &folder, false) - height(Part::Display, &all, false),
            28. + 6.
        );
    }
}
