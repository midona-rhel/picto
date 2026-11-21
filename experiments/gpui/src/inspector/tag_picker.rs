//! The tag picker that the inspector opens (`TagSelectPanel.tsx` in its
//! `OverlayShell`). It has a search field over the library's tags, a rail with
//! All, each tag group, Selected and Starred, and a list of tags. A click on a
//! tag in the list puts the tag on the selected items or takes it off, at
//! once. When the search text names a tag that does not exist, the list offers
//! to create it.
use super::membership;
use crate::i18n::t;
use crate::motion;
use crate::session::Session;
use crate::session::{Report, Reread};
use crate::tags::{Common, TagPager, View, record_name as name};
use crate::ui::{
    self,
    menu::{self, Menu},
    overlay_shell::{self, Geometry, Nav},
    text_input::{Kind, TextInput},
};
use gpui::*;
use picto_library::predicate::SetMatchMode;
use picto_library::{TagId, TagNamespaceRecord, TagRecord};
use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

mod model;
mod view;

use model::{Sources, create_candidate, display_tags, step};

/// `PAGE_SIZE`: how many tags are loaded at a time.
const PAGE: usize = 100;
/// The picker is 540 × 480 with the rail of groups and 340 × 480 without it.
const WIDE: f32 = 540.;
const NARROW: f32 = 340.;
const HEIGHT: f32 = 480.;
const RAIL: f32 = 200.;
const ROW: f32 = 25.;
/// The space in pixels between the fills of two rows, one above the other.
/// A row's line stays `ROW` tall; the row's own box is this much shorter.
const ROW_GAP: f32 = 2.;
/// The space in pixels between two columns of rows.
const COLUMN_GAP: f32 = 4.;
/// How long the search waits after the last key press before it asks the
/// library.
const SEARCH_DELAY: Duration = Duration::from_millis(150);
/// The next page of tags is loaded when the list has been scrolled to within
/// this many pixels of its end.
const LOAD_AHEAD: f32 = 200.;

/// `TagLayout`: whether the list has two columns or one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Grid,
    List,
}

/// What the picker asks its owner to do.
pub enum Event {
    /// Put the tag on the items (`add` is true) or take it off. `created` is
    /// true when the tag does not exist yet.
    Toggle {
        name: String,
        add: bool,
        created: bool,
    },
    /// Show the items that have this tag (the menu row "Filter Items with This
    /// Tag").
    Filter(TagId),
    /// A tag was renamed, so its name as shown elsewhere is out of date.
    Renamed,
    /// In the filter mode: the tags that are included and the tags that are
    /// excluded, each by id and name, and the match rule (`onApplyTagFilter`).
    FilterChanged {
        include: Vec<(TagId, String)>,
        exclude: Vec<(TagId, String)>,
        mode: SetMatchMode,
    },
}

/// The picker as the filter bar opens it (`onApplyTagFilter`). A click
/// includes a tag and a right-click excludes it, under a match rule. The list
/// has one column and no tag can be created.
struct FilterMode {
    excluded: BTreeSet<String>,
    mode: SetMatchMode,
    /// The id of each chosen tag, by the tag's name.
    ids: HashMap<String, TagId>,
}

impl EventEmitter<Event> for TagPicker {}

pub struct TagPicker {
    session: Entity<Session>,
    search: Entity<TextInput>,
    focus: FocusHandle,
    opening: Option<ui::popover::Opening>,
    geometry: Geometry,
    pinned: bool,
    sidebar: bool,
    /// The rail's width while it opens or closes over 200 ms, and its fade.
    rail_width: motion::Fade,
    rail_shown: motion::Fade,
    layout: Layout,
    counts: bool,
    settings_open: bool,
    query: String,
    pager: TagPager,
    namespaces: Vec<TagNamespaceRecord>,
    /// The tags of the selected items.
    entity: Vec<TagRecord>,
    /// The names of the tags that are shown as chosen. Each click changes
    /// this.
    selected: BTreeSet<String>,
    opening_names: Vec<String>,
    dirty: bool,
    view: View,
    focus_index: Option<usize>,
    scroll: ScrollHandle,
    search_timer: Option<Task<()>>,
    menu: menu::Host,
    menu_tag: Option<TagRecord>,
    filter: Option<FilterMode>,
    /// The layer the panel is drawn on. It is above a dialog when a dialog
    /// opened the picker.
    layer: usize,
    _subscriptions: Vec<Subscription>,
}

impl TagPicker {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new(Kind::Menu, cx).placeholder("Search..."));
        // When a row's menu has closed, the keyboard focus goes back to the
        // search field.
        let menu = menu::Host::deferred(
            crate::ui::layer::MENU,
            menu::Return::To(search.read(cx).focus_handle(cx)),
        );
        let subscriptions = vec![cx.observe(&search, |this, search, cx| {
            let query = search.read(cx).value();
            if query != this.query {
                this.query = query;
                this.focus_index = None;
                this.request(cx);
            }
        })];
        Self {
            session,
            search,
            focus: cx.focus_handle(),
            opening: None,
            geometry: Geometry {
                right: None,
                left: 0.,
                top: 0.,
            },
            pinned: false,
            sidebar: true,
            rail_width: motion::Fade::at(true),
            rail_shown: motion::Fade::at(true),
            layout: Layout::Grid,
            counts: true,
            settings_open: false,
            query: String::new(),
            pager: TagPager::new(PAGE),
            namespaces: Vec::new(),
            entity: Vec::new(),
            selected: BTreeSet::new(),
            opening_names: Vec::new(),
            dirty: false,
            view: View::All,
            focus_index: None,
            scroll: ScrollHandle::new(),
            search_timer: None,
            menu,
            menu_tag: None,
            filter: None,
            layer: crate::ui::layer::PANEL,
            _subscriptions: subscriptions,
        }
    }

    pub fn is_open(&self) -> bool {
        self.opening.is_some()
    }

    /// Reads the layout and the counts setting that were last chosen in any
    /// picker. Each `TagSelectPanel` in the Electron app reads
    /// `picto-tag-picker-layout` and `picto-tag-picker-counts` in the same way
    /// when it opens.
    fn read_layout(&mut self, cx: &App) {
        let kept = crate::state::get(cx);
        self.layout = if kept.tag_picker_list {
            Layout::List
        } else {
            Layout::Grid
        };
        self.counts = kept.tag_picker_counts;
    }

    /// Stores this picker's layout and counts setting, for the next picker
    /// that opens.
    fn keep_layout(&self, cx: &mut App) {
        let (list, counts) = (self.layout == Layout::List, self.counts);
        crate::state::update(cx, |state| {
            state.tag_picker_list = list;
            state.tag_picker_counts = counts;
        });
    }

    fn width(&self) -> f32 {
        NARROW + (WIDE - NARROW) * self.rail_width.value_with(motion::EASE_DEFAULT)
    }

    /// Opens the picker beside the inspector at `anchor`, or in the middle of
    /// the window when there is no anchor, showing the tags the items have
    /// now.
    pub fn open(
        &mut self,
        anchor: Option<Point<f32>>,
        entity: Vec<TagRecord>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.filter = None;
        self.layer = crate::ui::layer::PANEL;
        self.show(anchor, entity, window, cx);
    }

    /// Opens the picker over a dialog, with its bottom edge just above the
    /// dialog's button at `anchor` (`anchorPlacement: 'above'`), showing the
    /// tags that `entity` names.
    pub fn open_above(
        &mut self,
        anchor: Point<f32>,
        entity: Vec<TagRecord>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.filter = None;
        self.layer = crate::ui::layer::OVER_DIALOG;
        self.show(None, entity, window, cx);
        let view = window.viewport_size();
        self.geometry = overlay_shell::place(
            overlay_shell::Opens::Above(anchor),
            view,
            if self.sidebar { WIDE } else { NARROW },
            HEIGHT,
        );
    }

    /// Opens the picker in the filter mode below a control of the filter bar,
    /// showing the tags that the filter includes and excludes.
    pub fn open_filter(
        &mut self,
        below: Point<f32>,
        include: Vec<TagRecord>,
        exclude: Vec<TagRecord>,
        mode: SetMatchMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.filter = Some(FilterMode {
            excluded: exclude.iter().map(name).collect(),
            mode,
            ids: include
                .iter()
                .chain(&exclude)
                .map(|tag| (name(tag), tag.tag_id))
                .collect(),
        });
        self.pinned = false;
        self.layer = crate::ui::layer::PANEL;
        self.show(None, include, window, cx);
        let view = window.viewport_size();
        self.geometry = overlay_shell::place(
            overlay_shell::Opens::Below(below),
            view,
            if self.sidebar { WIDE } else { NARROW },
            HEIGHT,
        );
    }

    fn show(
        &mut self,
        anchor: Option<Point<f32>>,
        entity: Vec<TagRecord>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.opening.is_none() {
            self.opening = Some(ui::popover::Opening::new(window, cx));
            self.read_layout(cx);
        }
        let view = window.viewport_size();
        let width = if self.sidebar { WIDE } else { NARROW };
        self.geometry =
            overlay_shell::place(overlay_shell::Opens::beside(anchor), view, width, HEIGHT);
        self.query.clear();
        self.search.update(cx, |search, cx| search.clear(cx));
        self.pager.settled = None;
        self.settings_open = false;
        let names = entity.iter().map(name).collect::<Vec<_>>();
        self.selected = names.iter().cloned().collect();
        self.opening_names = names;
        self.entity = entity;
        self.dirty = false;
        self.view = View::All;
        self.focus_index = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
        let handle = self.search.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        self.load_groups(cx);
        self.request(cx);
        cx.notify();
    }

    /// Called when the items' tags have changed or have arrived. The Selected
    /// view shows the new tags. Until the user's first click, the rows shown
    /// as chosen follow them too.
    pub fn set_entity(&mut self, entity: Vec<TagRecord>, cx: &mut Context<Self>) {
        if !self.is_open() {
            return;
        }
        if !self.dirty {
            let names = entity.iter().map(name).collect::<Vec<_>>();
            self.selected = names.iter().cloned().collect();
            self.opening_names = names;
        }
        self.entity = entity;
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(opening) = self.opening.take() else {
            return;
        };
        self.search_timer = None;
        self.menu.close(cx);
        opening.finish(&self.focus, window, cx);
        cx.notify();
    }

    // ── Loading ─────────────────────────────────────────────────

    fn load_groups(&mut self, cx: &mut Context<Self>) {
        crate::tags::read_groups(&self.session, cx, |this, namespaces, cx| {
            this.namespaces = namespaces;
            cx.notify();
        });
    }

    /// Sends one request for the current search text and view. Without search
    /// text it is sent at once; with search text it is sent when typing has
    /// paused. An answer to an older request never replaces the answer to a
    /// newer one. The Selected and Starred views do not ask the library; they
    /// list tags the picker already has.
    fn request(&mut self, cx: &mut Context<Self>) {
        if !self.is_open() {
            return;
        }
        self.search_timer = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
        let page = self.pager.begin(&self.query, self.view.group());
        if !matches!(self.view, View::Selected | View::Starred) {
            let delay = (!self.query.trim().is_empty()).then_some(SEARCH_DELAY);
            self.search_timer = Some(crate::tags::fetch_page(
                &self.session,
                page,
                delay,
                cx,
                |this, page, answer, cx| {
                    match answer {
                        Ok((tags, next)) => {
                            if !this.pager.first(&page, tags, next) {
                                return;
                            }
                            this.focus_index = None;
                        }
                        Err(error) => eprintln!("tag picker: {error}"),
                    }
                    cx.notify();
                },
            ));
        }
        cx.notify();
    }

    /// Loads the next page when the list is near its end.
    fn load_more(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.pager.next() else {
            return;
        };
        crate::tags::fetch_page(&self.session, page, None, cx, |this, page, answer, cx| {
            if this.pager.more(&page, answer) {
                cx.notify();
            }
        })
        .detach();
    }

    /// Called after an edit. When a tag was created, the list and the groups
    /// are read again. Otherwise only the counts are refreshed and every tag
    /// stays in its place, so that nothing moves under the pointer.
    pub fn edited(&mut self, created: bool, cx: &mut Context<Self>) {
        if !self.is_open() {
            return;
        }
        if created {
            self.load_groups(cx);
            self.request(cx);
            return;
        }
        let ids = self
            .pager
            .tags
            .iter()
            .map(|tag| tag.tag_id)
            .collect::<Vec<_>>();
        let answer = self
            .session
            .read(cx)
            .run(move |library| library.tags_by_id(&ids));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(records)) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                for tag in &mut this.pager.tags {
                    if let Some(fresh) = records.iter().find(|fresh| fresh.tag_id == tag.tag_id) {
                        *tag = fresh.clone();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ── Choosing ────────────────────────────────────────────────

    fn sources_rows(&self, starred: &[String]) -> (Option<TagRecord>, Vec<TagRecord>) {
        let (settled, settled_group) = self
            .pager
            .settled
            .as_ref()
            .map_or(("", None), |(query, group)| {
                (query.as_str(), group.as_deref())
            });
        let entity = self.entity.iter().map(name).collect::<Vec<_>>();
        let rows = display_tags(&Sources {
            view: &self.view,
            query: &self.query,
            settled,
            entity: &entity,
            entity_records: &self.entity,
            opening: &self.opening_names,
            tags: &self.pager.tags,
            starred,
        });
        // The filter mode offers only tags that exist, so it never shows the
        // create row.
        let create = create_candidate(&self.view, settled, settled_group, &self.pager.tags)
            .filter(|_| self.filter.is_none());
        (create, rows)
    }

    /// `toggleTag`: puts the tag on the items or takes it off, at once. In the
    /// filter mode it includes the tag or stops including it, and never
    /// excludes it.
    fn toggle(&mut self, tag: &TagRecord, cx: &mut Context<Self>) {
        let tag_name = name(tag);
        if let Some(filter) = &mut self.filter {
            if tag.tag_id.0 == 0 {
                return;
            }
            filter.ids.insert(tag_name.clone(), tag.tag_id);
            filter.excluded.remove(&tag_name);
            membership::toggle(&mut self.selected, tag_name);
            self.dirty = true;
            self.emit_filter(cx);
            cx.notify();
            return;
        }
        self.dirty = true;
        let add = membership::toggle(&mut self.selected, tag_name.clone());
        cx.emit(Event::Toggle {
            name: tag_name,
            add,
            created: tag.tag_id.0 == 0 && add,
        });
        cx.notify();
    }

    /// `toggleExcludedTag`: in the filter mode, a right-click excludes the tag
    /// or stops excluding it.
    fn exclude(&mut self, tag: &TagRecord, cx: &mut Context<Self>) {
        let Some(filter) = &mut self.filter else {
            return;
        };
        if tag.tag_id.0 == 0 {
            return;
        }
        let tag_name = name(tag);
        filter.ids.insert(tag_name.clone(), tag.tag_id);
        self.selected.remove(&tag_name);
        membership::toggle(&mut filter.excluded, tag_name);
        self.dirty = true;
        self.emit_filter(cx);
        cx.notify();
    }

    fn set_mode(&mut self, mode: SetMatchMode, cx: &mut Context<Self>) {
        if let Some(filter) = &mut self.filter {
            filter.mode = mode;
            self.emit_filter(cx);
            cx.notify();
        }
    }

    /// Tells the filter bar which tags are included and which are excluded.
    fn emit_filter(&mut self, cx: &mut Context<Self>) {
        let Some(filter) = &self.filter else {
            return;
        };
        let choices = |names: &BTreeSet<String>| {
            names
                .iter()
                .filter_map(|name| Some((*filter.ids.get(name)?, name.clone())))
                .collect::<Vec<_>>()
        };
        cx.emit(Event::FilterChanged {
            include: choices(&self.selected),
            exclude: choices(&filter.excluded),
            mode: filter.mode,
        });
    }

    fn show_view(&mut self, view: View, cx: &mut Context<Self>) {
        if self.view != view {
            self.view = view;
            self.focus_index = None;
            self.request(cx);
        }
    }

    fn columns(&self) -> usize {
        if self.filter.is_some() {
            return 1;
        }
        match self.layout {
            Layout::Grid => 2,
            Layout::List => 1,
        }
    }

    fn navigate(&mut self, key: Nav, cx: &mut Context<Self>) -> bool {
        if self.settings_open || self.menu.is_open() {
            return false;
        }
        let starred = crate::settings::current(cx).starred_tags.clone();
        let (create, rows) = self.sources_rows(&starred);
        let count = rows.len() + create.is_some() as usize;
        match key {
            Nav::Left | Nav::Right if self.columns() == 1 => false,
            Nav::Up | Nav::Down | Nav::Left | Nav::Right => {
                self.focus_index = step(self.focus_index, key, self.columns(), count);
                self.reveal_focus();
                cx.notify();
                true
            }
            Nav::Confirm => {
                let at = self.focus_index.unwrap_or(0);
                let tag = match (&create, at) {
                    (Some(create), 0) => Some(create.clone()),
                    (Some(_), at) => rows.get(at - 1).cloned(),
                    (None, at) => rows.get(at).cloned(),
                };
                if let Some(tag) = tag {
                    self.toggle(&tag, cx);
                }
                true
            }
            Nav::Next => {
                if self.sidebar {
                    let groups = self.group_names(cx);
                    let next = crate::tags::next_view(&self.view, &groups, &View::Starred);
                    self.show_view(next, cx);
                }
                true
            }
        }
    }

    /// Scrolls the list just far enough to show the focused row
    /// (`scrollIntoView` with `nearest`).
    fn reveal_focus(&mut self) {
        let Some(at) = self.focus_index else { return };
        let row = (at / self.columns()) as f32;
        let top = 4. + row * ROW;
        let viewport = f32::from(self.scroll.bounds().size.height);
        let scrolled = -f32::from(self.scroll.offset().y);
        let next = if top < scrolled {
            top
        } else if top + ROW > scrolled + viewport {
            top + ROW - viewport
        } else {
            return;
        };
        self.scroll.set_offset(point(px(0.), px(-next.max(0.))));
    }

    fn group_names(&self, cx: &App) -> Vec<String> {
        if !crate::settings::current(cx).show_tag_groups {
            return Vec::new();
        }
        crate::tags::rail_groups(&self.namespaces, false)
            .into_iter()
            .map(|group| group.name)
            .collect()
    }

    // ── The row menu ────────────────────────────────────────────

    /// `buildCommonTagContextEntries`: the context menu of a row of the list.
    fn open_menu(
        &mut self,
        tag: TagRecord,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = crate::tags::common_menu(&tag, &self.namespaces, tag.tag_id.0 > 0, cx);
        let menu = cx.new(|cx| Menu::new(entries, position, cx));
        self.menu
            .open(menu, window, cx, |this, menu, action, window, cx| {
                let value = menu.read(cx).chosen_value();
                this.run_menu(action, value, window, cx);
            });
        self.menu_tag = Some(tag);
    }

    fn run_menu(
        &mut self,
        action: &str,
        value: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tag) = self.menu_tag.clone() else {
            return;
        };
        match crate::tags::run_common(action, value, &tag, cx) {
            Some(Common::Filter) => {
                self.close(window, cx);
                cx.emit(Event::Filter(tag.tag_id));
            }
            Some(Common::Move(next)) => {
                let (id, previous, target) = (tag.tag_id, name(&tag), next.clone());
                crate::session::edit_then(
                    &self.session,
                    Report::failure(t("Unable to move tag")),
                    Reread::NOTHING,
                    move |library| crate::tags::rename_or_merge(library, id, &target),
                    cx,
                    move |this, result, cx| {
                        if result.is_ok() {
                            crate::tags::rename_star(&previous, &next, cx);
                            this.request(cx);
                            cx.emit(Event::Renamed);
                        }
                    },
                );
            }
            Some(Common::Done) | None => {}
        }
        // When the menu has closed, the keyboard focus goes back to the search
        // field.
        let handle = self.search.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }
}
