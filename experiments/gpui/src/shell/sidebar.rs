//! The sidebar (`Sidebar.tsx`): the library, the system destinations in the
//! Electron order, then Folders and Smart Folders, over a tree filter.
use super::library::{LibraryPopover, Toggled};
use crate::grid::thumbnails::Thumbnails;
use crate::session::{Change, Session};
use crate::theme::Text as _;
use crate::ui::{self, field::Look, sidebar_row::Branch, text_input::TextInput};
use crate::{motion, theme};
use gpui::*;
use picto_library::query::ItemScope;
use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Folders,
    SmartFolders,
}

const SECTIONS: [(Section, &str); 2] = [
    (Section::Folders, "Folders"),
    (Section::SmartFolders, "Smart Folders"),
];

fn section_of(scope: &ItemScope, screen: bool) -> Option<Section> {
    match scope {
        _ if screen => None,
        ItemScope::Folder { .. } | ItemScope::FolderTree { .. } => Some(Section::Folders),
        ItemScope::SmartFolder { .. } => Some(Section::SmartFolders),
        _ => None,
    }
}

pub struct Sidebar {
    session: Entity<Session>,
    thumbnails: Entity<Thumbnails>,
    filter: Entity<TextInput>,
    /// Destinations whose children are folded away.
    collapsed: HashSet<usize>,
    /// Sections folded from their header.
    folded: [bool; 2],
    scroll: ScrollHandle,
    /// The library switcher's popover, and the switcher's bounds it opens
    /// under.
    library_popover: Entity<LibraryPopover>,
    switcher: Rc<Cell<Bounds<Pixels>>>,
    _subscriptions: Vec<Subscription>,
}

/// The library switcher's highlight and the pointer over it.
#[derive(Default)]
struct Switcher {
    hovered: bool,
    /// The highlight shows while hovered or open.
    lit: motion::Fade,
    /// Open: the highlight takes the active surface.
    open: motion::Fade,
}

/// The list scrolls this far before "Return to Top" appears.
const RETURN_TO_TOP_AFTER: f32 = 360.;

impl Sidebar {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| TextInput::sidebar("Filter", cx));
        let switcher = Rc::new(Cell::new(Bounds::default()));
        let library_popover = cx.new(|cx| {
            LibraryPopover::new(session.clone(), thumbnails.clone(), switcher.clone(), cx)
        });
        let subscriptions = vec![
            cx.observe(&filter, |_, _, cx| cx.notify()),
            // The switcher shows whether its popover is open.
            cx.subscribe(&library_popover, |_, _, _: &Toggled, cx| cx.notify()),
            cx.subscribe(&session, |_, _, change: &Change, cx| {
                if matches!(change, Change::Navigation) {
                    cx.notify();
                }
            }),
        ];
        Self {
            session,
            thumbnails,
            filter,
            collapsed: HashSet::new(),
            folded: [false; 2],
            scroll: ScrollHandle::new(),
            library_popover,
            switcher,
            _subscriptions: subscriptions,
        }
    }

    /// The library switcher (`LibrarySwitcherButton`): the cover, the
    /// library's name and a selector glyph, over a highlight shaped as the
    /// cover's circle joined to a bar. A click opens the library popover;
    /// while it is open the highlight takes the active surface and the
    /// glyph stays lit.
    fn library(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        let (cover, name) = {
            let session = self.session.read(cx);
            let name = session
                .library_name()
                .unwrap_or_else(|| "No library".into());
            (session.library_cover.clone(), name)
        };
        let open = self.library_popover.read(cx).is_open();
        let state = window.use_keyed_state("library-switcher", cx, |_, _| Switcher::default());
        let hint = window.use_keyed_state("library-switcher-tip", cx, |_, _| {
            ui::tooltip::Hover::default()
        });
        state.update(cx, |state, _| {
            state.lit.set(state.hovered || open, motion::SIDEBAR_HOVER);
            state.open.set(open, motion::SIDEBAR_HOVER);
        });
        let (lit, active) = {
            let state = state.read(cx);
            (state.lit.value(), state.open.value())
        };
        if state.read(cx).lit.animating()
            || state.read(cx).open.animating()
            || hint.read(cx).animating()
        {
            window.request_animation_frame();
        }
        // The cover stays decoded however far the grid scrolls.
        let pinned = cover.clone().map(|path| Resource::Path(path.into()));
        self.thumbnails
            .update(cx, |thumbnails, _| thumbnails.pin("library-cover", pinned));
        // A canvas takes no opacity of its own, so the fade is in the colour.
        let fill = theme::blend(
            gpui::transparent_black().into(),
            theme::blend(theme.surface_hover(), theme.surface_active(), active),
            lit,
        );
        let bounds = self.switcher.clone();
        let highlight = canvas(
            // The popover opens under the button's box, which this fills.
            move |button, _, _| bounds.set(button),
            move |bounds, _, window, _| {
                // A 24px-radius circle about the cover, and a 30px bar from
                // the circle's centre to the right edge, drawn as one outline
                // so the overlap is not painted twice.
                let (left, top, right) = (bounds.left(), bounds.top(), bounds.right());
                let meet = px(24. + (24f32 * 24. - 15. * 15.).sqrt());
                let corner = point(px(6.), px(6.));
                let mut path = PathBuilder::fill();
                path.move_to(point(left + meet, top + px(9.)));
                path.line_to(point(right - px(6.), top + px(9.)));
                path.arc_to(corner, px(0.), false, true, point(right, top + px(15.)));
                path.line_to(point(right, top + px(33.)));
                path.arc_to(
                    corner,
                    px(0.),
                    false,
                    true,
                    point(right - px(6.), top + px(39.)),
                );
                path.line_to(point(left + meet, top + px(39.)));
                path.arc_to(
                    point(px(24.), px(24.)),
                    px(0.),
                    true,
                    true,
                    point(left + meet, top + px(9.)),
                );
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, fill);
                }
            },
        )
        .absolute()
        .inset_0();
        let entered = (state, hint.clone());
        div()
            .id("library-switcher")
            .relative()
            .h(px(48.))
            .flex_none()
            .mt(px(4.))
            .mb(px(8.))
            .ml(px(theme::SIDEBAR_INSET))
            .mr(px(theme::SIDEBAR_INSET + 2.))
            .pr(px(9.))
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .on_hover(move |hovered, _, cx| {
                entered.0.update(cx, |state, cx| {
                    state.hovered = *hovered;
                    cx.notify();
                });
                entered.1.update(cx, |hint, cx| hint.set(*hovered, cx));
            })
            .on_click(cx.listener(|this, _, window, cx| {
                this.library_popover
                    .update(cx, |popover, cx| popover.toggle(window, cx));
            }))
            .child(highlight)
            .child(div().relative().ml(px(4.5)).flex_none().child(
                ui::library_avatar("library-switcher-avatar", cover, 39.).cache(&self.thumbnails),
            ))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .line_height(px(24.))
                    .child(name),
            )
            .child(
                ui::icon(
                    "selector",
                    14.,
                    theme::blend(theme.text_tertiary(), theme.text(), lit),
                )
                .relative()
                .flex_none()
                .opacity(0.7 + 0.3 * lit),
            )
            .children(hint.read(cx).layer(&ui::tip("Switch Library"), 48., cx))
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Sidebar);
        let theme = theme::theme(cx);
        let filter = self.filter.read(cx).value().trim().to_lowercase();
        let filtering = !filter.is_empty();
        let (entries, active) = {
            let session = self.session.read(cx);
            (session.nav.clone(), session.scope)
        };
        let section =
            |index: usize| section_of(&entries[index].scope, entries[index].screen.is_some());
        let count_of = |wanted| {
            (0..entries.len())
                .filter(|index| section(*index) == Some(wanted))
                .count()
        };
        // The tree filter keeps each matching folder and its ancestors, and
        // opens every branch (`treeFilter.ts`). System rows are not filtered.
        let mut kept = vec![!filtering; entries.len()];
        if filtering {
            for index in 0..entries.len() {
                if section(index).is_none() {
                    kept[index] = true;
                } else if entries[index].label.to_lowercase().contains(&filter) {
                    let mut at = Some(index);
                    while let Some(index) = at {
                        kept[index] = true;
                        at = entries[index].parent;
                    }
                }
            }
        }
        let shown_at = |index: usize| {
            if !kept[index] {
                return false;
            }
            if filtering {
                return true;
            }
            let mut parent = entries[index].parent;
            while let Some(ancestor) = parent {
                if self.collapsed.contains(&ancestor) {
                    return false;
                }
                parent = entries[ancestor].parent;
            }
            true
        };
        // Whether a row has a sibling below it among the rows that show.
        let has_next_sibling = |index: usize| {
            (index + 1..entries.len()).any(|other| {
                entries[other].parent == entries[index].parent
                    && section(other) == section(index)
                    && kept[other]
            })
        };

        let header = |slot: usize, cx: &mut Context<Self>| {
            let (which, label) = SECTIONS[slot];
            let (tip, id) = match which {
                Section::Folders => (ui::tip("New Folder").keys(&["⌘", "⇧", "N"]), "folders"),
                Section::SmartFolders => (ui::tip("New Smart Folder or Group"), "smart-folders"),
            };
            ui::section_header(id, label)
                .count(count_of(which))
                .expanded(!self.folded[slot])
                .on_toggle(cx.listener(move |this, _, _, cx| {
                    this.folded[slot] = !this.folded[slot];
                    cx.notify();
                }))
                // TODO: create the folder or smart folder and start renaming it.
                .on_add(tip, |_, _, _| {})
        };
        let mut list = div()
            .id("navigation")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .pr(px(ui::scrollbar::GUTTER + theme::SIDEBAR_LIST_PADDING))
            .pb(px(16.));
        let mut shown = [false; 2];
        // A section's first row sits 2px below its header.
        let mut after_header = false;
        let mut matches = 0;
        for (index, entry) in entries.iter().enumerate() {
            // Both headers always show, in order, even above an empty section.
            let slot = section(index).map(|which| which as usize);
            if let Some(slot) = slot {
                for earlier in 0..=slot {
                    if !shown[earlier] {
                        shown[earlier] = true;
                        list = list.child(header(earlier, cx));
                        after_header = true;
                    }
                }
                if self.folded[slot] && !filtering {
                    continue;
                }
            }
            if !shown_at(index) {
                continue;
            }
            matches += slot.is_some() as usize;
            if std::mem::take(&mut after_header) {
                list = list.child(div().h(px(2.)));
            }
            let has_children =
                (0..entries.len()).any(|other| entries[other].parent == Some(index) && kept[other]);
            let expanded = filtering || !self.collapsed.contains(&index);
            let icon = entry.icon.unwrap_or(match entry.scope {
                ItemScope::SmartFolder { .. } if entry.parent.is_none() => "layout-grid",
                _ if has_children && expanded => "folder-open",
                _ => "folder",
            });
            // Each ancestor's vertical guide continues through this row while
            // that ancestor has siblings still to come.
            let mut continues = vec![false; entry.depth];
            let mut ancestor = entry.parent;
            while let Some(at) = ancestor {
                let depth = entries[at].depth;
                if depth < continues.len() {
                    continues[depth] = has_next_sibling(at);
                }
                ancestor = entries[at].parent;
            }
            let mut row = ui::sidebar_row(index, icon, entry.label.clone())
                .depth(entry.depth)
                .count(entry.count)
                .active(active == index)
                .disclosure(has_children.then_some(expanded))
                .branch(Branch {
                    continues,
                    last_child: !has_next_sibling(index),
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.session
                        .update(cx, |session, cx| session.navigate(index, cx));
                }));
            // The filter holds every branch open.
            if !filtering {
                row = row.on_toggle(cx.listener(move |this, _, _, cx| {
                    if !this.collapsed.remove(&index) {
                        this.collapsed.insert(index);
                    }
                    cx.notify();
                }));
            }
            list = list.child(row);
        }
        for slot in 0..SECTIONS.len() {
            if !shown[slot] {
                list = list.child(header(slot, cx));
            }
        }
        if filtering && matches == 0 {
            list = list.child(
                div()
                    .mt(px(6.))
                    .ml(px(theme::SIDEBAR_INSET))
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child("No matching folders"),
            );
        }
        // The rail's edge is drawn over the app background, as the shell's
        // border is in the Electron layout, with the rail surface inside it.
        div()
            .id("sidebar")
            .size_full()
            .bg(theme.background())
            .border_r_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .size_full()
                    .bg(theme.rail())
                    .font_family(theme::FONT_INSPECTOR)
                    .flex()
                    .flex_col()
                    .child(self.library(window, cx))
                    .children(
                        self.library_popover
                            .read(cx)
                            .is_open()
                            .then(|| self.library_popover.clone()),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(list)
                            .child(ui::scrollbar("sidebar-scrollbar", &self.scroll, window, cx))
                            .children(ui::scroll_to_top(
                                "sidebar-return-to-top",
                                &self.scroll,
                                RETURN_TO_TOP_AFTER,
                                // 48px above the sidebar's foot, 38 of which
                                // is the filter below this list.
                                10.,
                                window,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .relative()
                            .h(px(38.))
                            .px(px(10.))
                            .pb(px(10.))
                            .flex_none()
                            // The footer covers the last 10px of the list.
                            .child(
                                div()
                                    .absolute()
                                    .top(px(-10.))
                                    .left_0()
                                    .right_0()
                                    .h(px(10.))
                                    .bg(theme.rail()),
                            )
                            .child(
                                ui::field("sidebar-filter", &self.filter, Look::SidebarFilter)
                                    .icon("toolbar-filter", 16., 8., theme.text())
                                    .clearable(),
                            ),
                    ),
            )
    }
}
