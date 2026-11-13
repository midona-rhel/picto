//! The tag and folder pickers the filter bar opens (`TagSelectPanel.tsx`
//! and `FolderPickerPanel.tsx` with an `onApply…Filter`): a floating panel
//! with a search, the match rule, and a list in which a click includes a
//! tag or folder and a right click excludes it. Each change filters the
//! grid at once.
use super::filters::{Filters, TagChoice};
use crate::screens::tags::model as tags;
use crate::session::{Change, Session};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    field::Look,
    overlay_shell::{self, CheckState, check_box, kbd_hint, overlay_shell, shell_button},
    segmented::{self, segment, segmented},
    text_input::{self, TextInput},
};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::predicate::SetMatchMode;
use picto_library::query::ItemScope;
use std::{cell::Cell, collections::HashSet, ops::Range, rc::Rc};

actions!(filter_picker, [PickerUp, PickerDown, PickerSwitch]);

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", PickerUp, Some("FilterPicker")),
        KeyBinding::new("down", PickerDown, Some("FilterPicker")),
        KeyBinding::new("tab", PickerSwitch, Some("FilterPicker")),
    ]);
}

#[derive(Clone, Copy, PartialEq)]
pub enum PickerKind {
    Tags,
    Folders,
}

/// The picker closed; the window drops it.
pub struct PickerClosed;

/// The tag picker's rail (`SidebarMode`).
#[derive(Clone, PartialEq)]
enum TagView {
    All,
    Group(String),
    Selected,
    Starred,
}

/// The folder picker's tabs (`FolderView`).
#[derive(Clone, Copy, PartialEq)]
enum FolderView {
    All,
    Recent,
    Selected,
}

/// Folders chosen in a picker this session, latest first
/// (`useRecentFolders`).
// TODO: production keeps the recent folders in the renderer's
// localStorage; these last the session.
#[derive(Default)]
struct RecentFolders(Vec<u32>);
impl Global for RecentFolders {}

fn record_recent(folder: u32, cx: &mut App) {
    let recent = &mut cx.default_global::<RecentFolders>().0;
    recent.retain(|id| *id != folder);
    recent.insert(0, folder);
    recent.truncate(20);
}

/// A tag as the list shows it.
#[derive(Clone)]
struct TagRow {
    tag_id: u32,
    /// `namespace:subname`, or the subname in General.
    name: String,
    namespace: String,
    subname: String,
    count: u64,
}

/// A folder as the tree shows it.
#[derive(Clone)]
struct FolderRow {
    folder_id: u32,
    name: String,
    tint: Option<u32>,
    depth: usize,
    parent: Option<u32>,
    has_children: bool,
    last_child: bool,
    /// For each ancestor below the top, whether its line continues past
    /// this row.
    guides: Vec<bool>,
    expanded: bool,
}

/// A folder of the tree before it is laid out.
#[derive(Clone)]
struct FolderNode {
    id: u32,
    name: String,
    tint: Option<u32>,
    parent: Option<u32>,
}

/// `FolderTree`'s rows: each folder after its parent, in order, the open
/// ones' children below them. While searching, only folders whose name or a
/// descendant's contains the search show, all opened (`expandedForSearch`).
fn folder_tree(nodes: &[FolderNode], query: &str, collapsed: &HashSet<u32>) -> Vec<FolderRow> {
    let mut children: std::collections::HashMap<Option<u32>, Vec<usize>> = Default::default();
    for (index, node) in nodes.iter().enumerate() {
        // A parent that is not listed puts the folder at the top.
        let parent = node
            .parent
            .filter(|parent| nodes.iter().any(|other| other.id == *parent));
        children.entry(parent).or_default().push(index);
    }
    // Whether each folder or a descendant matches, children before parents.
    let mut matching = vec![query.is_empty(); nodes.len()];
    if !query.is_empty() {
        fn mark(
            index: usize,
            nodes: &[FolderNode],
            children: &std::collections::HashMap<Option<u32>, Vec<usize>>,
            query: &str,
            matching: &mut [bool],
        ) -> bool {
            let mut any = nodes[index].name.to_lowercase().contains(query);
            for child in children.get(&Some(nodes[index].id)).into_iter().flatten() {
                any |= mark(*child, nodes, children, query, matching);
            }
            matching[index] = any;
            any
        }
        for root in children.get(&None).cloned().unwrap_or_default() {
            mark(root, nodes, &children, query, &mut matching);
        }
    }
    // Depth-first, in order: each parent's children after it.
    #[allow(clippy::too_many_arguments)]
    fn walk(
        parent: Option<u32>,
        depth: usize,
        guides: &[bool],
        nodes: &[FolderNode],
        children: &std::collections::HashMap<Option<u32>, Vec<usize>>,
        matching: &[bool],
        open: &dyn Fn(u32) -> bool,
        rows: &mut Vec<FolderRow>,
    ) {
        let list = children.get(&parent).cloned().unwrap_or_default();
        let count = list.len();
        for (position, index) in list.into_iter().enumerate() {
            if !matching[index] {
                continue;
            }
            let node = &nodes[index];
            let last = position + 1 == count;
            let has_children = children.contains_key(&Some(node.id));
            let expanded = has_children && open(node.id);
            rows.push(FolderRow {
                folder_id: node.id,
                name: node.name.clone(),
                tint: node.tint,
                depth,
                parent: None,
                has_children,
                last_child: last,
                guides: guides.to_vec(),
                expanded,
            });
            if expanded {
                let mut next = guides.to_vec();
                if depth > 0 {
                    next.push(!last);
                }
                walk(
                    Some(node.id),
                    depth + 1,
                    &next,
                    nodes,
                    children,
                    matching,
                    open,
                    rows,
                );
            }
        }
    }
    let searching = !query.is_empty();
    let open = |id: u32| searching || !collapsed.contains(&id);
    let mut rows = Vec::new();
    walk(None, 0, &[], nodes, &children, &matching, &open, &mut rows);
    rows
}

/// `.tagRow`, `.row`, `.sidebarItem`.
const TAG_ROW: f32 = 25.;
const FOLDER_ROW: f32 = 26.;
const SIDEBAR: f32 = 200.;
const CONTENT: f32 = 340.;
const HEIGHT: f32 = 480.;

pub struct FilterPicker {
    session: Entity<Session>,
    kind: PickerKind,
    search: Entity<TextInput>,
    /// Where it opens below, and where it is once placed or dragged.
    anchor: Point<Pixels>,
    origin: Option<Point<f32>>,
    /// A press on the header or footer: the panel's place and the pointer's
    /// then, and whether the drag has begun.
    drag: Rc<Cell<Option<(Point<f32>, Point<f32>)>>>,
    sidebar: bool,
    tag_view: TagView,
    folder_view: FolderView,
    collapsed: HashSet<u32>,
    last_clicked: Option<u32>,
    focused: Option<usize>,
    /// The tags chosen when it opened, which "Selected" lists.
    opening: Vec<String>,
    tag_rows: Rc<Vec<TagRow>>,
    folder_rows: Rc<Vec<FolderRow>>,
    list: UniformListScrollHandle,
    focus: FocusHandle,
    closed: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PickerClosed> for FilterPicker {}

impl Focusable for FilterPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.read(cx).focus_handle(cx)
    }
}

impl FilterPicker {
    pub fn new(
        session: Entity<Session>,
        kind: PickerKind,
        anchor: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(TextInput::menu_search);
        let opening = session
            .read(cx)
            .filters
            .include_tags
            .iter()
            .map(|tag| tag.name.clone())
            .collect();
        let subscriptions = vec![
            cx.observe(&search, |this, _, cx| {
                this.focused = None;
                this.list.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }),
            // The filters it shows changed.
            cx.subscribe(&session, |_, _, change: &Change, cx| {
                if matches!(change, Change::Results | Change::Navigation) {
                    cx.notify();
                }
            }),
        ];
        Self {
            session,
            kind,
            search,
            anchor,
            origin: None,
            drag: Rc::new(Cell::new(None)),
            sidebar: true,
            tag_view: TagView::All,
            folder_view: FolderView::All,
            collapsed: HashSet::new(),
            last_clicked: None,
            focused: None,
            opening,
            tag_rows: Rc::new(Vec::new()),
            folder_rows: Rc::new(Vec::new()),
            list: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            closed: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if !std::mem::replace(&mut self.closed, true) {
            cx.emit(PickerClosed);
        }
    }

    fn width(&self) -> f32 {
        match self.kind {
            PickerKind::Folders => 360.,
            PickerKind::Tags if self.sidebar => SIDEBAR + CONTENT,
            PickerKind::Tags => CONTENT,
        }
    }

    fn query(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_lowercase()
    }

    fn set_filters(&self, cx: &mut Context<Self>, change: impl FnOnce(&mut Filters)) {
        let mut filters = self.session.read(cx).filters.clone();
        change(&mut filters);
        self.session
            .update(cx, |session, cx| session.set_filters(filters, cx));
    }

    // ── Tags ────────────────────────────────────────────────────

    /// The tags of the current view and search: in the library's order
    /// (subname, group, id), ranked by the search, each visible name once.
    fn compute_tag_rows(&self, cx: &App) -> Vec<TagRow> {
        let session = self.session.read(cx);
        let preferences = tags::Preferences::from_settings(crate::settings::current(cx));
        let filters = &session.filters;
        let mut all = session
            .tags
            .values()
            .map(|tag| TagRow {
                tag_id: tag.tag_id.0,
                name: tags::tag_name(&tag.namespace, &tag.subname),
                namespace: tag.namespace.clone(),
                subname: tag.subname.clone(),
                count: tag.active_count,
            })
            .collect::<Vec<_>>();
        all.sort_by(|a, b| {
            a.subname
                .to_lowercase()
                .cmp(&b.subname.to_lowercase())
                .then_with(|| a.namespace.to_lowercase().cmp(&b.namespace.to_lowercase()))
                .then(a.tag_id.cmp(&b.tag_id))
        });
        let chosen = filters
            .include_tags
            .iter()
            .chain(&filters.exclude_tags)
            .map(|tag| tag.name.clone())
            .chain(self.opening.iter().cloned())
            .collect::<HashSet<_>>();
        let in_view = all
            .into_iter()
            .filter(|tag| match &self.tag_view {
                TagView::All => true,
                TagView::Group(group) => tag.namespace == *group,
                TagView::Selected => chosen.contains(&tag.name),
                TagView::Starred => preferences.is_starred(&tag.name),
            })
            .collect::<Vec<_>>();
        let query = self.query(cx);
        let by_name = query.contains(':');
        let ranked = tags::rank_matches(in_view, &query, |tag| {
            if by_name { &tag.name } else { &tag.subname }
        });
        let mut seen = HashSet::new();
        ranked
            .into_iter()
            .filter(|tag| {
                let shown = if tag.subname.is_empty() {
                    tag.name.to_lowercase()
                } else {
                    tag.subname.to_lowercase()
                };
                seen.insert(shown)
            })
            .collect()
    }

    /// The groups the rail lists, in their preset order.
    fn groups(&self, cx: &App) -> Vec<(String, usize)> {
        let session = self.session.read(cx);
        let mut groups: Vec<(String, usize)> = Vec::new();
        for tag in session.tags.values() {
            match groups.iter_mut().find(|(name, _)| *name == tag.namespace) {
                Some((_, count)) => *count += 1,
                None => groups.push((tag.namespace.clone(), 1)),
            }
        }
        groups.retain(|(name, _)| !name.is_empty() && name != "general");
        groups.sort_by(|a, b| {
            tags::group_order(&a.0)
                .cmp(&tags::group_order(&b.0))
                .then_with(|| a.0.cmp(&b.0))
        });
        groups
    }

    fn toggle_tag(&mut self, row: &TagRow, cx: &mut Context<Self>) {
        let choice = TagChoice {
            tag_id: row.tag_id,
            name: row.name.clone(),
        };
        self.set_filters(cx, |filters| {
            let included = filters
                .include_tags
                .iter()
                .any(|tag| tag.tag_id == row.tag_id);
            filters.include_tags.retain(|tag| tag.tag_id != row.tag_id);
            filters.exclude_tags.retain(|tag| tag.tag_id != row.tag_id);
            if !included {
                filters.include_tags.push(choice);
            }
        });
    }

    fn exclude_tag(&mut self, row: &TagRow, cx: &mut Context<Self>) {
        let choice = TagChoice {
            tag_id: row.tag_id,
            name: row.name.clone(),
        };
        self.set_filters(cx, |filters| {
            let excluded = filters
                .exclude_tags
                .iter()
                .any(|tag| tag.tag_id == row.tag_id);
            filters.include_tags.retain(|tag| tag.tag_id != row.tag_id);
            filters.exclude_tags.retain(|tag| tag.tag_id != row.tag_id);
            if !excluded {
                filters.exclude_tags.push(choice);
            }
        });
    }

    // ── Folders ─────────────────────────────────────────────────

    /// The folder tree of the current view: the sidebar's folders in their
    /// order, open unless folded, and while searching only the matches and
    /// their ancestors, opened. Recent and Selected list folders flat, each
    /// still naming its parent.
    fn compute_folder_rows(&self, cx: &App) -> Vec<FolderRow> {
        let session = self.session.read(cx);
        let folder_of = |index: usize| match session.nav.get(index) {
            Some(entry) if entry.screen.is_none() => match entry.scope {
                ItemScope::FolderTree { folder_id } | ItemScope::Folder { folder_id } => {
                    Some(folder_id.0)
                }
                _ => None,
            },
            _ => None,
        };
        let all = session
            .nav
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                Some(FolderNode {
                    id: folder_of(index)?,
                    name: entry.label.clone(),
                    tint: entry.tint,
                    parent: entry.parent.and_then(folder_of),
                })
            })
            .collect::<Vec<_>>();
        let flat = |ids: Vec<u32>| {
            ids.into_iter()
                .filter_map(|id| all.iter().find(|node| node.id == id))
                .map(|node| FolderNode {
                    parent: None,
                    ..node.clone()
                })
                .collect::<Vec<_>>()
        };
        let nodes = match self.folder_view {
            FolderView::All => all.clone(),
            FolderView::Recent => flat(
                cx.try_global::<RecentFolders>()
                    .map(|recent| recent.0.clone())
                    .unwrap_or_default(),
            ),
            FolderView::Selected => flat(session.filters.include_folders.clone()),
        };
        let mut rows = folder_tree(&nodes, &self.query(cx), &self.collapsed);
        for row in &mut rows {
            row.parent = all
                .iter()
                .find(|node| node.id == row.folder_id)
                .and_then(|node| node.parent);
        }
        rows
    }

    fn folder_name(&self, id: u32, cx: &App) -> Option<String> {
        let session = self.session.read(cx);
        session.nav.iter().find_map(|entry| match entry.scope {
            ItemScope::FolderTree { folder_id } | ItemScope::Folder { folder_id }
                if folder_id.0 == id && entry.screen.is_none() =>
            {
                Some(entry.label.clone())
            }
            _ => None,
        })
    }

    /// A click on a folder: Shift takes the range from the last one clicked
    /// (keeping the rest with Cmd), otherwise it toggles; either way it is
    /// no longer excluded.
    fn toggle_folder(&mut self, id: u32, modifiers: Modifiers, cx: &mut Context<Self>) {
        let order = self
            .folder_rows
            .iter()
            .map(|row| row.folder_id)
            .collect::<Vec<_>>();
        let last = self.last_clicked;
        self.set_filters(cx, |filters| {
            filters.exclude_folders.retain(|folder| *folder != id);
            let range = last.filter(|_| modifiers.shift).and_then(|last| {
                Some((
                    order.iter().position(|f| *f == last)?,
                    order.iter().position(|f| *f == id)?,
                ))
            });
            match range {
                Some((from, to)) => {
                    if !(modifiers.platform || modifiers.control) {
                        filters.include_folders.clear();
                    }
                    for folder in &order[from.min(to)..=from.max(to)] {
                        if !filters.include_folders.contains(folder) {
                            filters.include_folders.push(*folder);
                        }
                    }
                }
                None => match filters.include_folders.iter().position(|f| *f == id) {
                    Some(at) => {
                        filters.include_folders.remove(at);
                    }
                    None => filters.include_folders.push(id),
                },
            }
        });
        self.last_clicked = Some(id);
        record_recent(id, cx);
    }

    fn exclude_folder(&mut self, id: u32, cx: &mut Context<Self>) {
        self.set_filters(cx, |filters| {
            filters.include_folders.retain(|folder| *folder != id);
            match filters.exclude_folders.iter().position(|f| *f == id) {
                Some(at) => {
                    filters.exclude_folders.remove(at);
                }
                None => filters.exclude_folders.push(id),
            }
        });
        record_recent(id, cx);
    }

    // ── Keyboard ────────────────────────────────────────────────

    fn count(&self) -> usize {
        match self.kind {
            PickerKind::Tags => self.tag_rows.len(),
            PickerKind::Folders => self.folder_rows.len(),
        }
    }

    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.count();
        if count == 0 {
            return;
        }
        let next = match (self.focused, forward) {
            (None, _) => 0,
            (Some(at), true) => (at + 1).min(count - 1),
            (Some(at), false) => at.saturating_sub(1),
        };
        self.focused = Some(next);
        self.list.scroll_to_item(next, ScrollStrategy::Center);
        cx.notify();
    }

    /// Enter: the focused row, or the first.
    fn confirm(&mut self, cx: &mut Context<Self>) {
        let at = self.focused.unwrap_or(0);
        match self.kind {
            PickerKind::Tags => {
                if let Some(row) = self.tag_rows.get(at).cloned() {
                    self.toggle_tag(&row, cx);
                }
            }
            PickerKind::Folders => {
                if let Some(id) = self.folder_rows.get(at).map(|row| row.folder_id) {
                    self.toggle_folder(id, Modifiers::default(), cx);
                }
            }
        }
    }

    /// Tab: the next view.
    fn switch(&mut self, cx: &mut Context<Self>) {
        self.focused = None;
        match self.kind {
            PickerKind::Tags => {
                if !self.sidebar {
                    return;
                }
                let groups = if crate::settings::current(cx).show_tag_groups {
                    self.groups(cx)
                } else {
                    Vec::new()
                };
                self.tag_view = match &self.tag_view {
                    TagView::All => groups
                        .first()
                        .map(|(name, _)| TagView::Group(name.clone()))
                        .unwrap_or(TagView::Selected),
                    TagView::Group(current) => {
                        let at = groups.iter().position(|(name, _)| name == current);
                        match at.and_then(|at| groups.get(at + 1)) {
                            Some((name, _)) => TagView::Group(name.clone()),
                            None => TagView::Selected,
                        }
                    }
                    TagView::Selected => TagView::Starred,
                    TagView::Starred => TagView::All,
                };
            }
            PickerKind::Folders => {
                self.folder_view = match self.folder_view {
                    FolderView::All => FolderView::Recent,
                    FolderView::Recent => FolderView::Selected,
                    FolderView::Selected => FolderView::All,
                };
            }
        }
        cx.notify();
    }

    fn set_mode(&mut self, mode: SetMatchMode, cx: &mut Context<Self>) {
        let kind = self.kind;
        self.set_filters(cx, |filters| match kind {
            PickerKind::Tags => filters.tag_mode = mode,
            PickerKind::Folders => filters.folder_mode = mode,
        });
    }

    // ── Drawing ─────────────────────────────────────────────────

    fn highlighted(&self, text: &str, query: &str, cx: &App) -> StyledText {
        let theme = theme::theme(cx);
        let styled = StyledText::new(text.to_owned());
        if query.is_empty() {
            return styled;
        }
        let Some(at) = text.to_lowercase().find(query) else {
            return styled;
        };
        // Lower-casing can change byte lengths; keep to char boundaries.
        let end = (at + query.len()).min(text.len());
        if !text.is_char_boundary(at) || !text.is_char_boundary(end) {
            return styled;
        }
        styled.with_highlights(vec![(
            at..end,
            HighlightStyle {
                color: Some(theme.text().into()),
                font_weight: Some(FontWeight::BOLD),
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(theme.contrast(20.).into()),
                    wavy: false,
                }),
                ..Default::default()
            },
        )])
    }

    fn tag_row(&self, index: usize, row: &TagRow, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let settings = crate::settings::current(cx);
        let preferences = tags::Preferences::from_settings(settings);
        let show_prefix = settings.show_tag_prefixes;
        let filters = &self.session.read(cx).filters;
        let included = filters
            .include_tags
            .iter()
            .any(|tag| tag.tag_id == row.tag_id);
        let excluded = filters
            .exclude_tags
            .iter()
            .any(|tag| tag.tag_id == row.tag_id);
        let focused = self.focused == Some(index);
        let color = rgb(tags::group_color(&row.namespace, &preferences));
        let starred = preferences.is_starred(&row.name);
        // `color-mix(in srgb, var(--tag-color) 12%, var(--color-text-primary))`
        let name_color = theme::blend(theme.text(), color, 0.12);
        let dim = if excluded { 0.45 } else { 1. };
        let shown = if show_prefix {
            row.name.clone()
        } else {
            row.name
                .split_once(':')
                .map_or(row.name.clone(), |(_, rest)| rest.to_owned())
        };
        let query = self.query(cx);
        let (click, context) = (row.clone(), row.clone());
        let mut element = div()
            .id(("filter-tag", row.tag_id as usize))
            .h(px(TAG_ROW))
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(theme::RADIUS_MD))
            .whitespace_nowrap()
            .cursor_pointer()
            .text(theme::TEXT_MD)
            .line_height(px(TAG_ROW));
        element = if included {
            element.bg(theme.primary_at(20.))
        } else if focused {
            element.bg(theme.surface_active())
        } else {
            element.hover(move |style| style.bg(theme.surface_hover()))
        };
        if focused {
            element = element.border_1().border_color(if included {
                theme.primary_at(50.)
            } else {
                theme.contrast(25.)
            });
        }
        element
            .child(check_box(
                if excluded {
                    CheckState::Excluded
                } else if included {
                    CheckState::Included
                } else {
                    CheckState::Empty
                },
                cx,
            ))
            .child(if starred {
                ui::icon("tag-star", 10., theme.text())
                    .flex_none()
                    .into_any_element()
            } else {
                ui::icon(
                    if included {
                        "tag-bookmark-filled"
                    } else {
                        "tag-bookmark"
                    },
                    14.,
                    color,
                )
                .flex_none()
                .opacity(dim)
                .into_any_element()
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(name_color)
                    .opacity(dim)
                    .child(self.highlighted(&shown, &query, cx)),
            )
            .child(
                div()
                    .flex_none()
                    .ml_auto()
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_tertiary())
                    .opacity(0.7 * dim)
                    .child(format!("({})", row.count)),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_tag(&click, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.exclude_tag(&context, cx)
                }),
            )
            .into_any_element()
    }

    fn folder_row(&self, index: usize, row: &FolderRow, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let filters = &self.session.read(cx).filters;
        let selected = filters.include_folders.contains(&row.folder_id);
        let excluded = filters.exclude_folders.contains(&row.folder_id);
        let focused = self.focused == Some(index);
        let indent = 4. + row.depth as f32 * 24.;
        let query = self.query(cx);
        let parent = row.parent.and_then(|parent| self.folder_name(parent, cx));
        let id = row.folder_id;
        // `.row::before`: the row's fill starts at its indent.
        let mut fill = div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(indent))
            .right_0()
            .rounded(px(theme::RADIUS_MD));
        fill = if selected {
            fill.bg(theme.surface_active())
                .border_1()
                .border_color(theme.contrast(25.))
        } else if focused {
            fill.bg(theme.surface_hover())
                .border_1()
                .border_color(theme.border_focus())
        } else {
            fill.group_hover("folder-row", move |style| style.bg(theme.surface_hover()))
        };
        let mut element = div()
            .id(("filter-folder", id as usize))
            .group("folder-row")
            .relative()
            .h(px(FOLDER_ROW))
            .pl(px(indent))
            .pr(px(4.))
            .flex()
            .items_center()
            .gap(px(4.))
            .whitespace_nowrap()
            .cursor_pointer()
            .text(theme::TEXT_MD)
            .line_height(px(FOLDER_ROW))
            .opacity(if excluded { 0.58 } else { 1. })
            .child(fill);
        // The tree's guides and the branch into this row.
        if row.depth > 1 {
            for (at, continues) in row.guides.iter().enumerate() {
                if *continues {
                    element = element.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(14. + at as f32 * 24.))
                            .w(px(1.))
                            .bg(theme.tree_guide()),
                    );
                }
            }
        }
        if row.depth > 0 {
            element = element.child(
                ui::icon(
                    if row.last_child {
                        "tree-branch-last"
                    } else {
                        "tree-branch-middle"
                    },
                    16.,
                    theme.tree_guide(),
                )
                .absolute()
                .top_0()
                .left(px(14. + (row.depth - 1) as f32 * 24.))
                .w(px(16.))
                .h(px(26.)),
            );
        }
        let chevron = if row.has_children {
            let expanded = row.expanded;
            div()
                .id(("folder-expand", id as usize))
                .relative()
                .size(px(26.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .hover(move |style| style.bg(theme.surface_active()))
                .child({
                    let glyph = ui::icon("tree-chevron", 12., theme.text_secondary());
                    if expanded {
                        glyph.with_transformation(Transformation::rotate(radians(
                            std::f32::consts::FRAC_PI_2,
                        )))
                    } else {
                        glyph
                    }
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if !this.collapsed.remove(&id) {
                        this.collapsed.insert(id);
                    }
                    cx.notify();
                }))
                .into_any_element()
        } else {
            div().size(px(26.)).flex_none().into_any_element()
        };
        element
            .child(
                div()
                    .relative()
                    .size(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(check_box(
                        if excluded {
                            CheckState::Excluded
                        } else if selected {
                            CheckState::Included
                        } else {
                            CheckState::Empty
                        },
                        cx,
                    )),
            )
            .child(
                ui::icon("tree-folder", 20., row.tint.map_or(theme.text(), rgb))
                    .relative()
                    .flex_none(),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .opacity(if excluded { 0.7 } else { 1. })
                    .child(self.highlighted(&row.name, &query, cx)),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w(px(26.))
                    .max_w(relative(0.4))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .children(parent.map(|name| {
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text(theme::TEXT_CAPTION)
                            .text_color(theme.text_secondary())
                            .group_hover("folder-row", move |style| style.text_color(theme.text()))
                            .child(name)
                    }))
                    .child(chevron),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                this.toggle_folder(id, event.modifiers(), cx)
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.exclude_folder(id, cx)
                }),
            )
            .into_any_element()
    }

    fn tag_rail(&self, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let (preferences, show_groups) = {
            let settings = crate::settings::current(cx);
            (
                tags::Preferences::from_settings(settings),
                settings.show_tag_groups,
            )
        };
        let total = self.session.read(cx).tags.len();
        let chosen = {
            let filters = &self.session.read(cx).filters;
            let mut names = self.opening.clone();
            for tag in filters.include_tags.iter().chain(&filters.exclude_tags) {
                if !names.contains(&tag.name) {
                    names.push(tag.name.clone());
                }
            }
            names.len()
        };
        let item = |id: SharedString,
                    active: bool,
                    glyph: AnyElement,
                    name: String,
                    count: usize,
                    view: TagView,
                    cx: &mut Context<Self>| {
            div()
                .id(id)
                .group("rail-item")
                .h(px(24.))
                .flex_none()
                .pr(px(8.))
                .flex()
                .items_center()
                .rounded(px(theme::RADIUS_MD))
                .text(theme::TEXT_MD)
                .line_height(px(24.))
                .whitespace_nowrap()
                .cursor_pointer()
                .when(active, |row| row.bg(theme.surface_active()))
                .when(!active, |row| {
                    row.hover(move |style| style.bg(theme.surface_hover()))
                })
                .child(glyph)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(18.))
                        .line_height(px(18.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(name),
                )
                .child(
                    div()
                        .flex_none()
                        .ml_auto()
                        .text(theme::TEXT_CAPTION)
                        .text_color(theme.text_tertiary())
                        .opacity(if active { 1. } else { 0.7 })
                        .group_hover("rail-item", |style| style.opacity(1.))
                        .child(count.to_string()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tag_view = view.clone();
                    this.focused = None;
                    cx.notify();
                }))
        };
        let bookmark = |active: bool| {
            ui::icon(
                if active {
                    "tag-group-bookmark-filled"
                } else {
                    "tag-group-bookmark"
                },
                12.,
                theme.text_secondary(),
            )
            .mx(px(5.))
            .flex_none()
            .into_any_element()
        };
        let mut rail = div()
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .w(px(SIDEBAR))
            .p(px(8.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .overflow_hidden()
            .child(item(
                "rail-all".into(),
                self.tag_view == TagView::All,
                bookmark(self.tag_view == TagView::All),
                "All".into(),
                total,
                TagView::All,
                cx,
            ));
        if show_groups {
            for (name, count) in self.groups(cx) {
                let active = self.tag_view == TagView::Group(name.clone());
                let dot = div()
                    .size(px(10.))
                    .mx(px(6.))
                    .flex_none()
                    .rounded_full()
                    .bg(rgb(tags::group_color(&name, &preferences)))
                    .opacity(if active { 1. } else { 0.85 })
                    .into_any_element();
                rail = rail.child(item(
                    format!("rail-group-{name}").into(),
                    active,
                    dot,
                    name.clone(),
                    count,
                    TagView::Group(name),
                    cx,
                ));
            }
        }
        rail.child(
            div()
                .h(px(1.))
                .my(px(4.))
                .flex_none()
                .bg(theme.border_secondary()),
        )
        .child(item(
            "rail-selected".into(),
            self.tag_view == TagView::Selected,
            bookmark(self.tag_view == TagView::Selected),
            "Selected".into(),
            chosen,
            TagView::Selected,
            cx,
        ))
        .child(item(
            "rail-starred".into(),
            self.tag_view == TagView::Starred,
            ui::icon("tag-star", 10., theme.text())
                .mx(px(6.))
                .flex_none()
                .into_any_element(),
            "Starred".into(),
            preferences.starred.len(),
            TagView::Starred,
            cx,
        ))
    }

    fn list(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = theme::theme(cx);
        let count = self.count();
        if count == 0 {
            let message = match (self.kind, &self.tag_view) {
                (PickerKind::Folders, _) => "No folders",
                (PickerKind::Tags, TagView::Selected) => "No tags on this entity",
                (PickerKind::Tags, _) => "No tags found",
            };
            return div()
                .h(px(60.))
                .flex()
                .items_center()
                .justify_center()
                .text(theme::TEXT_SM)
                .text_color(theme.text_tertiary())
                .child(message)
                .into_any_element();
        }
        let kind = self.kind;
        uniform_list(
            "filter-picker-rows",
            count,
            cx.processor(move |this, range: Range<usize>, _, cx| {
                range
                    .filter_map(|index| match kind {
                        PickerKind::Tags => {
                            let row = this.tag_rows.get(index)?.clone();
                            Some(this.tag_row(index, &row, cx))
                        }
                        PickerKind::Folders => {
                            let row = this.folder_rows.get(index)?.clone();
                            Some(this.folder_row(index, &row, cx))
                        }
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.list)
        .p(px(4.))
        .size_full()
        .into_any_element()
    }

    /// While a press on the header or footer is held, the panel follows
    /// the pointer.
    fn drag_listener(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let drag = self.drag.clone();
        let picker = cx.entity().downgrade();
        let (width, height) = (self.width(), HEIGHT);
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moving = drag.clone();
                let target = picker.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || !event.dragging() {
                        return;
                    }
                    let Some((origin, start)) = moving.get() else {
                        return;
                    };
                    let viewport = window.viewport_size();
                    let pointer = point(f32::from(event.position.x), f32::from(event.position.y));
                    if let Some(next) = overlay_shell::dragged(
                        origin,
                        start,
                        pointer,
                        width,
                        height,
                        size(f32::from(viewport.width), f32::from(viewport.height)),
                    ) {
                        let _ = target.update(cx, |this, cx| {
                            this.origin = Some(next);
                            cx.notify();
                        });
                    }
                });
                let up = drag.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
                    if phase == DispatchPhase::Capture {
                        up.set(None);
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }
}

impl Render for FilterPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        match self.kind {
            PickerKind::Tags => self.tag_rows = Rc::new(self.compute_tag_rows(cx)),
            PickerKind::Folders => self.folder_rows = Rc::new(self.compute_folder_rows(cx)),
        }
        if self.focused.is_some_and(|at| at >= self.count()) {
            self.focused = None;
        }
        let (width, height) = (self.width(), HEIGHT);
        let viewport = window.viewport_size();
        let viewport = size(f32::from(viewport.width), f32::from(viewport.height));
        let origin = *self.origin.get_or_insert_with(|| {
            overlay_shell::place_below(
                point(f32::from(self.anchor.x), f32::from(self.anchor.y)),
                width,
                height,
                viewport,
            )
        });
        let mode = {
            let filters = &self.session.read(cx).filters;
            match self.kind {
                PickerKind::Tags => filters.tag_mode,
                PickerKind::Folders => filters.folder_mode,
            }
        };
        let modes = [SetMatchMode::Any, SetMatchMode::All, SetMatchMode::Exact];
        let picker = cx.entity().downgrade();
        let logic = segmented(
            "picker-logic",
            segmented::Look::Logic,
            vec![
                segment("logic-any", "Match any"),
                segment("logic-all", "Match all"),
                segment("logic-exact", "Match exactly"),
            ],
            modes.iter().position(|value| *value == mode),
        )
        .on_select(move |index, _, cx| {
            let _ = picker.update(cx, |this, cx| this.set_mode(modes[index], cx));
        });
        let search = div().flex_1().min_w_0().child(
            ui::field("picker-search", &self.search, Look::Picker).icon(
                "shell-search",
                14.,
                4.,
                theme.text_secondary(),
            ),
        );
        let drag = self.drag.clone();
        let mut shell = overlay_shell("filter-picker", origin, width, height)
            .on_close({
                let picker = cx.entity().downgrade();
                move |_, cx| {
                    let _ = picker.update(cx, |this, cx| this.close(cx));
                }
            })
            .on_drag_start(move |position, _, _| {
                drag.set(Some((
                    origin,
                    point(f32::from(position.x), f32::from(position.y)),
                )));
            })
            .header(search);
        match self.kind {
            PickerKind::Tags => {
                let sidebar = self.sidebar;
                shell = shell.header(logic).header(
                    shell_button(
                        "picker-sidebar",
                        "shell-sidebar",
                        ui::tip(if sidebar {
                            "Hide sidebar"
                        } else {
                            "Show sidebar"
                        }),
                        window,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sidebar = !this.sidebar;
                        if !this.sidebar {
                            this.tag_view = TagView::All;
                        }
                        cx.notify();
                    })),
                );
                let content = div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .w(px(CONTENT))
                    .flex()
                    .flex_col()
                    .border_l_1()
                    .border_color(if sidebar {
                        theme.border_secondary()
                    } else {
                        gpui::transparent_black().into()
                    })
                    .child(self.list(cx));
                shell = shell
                    .body(
                        div()
                            .relative()
                            .size_full()
                            .when(sidebar, |body| body.child(self.tag_rail(cx)))
                            .child(content),
                    )
                    .footer(kbd_hint("Select", &["L-Click"], cx))
                    .footer(kbd_hint("Exclude", &["R-Click"], cx))
                    .footer(kbd_hint("Close", &["Esc"], cx));
            }
            PickerKind::Folders => {
                let views = [FolderView::All, FolderView::Recent, FolderView::Selected];
                let picker = cx.entity().downgrade();
                let tabs = segmented(
                    "picker-views",
                    segmented::Look::View,
                    vec![
                        segment("shell-folders", "All folders"),
                        segment("shell-history", "Recent folders"),
                        segment("shell-checks", "Selected folders"),
                    ],
                    views.iter().position(|view| *view == self.folder_view),
                )
                .on_select(move |index, _, cx| {
                    let _ = picker.update(cx, |this, cx| {
                        this.folder_view = views[index];
                        this.focused = None;
                        cx.notify();
                    });
                });
                shell = shell
                    .header(tabs)
                    .header(logic)
                    .body(self.list(cx))
                    .footer(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .min_w_0()
                            .child(kbd_hint("Select", &["L-Click"], cx))
                            .child(kbd_hint("Exclude", &["R-Click"], cx)),
                    )
                    .footer(
                        div()
                            .ml_auto()
                            .flex_none()
                            .child(kbd_hint("Close", &["Esc"], cx)),
                    );
            }
        }
        div()
            .id("filter-picker-layer")
            .absolute()
            .inset_0()
            .track_focus(&self.focus)
            .key_context("FilterPicker")
            .on_action(cx.listener(|this, _: &PickerUp, _, cx| this.step(false, cx)))
            .on_action(cx.listener(|this, _: &PickerDown, _, cx| this.step(true, cx)))
            .on_action(cx.listener(|this, _: &PickerSwitch, _, cx| this.switch(cx)))
            .capture_action(cx.listener(|this, _: &text_input::Commit, _, cx| {
                this.confirm(cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &text_input::Cancel, _, cx| {
                this.close(cx);
                cx.stop_propagation();
            }))
            .child(shell)
            .child(self.drag_listener(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{FolderNode, folder_tree};
    use std::collections::HashSet;

    fn node(id: u32, name: &str, parent: Option<u32>) -> FolderNode {
        FolderNode {
            id,
            name: name.into(),
            tint: None,
            parent,
        }
    }

    #[test]
    fn the_folder_tree_nests_folds_and_searches() {
        let nodes = [
            node(1, "Art", None),
            node(2, "Sketches", Some(1)),
            node(3, "Ink", Some(2)),
            node(4, "Paint", Some(1)),
            node(5, "Photos", None),
        ];
        let rows = folder_tree(&nodes, "", &HashSet::new());
        let order = rows.iter().map(|row| row.folder_id).collect::<Vec<_>>();
        assert_eq!(order, [1, 2, 3, 4, 5]);
        assert_eq!(rows[2].depth, 2);
        // Ink's guide: Sketches has a sibling below it, so its line runs on.
        assert_eq!(rows[2].guides, [true]);
        assert!(rows[3].last_child && rows[0].has_children && !rows[4].has_children);
        // Folding Art hides its branch.
        let rows = folder_tree(&nodes, "", &[1].into_iter().collect());
        assert_eq!(
            rows.iter().map(|row| row.folder_id).collect::<Vec<_>>(),
            [1, 5]
        );
        // A search keeps the matches with their ancestors, opened.
        let rows = folder_tree(&nodes, "ink", &[1].into_iter().collect());
        assert_eq!(
            rows.iter().map(|row| row.folder_id).collect::<Vec<_>>(),
            [1, 2, 3]
        );
    }
}
