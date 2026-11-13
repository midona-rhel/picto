//! The sidebar (`Sidebar.tsx`): the library, the system destinations in the
//! Electron order, then Folders and Smart Folders, over a tree filter.
use crate::grid::thumbnails::Thumbnails;
use crate::session::{Change, Session};
use crate::theme;
use crate::ui::{self, text_input::TextInput};
use gpui::*;
use picto_library::query::ItemScope;
use std::collections::HashSet;

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
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl Sidebar {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| {
            let mut field = TextInput::new(cx);
            field.set_placeholder("Filter");
            field
        });
        let subscriptions = vec![
            cx.observe(&filter, |_, _, cx| cx.notify()),
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
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Sidebar);
        let theme = theme::theme(cx);
        let filter = self.filter.read(cx).value().to_lowercase();
        let (entries, active, cover) = {
            let session = self.session.read(cx);
            (
                session.nav.clone(),
                session.scope,
                session.library_cover.clone(),
            )
        };
        let count_of = |section| {
            entries
                .iter()
                .filter(|entry| section_of(&entry.scope, entry.screen) == Some(section))
                .count()
        };
        let mut list = div()
            .id("navigation")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .pr(px(ui::scrollbar::GUTTER + theme::SIDEBAR_LIST_PADDING));
        let mut shown = [false; 2];
        // A section's first row sits 2px below its header.
        let mut after_header = false;
        for (index, entry) in entries.iter().enumerate() {
            // Both headers always show, in order, even above an empty section.
            if let Some(section) = section_of(&entry.scope, entry.screen) {
                for (slot, (header, label)) in SECTIONS.into_iter().enumerate() {
                    let due = section == header || (slot == 0 && !shown[0]);
                    if due && !shown[slot] {
                        shown[slot] = true;
                        list = list.child(ui::section_header(label, count_of(header), cx));
                        after_header = true;
                    }
                }
            }
            if !filter.is_empty() && !entry.label.to_lowercase().contains(&filter) {
                continue;
            }
            let mut parent = entry.parent;
            let mut hidden = false;
            while let Some(ancestor) = parent {
                if self.collapsed.contains(&ancestor) {
                    hidden = true;
                    break;
                }
                parent = entries[ancestor].parent;
            }
            if hidden {
                continue;
            }
            if std::mem::take(&mut after_header) {
                list = list.child(div().h(px(2.)));
            }
            let has_children = entries.iter().any(|other| other.parent == Some(index));
            let expanded = !self.collapsed.contains(&index);
            let icon = entry.icon.unwrap_or(match entry.scope {
                ItemScope::SmartFolder { .. } if entry.parent.is_none() => "layout-grid",
                _ if has_children && expanded => "folder-open",
                _ => "folder",
            });
            list = list.child(
                ui::sidebar_row(index, icon, entry.label.clone())
                    .depth(entry.depth)
                    .count(entry.count)
                    .active(active == index)
                    .disclosure(has_children.then_some(expanded))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.session
                            .update(cx, |session, cx| session.navigate(index, cx));
                    }))
                    .on_toggle(cx.listener(move |this, _, _, cx| {
                        if !this.collapsed.remove(&index) {
                            this.collapsed.insert(index);
                        }
                        cx.notify();
                    })),
            );
        }
        for (slot, (header, label)) in SECTIONS.into_iter().enumerate() {
            if !shown[slot] {
                list = list.child(ui::section_header(label, count_of(header), cx));
            }
        }
        let avatar = match cover {
            Some(path) => img(path)
                .image_cache(&self.thumbnails)
                .size(px(39.))
                .rounded_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => ui::icon("books", 28., theme.text()).into_any_element(),
        };
        div()
            .id("sidebar")
            .size_full()
            .bg(theme.rail())
            .border_r_1()
            .border_color(theme.border_secondary())
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(48.))
                    .mt(px(4.))
                    .mb(px(8.))
                    .pl(px(20.5))
                    .pr(px(14.))
                    .flex()
                    .items_center()
                    .text_size(px(theme::TEXT_LG))
                    .gap(px(6.))
                    .child(
                        div()
                            .size(px(39.))
                            .flex_none()
                            .rounded_full()
                            .overflow_hidden()
                            .bg(theme.surface_2())
                            .border_1()
                            .border_color(theme.border())
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(avatar),
                    )
                    .child(div().flex_1().child("Main"))
                    .child(ui::icon("selector", 14., theme.text_tertiary())),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(list)
                    .child(ui::scrollbar("sidebar-scrollbar", &self.scroll, window, cx)),
            )
            .child(
                div().h(px(38.)).px(px(10.)).pb(px(10.)).flex_none().child(
                    div()
                        .h(px(theme::CONTROL_MD))
                        .rounded(px(theme::RADIUS_MD))
                        .border_1()
                        .border_color(theme.border_secondary())
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .child(div().pl(px(8.)).flex_none().child(ui::icon(
                            "toolbar-filter",
                            14.,
                            theme.text_tertiary(),
                        )))
                        .child(div().flex_1().h_full().child(self.filter.clone())),
                ),
            )
    }
}
