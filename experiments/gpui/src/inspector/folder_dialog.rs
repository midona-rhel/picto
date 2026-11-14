//! The Folders dialog (`FolderPickerModal.tsx`) that F, ⌘⇧J and the grid
//! menu's Add to Folder open: a centred dialog with a search, a rail of
//! Recent, Members and All, and the folder tree with check boxes. Clicks
//! only mark folders to add the items to or take them out of; Apply does
//! it, Cancel and Escape forget it. The folders checked at first are those
//! of the item the inspector shows or last showed.
use super::folder_picker::{Folder, Row, folders, rows};
use super::membership;
use crate::grid::format;
use crate::session::Session;
use crate::theme;
use crate::ui::{
    self,
    action_button::Look,
    check::{self, Mark, Size as CheckSize},
    menu::Return,
    modal::{self, Size},
    selection_modal::{self as shell, Key, ROW},
    text_input::TextInput,
};
use gpui::*;
use picto_library::FolderId;
use std::collections::{BTreeSet, HashSet};

/// `.panelBody`'s height.
const BODY: f32 = 480.;
/// `.sidebar`'s width.
const RAIL: f32 = 180.;
/// `useRecentFolders(15)`
const RECENT_SHOWN: usize = 15;
/// A level of the tree indents a row this much.
const INDENT: f32 = 20.;

/// `SidebarMode`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Recent,
    Members,
    All,
}

pub enum Event {
    /// Apply: the folders to add the selected items to and to take them
    /// out of.
    Apply {
        add: Vec<FolderId>,
        remove: Vec<FolderId>,
    },
}

impl EventEmitter<Event> for FolderDialog {}

pub struct FolderDialog {
    session: Entity<Session>,
    search: Entity<TextInput>,
    host: modal::Host<()>,
    query: String,
    /// `memberOf`: the folders of the item the inspector shows.
    members: BTreeSet<FolderId>,
    add: Vec<FolderId>,
    remove: Vec<FolderId>,
    /// The folded branches. Every branch opens again whenever the folders
    /// change (`setExpanded` on each new tree), not on each opening.
    collapsed: HashSet<FolderId>,
    /// The folders the fold state belongs to.
    tree_of: Vec<Folder>,
    view: View,
    /// How many items are selected, for the footer.
    selected: usize,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl FolderDialog {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::list_search("Search folders...", cx));
        let subscriptions = vec![cx.observe(&search, |this, search, cx| {
            let query = search.read(cx).value();
            if query != this.query {
                this.query = query;
                cx.notify();
            }
        })];
        Self {
            session,
            search,
            host: modal::Host::default(),
            query: String::new(),
            members: BTreeSet::new(),
            add: Vec::new(),
            remove: Vec::new(),
            collapsed: HashSet::new(),
            tree_of: Vec::new(),
            view: View::All,
            selected: 0,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    /// Opens on the shown item's folders for `selected` items, the search
    /// empty and nothing marked.
    pub fn open(&mut self, members: BTreeSet<FolderId>, selected: usize, cx: &mut Context<Self>) {
        self.members = members;
        self.selected = selected;
        self.query.clear();
        self.search.update(cx, |search, cx| search.clear(cx));
        self.add.clear();
        self.remove.clear();
        self.view = View::All;
        self.scroll.set_offset(point(px(0.), px(0.)));
        let field = self.search.read(cx).focus_handle(cx);
        self.host.open((), Return::Opener, cx).focus_first(field);
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.host.dismiss(cx);
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.host.close(cx);
    }

    fn toggle(&mut self, folder: FolderId, cx: &mut Context<Self>) {
        membership::mark(&self.members, &mut self.add, &mut self.remove, folder);
        cx.notify();
    }

    /// `applyFolders`
    fn apply(&mut self, cx: &mut Context<Self>) {
        if self.add.is_empty() && self.remove.is_empty() {
            return;
        }
        cx.emit(Event::Apply {
            add: std::mem::take(&mut self.add),
            remove: std::mem::take(&mut self.remove),
        });
        self.close(cx);
    }

    /// The search field's keys: Enter is the dialog's Apply, while there is
    /// something to apply; the others are the browser's own.
    fn key(&mut self, key: Key, cx: &mut Context<Self>) {
        match key {
            Key::Confirm => self.apply(cx),
            Key::Dismiss => self.dismiss(cx),
            Key::Up | Key::Down | Key::Next => {}
        }
    }

    /// A click on the rail or the list leaves the search.
    fn leave_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.host.get_mut().map(|dialog| dialog.focus.clone()) {
            window.focus(&focus, cx);
        }
    }
}

// ── Drawing ─────────────────────────────────────────────────────

impl FolderDialog {
    fn rail(&self, folders: &[Folder], recent: usize, cx: &mut Context<Self>) -> Div {
        let item = |id: &'static str,
                    label: &'static str,
                    count: usize,
                    view: View,
                    cx: &mut Context<Self>| {
            shell::rail_item(
                id,
                label,
                count.to_string(),
                self.view == view,
                None,
                None,
                cx,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.leave_search(window, cx);
                this.view = view;
                cx.notify();
            }))
        };
        shell::rail(RAIL, cx)
            .child(item(
                "folder-dialog-recent",
                "Recent",
                recent,
                View::Recent,
                cx,
            ))
            .child(item(
                "folder-dialog-members",
                "Members",
                self.members.len(),
                View::Members,
                cx,
            ))
            .child(shell::rail_separator(cx))
            .child(item(
                "folder-dialog-all",
                "All",
                folders.len(),
                View::All,
                cx,
            ))
    }

    /// `FolderRow`: in the tree with its fold button (or the room for one)
    /// and indent; flat in the Recent and Members views.
    fn row(
        &self,
        index: usize,
        folder: &Folder,
        tree: Option<&Row>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let id = folder.id;
        let member = self.members.contains(&id);
        let removing = member && self.remove.contains(&id);
        let checked = (member && !removing) || self.add.contains(&id);
        let group = SharedString::from(format!("folder-dialog-row-{index}"));
        let mut row = shell::item_row(("folder-dialog-row", index), group.clone(), false, cx);
        if let Some(line) = tree {
            if line.depth > 0 {
                row = row.pl(px(8. + line.depth as f32 * INDENT));
            }
            let slot = div().size(px(ROW)).flex_none();
            row = row.child(if line.children {
                let expanded = line.expanded;
                let fold = SharedString::from(format!("folder-dialog-fold-{index}"));
                slot.id(("folder-dialog-fold", index))
                    .group(fold.clone())
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(theme::RADIUS_MD))
                    .hover(move |style| style.bg(theme.surface_active()))
                    .child(
                        ui::icon("selection-chevron", 12., theme.text_tertiary())
                            .group_hover(fold, move |style| style.text_color(theme.text()))
                            .with_transformation(Transformation::rotate(if expanded {
                                radians(std::f32::consts::FRAC_PI_2)
                            } else {
                                radians(0.)
                            })),
                    )
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.leave_search(window, cx);
                        if !this.collapsed.remove(&id) {
                            this.collapsed.insert(id);
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            } else {
                slot.into_any_element()
            });
        }
        let tint = folder.color.map_or(theme.text(), rgb);
        row.child(check::check_box(
            if removing {
                Mark::Removed
            } else if checked {
                Mark::Checked
            } else {
                Mark::Empty
            },
            CheckSize::Compact,
        ))
        // TODO: a folder with its own glyph shows that glyph, filled, as
        // `DynamicIcon` draws it.
        .child(
            div()
                .flex_none()
                .opacity(0.6)
                .group_hover(group.clone(), |style| style.opacity(1.))
                .child(ui::icon("selection-folder", 14., tint)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(folder.name.clone()),
        )
        .children(folder.count.map(|count| {
            shell::badge(group, false, cx)
                .ml_auto()
                .child(format::grouped(count))
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.leave_search(window, cx)),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(id, cx)))
    }

    fn content(
        &self,
        folders: &[Folder],
        recent: &[FolderId],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut list = div().w_full().flex().flex_col();
        let flat = match self.view {
            View::Recent => Some(
                recent
                    .iter()
                    .take(RECENT_SHOWN)
                    .filter_map(|id| folders.iter().find(|folder| folder.id == *id))
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            View::Members => Some(
                folders
                    .iter()
                    .filter(|folder| self.members.contains(&folder.id))
                    .cloned()
                    .collect(),
            ),
            View::All => None,
        };
        match flat {
            Some(flat) if flat.is_empty() => {
                list = list.child(shell::empty_state(
                    if self.view == View::Recent {
                        "No recently used folders"
                    } else {
                        "Not a member of any folders"
                    },
                    cx,
                ));
            }
            Some(flat) => {
                for (index, folder) in flat.iter().enumerate() {
                    list = list.child(self.row(index, folder, None, cx));
                }
            }
            None if folders.is_empty() => {
                list = list.child(shell::empty_state("No folders", cx));
            }
            None => {
                for (index, line) in rows(folders, &self.collapsed, &self.query)
                    .iter()
                    .enumerate()
                {
                    list = list.child(self.row(index, &line.folder, Some(line), cx));
                }
            }
        }
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                div()
                    .id("folder-dialog-list")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .py(px(4.))
                    .pr(px(ui::scrollbar::GUTTER))
                    .child(list),
            )
            .child(ui::scrollbar(
                "folder-dialog-scrollbar",
                &self.scroll,
                window,
                cx,
            ))
    }
}

impl Render for FolderDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(layer) = self.host.layer(window, cx) else {
            return div().into_any_element();
        };
        let (presence, focus) = (layer.presence, layer.focus.clone());
        let (folders, recent) = {
            let session = self.session.read(cx);
            (folders(&session.nav), session.recent_folders.clone())
        };
        if folders != self.tree_of {
            self.collapsed.clear();
            self.tree_of = folders.clone();
        }
        let recent_count = recent
            .iter()
            .take(RECENT_SHOWN)
            .filter(|id| folders.iter().any(|folder| folder.id == **id))
            .count();
        let pending = self.add.len() + self.remove.len();
        let summary = shell::summary(self.add.len(), self.remove.len(), self.selected);
        let entity = cx.entity().downgrade();
        let keys = entity.clone();
        let search = shell::search_bar(
            &self.search,
            false,
            move |key, _, cx| {
                let _ = keys.update(cx, |this, cx| this.key(key, cx));
            },
            cx,
        );
        let body = div().h(px(BODY)).flex().flex_col().child(search).child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(self.rail(&folders, recent_count, cx))
                .child(self.content(&folders, &recent, window, cx)),
        );
        let mut buttons = div().flex().items_center().gap(px(6.)).child(
            ui::action_button("folder-dialog-cancel", Look::Standard)
                .label("Cancel")
                .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
        );
        if pending > 0 {
            buttons = buttons.child(
                ui::action_button("folder-dialog-apply", Look::Primary)
                    .label(format!("Apply ({pending})"))
                    .on_click(cx.listener(|this, _, _, cx| this.apply(cx))),
            );
        }
        ui::modal("folder-dialog", "Folders", presence)
            .size(Size::Medium)
            .flush()
            .focus(&focus)
            .on_close(move |_, cx| {
                let _ = entity.update(cx, |this, cx| this.dismiss(cx));
            })
            .child(body)
            .footer(shell::footer_left(summary, false, cx))
            .footer(buttons)
            .into_any_element()
    }
}
