//! The tag picker the inspector opens (`TagSelectPanel.tsx` in its
//! `OverlayShell`): a search over the library's tags, a rail of All, the
//! tag groups, Selected and Starred, and a list in which a click puts a tag
//! on the items or takes it off at once. A search for a tag that does not
//! exist offers to create it.
use super::membership;
use crate::motion;
use crate::session::Session;
use crate::tags::record_name as name;
use crate::ui::{
    self,
    menu::{self, Menu},
    overlay_shell::{self, Geometry, Nav},
    text_input::TextInput,
};
use gpui::*;
use picto_library::predicate::SetMatchMode;
use picto_library::{TagId, TagNamespaceRecord, TagRecord};
use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

mod model;
mod view;

use model::{Sources, create_candidate, display_tags, next_view, rail_groups, step};

/// `PAGE_SIZE`
const PAGE: usize = 100;
/// 540 × 480 with the group rail, 340 × 480 without.
const WIDE: f32 = 540.;
const NARROW: f32 = 340.;
const HEIGHT: f32 = 480.;
const RAIL: f32 = 200.;
const ROW: f32 = 25.;
/// The search waits this long after typing before it asks the library.
const SEARCH_DELAY: Duration = Duration::from_millis(150);
/// More tags load once the list is scrolled this near its end.
const LOAD_AHEAD: f32 = 200.;

/// What the list shows (`SidebarMode`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    All,
    Group(String),
    /// The tags the items have.
    Selected,
    Starred,
}

/// `TagLayout`: two columns, or one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Grid,
    List,
}

/// What the picker asks its owner to do.
pub enum Event {
    /// Put the tag on the items (`add`) or take it off. `created` when the
    /// tag does not exist yet.
    Toggle {
        name: String,
        add: bool,
        created: bool,
    },
    /// Show the items with this tag ("Filter Items with This Tag").
    Filter(TagId),
    /// A tag was renamed, so labels shown elsewhere are out of date.
    Renamed,
    /// In the filter mode, the tags included and excluded (by id and name)
    /// and the match rule (`onApplyTagFilter`).
    FilterChanged {
        include: Vec<(TagId, String)>,
        exclude: Vec<(TagId, String)>,
        mode: SetMatchMode,
    },
}

/// The picker as the filter bar opens it (`onApplyTagFilter`): a click
/// includes a tag, a right click excludes it, under a match rule; the list
/// is one column and nothing is created.
struct FilterMode {
    excluded: BTreeSet<String>,
    mode: SetMatchMode,
    /// The ids of the tags chosen, by name.
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
    /// The rail's width (200ms) and its fade.
    rail_width: motion::Fade,
    rail_shown: motion::Fade,
    layout: Layout,
    counts: bool,
    settings_open: bool,
    query: String,
    /// The search and group the loaded tags answer, once one has arrived.
    settled: Option<(String, Option<String>)>,
    tags: Vec<TagRecord>,
    cursor: Option<usize>,
    loading_more: bool,
    generation: u64,
    namespaces: Vec<TagNamespaceRecord>,
    /// The items' tags.
    entity: Vec<TagRecord>,
    /// The names shown checked, changed by each click.
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
    /// The layer the panel is drawn on, above a dialog that opened it.
    layer: usize,
    _subscriptions: Vec<Subscription>,
}

impl TagPicker {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        let search = cx.new(TextInput::menu_search);
        // The keyboard goes back to the search once a row's menu has gone.
        let menu = menu::Host::deferred(30, menu::Return::To(search.read(cx).focus_handle(cx)));
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
            settled: None,
            tags: Vec::new(),
            cursor: None,
            loading_more: false,
            generation: 0,
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
            layer: 5,
            _subscriptions: subscriptions,
        }
    }

    pub fn is_open(&self) -> bool {
        self.opening.is_some()
    }

    /// Takes the layout and counts last chosen in any picker, as each
    /// `TagSelectPanel` reads `picto-tag-picker-layout` and
    /// `picto-tag-picker-counts` when it opens.
    fn read_layout(&mut self, cx: &App) {
        let kept = crate::state::get(cx);
        self.layout = if kept.tag_picker_list {
            Layout::List
        } else {
            Layout::Grid
        };
        self.counts = kept.tag_picker_counts;
    }

    /// Keeps this picker's layout and counts for the next one to open.
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

    /// Opens beside the inspector at `anchor` (or centred), on the items'
    /// current tags.
    pub fn open(
        &mut self,
        anchor: Option<Point<f32>>,
        entity: Vec<TagRecord>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.filter = None;
        self.layer = 5;
        self.show(anchor, entity, window, cx);
    }

    /// Opens over a dialog, its bottom just above the dialog's button at
    /// `anchor` (`anchorPlacement: 'above'`), on the tags `entity` names.
    pub fn open_above(
        &mut self,
        anchor: Point<f32>,
        entity: Vec<TagRecord>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.filter = None;
        self.layer = 15;
        self.show(None, entity, window, cx);
        let view = window.viewport_size();
        self.geometry = overlay_shell::place_above(
            anchor,
            size(f32::from(view.width), f32::from(view.height)),
            if self.sidebar { WIDE } else { NARROW },
            HEIGHT,
        );
    }

    /// Opens in the filter mode below a control of the filter bar, on the
    /// tags the filter includes and excludes.
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
        self.layer = 5;
        self.show(None, include, window, cx);
        let view = window.viewport_size();
        self.geometry = overlay_shell::place_below(
            below,
            size(f32::from(view.width), f32::from(view.height)),
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
        self.geometry = overlay_shell::place(
            anchor,
            size(f32::from(view.width), f32::from(view.height)),
            width,
            HEIGHT,
        );
        self.query.clear();
        self.search.update(cx, |search, cx| search.clear(cx));
        self.settled = None;
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

    /// The items' tags changed (or arrived): the Selected view follows, and
    /// until the first click so do the checks.
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
        let answer = self
            .session
            .read(cx)
            .run(|library| library.tag_namespaces());
        cx.spawn(async move |this, cx| {
            let Ok(Ok(namespaces)) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.namespaces = namespaces;
                cx.notify();
            });
        })
        .detach();
    }

    fn browsed_group(&self) -> Option<String> {
        match &self.view {
            View::Group(group) => Some(group.clone()),
            _ => None,
        }
    }

    /// One request for each search and view: at once without a search, else
    /// once typing pauses; an older answer never replaces a newer one.
    fn request(&mut self, cx: &mut Context<Self>) {
        if !self.is_open() {
            return;
        }
        self.generation += 1;
        self.loading_more = false;
        self.search_timer = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
        if matches!(self.view, View::Selected | View::Starred) {
            cx.notify();
            return;
        }
        let generation = self.generation;
        let query = self.query.clone();
        let group = self.browsed_group();
        if query.trim().is_empty() {
            self.fetch(query, group, generation, cx);
        } else {
            self.search_timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SEARCH_DELAY).await;
                let _ = this.update(cx, |this, cx| this.fetch(query, group, generation, cx));
            }));
        }
        cx.notify();
    }

    fn fetch(
        &mut self,
        query: String,
        group: Option<String>,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let (search, namespace) = (query.trim().to_owned(), group.clone());
        let answer = self.session.read(cx).run(move |library| {
            let search = (!search.is_empty()).then_some(search.as_str());
            library.tag_page(namespace.as_deref(), search, 0, PAGE)
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = answer.await else { return };
            let _ = this.update(cx, |this, cx| {
                if generation != this.generation {
                    return;
                }
                match result {
                    Ok((tags, next, _)) => {
                        this.tags = tags;
                        this.cursor = next;
                        this.settled = Some((query, group));
                        this.focus_index = None;
                    }
                    Err(error) => eprintln!("tag picker: {error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The next page, once the list nears its end.
    fn load_more(&mut self, cx: &mut Context<Self>) {
        let Some(offset) = self.cursor.filter(|_| !self.loading_more) else {
            return;
        };
        let Some((query, group)) = self.settled.clone() else {
            return;
        };
        if query != self.query || group != self.browsed_group() {
            return;
        }
        self.loading_more = true;
        let generation = self.generation;
        let (search, namespace) = (query.trim().to_owned(), group);
        let answer = self.session.read(cx).run(move |library| {
            let search = (!search.is_empty()).then_some(search.as_str());
            library.tag_page(namespace.as_deref(), search, offset, PAGE)
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = answer.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.loading_more = false;
                if generation != this.generation {
                    return;
                }
                if let Ok((tags, next, _)) = result {
                    let known = this
                        .tags
                        .iter()
                        .map(|tag| tag.tag_id)
                        .collect::<BTreeSet<_>>();
                    this.tags
                        .extend(tags.into_iter().filter(|tag| !known.contains(&tag.tag_id)));
                    this.cursor = next.filter(|next| *next != offset);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// After an edit: a created tag means the list and groups are read
    /// again; otherwise the counts are refreshed where the tags stand, so
    /// nothing moves under the pointer.
    pub fn edited(&mut self, created: bool, cx: &mut Context<Self>) {
        if !self.is_open() {
            return;
        }
        if created {
            self.load_groups(cx);
            self.request(cx);
            return;
        }
        let ids = self.tags.iter().map(|tag| tag.tag_id).collect::<Vec<_>>();
        let answer = self
            .session
            .read(cx)
            .run(move |library| library.tags_by_id(&ids));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(records)) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                for tag in &mut this.tags {
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
        let (settled, settled_group) =
            self.settled.as_ref().map_or(("", None), |(query, group)| {
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
            tags: &self.tags,
            starred,
        });
        // The filter mode offers only tags that exist.
        let create = create_candidate(&self.view, settled, settled_group, &self.tags)
            .filter(|_| self.filter.is_none());
        (create, rows)
    }

    /// `toggleTag`: the tag goes on the items or comes off, at once; in the
    /// filter mode it is included or no longer, and never excluded.
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

    /// `toggleExcludedTag`: in the filter mode, a right click excludes the
    /// tag, or no longer.
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

    /// Tells the filter bar what is included and excluded.
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
                    let next = next_view(&self.view, &groups);
                    self.show_view(next, cx);
                }
                true
            }
        }
    }

    /// Scrolls the list so the focused row shows (`scrollIntoView` nearest).
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
        rail_groups(&self.namespaces)
            .into_iter()
            .map(|group| group.name)
            .collect()
    }

    // ── The row menu ────────────────────────────────────────────

    /// `buildCommonTagContextEntries` for a row of the list.
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
        let tag_name = name(&tag);
        match action {
            "filter" => {
                self.close(window, cx);
                cx.emit(Event::Filter(tag.tag_id));
            }
            "star" => crate::tags::set_starred(&tag_name, true, cx),
            "unstar" => crate::tags::set_starred(&tag_name, false, cx),
            "copy" => cx.write_to_clipboard(ClipboardItem::new_string(tag_name)),
            "move-to-group" | "remove-from-group" => {
                let group = value.filter(|_| action == "move-to-group");
                let next = crate::tags::name_in_group(&tag.subname, group.as_deref());
                let id = tag.tag_id;
                let target = next.clone();
                let answer = self
                    .session
                    .read(cx)
                    .run(move |library| library.rename_tag(id, &target));
                cx.spawn(async move |this, cx| {
                    let Ok(result) = answer.await else { return };
                    let _ = this.update(cx, |this, cx| match result {
                        Ok(_) => {
                            crate::tags::rename_star(&tag_name, &next, cx);
                            this.request(cx);
                            cx.emit(Event::Renamed);
                        }
                        Err(error) => {
                            crate::ui::notify::error("Unable to move tag", error.to_string(), cx)
                        }
                    });
                })
                .detach();
            }
            _ => {}
        }
        // The keyboard goes back to the search once the menu has gone.
        let handle = self.search.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }
}
