//! Holds what is known about tags apart from any one view: the names of tags
//! and of their groups, the colours and the order of the groups, the parts
//! that the tag browsers share (the views, the rail and the paging of the Add
//! Tags dialog, the tag picker and the Tags screen), the rows that every tag
//! context menu starts with and what those rows do, and the starred tags and
//! the group colours that are kept in the library's application settings.
use crate::i18n::t;
use crate::session::Session;
use crate::theme;
use crate::ui::menu;
use gpui::{App, ClipboardItem, Context, Entity, SharedString, Task};
use picto_library::{Library, TagId, TagNamespaceId, TagNamespaceRecord, TagRecord};
use std::collections::BTreeMap;
use std::time::Duration;

// The functions in this section build the names of tags and take them apart.

/// Returns the group that a namespace names, trimmed and in lower case, or
/// `None` for the General group.
pub fn group_of(namespace: &str) -> Option<String> {
    let normalized = namespace.trim().to_lowercase();
    (!normalized.is_empty() && normalized != "general").then_some(normalized)
}

/// Returns the full name of a tag: `namespace:subname`, or the subname alone
/// for a tag in General. The library adds, removes and stars a tag by this
/// name.
pub fn name(namespace: &str, subname: &str) -> String {
    name_in_group(subname, group_of(namespace).as_deref())
}

/// Returns the full name ([`name`]) of a tag record.
pub fn record_name(tag: &TagRecord) -> String {
    name(&tag.namespace, &tag.subname)
}

/// Returns the name that a tag has once it is moved into `group`, or out of
/// every group when `group` is `None`.
pub fn name_in_group(subname: &str, group: Option<&str>) -> String {
    match group {
        Some(group) => format!("{group}:{subname}"),
        None => subname.to_owned(),
    }
}

/// Splits a typed name into its namespace and its subname at the first colon.
/// A name without a colon has no namespace.
pub fn split(name: &str) -> (Option<&str>, &str) {
    match name.split_once(':') {
        Some((namespace, subname)) => (Some(namespace), subname),
        None => (None, name),
    }
}

/// Returns a record for a tag that is known only by its name, which is split
/// at the first colon. A name without a group is put in General, the default
/// group that every tag without a group belongs to. The record's ids are 0.
pub fn record(name: &str) -> TagRecord {
    let (namespace, subname) = split(name);
    TagRecord {
        tag_id: TagId(0),
        namespace_id: TagNamespaceId(0),
        namespace: namespace.unwrap_or("general").to_owned(),
        subname: subname.to_owned(),
        active_count: 0,
        assignment_count: 0,
    }
}

/// Returns whether a chip may show the namespace before the subname. It may
/// not for an empty namespace, for `general` and for `default`.
pub fn chip_shows_namespace(namespace: &str) -> bool {
    !matches!(namespace, "" | "general" | "default")
}

// The functions in this section give the groups their colours and their
// order.

/// Returns the colour of a chip for a namespace. It knows more namespaces
/// than the presets of the groups below: `studio`, `person` and `system` as
/// well.
pub fn chip_color(namespace: &str) -> u32 {
    match namespace.to_lowercase().as_str() {
        "creator" | "studio" => theme::TAG_CREATOR,
        "character" | "person" => theme::TAG_CHARACTER,
        "series" => theme::TAG_SERIES,
        "species" => theme::TAG_SPECIES,
        "rating" => theme::TAG_RATING,
        "system" => theme::TAG_SYSTEM,
        _ => theme::TAG_GENERAL,
    }
}

/// Returns the preset colour of a group and the position at which the group
/// sorts among the groups.
fn preset(namespace: &str) -> (u32, u8) {
    match namespace.to_lowercase().as_str() {
        "creator" => (theme::TAG_CREATOR, 0),
        "series" => (theme::TAG_SERIES, 1),
        "character" => (theme::TAG_CHARACTER, 2),
        "species" => (theme::TAG_SPECIES, 3),
        "rating" => (theme::TAG_RATING, 4),
        _ => (theme::TAG_GENERAL, 10),
    }
}

/// Returns the position at which a group sorts among the groups.
pub fn group_order(namespace: &str) -> u8 {
    preset(namespace).1
}

/// Returns the preset colour of a group, whatever colour the user chose for
/// it.
pub fn group_preset_color(namespace: &str) -> u32 {
    preset(namespace).0
}

/// Returns the colour the user chose for the group, or the group's preset
/// colour when the user chose none.
pub fn group_color(namespace: &str, preferences: &Preferences) -> u32 {
    preferences
        .colors
        .get(namespace)
        .and_then(|color| crate::theme::parse_css_color(color))
        .unwrap_or(group_preset_color(namespace))
}

/// Returns the position at which the inspector lists the group of a tag: the
/// group order, with General after every named group.
pub fn inspector_order(namespace: &str) -> u8 {
    match namespace.to_lowercase().as_str() {
        "" | "general" => 99,
        other => group_order(other),
    }
}

/// Sorts tags into the order in which the library returns tags that are asked
/// for by id (`tags.get_many`): by group name, then by name, both ignoring
/// ASCII case, then by id. The inspector lists the tags of an item in this
/// order.
pub fn sort_as_library(tags: &mut [TagRecord]) {
    tags.sort_by(|left, right| {
        left.namespace
            .to_ascii_lowercase()
            .cmp(&right.namespace.to_ascii_lowercase())
            .then_with(|| {
                left.subname
                    .to_ascii_lowercase()
                    .cmp(&right.subname.to_ascii_lowercase())
            })
            .then(left.tag_id.0.cmp(&right.tag_id.0))
    });
}

// This section holds what the tag browsers share: their views and their rail.

/// Says what a tag browser lists: every tag (`All`), the tags of one group
/// (`Group`), the tags of the items (`Selected`), the recent tags of the Add
/// Tags dialog (`Recent`), or the starred tags of the tag picker (`Starred`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    All,
    Group(String),
    Selected,
    Recent,
    Starred,
}

impl View {
    /// Returns the group to ask the library for, when the view is a group.
    pub fn group(&self) -> Option<String> {
        match self {
            View::Group(group) => Some(group.clone()),
            _ => None,
        }
    }
}

/// Returns the view that Tab moves to from `view`: from All to each of
/// `groups` in turn, then Selected, then `last` (Recent in the dialog,
/// Starred in the picker), then All again.
pub fn next_view(view: &View, groups: &[String], last: &View) -> View {
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
        View::Selected => last.clone(),
        View::Recent | View::Starred => View::All,
    }
}

/// Returns the groups that a rail lists, in their preset order. With
/// `general` the General group is one of them, as in the rail of the Add Tags
/// dialog. Without it General is left out, as in the tag picker, whose All
/// stands for it.
pub fn rail_groups(namespaces: &[TagNamespaceRecord], general: bool) -> Vec<TagNamespaceRecord> {
    let mut groups = namespaces
        .iter()
        .filter(|group| general || group_of(&group.name).is_some())
        .cloned()
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| group_order(&group.name));
    groups
}

/// Returns the records of the Selected view: the tags of the items, given by
/// name, sorted by group and then by name. The two tag browsers put the tags
/// without a group in different places. The Add Tags dialog puts them before
/// every group, and `general_first` selects that order. The tag picker files
/// them under General, which sorts among the other groups by its name.
pub fn records_by_group<'a>(
    names: impl IntoIterator<Item = &'a String>,
    general_first: bool,
) -> Vec<TagRecord> {
    let mut records = names
        .into_iter()
        .map(|name| record(name))
        .collect::<Vec<_>>();
    let group = |record: &TagRecord| {
        let name = record.namespace.to_lowercase();
        if general_first && name == "general" {
            String::new()
        } else {
            name
        }
    };
    records.sort_by(|left, right| {
        group(left)
            .cmp(&group(right))
            .then_with(|| crate::text::natural_order(&left.subname, &right.subname))
    });
    records
}

/// Reads the tag groups of the library with their counts, and gives them to
/// `then` on the view once they are read.
pub fn read_groups<V: 'static>(
    session: &Entity<Session>,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, Vec<TagNamespaceRecord>, &mut Context<V>) + 'static,
) {
    let answer = session.read(cx).run(|library| library.tag_namespaces());
    cx.spawn(async move |view, cx| {
        if let Ok(Ok(namespaces)) = answer.await {
            let _ = view.update(cx, |view, cx| then(view, namespaces, cx));
        }
    })
    .detach();
}

// The types and functions in this section load a listing of tags page by
// page.

/// One page of a listing. The listing is a search within a group, or every
/// tag.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    generation: u64,
    pub query: String,
    pub group: Option<String>,
    offset: usize,
    size: usize,
}

/// The answer for a page: its tags, and the offset at which the next page
/// starts, if there is a next page.
pub type Answer = Result<(Vec<TagRecord>, Option<usize>), String>;

impl Page {
    /// Reads the page from the library's sorted list of tags.
    pub fn fetch(&self, library: &Library) -> Answer {
        let search = self.query.trim();
        library
            .tag_page(
                self.group.as_deref(),
                (!search.is_empty()).then_some(search),
                self.offset,
                self.size,
            )
            .map(|(tags, next, _)| (tags, next))
            .map_err(|error| error.to_string())
    }
}

/// Holds the tags that a browser has loaded of its listing, page by page.
/// When a new listing is started, every answer that is still on its way for
/// an older listing is ignored. Each tag is listed once.
#[derive(Debug)]
pub struct TagPager {
    pub tags: Vec<TagRecord>,
    /// The offset at which the next page starts, while there is a next page.
    pub cursor: Option<usize>,
    pub loading_more: bool,
    /// The search and the group of the listing that is shown, once its first
    /// page has arrived.
    pub settled: Option<(String, Option<String>)>,
    asked: Option<(String, Option<String>)>,
    generation: u64,
    size: usize,
}

impl TagPager {
    /// Creates a pager whose pages hold `size` tags.
    pub fn new(size: usize) -> Self {
        Self {
            tags: Vec::new(),
            cursor: None,
            loading_more: false,
            settled: None,
            asked: None,
            generation: 0,
            size,
        }
    }

    /// Starts a new listing and returns its first page, for the caller to ask
    /// the library for.
    pub fn begin(&mut self, query: &str, group: Option<String>) -> Page {
        self.generation += 1;
        self.loading_more = false;
        self.asked = Some((query.to_owned(), group.clone()));
        Page {
            generation: self.generation,
            query: query.to_owned(),
            group,
            offset: 0,
            size: self.size,
        }
    }

    /// Returns whether `page` belongs to the listing that was asked for last.
    pub fn current(&self, page: &Page) -> bool {
        page.generation == self.generation
    }

    /// Replaces what was loaded with the first page of a listing, unless a
    /// newer listing was asked for since. Returns true when the page was
    /// taken.
    pub fn first(&mut self, page: &Page, tags: Vec<TagRecord>, next: Option<usize>) -> bool {
        if !self.current(page) {
            return false;
        }
        self.tags = merge_unique(Vec::new(), tags);
        self.cursor = next;
        self.settled = Some((page.query.clone(), page.group.clone()));
        true
    }

    /// Returns the page after those loaded. Returns `None` when there is no
    /// further page, when a page is on its way, or when what is shown is not
    /// the listing that was asked for last.
    pub fn next(&mut self) -> Option<Page> {
        let offset = self.cursor.filter(|_| !self.loading_more)?;
        let (query, group) = self
            .settled
            .clone()
            .filter(|_| self.settled == self.asked)?;
        self.loading_more = true;
        Some(Page {
            generation: self.generation,
            query,
            group,
            offset,
            size: self.size,
        })
    }

    /// Adds a further page to what is loaded, unless the listing changed
    /// since the page was asked for. Returns true when the answer was for the
    /// current listing.
    pub fn more(&mut self, page: &Page, answer: Answer) -> bool {
        if !self.current(page) {
            return false;
        }
        self.loading_more = false;
        if let Ok((tags, next)) = answer {
            self.tags = merge_unique(std::mem::take(&mut self.tags), tags);
            // A next page that starts where this page started would be asked
            // for again and again, so the listing ends here.
            self.cursor = next.filter(|next| *next != page.offset);
        }
        true
    }
}

/// Asks the library for `page`, after `delay` when one is given, and gives
/// the answer to `then` on the view if the view still exists. Dropping the
/// returned task cancels the request.
pub fn fetch_page<V: 'static>(
    session: &Entity<Session>,
    page: Page,
    delay: Option<Duration>,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, Page, Answer, &mut Context<V>) + 'static,
) -> Task<()> {
    let session = session.clone();
    cx.spawn(async move |view, cx| {
        if let Some(delay) = delay {
            cx.background_executor().timer(delay).await;
        }
        let job = page.clone();
        let answer = session.read_with(cx, |session, _| {
            session.run(move |library| job.fetch(library))
        });
        let Ok(answer) = answer.await else { return };
        let _ = view.update(cx, |view, cx| then(view, page, answer, cx));
    })
}

/// Returns `current` with the tags of `incoming` added after it. Each tag is
/// listed once: a tag that `current` already lists is replaced where it
/// stands.
pub fn merge_unique(mut current: Vec<TagRecord>, incoming: Vec<TagRecord>) -> Vec<TagRecord> {
    let mut position = current
        .iter()
        .enumerate()
        .map(|(index, tag)| (tag.tag_id, index))
        .collect::<std::collections::HashMap<_, _>>();
    for tag in incoming {
        match position.get(&tag.tag_id) {
            Some(index) => current[*index] = tag,
            None => {
                position.insert(tag.tag_id, current.len());
                current.push(tag);
            }
        }
    }
    current
}

// This section builds the rows that every tag context menu starts with, and
// runs them.

/// Returns the rows that every tag context menu starts with: Filter, Star,
/// the group moves when `groups` is true (into each other group of
/// `namespaces`, and out of the tag's own group), then Copy. The actions of
/// these rows are `filter`, `star` or `unstar`, `move-to-group` (with the
/// group as the value), `remove-from-group` and `copy`. A caller appends its
/// own rows.
pub fn common_menu(
    tag: &TagRecord,
    namespaces: &[TagNamespaceRecord],
    groups: bool,
    cx: &App,
) -> Vec<menu::Entry> {
    let starred =
        Preferences::from_settings(crate::settings::current(cx)).is_starred(&record_name(tag));
    let current = group_of(&tag.namespace);
    let mut entries: Vec<menu::Entry> = vec![
        // `menu-filter` is the magnifier icon with the stroke width of the
        // menu rows, 1.25. `menu-search` is the same magnifier with the
        // stroke width of the search field, 1.5.
        menu::item("filter", "Filter Items with This Tag")
            .icon("menu-filter")
            .disabled(tag.tag_id.0 == 0)
            .into(),
        if starred {
            menu::item("unstar", "Remove from Starred").icon("menu-star-off")
        } else {
            menu::item("star", "Add to Starred").icon("menu-star")
        }
        .into(),
    ];
    if groups {
        let mut targets = namespaces
            .iter()
            .filter_map(|namespace| group_of(&namespace.name))
            .filter(|group| Some(group) != current.as_ref())
            .collect::<Vec<_>>();
        targets.sort_by(|left, right| crate::text::natural_order(left, right));
        targets.dedup();
        if !targets.is_empty() {
            entries.push(
                menu::submenu(
                    if current.is_some() {
                        t("Move to Group…")
                    } else {
                        t("Add to Group…")
                    },
                    targets
                        .into_iter()
                        .map(|group| {
                            menu::item("move-to-group", group.clone())
                                .icon("menu-bookmark")
                                .value(group)
                                .into()
                        })
                        .collect(),
                )
                .icon("menu-bookmarks")
                .into(),
            );
        }
        if current.is_some() {
            entries.push(
                menu::item("remove-from-group", "Remove from this Group")
                    .icon("menu-bookmark")
                    .into(),
            );
        }
    }
    entries.push(menu::Entry::Separator);
    entries.push(menu::item("copy", "Copy Tag").icon("menu-copy").into());
    entries
}

/// Says what a row of [`common_menu`] leaves for the view to do, after
/// [`run_common`] has done the part that is the same in every view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Common {
    /// Nothing is left to do: the tag was starred, unstarred or copied.
    Done,
    /// Show the items that have the tag.
    Filter,
    /// Move the tag into another group, or out of its own group. The value is
    /// the tag's new name, for [`rename_or_merge`].
    Move(String),
}

/// Runs a row of [`common_menu`]. It stars, unstars or copies the tag itself,
/// and returns what else the row asks of the view. Returns `None` for a row
/// that the view added itself.
pub fn run_common(
    action: &str,
    value: Option<SharedString>,
    tag: &TagRecord,
    cx: &mut App,
) -> Option<Common> {
    let name = record_name(tag);
    Some(match action {
        "filter" => Common::Filter,
        "star" | "unstar" => {
            set_starred(&name, action == "star", cx);
            Common::Done
        }
        "copy" => {
            cx.write_to_clipboard(ClipboardItem::new_string(name));
            Common::Done
        }
        "move-to-group" => Common::Move(name_in_group(&tag.subname, Some(value?.as_ref()))),
        "remove-from-group" => Common::Move(name_in_group(&tag.subname, None)),
        _ => return None,
    })
}

/// Renames a tag, or merges it into the tag that already has the name.
pub fn rename_or_merge(library: &Library, id: TagId, name: &str) -> Result<(), String> {
    let name = name.trim();
    let target = library
        .projections()
        .snapshot()
        .tag_ids_by_name
        .get(name)
        .copied();
    match target {
        Some(target) if target != id => library.merge_tags(id, target),
        _ => library.rename_tag(id, name),
    }
    .map(|_| ())
    .map_err(|error| error.to_string())
}

/// Returns the tag search that a
/// [`TagTokenInput`](crate::ui::tag_token_input) offers: the first twelve
/// tags of the library that match what was typed.
pub fn token_search(
    session: gpui::Entity<crate::session::Session>,
) -> crate::ui::tag_token_input::Search {
    std::rc::Rc::new(move |query, cx| {
        session.read(cx).run(move |library| {
            library
                .tag_page(None, Some(&query), 0, 12)
                .map(|(tags, _, _)| {
                    tags.into_iter()
                        .map(|tag| crate::ui::tag_token_input::Found {
                            namespace: tag.namespace,
                            subname: tag.subname,
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
    })
}

// This section holds the tag preferences that are kept in the library's
// application settings.

/// The tag preferences in the library's application settings: the starred
/// tags, by name, and the colours chosen for groups.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preferences {
    pub starred: Vec<String>,
    pub colors: BTreeMap<String, String>,
}

impl Preferences {
    /// Reads the preferences from the settings model.
    pub fn from_settings(settings: &crate::settings::model::AppSettings) -> Self {
        Self {
            starred: settings.starred_tags.clone(),
            colors: settings
                .tag_group_colors
                .iter()
                .filter_map(|(group, color)| Some((group.clone(), color.as_str()?.to_owned())))
                .collect(),
        }
    }

    /// Returns the group colours in the form in which the settings model
    /// stores them.
    pub fn colors_json(&self) -> serde_json::Map<String, serde_json::Value> {
        self.colors
            .iter()
            .map(|(group, color)| (group.clone(), serde_json::Value::from(color.clone())))
            .collect()
    }

    pub fn is_starred(&self, name: &str) -> bool {
        self.starred.iter().any(|starred| starred == name)
    }

    /// Returns the starred list with `name` added, and then sorted, or with
    /// `name` removed.
    pub fn with_star(&self, name: &str, starred: bool) -> Vec<String> {
        let mut tags = self.starred.clone();
        tags.retain(|tag| tag != name);
        if starred {
            tags.push(name.to_owned());
            tags.sort();
        }
        tags
    }

    /// Returns the starred list with the new name of a renamed tag in place
    /// of the old name, or `None` when the old name was not starred.
    pub fn renamed_star(&self, previous: &str, next: &str) -> Option<Vec<String>> {
        if previous == next || !self.is_starred(previous) {
            return None;
        }
        let mut tags = self
            .starred
            .iter()
            .filter(|tag| *tag != previous)
            .cloned()
            .chain([next.to_owned()])
            .collect::<Vec<_>>();
        tags.sort();
        tags.dedup();
        Some(tags)
    }

    /// Returns the group colours with the colour of `group` set to `color`,
    /// or removed when `color` is `None`.
    pub fn with_color(&self, group: &str, color: Option<&str>) -> BTreeMap<String, String> {
        let mut colors = self.colors.clone();
        match color {
            Some(color) => colors.insert(group.to_owned(), color.to_owned()),
            None => colors.remove(group),
        };
        colors
    }

    /// Returns the group colours with the colour of a renamed group moved to
    /// the group's new name, or `None` when the group had no colour.
    pub fn renamed_color(&self, previous: &str, next: &str) -> Option<BTreeMap<String, String>> {
        if previous == next {
            return None;
        }
        let color = self.colors.get(previous)?;
        let mut colors = self.colors.clone();
        colors.insert(next.to_owned(), color.clone());
        colors.remove(previous);
        Some(colors)
    }
}

/// Stores a starred list through the settings model, which writes it to the
/// library's application settings and emits `Applied`, so that the app
/// follows the change (`SettingsModel::apply_now`).
fn store_starred(starred: Vec<String>, cx: &mut App) {
    crate::settings::model(cx).update(cx, |model, cx| {
        model.apply_now(|app| app.starred_tags = starred.clone(), cx)
    });
}

/// Stars the tag named `name`, or removes its star.
pub fn set_starred(name: &str, starred: bool, cx: &mut App) {
    let next = Preferences::from_settings(crate::settings::current(cx)).with_star(name, starred);
    store_starred(next, cx);
}

/// Moves the star of a renamed tag to the tag's new name.
pub fn rename_star(previous: &str, next: &str, cx: &mut App) {
    let preferences = Preferences::from_settings(crate::settings::current(cx));
    if let Some(starred) = preferences.renamed_star(previous, next) {
        store_starred(starred, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn group(name: &str) -> TagNamespaceRecord {
        TagNamespaceRecord {
            namespace_id: TagNamespaceId(0),
            name: name.into(),
            tag_count: 1,
        }
    }

    #[test]
    fn tab_steps_through_all_the_groups_selected_and_the_last_view() {
        let groups = ["creator".to_owned(), "series".to_owned()];
        for last in [View::Recent, View::Starred] {
            let mut view = View::All;
            let mut seen = Vec::new();
            for _ in 0..5 {
                view = next_view(&view, &groups, &last);
                seen.push(view.clone());
            }
            assert_eq!(
                seen,
                [
                    View::Group("creator".into()),
                    View::Group("series".into()),
                    View::Selected,
                    last.clone(),
                    View::All
                ]
            );
            assert_eq!(next_view(&View::All, &[], &last), View::Selected);
        }
        assert_eq!(View::Group("x".into()).group().as_deref(), Some("x"));
        assert_eq!(View::Selected.group(), None);
    }

    #[test]
    fn the_rails_list_the_groups_in_their_order_with_general_last_or_not_at_all() {
        let namespaces = ["character", "creator", "", "rating", "series", "species"].map(group);
        let names = |general| {
            rail_groups(&namespaces, general)
                .into_iter()
                .map(|group| group.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(true),
            ["creator", "series", "character", "species", "rating", ""]
        );
        assert_eq!(
            names(false),
            ["creator", "series", "character", "species", "rating"]
        );
    }

    #[test]
    fn the_selected_view_lists_by_group_then_name() {
        let names = ["zeta", "series:x", "alpha", "creator:b", "item10", "item9"].map(String::from);
        // The picker's order: General sorts among the groups, by its name.
        let records = records_by_group(&names, false);
        assert_eq!(
            records.iter().map(record_name).collect::<Vec<_>>(),
            ["creator:b", "alpha", "item9", "item10", "zeta", "series:x"]
        );
        // The dialog's order: tags without a group come before every group.
        let records = records_by_group(&names, true);
        assert_eq!(
            records.iter().map(record_name).collect::<Vec<_>>(),
            ["alpha", "item9", "item10", "zeta", "creator:b", "series:x"]
        );
    }

    #[test]
    fn the_common_rows_copy_here_and_leave_filter_and_moves_to_the_view() {
        let cx = gpui::TestAppContext::single();
        let cat = tag(4, "species", "cat");
        cx.update(|cx| {
            assert_eq!(run_common("filter", None, &cat, cx), Some(Common::Filter));
            let group = Some(SharedString::from("creator"));
            assert_eq!(
                run_common("move-to-group", group, &cat, cx),
                Some(Common::Move("creator:cat".into()))
            );
            assert_eq!(
                run_common("remove-from-group", None, &cat, cx),
                Some(Common::Move("cat".into()))
            );
            assert_eq!(run_common("move-to-group", None, &cat, cx), None);
            assert_eq!(run_common("copy", None, &cat, cx), Some(Common::Done));
            let copied = cx.read_from_clipboard().and_then(|item| item.text());
            assert_eq!(copied.as_deref(), Some("species:cat"));
            // A row that a view added itself is not run here.
            assert_eq!(run_common("edit", None, &cat, cx), None);
        });
    }

    #[test]
    fn a_pager_keeps_the_newest_listing_and_lists_each_tag_once() {
        let mut pager = TagPager::new(2);
        let old = pager.begin("ca", None);
        let page = pager.begin("cat", Some("species".into()));
        assert_eq!(
            (page.query.as_str(), page.group.as_deref()),
            ("cat", Some("species"))
        );
        // An answer for the older listing is ignored.
        assert!(!pager.first(&old, vec![tag(9, "", "x")], Some(2)));
        assert!(pager.tags.is_empty() && pager.settled.is_none());
        assert!(pager.first(
            &page,
            vec![tag(1, "species", "cat"), tag(2, "species", "cats")],
            Some(2)
        ));
        assert_eq!(pager.settled, Some(("cat".into(), Some("species".into()))));
        // Only one further page is asked for at a time.
        let more = pager.next().unwrap();
        assert!(pager.loading_more && pager.next().is_none());
        assert!(pager.more(
            &more,
            Ok((
                vec![tag(2, "species", "cats!"), tag(3, "species", "catx")],
                Some(4)
            ))
        ));
        assert_eq!(
            pager
                .tags
                .iter()
                .map(|tag| tag.subname.as_str())
                .collect::<Vec<_>>(),
            ["cat", "cats!", "catx"]
        );
        assert_eq!(pager.cursor, Some(4));
        // A next page that starts where the page before it started ends the
        // listing.
        let stuck = pager.next().unwrap();
        assert!(pager.more(&stuck, Ok((Vec::new(), Some(4)))));
        assert_eq!(pager.cursor, None);
        assert!(pager.next().is_none());
    }

    #[test]
    fn a_pager_asks_for_no_more_until_the_new_listing_is_in() {
        let mut pager = TagPager::new(2);
        let page = pager.begin("", None);
        assert!(pager.first(&page, vec![tag(1, "", "a")], Some(2)));
        let more = pager.next().unwrap();
        // The search changed while the page was on its way.
        let fresh = pager.begin("b", None);
        assert!(!pager.loading_more && pager.next().is_none());
        assert!(!pager.more(&more, Ok((vec![tag(2, "", "b")], None))));
        assert!(pager.first(&fresh, vec![tag(2, "", "b")], None));
        assert!(pager.next().is_none());
    }

    #[test]
    fn names_leave_out_the_general_group() {
        assert_eq!(name("", "cat"), "cat");
        assert_eq!(name("General", "cat"), "cat");
        assert_eq!(name(" Creator ", "someone"), "creator:someone");
        // `default` is a group of its own in a name, although a chip does not
        // show it.
        assert_eq!(name("default", "cat"), "default:cat");
        assert_eq!(name_in_group("cat", Some("species")), "species:cat");
        assert_eq!(name_in_group("cat", None), "cat");
        assert_eq!(split("cat"), (None, "cat"));
        assert_eq!(split("species:cat:x"), (Some("species"), "cat:x"));
        assert!(!chip_shows_namespace("default") && chip_shows_namespace("species"));
    }

    #[test]
    fn a_tag_known_by_name_without_a_group_is_in_general() {
        let cat = record("cat");
        assert_eq!(cat.namespace, "general");
        assert_eq!(cat.tag_id, TagId(0));
        // Its full name is still the name without a group, so it is found,
        // starred and shown by that name.
        assert_eq!(record_name(&cat), "cat");
        assert!(!chip_shows_namespace(&cat.namespace));
        let cat = record("species:cat:x");
        assert_eq!(
            (cat.namespace.as_str(), cat.subname.as_str()),
            ("species", "cat:x")
        );
        assert_eq!(record_name(&cat), "species:cat:x");
    }

    #[test]
    fn chips_know_more_namespaces_than_the_group_presets() {
        assert_eq!(chip_color("studio"), 0xff6667);
        assert_eq!(chip_color("Person"), 0x30d159);
        assert_eq!(chip_color("system"), 0xff9f0a);
        assert_eq!(chip_color("meta"), 0xbdbec0);
        let preferences = Preferences::default();
        assert_eq!(group_color("studio", &preferences), 0xbdbec0);
        assert_eq!(group_color("system", &preferences), 0xbdbec0);
    }

    #[test]
    fn groups_take_their_preset_colour_and_order_unless_one_was_chosen() {
        let mut preferences = Preferences::default();
        assert_eq!(group_color("character", &preferences), 0x30d159);
        assert_eq!(group_color("anything", &preferences), 0xbdbec0);
        preferences
            .colors
            .insert("character".into(), "#FA5252".into());
        assert_eq!(group_color("character", &preferences), 0xfa5252);
        assert_eq!(group_order("creator"), 0);
        assert_eq!(group_order("rating"), 4);
        assert_eq!(group_order("meta"), 10);
    }

    #[test]
    fn the_inspector_lists_general_tags_after_every_named_group() {
        assert!(inspector_order("creator") < inspector_order("series"));
    }

    #[test]
    fn tags_by_id_come_by_group_then_name_ignoring_case() {
        let tag = |id: u32, namespace: &str, subname: &str| TagRecord {
            tag_id: picto_library::TagId(id),
            namespace_id: picto_library::TagNamespaceId(0),
            namespace: namespace.into(),
            subname: subname.into(),
            active_count: 0,
            assignment_count: 0,
        };
        let mut tags = vec![
            tag(1, "species", "humanoid"),
            tag(2, "general", "vulva"),
            tag(3, "rating", "explicit"),
            tag(4, "general", "Anus"),
            tag(5, "general", "big_butt"),
            tag(6, "general", "anus"),
        ];
        super::sort_as_library(&mut tags);
        let order = tags.iter().map(|tag| tag.tag_id.0).collect::<Vec<_>>();
        assert_eq!(order, [4, 6, 5, 2, 3, 1]);
        assert!(inspector_order("rating") < inspector_order("somethingelse"));
        assert!(inspector_order("somethingelse") < inspector_order("general"));
        assert_eq!(inspector_order(""), inspector_order("General"));
    }

    #[test]
    fn preferences_read_star_and_rename_as_the_settings_store_them() {
        let settings = crate::settings::model::AppSettings::from_json(&serde_json::json!({
            "starredTags": ["b", "a"],
            "tagGroupColors": {"species": "#40C057"},
        }));
        let preferences = Preferences::from_settings(&settings);
        assert!(preferences.is_starred("a"));
        assert_eq!(preferences.with_star("c", true), ["a", "b", "c"]);
        assert_eq!(preferences.with_star("b", false), ["a"]);
        assert_eq!(
            preferences.renamed_star("b", "species:b"),
            Some(vec!["a".to_owned(), "species:b".to_owned()])
        );
        assert_eq!(preferences.renamed_star("x", "y"), None);
        let moved = preferences.renamed_color("species", "animal").unwrap();
        assert_eq!(moved.get("animal").map(String::as_str), Some("#40C057"));
        assert!(!moved.contains_key("species"));
        assert!(preferences.with_color("species", None).is_empty());
        assert_eq!(
            preferences.colors_json().get("species"),
            Some(&serde_json::json!("#40C057"))
        );
    }
}
