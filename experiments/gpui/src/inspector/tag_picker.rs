//! The tag picker the inspector opens (`TagSelectPanel.tsx` in its
//! `OverlayShell`): a search over the library's tags, a rail of All, the
//! tag groups, Selected and Starred, and a list in which a click puts a tag
//! on the items or takes it off at once. A search for a tag that does not
//! exist offers to create it.
use super::membership;
use crate::session::Session;
use crate::tags::{Preferences, record_name as name};
use crate::theme::Text as _;
use crate::ui::{
    self,
    menu::{self, Menu},
    overlay_shell::{self, Geometry, Mark, Nav},
    text_input::TextInput,
};
use crate::{grid::format, motion, theme};
use gpui::*;
use picto_library::predicate::SetMatchMode;
use picto_library::{TagId, TagNamespaceId, TagNamespaceRecord, TagRecord};
use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

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

/// `tagRecord`: a record for a tag known only by name, in General when the
/// name has no group.
pub fn record(name: &str) -> TagRecord {
    let (namespace, subname) = crate::tags::split(name);
    let namespace = namespace.unwrap_or("general");
    TagRecord {
        tag_id: TagId(0),
        namespace_id: TagNamespaceId(0),
        namespace: namespace.to_owned(),
        subname: subname.to_owned(),
        active_count: 0,
        assignment_count: 0,
    }
}

/// `displayTagName`: the name, without its group unless prefixes show.
fn display_name(tag: &TagRecord, prefixes: bool) -> String {
    let full = name(tag);
    if prefixes {
        return full;
    }
    crate::tags::split(&full).1.to_owned()
}

/// Everything [`display_tags`] chooses from.
pub struct Sources<'a> {
    pub view: &'a View,
    /// What the search field holds now, and the search the loaded `tags`
    /// answer.
    pub query: &'a str,
    pub settled: &'a str,
    /// The items' tags by name, and their records where known.
    pub entity: &'a [String],
    pub entity_records: &'a [TagRecord],
    /// The names that were selected when the picker opened, which list first.
    pub opening: &'a [String],
    pub tags: &'a [TagRecord],
    pub starred: &'a [String],
}

/// `displayTags`: the rows the list shows, best match first, each visible
/// name once.
pub fn display_tags(sources: &Sources) -> Vec<TagRecord> {
    let local = matches!(sources.view, View::Selected | View::Starred);
    let candidates = match sources.view {
        View::Selected => {
            let mut records = sources
                .entity
                .iter()
                .map(|name| record(name))
                .collect::<Vec<_>>();
            records.sort_by(|left, right| {
                left.namespace
                    .to_lowercase()
                    .cmp(&right.namespace.to_lowercase())
                    .then_with(|| crate::text::natural_order(&left.subname, &right.subname))
            });
            records
        }
        View::Starred => {
            let mut records = sources
                .starred
                .iter()
                .map(|name| record(name))
                .collect::<Vec<_>>();
            records.sort_by(|left, right| crate::text::natural_order(&name(left), &name(right)));
            records
        }
        View::All | View::Group(_) => {
            let known = |wanted: &str| {
                sources
                    .tags
                    .iter()
                    .chain(sources.entity_records)
                    .find(|tag| name(tag) == wanted)
                    .cloned()
            };
            let group = match sources.view {
                View::Group(group) => Some(group.as_str()),
                _ => None,
            };
            let opening = sources
                .opening
                .iter()
                .map(|wanted| known(wanted).unwrap_or_else(|| record(wanted)))
                .filter(|tag| group.is_none_or(|group| tag.namespace == group))
                .collect::<Vec<_>>();
            let opening_names = opening.iter().map(name).collect::<BTreeSet<_>>();
            opening
                .into_iter()
                .chain(
                    sources
                        .tags
                        .iter()
                        .filter(|tag| !opening_names.contains(&name(tag)))
                        .cloned(),
                )
                .collect()
        }
    };
    let query = if local {
        sources.query
    } else {
        sources.settled
    }
    .trim()
    .to_lowercase();
    // `namespace:subname` searches match the full name; others the subname.
    let full = query.contains(':');
    let keyed = candidates
        .into_iter()
        .map(|tag| {
            let key = if full || tag.subname.is_empty() {
                name(&tag)
            } else {
                tag.subname.clone()
            };
            (key, tag)
        })
        .collect::<Vec<_>>();
    let ranked = crate::text::rank_matches(keyed, &query, |(key, _)| key.as_str());
    let mut seen = BTreeSet::new();
    ranked
        .into_iter()
        .map(|(_, tag)| tag)
        .filter(|tag| {
            let visible = if tag.subname.is_empty() {
                name(tag)
            } else {
                tag.subname.clone()
            };
            seen.insert(visible.to_lowercase())
        })
        .collect()
}

/// `createTag`: the tag a search would create, when it names none loaded:
/// what was typed, in the browsed group unless it names one itself.
pub fn create_candidate(
    view: &View,
    settled: &str,
    settled_group: Option<&str>,
    tags: &[TagRecord],
) -> Option<TagRecord> {
    if matches!(view, View::Selected | View::Starred) {
        return None;
    }
    let typed = settled.trim();
    if typed.is_empty() {
        return None;
    }
    let wanted = match settled_group {
        Some(group) if !typed.contains(':') => format!("{group}:{typed}"),
        _ => typed.to_owned(),
    };
    let exists = tags
        .iter()
        .any(|tag| name(tag).to_lowercase() == wanted.to_lowercase());
    (!exists).then(|| record(&wanted))
}

/// Tab: All, then each group, then Selected, then Starred, then All again.
pub fn next_view(view: &View, groups: &[String]) -> View {
    match view {
        View::All => groups
            .first()
            .map_or(View::Selected, |group| View::Group(group.clone())),
        View::Group(current) => {
            let at = groups.iter().position(|group| group == current);
            match at.and_then(|at| groups.get(at + 1)) {
                Some(next) => View::Group(next.clone()),
                None => View::Selected,
            }
        }
        View::Selected => View::Starred,
        View::Starred => View::All,
    }
}

/// The row the arrow keys move to among `count` rows in `columns`.
pub fn step(focus: Option<usize>, key: Nav, columns: usize, count: usize) -> Option<usize> {
    if count == 0 {
        return focus;
    }
    let last = count - 1;
    let Some(at) = focus else {
        return Some(0);
    };
    Some(match key {
        Nav::Down => (at + columns).min(last),
        Nav::Up => at.saturating_sub(columns),
        Nav::Right => (at + 1).min(last),
        Nav::Left => at.saturating_sub(1),
        _ => at,
    })
}

/// The groups the rail lists, in their preset order: General is All.
fn rail_groups(namespaces: &[TagNamespaceRecord]) -> Vec<TagNamespaceRecord> {
    let mut groups = namespaces
        .iter()
        .filter(|group| !matches!(group.name.as_str(), "" | "general"))
        .cloned()
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| crate::tags::group_order(&group.name));
    groups
}

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
            _subscriptions: subscriptions,
        }
    }

    pub fn is_open(&self) -> bool {
        self.opening.is_some()
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
        self.show(anchor, entity, window, cx);
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
                        // TODO: show this as the notification production
                        // shows ("Unable to move tag").
                        Err(error) => eprintln!("tag picker: {error}"),
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

// ── Drawing ─────────────────────────────────────────────────────

impl TagPicker {
    /// `.sidebarItem`: a 24px rail row, its badge brightening under the
    /// pointer or when chosen.
    fn rail_item(
        &self,
        id: &'static str,
        index: usize,
        mark: AnyElement,
        label: SharedString,
        count: u64,
        view: View,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let active = self.view == view;
        let group = SharedString::from(format!("{id}-{index}"));
        div()
            .id((id, index))
            .group(group.clone())
            .h(px(24.))
            .flex_none()
            .pr(px(8.))
            .flex()
            .items_center()
            .rounded(px(theme::RADIUS_MD))
            .text(theme::TEXT_MD)
            .line_height(px(24.))
            .text_color(theme.text())
            .whitespace_nowrap()
            .bg(if active {
                theme.surface_active()
            } else {
                gpui::transparent_black().into()
            })
            .when_not_active(active, move |row| {
                row.hover(move |style| style.bg(theme.surface_hover()))
            })
            .child(mark)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(18.))
                    .line_height(px(18.))
                    .truncate()
                    .child(label),
            )
            .child(
                div()
                    .flex_none()
                    .ml_auto()
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_tertiary())
                    .opacity(if active { 1. } else { 0.7 })
                    .group_hover(group, |style| style.opacity(1.))
                    .child(format::grouped(count)),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.show_view(view.clone(), cx)))
    }

    fn rail(&self, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let settings = crate::settings::current(cx).clone();
        let preferences = Preferences::from_settings(&settings);
        let total = self.namespaces.iter().map(|group| group.tag_count).sum();
        let bookmark = |filled: bool| {
            ui::icon(
                if filled {
                    "picker-rail-bookmark-filled"
                } else {
                    "picker-rail-bookmark"
                },
                12.,
                theme.text_secondary(),
            )
            .flex_none()
            .mx(px(5.))
            .into_any_element()
        };
        let mut rail = div()
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .w(px(RAIL))
            .overflow_hidden()
            .p(px(8.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .cursor_pointer()
            .opacity(self.rail_shown.value_with(motion::EASE))
            .child(self.rail_item(
                "rail-all",
                0,
                bookmark(self.view == View::All),
                "All".into(),
                total,
                View::All,
                cx,
            ));
        if settings.show_tag_groups {
            for (index, group) in rail_groups(&self.namespaces).into_iter().enumerate() {
                let view = View::Group(group.name.clone());
                let hovered = SharedString::from(format!("rail-group-{index}"));
                let dot = div()
                    .size(px(10.))
                    .flex_none()
                    .mx(px(6.))
                    .rounded_full()
                    .bg(rgb(crate::tags::group_color(&group.name, &preferences)))
                    .opacity(if self.view == view { 1. } else { 0.85 })
                    .group_hover(hovered, |style| style.opacity(1.))
                    .into_any_element();
                let label = if group.name.is_empty() {
                    "general".to_owned()
                } else {
                    group.name.clone()
                };
                rail = rail.child(self.rail_item(
                    "rail-group",
                    index,
                    dot,
                    label.into(),
                    group.tag_count,
                    view,
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
        .child(self.rail_item(
            "rail-selected",
            0,
            bookmark(self.view == View::Selected),
            "Selected".into(),
            self.entity.len() as u64,
            View::Selected,
            cx,
        ))
        .child(
            self.rail_item(
                "rail-starred",
                0,
                ui::icon("picker-star", 10., theme.text())
                    .flex_none()
                    .mx(px(6.))
                    .into_any_element(),
                "Starred".into(),
                settings.starred_tags.len() as u64,
                View::Starred,
                cx,
            ),
        )
    }

    /// `.tagRow`
    fn row(
        &self,
        index: usize,
        tag: &TagRecord,
        preferences: &Preferences,
        prefixes: bool,
        query: &str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let tag_name = name(tag);
        let checked = self.selected.contains(&tag_name);
        let excluded = self
            .filter
            .as_ref()
            .is_some_and(|filter| filter.excluded.contains(&tag_name));
        // `.tagRowExcluded`: the name, mark and count fade.
        let dim = if excluded { 0.45 } else { 1. };
        let focused = self.focus_index == Some(index);
        let starred = preferences.is_starred(&tag_name);
        let color = crate::tags::group_color(&tag.namespace, preferences);
        let group = SharedString::from(format!("picker-row-{index}"));
        let fill = if checked {
            theme.primary_at(20.)
        } else if focused {
            theme.surface_active()
        } else {
            gpui::transparent_black().into()
        };
        let outline = match (checked, focused) {
            (true, true) => Some(theme.primary_at(50.)),
            (false, true) => Some(theme.contrast(25.)),
            _ => None,
        };
        let mark = if starred {
            ui::icon("picker-star", 10., theme.text()).flex_none()
        } else {
            ui::icon(
                if checked {
                    "picker-bookmark-filled"
                } else {
                    "picker-bookmark"
                },
                14.,
                rgb(color),
            )
            .flex_none()
        };
        let label = display_name(tag, prefixes);
        let text = overlay_shell::search_match(label, query, cx);
        let row_tag = tag.clone();
        let menu_tag = tag.clone();
        let mut row = div()
            .id(("picker-tag", index))
            .group(group.clone())
            .relative()
            .h(px(ROW))
            .min_w_0()
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(theme::RADIUS_MD))
            .cursor_pointer()
            .text(theme::TEXT_MD)
            .line_height(px(ROW))
            .bg(fill);
        if self.filter.is_some() {
            row = row.child(overlay_shell::check_box(
                if excluded {
                    Mark::Excluded
                } else if checked {
                    Mark::Included
                } else {
                    Mark::Empty
                },
                cx,
            ));
        }
        row = row.child(mark.opacity(dim)).child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .opacity(dim)
                .text_color(theme::blend(theme.text(), rgb(color), 0.12))
                .child(text),
        );
        if !checked && !focused {
            row = row.hover(move |style| style.bg(theme.surface_hover()));
        }
        if self.counts {
            row = row.child(
                div()
                    .flex_none()
                    .ml_auto()
                    .text(theme::TEXT_CAPTION)
                    .text_color(theme.text_tertiary())
                    .opacity(0.7 * dim)
                    .group_hover(group, move |style| style.opacity(dim))
                    .child(format!("({})", format::grouped(tag.active_count))),
            );
        }
        if let Some(outline) = outline {
            row = row.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(theme::RADIUS_MD))
                    .border_1()
                    .border_color(outline),
            );
        }
        row.on_click(cx.listener(move |this, _, _, cx| {
            this.focus_index = Some(index);
            this.toggle(&row_tag, cx);
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                if this.filter.is_some() {
                    this.exclude(&menu_tag, cx);
                } else {
                    this.open_menu(menu_tag.clone(), event.position, window, cx);
                }
            }),
        )
    }

    /// `.createTagRow`: Create "name".
    fn create_row(&self, tag: &TagRecord, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let focused = self.focus_index == Some(0);
        let label = name(tag);
        let created = tag.clone();
        let mut row = div()
            .id("picker-create")
            .relative()
            .h(px(ROW))
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(theme::RADIUS_MD))
            .cursor_pointer()
            .text(theme::TEXT_MD)
            .line_height(px(ROW))
            .text_color(theme.text())
            .bg(if focused {
                theme.surface_active()
            } else {
                theme.glass()
            })
            .child(ui::icon("picker-plus", 10., theme.text_tertiary()).opacity(0.65))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child("Create \"")
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .border_b_1()
                            .border_color(theme.contrast(20.))
                            .h(px(18.))
                            .line_height(px(18.))
                            .child(label),
                    )
                    .child("\""),
            );
        if focused {
            row = row.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(theme::RADIUS_MD))
                    .border_1()
                    .border_color(theme.contrast(25.)),
            );
        } else {
            row = row.hover(move |style| style.bg(theme.surface_hover()));
        }
        row.on_click(cx.listener(move |this, _, _, cx| this.toggle(&created, cx)))
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
    }

    fn list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let settings = crate::settings::current(cx).clone();
        let preferences = Preferences::from_settings(&settings);
        let (create, rows) = self.sources_rows(&settings.starred_tags);
        let local = matches!(self.view, View::Selected | View::Starred);
        let shown_query = if local {
            self.query.clone()
        } else {
            self.settled
                .as_ref()
                .map_or(String::new(), |(query, _)| query.clone())
        };
        let can_show_empty = local || self.settled.is_some();
        // Near the end of what is loaded, the next page.
        let max = f32::from(self.scroll.max_offset().y);
        let scrolled = -f32::from(self.scroll.offset().y);
        if self.cursor.is_some() && !self.loading_more && max - scrolled < LOAD_AHEAD {
            self.load_more(cx);
        }
        let mut content = div().w_full().p(px(4.)).flex().flex_col();
        let offset = create.is_some() as usize;
        if let Some(create) = &create {
            content = content.child(self.create_row(create, cx));
        }
        let columns = self.columns();
        for (chunk_index, chunk) in rows.chunks(columns).enumerate() {
            let mut line = div().w_full().h(px(ROW)).flex_none().flex();
            for (column, tag) in chunk.iter().enumerate() {
                let index = offset + chunk_index * columns + column;
                line = line.child(div().flex_1().min_w_0().child(self.row(
                    index,
                    tag,
                    &preferences,
                    settings.show_tag_prefixes,
                    &shown_query,
                    cx,
                )));
            }
            for _ in chunk.len()..columns {
                line = line.child(div().flex_1().min_w_0());
            }
            content = content.child(line);
        }
        let empty = rows.is_empty() && create.is_none();
        let scroller = div()
            .id("picker-tags")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .pr(px(ui::scrollbar::GUTTER))
            .on_scroll(cx.listener(|_, _, _, cx| cx.notify()));
        let scroller = if empty && can_show_empty {
            scroller.child(
                div()
                    .h(px(60.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child(if self.view == View::Selected {
                        "No tags on this entity"
                    } else {
                        "No tags found"
                    }),
            )
        } else if empty {
            scroller
        } else {
            scroller.child(content)
        };
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(NARROW))
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(if self.sidebar {
                theme.border_secondary()
            } else {
                gpui::transparent_black().into()
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(scroller)
                    .child(ui::scrollbar(
                        "picker-tags-scrollbar",
                        &self.scroll,
                        window,
                        cx,
                    )),
            )
    }

    /// The layout and counts settings (`.settingsPanel`).
    fn settings_panel(&self, window: &mut Window, cx: &mut Context<Self>) -> [AnyElement; 2] {
        let theme = theme::theme(cx);
        let layout = self.layout;
        let picked = cx.entity().downgrade();
        let tabs = overlay_shell::view_tabs(
            "picker-layout",
            &[
                ("picker-layout-list", "List"),
                ("picker-layout-grid", "Grid"),
            ],
            if layout == Layout::List { 0 } else { 1 },
            move |index, _, cx| {
                let _ = picked.update(cx, |this, cx| {
                    this.layout = if index == 0 {
                        Layout::List
                    } else {
                        Layout::Grid
                    };
                    this.focus_index = None;
                    cx.notify();
                });
            },
            window,
            cx,
        );
        let row = || {
            div()
                .w_full()
                .min_h(px(32.))
                .px(px(4.))
                .py(px(2.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .rounded(px(theme::RADIUS_MD))
                .text_color(theme.text())
        };
        let edge = || div().absolute().inset_0().rounded(px(theme::RADIUS_LG));
        let backdrop = div()
            .id("picker-settings-backdrop")
            .absolute()
            .inset_0()
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings_open = false;
                cx.notify();
            }));
        let panel = div()
            .id("picker-settings")
            .occlude()
            .absolute()
            .top(px(32.))
            .right(px(32.))
            .w(px(240.))
            .p(px(16.))
            .rounded(px(theme::RADIUS_LG))
            .bg(theme.glass())
            .shadow(theme::panel_shadow())
            .child(edge().border_1().border_color(theme.glass_border_bottom()))
            .child(
                edge()
                    .border_l_1()
                    .border_r_1()
                    .border_color(theme.glass_border()),
            )
            .child(edge().border_t_1().border_color(theme.glass_border_top()))
            .child(row().child("Layout").child(tabs))
            .child(
                row()
                    .id("picker-counts")
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.surface_hover()))
                    .child("Show counts")
                    .child(overlay_shell::check_box(
                        if self.counts {
                            Mark::Checked
                        } else {
                            Mark::Empty
                        },
                        cx,
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.counts = !this.counts;
                        cx.notify();
                    })),
            );
        [backdrop.into_any_element(), panel.into_any_element()]
    }
}

/// `when` for a condition that leaves the row as it is.
trait WhenNotActive: Sized {
    fn when_not_active(self, active: bool, then: impl FnOnce(Self) -> Self) -> Self {
        if active { self } else { then(self) }
    }
}
impl WhenNotActive for Stateful<Div> {}

impl Render for TagPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = self.menu.layer(window, cx);
        if !self.is_open() {
            return div().into_any_element();
        }
        let theme = theme::theme(cx);
        if self.rail_width.animating() || self.rail_shown.animating() {
            window.request_animation_frame();
        }
        let width = self.width();
        let entity = cx.entity().downgrade();
        let (moved, nav) = (entity.clone(), entity.clone());
        let close = entity.clone();
        let sidebar = self.sidebar;
        let search_handle = self.search.read(cx).focus_handle(cx);
        let search = div()
            .flex_1()
            .min_w_0()
            .h(px(36.))
            .pl(px(3.))
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                window.focus(&search_handle, cx);
            })
            .child(ui::icon("picker-search", 14., theme.text_secondary()).flex_none())
            .child(div().flex_1().min_w_0().h_full().child(self.search.clone()));
        let toggle_rail = overlay_shell::header_button(
            "picker-sidebar",
            "picker-layout-sidebar",
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
            this.rail_width.set(this.sidebar, motion::NORMAL);
            this.rail_shown.set(
                this.sidebar,
                if this.sidebar {
                    motion::PICKER_RAIL_IN
                } else {
                    motion::PICKER_RAIL_OUT
                },
            );
            if !this.sidebar && !matches!(this.view, View::All) {
                this.view = View::All;
                this.request(cx);
            }
            cx.notify();
        }));
        let settings = overlay_shell::header_button(
            "picker-settings-button",
            "picker-adjustments",
            ui::tip("Tag picker settings"),
            window,
            cx,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.settings_open = !this.settings_open;
            cx.notify();
        }));
        let pin = overlay_shell::header_button(
            "picker-pin",
            if self.pinned {
                "picker-pin-filled"
            } else {
                "picker-pin"
            },
            ui::tip(if self.pinned {
                "Unpin"
            } else {
                "Pin (keep open)"
            }),
            window,
            cx,
        )
        .ml_auto()
        .on_click(cx.listener(|this, _, _, cx| {
            this.pinned = !this.pinned;
            cx.notify();
        }));

        let mut body = div()
            .relative()
            .size_full()
            .child(self.rail(cx))
            .child(self.list(window, cx));
        if self.settings_open && self.filter.is_none() {
            body = body.children(self.settings_panel(window, cx));
        }

        let mut footer_move = overlay_shell::hint(cx)
            .child("Move ")
            .child(overlay_shell::kbd("↑", cx))
            .child(overlay_shell::kbd("↓", cx));
        if self.layout == Layout::Grid {
            footer_move = footer_move
                .child(overlay_shell::kbd("←", cx))
                .child(overlay_shell::kbd("→", cx));
        }
        footer_move = footer_move
            .child("Select ")
            .child(overlay_shell::kbd("↵", cx));
        let mut shell = overlay_shell::overlay_shell(
            "tag-picker",
            self.geometry,
            width,
            HEIGHT,
            &self.focus,
            move |window, cx| {
                let _ = close.update(cx, |this, cx| this.close(window, cx));
            },
        )
        .pinned(self.pinned)
        .on_move(move |geometry, _, cx| {
            let _ = moved.update(cx, |this, cx| {
                this.geometry = geometry;
                cx.notify();
            });
        })
        .on_nav(move |key, _, cx| {
            nav.update(cx, |this, cx| this.navigate(key, cx))
                .unwrap_or(false)
        })
        .header(search);
        if let Some(filter) = &self.filter {
            // The match rule in place of the settings and the pin.
            let modes = [SetMatchMode::Any, SetMatchMode::All, SetMatchMode::Exact];
            let logic = cx.entity().downgrade();
            shell = shell
                .header(
                    ui::segmented::segmented(
                        "tag-picker-logic",
                        ui::segmented::Look::Logic,
                        vec![
                            ui::segmented::segment("logic-any", "Match any"),
                            ui::segmented::segment("logic-all", "Match all"),
                            ui::segmented::segment("logic-exact", "Match exactly"),
                        ],
                        modes.iter().position(|mode| *mode == filter.mode),
                    )
                    .on_select(move |index, _, cx| {
                        let _ = logic.update(cx, |this, cx| this.set_mode(modes[index], cx));
                    }),
                )
                .header(toggle_rail)
                .child(body)
                .footer(
                    overlay_shell::hint(cx)
                        .child("Select")
                        .child(overlay_shell::kbd("L-Click", cx)),
                )
                .footer(
                    overlay_shell::hint(cx)
                        .child("Exclude")
                        .child(overlay_shell::kbd("R-Click", cx)),
                );
        } else {
            shell = shell
                .header(toggle_rail)
                .header(settings)
                .header(pin)
                .child(body);
            if self.sidebar {
                shell = shell.footer(
                    overlay_shell::hint(cx)
                        .child("Switch ")
                        .child(overlay_shell::tab_key(cx)),
                );
            }
            shell = shell.footer(footer_move);
        }
        shell = shell.footer(
            overlay_shell::hint(cx)
                .child("Close ")
                .child(overlay_shell::kbd("Esc", cx)),
        );
        div().child(shell).children(menu).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Sources, View, create_candidate, display_tags, name, next_view, record, step};
    use crate::ui::overlay_shell::Nav;
    use picto_library::{TagId, TagNamespaceId, TagRecord};

    fn tag(id: u32, namespace: &str, subname: &str) -> TagRecord {
        TagRecord {
            tag_id: TagId(id),
            namespace_id: TagNamespaceId(0),
            namespace: namespace.into(),
            subname: subname.into(),
            active_count: 0,
            assignment_count: 0,
        }
    }

    fn names(tags: &[TagRecord]) -> Vec<String> {
        tags.iter().map(name).collect()
    }

    #[test]
    fn records_by_name_fall_in_general_without_a_group() {
        assert_eq!(name(&record("cat")), "cat");
        assert_eq!(record("cat").namespace, "general");
        assert_eq!(name(&record("species:cat")), "species:cat");
    }

    #[test]
    fn the_tags_selected_on_opening_list_first_then_the_rest() {
        let tags = [
            tag(1, "", "apple"),
            tag(2, "", "banana"),
            tag(3, "series", "cherry"),
        ];
        let opening = ["series:cherry".to_owned()];
        let rows = display_tags(&Sources {
            view: &View::All,
            query: "",
            settled: "",
            entity: &opening,
            entity_records: &[],
            opening: &opening,
            tags: &tags,
            starred: &[],
        });
        assert_eq!(names(&rows), ["series:cherry", "apple", "banana"]);
        // In a group, only that group's opening tags come first.
        let rows = display_tags(&Sources {
            view: &View::Group("creator".into()),
            query: "",
            settled: "",
            entity: &opening,
            entity_records: &[],
            opening: &opening,
            tags: &tags[..2],
            starred: &[],
        });
        assert_eq!(names(&rows), ["apple", "banana"]);
    }

    #[test]
    fn searches_rank_by_subname_and_show_each_visible_name_once() {
        let tags = [
            tag(1, "", "bobcat"),
            tag(2, "", "cat"),
            tag(3, "species", "cat"),
            tag(4, "", "catgirl"),
        ];
        let rows = display_tags(&Sources {
            view: &View::All,
            query: "cat",
            settled: "cat",
            entity: &[],
            entity_records: &[],
            opening: &[],
            tags: &tags,
            starred: &[],
        });
        // `species:cat` has the same visible name as `cat` and is dropped.
        assert_eq!(names(&rows), ["cat", "catgirl", "bobcat"]);
        // A search with a colon matches the full name.
        let rows = display_tags(&Sources {
            view: &View::All,
            query: "species:",
            settled: "species:",
            entity: &[],
            entity_records: &[],
            opening: &[],
            tags: &tags,
            starred: &[],
        });
        assert_eq!(names(&rows), ["species:cat"]);
    }

    #[test]
    fn the_selected_and_starred_views_list_by_name_and_search_locally() {
        let entity = ["b".to_owned(), "creator:a".to_owned(), "a".to_owned()];
        let rows = display_tags(&Sources {
            view: &View::Selected,
            query: "",
            settled: "zzz",
            entity: &entity,
            entity_records: &[],
            opening: &[],
            tags: &[],
            starred: &[],
        });
        // By group, then name; `a` shows the same name as `creator:a` and
        // is listed once, as production's de-duplication does.
        assert_eq!(names(&rows), ["creator:a", "b"]);
        let starred = ["zeta".to_owned(), "alpha".to_owned()];
        let rows = display_tags(&Sources {
            view: &View::Starred,
            query: "zet",
            settled: "",
            entity: &[],
            entity_records: &[],
            opening: &[],
            tags: &[],
            starred: &starred,
        });
        assert_eq!(names(&rows), ["zeta"]);
    }

    #[test]
    fn a_search_naming_no_loaded_tag_offers_to_create_it_in_the_browsed_group() {
        let tags = [tag(1, "", "cat")];
        assert!(create_candidate(&View::All, "Cat", None, &tags).is_none());
        let created = create_candidate(&View::All, " dog ", None, &tags).unwrap();
        assert_eq!(name(&created), "dog");
        assert_eq!(created.tag_id, TagId(0));
        let grouped = create_candidate(
            &View::Group("species".into()),
            "dog",
            Some("species"),
            &tags,
        );
        assert_eq!(name(&grouped.unwrap()), "species:dog");
        let named = create_candidate(&View::All, "creator:x", Some("species"), &tags);
        assert_eq!(name(&named.unwrap()), "creator:x");
        assert!(create_candidate(&View::Selected, "dog", None, &tags).is_none());
        assert!(create_candidate(&View::All, "  ", None, &tags).is_none());
    }

    #[test]
    fn tab_steps_through_all_the_groups_selected_and_starred() {
        let groups = ["creator".to_owned(), "series".to_owned()];
        let mut view = View::All;
        let mut seen = Vec::new();
        for _ in 0..5 {
            view = next_view(&view, &groups);
            seen.push(view.clone());
        }
        assert_eq!(
            seen,
            [
                View::Group("creator".into()),
                View::Group("series".into()),
                View::Selected,
                View::Starred,
                View::All
            ]
        );
        assert_eq!(next_view(&View::All, &[]), View::Selected);
    }

    #[test]
    fn the_arrows_move_by_row_and_column_and_stop_at_the_ends() {
        assert_eq!(step(None, Nav::Down, 2, 5), Some(0));
        assert_eq!(step(Some(0), Nav::Down, 2, 5), Some(2));
        assert_eq!(step(Some(3), Nav::Down, 2, 5), Some(4));
        assert_eq!(step(Some(1), Nav::Up, 2, 5), Some(0));
        assert_eq!(step(Some(4), Nav::Right, 2, 5), Some(4));
        assert_eq!(step(Some(0), Nav::Left, 2, 5), Some(0));
        assert_eq!(step(Some(2), Nav::Down, 1, 0), Some(2));
    }
}
