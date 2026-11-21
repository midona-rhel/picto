//! The Quick Search panel. It is a panel in the middle of the main window,
//! over a dimmed backdrop, with a search field, a row of tabs and a list.
//! The user types a name and chooses a row with the keyboard or the pointer.
//! The structure and the measurements follow the quick-search panel of the
//! Eagle app. The Electron app has no such panel.
//!
//! The list holds four kinds of rows, and each kind has a tab. A fifth tab,
//! All, comes first and lists the best matches of every kind in groups.
//!
//! - **Actions** are the commands of the shortcut registry that can run in
//!   the window's current state (`sources::actions`). Choosing one gives the
//!   keyboard focus back to where it was before the panel opened and then
//!   dispatches the command's action there, as the application menu does.
//! - **Places** are the sidebar's destinations and the subscriptions.
//!   Choosing one opens it, as a click in the sidebar does.
//! - **Tags** are the library's tags. Choosing one opens the All destination
//!   filtered to the items that have the tag, as "Show items" on the Tags
//!   screen does.
//! - **Items** are the library's files and collections, found by the search
//!   that the grid's search field uses. Choosing one opens the destination
//!   that lists the items with that item's media
//!   (`Session::open_media_matches`) and selects the item there. A
//!   collection is then opened in the viewer.
//!
//! The actions, the places and the tags are lists in memory. They are
//! searched on a background thread each time the text changes
//! (`matching::search`). The items are searched by the library on the
//! library's thread, 50 ms after the last change of the text. An answer that
//! arrives after the text has changed again is dropped. Nothing is searched
//! on the thread that draws.
//!
//! While the field is empty, each tab lists the choices made most recently
//! first. They are kept for each library in the state file
//! (`State::quick_search_recent`).
mod matching;
mod sources;
mod view;

use crate::backend;
use crate::grid::thumbnails::Thumbnails;
use crate::i18n::{en, t};
use crate::session::{Change, Session};
use crate::ui::text_input::{self, TextInput};
use gpui::*;
use matching::Hits;
use picto_library::RootId;
use picto_library::predicate::{FilterExpr, ViewQuerySpec};
use picto_library::query::{ItemScope, RootQuery, RootSummary, WindowRequest};
use serde_json::Value;
use sources::{Entry, Target};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// A tab of the panel, and the kind of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    All,
    Actions,
    Places,
    Tags,
    Items,
}

impl Kind {
    /// The tabs, from left to right.
    pub const TABS: [Kind; 5] = [
        Kind::All,
        Kind::Actions,
        Kind::Places,
        Kind::Tags,
        Kind::Items,
    ];

    /// Returns the name on the kind's tab and above the kind's group in the
    /// All tab.
    pub fn label(self) -> &'static str {
        match self {
            Kind::All => t("All"),
            Kind::Actions => en("Actions"),
            Kind::Places => en("Places"),
            Kind::Tags => t("Tags"),
            Kind::Items => t("Items"),
        }
    }
}

/// The largest number of rows that a tab of one kind lists.
const LIMIT: usize = 200;
/// The largest number of items that one search asks the library for.
const ITEM_LIMIT: usize = 50;
/// The largest number of rows of each of Actions, Places and Tags in the All
/// tab.
const GROUP_LIMIT: usize = 5;
/// The largest number of items in the All tab. An item's row is as tall as
/// three and a half other rows.
const ITEM_GROUP_LIMIT: usize = 3;
/// The largest number of recent choices at the top of the All tab.
const RECENT_GROUP_LIMIT: usize = 6;
/// The number of recent choices that are remembered for a library.
const REMEMBERED: usize = 30;
/// The time between the last change of the text and the search of the
/// library's items.
const ITEM_SEARCH_DELAY: Duration = Duration::from_millis(50);

/// The three lists that are held in memory.
#[derive(Clone, Default)]
struct Lists {
    actions: Arc<Vec<Entry>>,
    places: Arc<Vec<Entry>>,
    tags: Arc<Vec<Entry>>,
}

/// The matches of one query in the three lists.
#[derive(Clone, Default)]
struct Found {
    actions: Hits,
    places: Hits,
    tags: Hits,
}

impl Found {
    /// Searches the three lists for `query`. `recent` holds the keys of the
    /// recent choices, the newest first.
    fn search(lists: &Lists, query: &str, recent: &[String]) -> Found {
        // The age of each recent choice by its key: 0 for the newest.
        let ages: HashMap<&str, usize> = recent
            .iter()
            .enumerate()
            .map(|(age, key)| (key.as_str(), age))
            .collect();
        let one = |list: &[Entry]| {
            // The positions in this list of the recent choices, the newest
            // first. They are needed only while the field is empty.
            let mut positions: Vec<(usize, usize)> = if query.trim().is_empty() && !ages.is_empty()
            {
                list.iter()
                    .enumerate()
                    .filter_map(|(index, entry)| Some((*ages.get(entry.key.as_str())?, index)))
                    .collect()
            } else {
                Vec::new()
            };
            positions.sort_unstable();
            let recent: Vec<usize> = positions.into_iter().map(|(_, index)| index).collect();
            matching::search(
                list.iter().map(|entry| entry.label.as_str()),
                query,
                &recent,
                LIMIT,
            )
        };
        Found {
            actions: one(&lists.actions),
            places: one(&lists.places),
            tags: one(&lists.tags),
        }
    }
}

/// The items that the library found for the text in the field.
#[derive(Default)]
enum Items {
    /// No answer has arrived yet.
    #[default]
    Waiting,
    /// The text is one or two characters long. The library's search needs
    /// three, so no search was made.
    TooShort,
    /// The library's answer: the rows, each with the matched letters of its
    /// name, and the number of items that match in the whole library.
    Found {
        rows: Vec<(Entry, Vec<Range<usize>>)>,
        total: u64,
    },
    /// The search failed, with the library's message.
    Failed(String),
}

/// One line of the list.
pub enum Row {
    /// The name of a group of rows.
    Header(SharedString),
    /// A row that can be chosen. `ranges` are the matched letters of the
    /// row's name.
    Entry {
        entry: Entry,
        ranges: Vec<Range<usize>>,
    },
}

/// What the panel holds while it is open.
struct Open {
    /// When the panel opened. The panel fades in from this moment.
    opened: Instant,
    /// The element that had the keyboard focus before the panel opened.
    back: Option<FocusHandle>,
    /// True until the first frame of the panel has given the keyboard focus
    /// to the search field.
    focus_field: bool,
    kind: Kind,
    /// The text of the search field that the lists were last searched for.
    query: String,
    lists: Lists,
    /// The action of each row of `lists.actions`, at the position that the
    /// row's `Target::Action` holds.
    actions: Vec<Box<dyn Action>>,
    /// The recent choices of this library, the newest first, as the state
    /// file holds them.
    recent: Vec<Value>,
    found: Found,
    items: Items,
    /// The lines that the list shows, for `kind`, `found` and `items`.
    rows: Vec<Row>,
    /// The position in `rows` of the highlighted row.
    highlighted: Option<usize>,
    /// The number of the current search. It grows by one each time the text
    /// changes, and an answer for an earlier number is dropped.
    generation: u64,
    /// The number of the newest search of the three lists. It grows by one
    /// each time such a search starts, and the answer of an earlier search
    /// is dropped.
    list_search: u64,
    /// The search of the library's items that is waiting or running.
    /// Replacing it cancels a search that has not started yet.
    item_search: Option<Task<()>>,
    /// Where the pointer was when it last moved over a row. A row is
    /// highlighted for the pointer only when the pointer really moved, so
    /// that the list scrolling under a resting pointer does not move the
    /// highlight.
    pointer: Option<Point<Pixels>>,
}

/// An item that was chosen and that the grid has not listed yet.
struct Reveal {
    root: RootId,
    collection: bool,
}

pub struct QuickSearch {
    session: Entity<Session>,
    thumbnails: Entity<Thumbnails>,
    /// The search field.
    input: Entity<TextInput>,
    /// The keyboard focus of the panel while the search field does not have
    /// it, for example during a click on a tab.
    focus: FocusHandle,
    /// The scroll position of the list.
    scroll: ScrollHandle,
    open: Option<Open>,
    /// The number of the newest search of the library's items. The work on
    /// the library's thread reads it and skips a search that is no longer
    /// the newest.
    newest_item_search: Arc<AtomicU64>,
    /// The item to select once the grid lists it.
    reveal: Option<Reveal>,
    _subscriptions: Vec<Subscription>,
}

impl QuickSearch {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
        cx: &mut Context<Self>,
    ) -> Self {
        let input =
            cx.new(|cx| TextInput::new(text_input::Kind::Form, cx).placeholder(t("Search...")));
        let subscriptions = vec![
            cx.observe(&input, |this, input, cx| {
                let text = input.read(cx).value();
                this.typed(text, cx);
            }),
            cx.subscribe(&session, |this, _, change: &Change, cx| {
                if matches!(change, Change::Results | Change::Navigation) {
                    this.reveal_chosen_item(cx);
                }
            }),
        ];
        Self {
            session,
            thumbnails,
            input,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            open: None,
            newest_item_search: Arc::new(AtomicU64::new(0)),
            reveal: None,
            _subscriptions: subscriptions,
        }
    }

    /// Opens the panel on the tab `kind`. When the panel is already open, it
    /// only changes the tab.
    ///
    /// The list of actions is made here, before the panel takes the keyboard
    /// focus, because which commands can run depends on the element that has
    /// the focus.
    pub fn open(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.is_some() {
            self.show_kind(kind, cx);
            return;
        }
        let (actions, performed) = sources::actions(window, cx);
        let (places, recent) = {
            let session = self.session.read(cx);
            let recent = crate::state::get(cx)
                .quick_search_recent
                .get(&crate::state::library_key(session.root()))
                .cloned()
                .unwrap_or_default();
            (sources::places(session), recent)
        };
        // The tags and the subscriptions are added when they have been
        // read (`read_tags`, `read_subscriptions`).
        let lists = Lists {
            actions: Arc::new(actions),
            places: Arc::new(places),
            tags: Arc::new(Vec::new()),
        };
        let found = Found::search(&lists, "", &recent_keys(&recent));
        self.input.update(cx, |input, cx| input.clear(cx));
        self.scroll.set_offset(point(px(0.), px(0.)));
        let mut open = Open {
            opened: Instant::now(),
            back: window.focused(cx),
            focus_field: true,
            kind,
            query: String::new(),
            lists,
            actions: performed,
            recent,
            found,
            items: Items::Waiting,
            rows: Vec::new(),
            highlighted: None,
            generation: 0,
            list_search: 0,
            item_search: None,
            pointer: None,
        };
        open.rebuild(None);
        self.open = Some(open);
        self.search_items(cx);
        self.read_tags(cx);
        self.read_subscriptions(cx);
        cx.notify();
    }

    /// Closes the panel. The keyboard focus goes back to the element that
    /// had it before the panel opened, when the panel still has the focus.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.open.take() else {
            return;
        };
        self.give_focus_back(open.back.as_ref(), window, cx);
        self.thumbnails.update(cx, |thumbnails, _| {
            for holder in view::PINS {
                thumbnails.pin(holder, None);
            }
        });
        cx.notify();
    }

    /// Gives the keyboard focus to `back` when the panel or its search field
    /// has the focus now, or when nothing has it.
    fn give_focus_back(&self, back: Option<&FocusHandle>, window: &mut Window, cx: &mut App) {
        let in_panel = self.focus.contains_focused(window, cx)
            || self.input.focus_handle(cx).contains_focused(window, cx);
        if let Some(back) = back
            && (in_panel || window.focused(cx).is_none())
        {
            window.focus(back, cx);
        }
    }

    /// Shows the tab `kind` and highlights its first row.
    fn show_kind(&mut self, kind: Kind, cx: &mut Context<Self>) {
        let Some(open) = &mut self.open else {
            return;
        };
        if open.kind != kind {
            open.kind = kind;
            open.rebuild(None);
            self.scroll.set_offset(point(px(0.), px(0.)));
            cx.notify();
        }
    }

    /// Shows the tab after the current one, or the tab before it when
    /// `forward` is false. After the last tab comes the first.
    fn step_kind(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(open) = &self.open else {
            return;
        };
        let count = Kind::TABS.len();
        let at = Kind::TABS
            .iter()
            .position(|kind| *kind == open.kind)
            .unwrap_or(0);
        let next = if forward {
            (at + 1) % count
        } else {
            (at + count - 1) % count
        };
        self.show_kind(Kind::TABS[next], cx);
    }

    /// Called when the search field changes. When its text differs from the
    /// text that was last searched for, the three lists are searched again,
    /// and a search of the library's items is scheduled.
    fn typed(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(open) = &mut self.open else {
            return;
        };
        if open.query == text {
            return;
        }
        open.query = text;
        open.generation += 1;
        self.search_lists(true, cx);
        self.search_items(cx);
    }

    /// Searches the three lists for the current text on a background thread
    /// and shows the matches when they arrive. The matches are dropped when
    /// another search of the lists has started in the meantime.
    ///
    /// `first_row` is true when the text has changed. The list then scrolls
    /// to its top and the first row is highlighted. It is false when only a
    /// list has changed, because the tags or the subscriptions have just
    /// been read. The highlight then stays on the row it is on.
    fn search_lists(&mut self, first_row: bool, cx: &mut Context<Self>) {
        let Some(open) = &mut self.open else {
            return;
        };
        open.list_search += 1;
        let number = open.list_search;
        let lists = open.lists.clone();
        let query = open.query.clone();
        let recent = recent_keys(&open.recent);
        let search = cx.background_spawn(async move { Found::search(&lists, &query, &recent) });
        cx.spawn(async move |this, cx| {
            let found = search.await;
            let _ = this.update(cx, |this, cx| {
                let Some(open) = &mut this.open else {
                    return;
                };
                if open.list_search != number {
                    return;
                }
                open.found = found;
                if first_row {
                    open.rebuild(None);
                    this.scroll.set_offset(point(px(0.), px(0.)));
                } else {
                    let keep = open.highlighted_key();
                    open.rebuild(keep);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Schedules a search of the library's items for the current text.
    ///
    /// An empty text asks for the first items of the All destination. A text
    /// of one or two characters asks for nothing, because the library's text
    /// search needs three characters (`backend::text_filter`), as the grid's
    /// search field does. Any other text is searched for 50 ms after it was
    /// typed.
    ///
    /// The search runs on the library's thread (`Session::run`). Before it
    /// reads anything it checks that it is still the newest search, so the
    /// searches for text that the user has already typed over do no work.
    fn search_items(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &mut self.open else {
            return;
        };
        let query = open.query.trim().to_owned();
        let filter = backend::text_filter(&query);
        if !query.is_empty() && filter == FilterExpr::default() {
            open.items = Items::TooShort;
            open.item_search = None;
            return;
        }
        let generation = open.generation;
        self.newest_item_search.store(generation, Ordering::Relaxed);
        let newest = self.newest_item_search.clone();
        let delay = if query.is_empty() {
            Duration::ZERO
        } else {
            ITEM_SEARCH_DELAY
        };
        open.item_search = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let Ok(read) = this.update(cx, |this, cx| {
                this.session.read(cx).run(move |library| {
                    if newest.load(Ordering::Relaxed) != generation {
                        return None;
                    }
                    let query = RootQuery {
                        scope: ItemScope::All,
                        view: ViewQuerySpec {
                            filter,
                            sort: Default::default(),
                        },
                    };
                    Some(
                        library
                            .query_window(&query, &WindowRequest::at(0, ITEM_LIMIT))
                            .map(|window| (window.page.items, window.page.total))
                            .map_err(|error| error.to_string()),
                    )
                })
            }) else {
                return;
            };
            let Ok(Some(answer)) = read.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.items_found(generation, answer, cx));
        }));
    }

    /// Takes the library's answer to the search with the number
    /// `generation`. The answer is dropped when the text has changed since.
    /// The items whose names match the text better come first, and the items
    /// that the library found by their notes or links, whose names do not
    /// match, come last in the library's order.
    fn items_found(
        &mut self,
        generation: u64,
        answer: Result<(Vec<RootSummary>, u64), String>,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = &mut self.open else {
            return;
        };
        if open.generation != generation {
            return;
        }
        open.items = match answer {
            Ok((items, total)) => {
                let mut rows: Vec<(u32, Entry, Vec<Range<usize>>)> = items
                    .iter()
                    .map(|item| {
                        let entry = sources::item(item);
                        match matching::find(&entry.label, &open.query) {
                            Some(found) => (found.rank, entry, found.ranges),
                            None => (u32::MAX, entry, Vec::new()),
                        }
                    })
                    .collect();
                rows.sort_by_key(|(rank, _, _)| *rank);
                Items::Found {
                    rows: rows
                        .into_iter()
                        .map(|(_, entry, ranges)| (entry, ranges))
                        .collect(),
                    total,
                }
            }
            Err(error) => Items::Failed(error),
        };
        let keep = open.highlighted_key();
        open.rebuild(keep);
        cx.notify();
    }

    /// Makes the list of tags on a background thread and searches it when
    /// it is ready. A library can have many thousands of tags, and putting
    /// them in order takes too long for the thread that draws.
    fn read_tags(&mut self, cx: &mut Context<Self>) {
        let records: Vec<_> = self.session.read(cx).tags.values().cloned().collect();
        let made = cx.background_spawn(async move { sources::tags(records) });
        cx.spawn(async move |this, cx| {
            let tags = made.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(open) = &mut this.open {
                    open.lists.tags = Arc::new(tags);
                    this.search_lists(false, cx);
                }
            });
        })
        .detach();
    }

    /// Reads the subscriptions with the core's `subscriptions.list` command
    /// and adds them to the places. The command runs on the application's
    /// runtime. The places are searched again when the answer arrives.
    fn read_subscriptions(&mut self, cx: &mut Context<Self>) {
        let answer = self
            .session
            .read(cx)
            .call("subscriptions.list", serde_json::json!({}));
        cx.spawn(async move |this, cx| {
            let Ok(list) = answer.await else {
                return;
            };
            let found = sources::subscriptions(&list);
            if found.is_empty() {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                if let Some(open) = &mut this.open {
                    let mut places = open.lists.places.as_ref().clone();
                    places.extend(found);
                    open.lists.places = Arc::new(places);
                    this.search_lists(false, cx);
                }
            });
        })
        .detach();
    }

    /// Moves the highlight to the next row that can be chosen, or to the
    /// one before when `forward` is false, and scrolls the list so that the
    /// row is visible. After the last row comes the first.
    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(open) = &mut self.open else {
            return;
        };
        let stops: Vec<usize> = open
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, Row::Entry { .. }))
            .map(|(index, _)| index)
            .collect();
        if stops.is_empty() {
            return;
        }
        let at = open
            .highlighted
            .and_then(|highlighted| stops.iter().position(|stop| *stop == highlighted));
        let next = match (at, forward) {
            (Some(at), true) => (at + 1) % stops.len(),
            (Some(at), false) => (at + stops.len() - 1) % stops.len(),
            (None, true) => 0,
            (None, false) => stops.len() - 1,
        };
        open.highlighted = Some(stops[next]);
        // Scrolling to the first row also shows the group name above it.
        let target = if next == 0 { 0 } else { stops[next] };
        self.scroll.scroll_to_item(target);
        cx.notify();
    }

    /// Highlights the row at `index` because the pointer moved onto it.
    fn point_at(&mut self, index: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(open) = &mut self.open else {
            return;
        };
        if open.pointer == Some(position) {
            return;
        }
        open.pointer = Some(position);
        if open.highlighted != Some(index) {
            open.highlighted = Some(index);
            cx.notify();
        }
    }

    /// Chooses the highlighted row.
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.open.as_ref().and_then(|open| open.highlighted) {
            self.choose(index, window, cx);
        }
    }

    /// Chooses the row at `index`: remembers it as the newest recent choice,
    /// closes the panel and does what the row is for.
    fn choose(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = &mut self.open else {
            return;
        };
        let Some(Row::Entry { entry, .. }) = open.rows.get(index) else {
            return;
        };
        let entry = entry.clone();
        let action = match entry.target {
            Target::Action(at) => open.actions.get(at).map(|action| action.boxed_clone()),
            _ => None,
        };
        self.remember(&entry, cx);
        // Closing gives the keyboard focus back to the element that had it
        // before the panel opened. An action is dispatched from that
        // element, as a row of the application menu is.
        self.close(window, cx);
        match entry.target {
            Target::Action(_) => {
                if let Some(action) = action {
                    window.dispatch_action(action, cx);
                }
            }
            Target::Place(id) => self.session.update(cx, |session, cx| {
                if let Some(index) = session.nav.iter().position(|entry| entry.id() == id) {
                    session.navigate(index, cx);
                }
            }),
            Target::Subscription(id) => self.session.update(cx, |session, cx| {
                if let Some(index) = sources::subscriptions_screen(session) {
                    session.navigate(index, cx);
                    session.open_place(Some(id), cx);
                }
            }),
            Target::Tag(tag) => self.session.update(cx, |session, cx| {
                // The same destination and filter as "Show items" on the
                // Tags screen: the All destination with the tag as the only
                // filter.
                let Some(all) = session.nav.iter().position(|entry| {
                    entry.screen.is_none() && !entry.random && entry.scope == ItemScope::All
                }) else {
                    return;
                };
                let name = session
                    .tags
                    .get(&tag)
                    .map(crate::tags::record_name)
                    .unwrap_or_default();
                let filters = crate::grid::filters::Filters {
                    include_tags: vec![crate::grid::filters::TagChoice { tag_id: tag, name }],
                    ..Default::default()
                };
                session.navigate_with_filters(all, filters, cx);
            }),
            Target::Item { root, collection } => {
                self.reveal = Some(Reveal { root, collection });
                self.session
                    .update(cx, |session, cx| session.open_media_matches(root, cx));
                self.reveal_chosen_item(cx);
            }
        }
    }

    /// Selects the chosen item once the grid lists it, and opens the viewer
    /// when the item is a collection. It is called each time the session
    /// reports new results. When the user has gone to another destination in
    /// the meantime, the item is forgotten.
    fn reveal_chosen_item(&mut self, cx: &mut Context<Self>) {
        let Some(reveal) = &self.reveal else {
            return;
        };
        let (root, collection) = (reveal.root, reveal.collection);
        let wanted = ItemScope::MediaMatches { item_id: root.0 };
        let (there, ordinal) = {
            let session = self.session.read(cx);
            let there = session
                .nav
                .get(session.scope)
                .is_some_and(|entry| entry.screen.is_none() && entry.scope == wanted);
            let listed = there && session.displayed == session.scope && !session.loading;
            (there, listed.then(|| session.position_of(root)).flatten())
        };
        if !there {
            self.reveal = None;
            return;
        }
        if let Some(ordinal) = ordinal {
            self.reveal = None;
            self.session.update(cx, |session, cx| {
                session.select(ordinal, cx);
                if collection {
                    session.open_viewer(cx);
                }
            });
        }
    }

    /// Puts `entry` first in the recent choices of the open library and
    /// keeps the 30 newest.
    fn remember(&mut self, entry: &Entry, cx: &mut Context<Self>) {
        let library = crate::state::library_key(self.session.read(cx).root());
        let record = entry.to_recent();
        let key = entry.key.clone();
        crate::state::update(cx, |state| {
            let recent = state.quick_search_recent.entry(library).or_default();
            recent.retain(|other| Entry::recent_key(other) != Some(key.as_str()));
            recent.insert(0, record);
            recent.truncate(REMEMBERED);
        });
    }
}

/// Returns the keys of the recent choices, the newest first.
fn recent_keys(recent: &[Value]) -> Vec<String> {
    recent
        .iter()
        .filter_map(|record| Entry::recent_key(record).map(str::to_owned))
        .collect()
}

impl Open {
    /// Returns the key of the highlighted row, to keep that row highlighted
    /// when rows arrive later (the subscriptions, the library's items).
    /// Returns `None` while the highlight is still on the first row that can
    /// be chosen, which is where it is put when the text or the tab changes.
    /// The highlight then goes to the first row of the new list, which may
    /// be a row that has just arrived.
    fn highlighted_key(&self) -> Option<String> {
        let highlighted = self.highlighted?;
        let first = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Entry { .. }));
        if first == Some(highlighted) {
            return None;
        }
        match self.rows.get(highlighted)? {
            Row::Entry { entry, .. } => Some(entry.key.clone()),
            Row::Header(_) => None,
        }
    }

    /// Returns the number that the tab of `kind` shows after its name: how
    /// many rows of that kind match the text. The All tab shows no number.
    /// The Items tab shows none before the library has answered, and none
    /// while the text is too short to search the items.
    fn count(&self, kind: Kind) -> Option<u64> {
        match kind {
            Kind::All => None,
            Kind::Actions => Some(self.found.actions.total as u64),
            Kind::Places => Some(self.found.places.total as u64),
            Kind::Tags => Some(self.found.tags.total as u64),
            Kind::Items => match &self.items {
                Items::Found { total, .. } => Some(*total),
                Items::Waiting | Items::TooShort | Items::Failed(_) => None,
            },
        }
    }

    /// Returns the rows of the recent choices that are items, the newest
    /// first.
    fn recent_items(&self) -> Vec<Entry> {
        self.recent
            .iter()
            .filter_map(Entry::item_from_recent)
            .collect()
    }

    /// Makes `rows` again for the current tab, matches and items. The row
    /// with the key `keep` stays highlighted when the new rows have it.
    /// Otherwise the first row that can be chosen is highlighted.
    fn rebuild(&mut self, keep: Option<String>) {
        let empty = self.query.trim().is_empty();
        let mut rows: Vec<Row> = Vec::new();
        let header = |rows: &mut Vec<Row>, label: &str| {
            rows.push(Row::Header(SharedString::from(label.to_owned())));
        };
        let list = |kind: Kind| match kind {
            Kind::Actions => (&self.lists.actions, &self.found.actions),
            Kind::Places => (&self.lists.places, &self.found.places),
            _ => (&self.lists.tags, &self.found.tags),
        };
        let found_items: &[(Entry, Vec<Range<usize>>)] = match &self.items {
            Items::Found { rows, .. } => rows,
            _ => &[],
        };
        match self.kind {
            Kind::All if empty => {
                // The newest choices of every kind, then the first rows of
                // each list that are not among them.
                let mut shown: Vec<String> = Vec::new();
                for record in &self.recent {
                    if shown.len() == RECENT_GROUP_LIMIT {
                        break;
                    }
                    let Some(key) = Entry::recent_key(record) else {
                        continue;
                    };
                    let entry = Entry::item_from_recent(record).or_else(|| {
                        [&self.lists.actions, &self.lists.places, &self.lists.tags]
                            .into_iter()
                            .find_map(|list| list.iter().find(|entry| entry.key == key))
                            .cloned()
                    });
                    if let Some(entry) = entry {
                        if shown.is_empty() {
                            header(&mut rows, t("Recent"));
                        }
                        shown.push(entry.key.clone());
                        rows.push(Row::Entry {
                            entry,
                            ranges: Vec::new(),
                        });
                    }
                }
                for kind in [Kind::Actions, Kind::Places, Kind::Tags] {
                    let (entries, hits) = list(kind);
                    let rest: Vec<&Entry> = hits
                        .hits
                        .iter()
                        .filter_map(|hit| entries.get(hit.index))
                        .filter(|entry| !shown.contains(&entry.key))
                        .take(GROUP_LIMIT)
                        .collect();
                    if !rest.is_empty() {
                        header(&mut rows, kind.label());
                    }
                    rows.extend(rest.into_iter().map(|entry| Row::Entry {
                        entry: entry.clone(),
                        ranges: Vec::new(),
                    }));
                }
            }
            Kind::All => {
                for kind in [Kind::Actions, Kind::Places, Kind::Tags] {
                    let (entries, hits) = list(kind);
                    if !hits.hits.is_empty() {
                        header(&mut rows, kind.label());
                    }
                    rows.extend(hits.hits.iter().take(GROUP_LIMIT).filter_map(|hit| {
                        Some(Row::Entry {
                            entry: entries.get(hit.index)?.clone(),
                            ranges: hit.ranges.clone(),
                        })
                    }));
                }
                if !found_items.is_empty() {
                    header(&mut rows, Kind::Items.label());
                }
                rows.extend(
                    found_items
                        .iter()
                        .take(ITEM_GROUP_LIMIT)
                        .map(|(entry, ranges)| Row::Entry {
                            entry: entry.clone(),
                            ranges: ranges.clone(),
                        }),
                );
            }
            Kind::Actions | Kind::Places | Kind::Tags => {
                let (entries, hits) = list(self.kind);
                for (position, hit) in hits.hits.iter().enumerate() {
                    // The matches belong to the lists they were searched
                    // in. A list that has grown since is searched again at
                    // once, and until that answer arrives a position is
                    // looked up without assuming that the list has it.
                    let Some(entry) = entries.get(hit.index) else {
                        continue;
                    };
                    // While the field is empty and there are recent choices,
                    // they are a group of their own above the other rows.
                    if hits.recent > 0 && position == 0 {
                        header(&mut rows, t("Recent"));
                    }
                    if hits.recent > 0 && position == hits.recent {
                        header(&mut rows, self.kind.label());
                    }
                    rows.push(Row::Entry {
                        entry: entry.clone(),
                        ranges: hit.ranges.clone(),
                    });
                }
            }
            Kind::Items => {
                let recent = if empty {
                    self.recent_items()
                } else {
                    Vec::new()
                };
                if !recent.is_empty() {
                    header(&mut rows, t("Recent"));
                }
                let recent_keys: Vec<String> =
                    recent.iter().map(|entry| entry.key.clone()).collect();
                rows.extend(recent.into_iter().map(|entry| Row::Entry {
                    entry,
                    ranges: Vec::new(),
                }));
                let rest: Vec<&(Entry, Vec<Range<usize>>)> = found_items
                    .iter()
                    .filter(|(entry, _)| !recent_keys.contains(&entry.key))
                    .collect();
                if !recent_keys.is_empty() && !rest.is_empty() {
                    header(&mut rows, Kind::Items.label());
                }
                rows.extend(rest.into_iter().map(|(entry, ranges)| Row::Entry {
                    entry: entry.clone(),
                    ranges: ranges.clone(),
                }));
            }
        }
        let first = rows.iter().position(|row| matches!(row, Row::Entry { .. }));
        let kept = keep.and_then(|key| {
            rows.iter()
                .position(|row| matches!(row, Row::Entry { entry, .. } if entry.key == key))
        });
        self.highlighted = kept.or(first);
        self.rows = rows;
    }
}

#[cfg(test)]
mod tests {
    use super::sources::{Entry, Target};
    use super::{Found, Items, Kind, Lists, Open, Row};
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Instant;

    fn entry(kind: Kind, key: &str, label: &str) -> Entry {
        Entry {
            kind,
            key: key.into(),
            label: label.into(),
            detail: String::new(),
            icon: String::new(),
            tint: None,
            target: Target::Tag(0),
            picture: None,
        }
    }

    /// Returns a panel with two actions, three places and two tags, opened
    /// on the tab `kind` with `query` in the field and `recent` as the keys
    /// of the recent choices.
    fn panel(kind: Kind, query: &str, recent: &[&str]) -> Open {
        let lists = Lists {
            actions: Arc::new(vec![
                entry(Kind::Actions, "action:nav.trash", "Trash"),
                entry(Kind::Actions, "action:file.import", "Import Files"),
            ]),
            places: Arc::new(vec![
                entry(Kind::Places, "place:system:active", "All"),
                entry(Kind::Places, "place:system:trash", "Trash"),
                entry(Kind::Places, "place:folder:4", "Travel"),
            ]),
            tags: Arc::new(vec![
                entry(Kind::Tags, "tag:1", "tree"),
                entry(Kind::Tags, "tag:2", "harbour"),
            ]),
        };
        let recent: Vec<serde_json::Value> =
            recent.iter().map(|key| json!({ "key": key })).collect();
        let found = Found::search(&lists, query, &super::recent_keys(&recent));
        let mut open = Open {
            opened: Instant::now(),
            back: None,
            focus_field: false,
            kind,
            query: query.into(),
            lists,
            actions: Vec::new(),
            recent,
            found,
            items: Items::Waiting,
            rows: Vec::new(),
            highlighted: None,
            generation: 0,
            list_search: 0,
            item_search: None,
            pointer: None,
        };
        open.rebuild(None);
        open
    }

    /// Writes the lines of the list as text: a group name in brackets, and
    /// a row as its key.
    fn lines(open: &Open) -> Vec<String> {
        open.rows
            .iter()
            .map(|row| match row {
                Row::Header(label) => format!("[{label}]"),
                Row::Entry { entry, .. } => entry.key.clone(),
            })
            .collect()
    }

    #[test]
    fn the_empty_all_tab_lists_the_recent_choices_and_then_each_kind() {
        let open = panel(Kind::All, "", &["tag:2", "place:folder:4", "tag:99"]);
        assert_eq!(
            lines(&open),
            [
                "[Recent]",
                "tag:2",
                "place:folder:4",
                "[Actions]",
                "action:nav.trash",
                "action:file.import",
                "[Places]",
                "place:system:active",
                "place:system:trash",
                "[Tags]",
                "tag:1",
            ]
        );
        // The first row that can be chosen is highlighted.
        assert_eq!(open.highlighted, Some(1));
        // Without recent choices there is no Recent group.
        assert_eq!(lines(&panel(Kind::All, "", &[]))[0], "[Actions]");
    }

    #[test]
    fn a_typed_text_lists_the_matches_of_each_kind_best_first() {
        let open = panel(Kind::All, "tr", &["tag:2"]);
        assert_eq!(
            lines(&open),
            [
                "[Actions]",
                "action:nav.trash",
                "[Places]",
                "place:system:trash",
                "place:folder:4",
                "[Tags]",
                "tag:1",
            ]
        );
        assert_eq!(open.count(Kind::Places), Some(2));
        assert_eq!(open.count(Kind::All), None);
        // A tab of one kind has no group names while a text is typed.
        assert_eq!(
            lines(&panel(Kind::Places, "tr", &[])),
            ["place:system:trash", "place:folder:4"]
        );
    }

    #[test]
    fn the_empty_tab_of_one_kind_puts_its_recent_choices_in_a_group() {
        let open = panel(Kind::Places, "", &["place:folder:4", "tag:2"]);
        assert_eq!(
            lines(&open),
            [
                "[Recent]",
                "place:folder:4",
                "[Places]",
                "place:system:active",
                "place:system:trash",
            ]
        );
        assert_eq!(
            lines(&panel(Kind::Places, "", &[])),
            [
                "place:system:active",
                "place:system:trash",
                "place:folder:4"
            ]
        );
    }

    #[test]
    fn rows_that_arrive_later_do_not_move_a_highlight_the_user_moved() {
        let mut open = panel(Kind::Places, "", &[]);
        // The highlight is still on the first row, so it is not kept.
        assert_eq!(open.highlighted_key(), None);
        open.highlighted = Some(2);
        let keep = open.highlighted_key();
        assert_eq!(keep.as_deref(), Some("place:folder:4"));
        open.recent = vec![json!({ "key": "place:system:trash" })];
        open.found = Found::search(&open.lists, "", &super::recent_keys(&open.recent));
        open.rebuild(keep);
        // The rows are now: [Recent], Trash, [Places], All, Travel.
        assert_eq!(open.highlighted, Some(4));
    }
}
