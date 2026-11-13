//! The Tags screen (`TagManagerScreen.tsx`): a rail of tag groups beside the
//! tags of the chosen group, in letter sections. A tag opens its editor;
//! tags and groups have context menus; the titlebar holds the search.
mod dialogs;
pub mod model;

use crate::backend::Screen;
use crate::grid::thumbnails::Thumbnails;
use crate::session::{Change, Reread, Session};
use crate::tags::Preferences;
use crate::theme::Text as _;
use crate::ui::{
    self,
    field::Look,
    menu::{self, Menu},
    modal::Presence,
    text_input::TextInput,
};
use crate::{motion, theme};
use dialogs::{Dialog, Layer};
use gpui::{prelude::FluentBuilder as _, *};
use model::{Row, Rows};
use picto_library::{Library, TagId, TagNamespaceRecord, TagRecord};
use std::time::{Duration, Instant};

/// Tags fetched at a time.
const PAGE_SIZE: usize = 500;
/// `.namespaceRail`
const RAIL_WIDTH: f32 = 184.;
/// `.titlebarSearch`
const SEARCH_WIDTH: f32 = 125.;
/// `.viewLeaving`: the list fades out before another group shows.
const VIEW_LEAVE: Duration = Duration::from_millis(100);
/// `.viewEntering`: the new list fades in.
const VIEW_ENTER: Duration = Duration::from_millis(140);
/// `.tagListInner`: the rows fade in each time the list is replaced.
const LIST_FADE: Duration = Duration::from_millis(90);
/// `tagSkeleton`: one pass of the placeholder shimmer.
const SHIMMER: Duration = Duration::from_millis(800);
/// A load near the end of what is loaded starts this many rows early.
const LOAD_AHEAD: usize = 4;

/// What the list shows: every tag, one group (`""` is General), or the
/// starred tags.
#[derive(Clone, Debug, PartialEq)]
enum Browse {
    All,
    Group(String),
    Starred,
}

/// Changing what the list shows, as `handleNamespaceChange` stages it.
#[derive(Clone, Copy)]
enum ViewFade {
    Idle,
    /// The list fades out, then the new browse is applied.
    Leaving(Instant),
    /// Gone, until the new tags arrive.
    Hidden,
    Entering(Instant),
}

impl ViewFade {
    fn opacity(&self) -> f32 {
        let eased = |since: &Instant, duration| {
            motion::EASE_DEFAULT.ease(motion::progress(since.elapsed(), duration))
        };
        match self {
            ViewFade::Idle => 1.,
            ViewFade::Leaving(since) => 1. - eased(since, VIEW_LEAVE),
            ViewFade::Hidden => 0.,
            ViewFade::Entering(since) => eased(since, VIEW_ENTER),
        }
    }
}

/// Whose context menu is open.
enum MenuTarget {
    Tag(TagRecord),
    Group(TagNamespaceRecord),
}

/// The titlebar's part while the Tags screen shows: its search field.
pub struct TagsToolbar {
    search: Entity<TextInput>,
    _observe: Subscription,
}

impl TagsToolbar {
    /// The search field, which `nav.search` gives the keyboard to, as
    /// `TagsToolbar`'s shortcut scope does.
    pub fn search_focus(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl Render for TagsToolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        div().w(px(SEARCH_WIDTH)).flex_none().child(
            ui::field("tag-search", &self.search, Look::TitlebarSearch)
                .icon("tags-search", 13., 7., theme.text_tertiary())
                .clearable(),
        )
    }
}

pub struct TagsScreen {
    session: Entity<Session>,
    _thumbnails: Entity<Thumbnails>,
    focus: FocusHandle,
    /// The titlebar's part while this screen shows.
    toolbar: Entity<TagsToolbar>,
    query: String,

    // What the library holds.
    namespaces: Vec<TagNamespaceRecord>,
    unused: u64,
    preferences: Preferences,
    tags: Vec<TagRecord>,
    /// Where the next page starts, while there is one.
    cursor: Option<usize>,
    loading: bool,
    loading_more: bool,
    /// Counts list loads, so an older answer never replaces a newer one.
    generation: u64,
    summary_generation: u64,
    /// The browse and search the shown tags are for.
    identity: Option<(Browse, String)>,
    error: Option<String>,
    /// An edit is running; its controls wait.
    busy: bool,

    browse: Browse,
    /// The browse to apply once the list has faded out.
    pending_browse: Option<Browse>,
    fade: ViewFade,
    /// When the rows were last replaced, for their own fade-in.
    list_shown: Instant,
    rows: Rows,
    rows_key: Option<(u64, usize, usize, bool)>,
    /// Counts replacements of `tags`, for `rows_key`.
    tags_epoch: u64,

    scroll: ScrollHandle,
    rail_scroll: ScrollHandle,
    bounds: Bounds<Pixels>,
    clock: Instant,

    menu: menu::Host,
    menu_target: Option<MenuTarget>,
    /// Open dialogs, the topmost last.
    layers: Vec<Layer>,
    /// The keyboard should move to the topmost dialog, or back to the list.
    refocus: bool,
    /// Whether the session shows this screen, so returning to it reloads.
    showing: bool,
    _subscriptions: Vec<Subscription>,
}

impl TagsScreen {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| TextInput::dialog("Search tags", cx));
        let toolbar = cx.new(|cx| TagsToolbar {
            _observe: cx.observe(&search, |_, _, cx| cx.notify()),
            search: search.clone(),
        });
        let subscriptions = vec![
            cx.observe(&search, |this, search, cx| {
                let query = search.read(cx).value();
                if query != this.query {
                    this.query = query;
                    // `useEffect([query])`: a new search closes the editor.
                    this.close_editor(cx);
                    this.load(cx);
                }
            }),
            cx.subscribe(
                &crate::settings::model(cx),
                |this, _, _: &crate::settings::Applied, cx| this.follow_settings(cx),
            ),
            cx.subscribe(&session, |this, session, change: &Change, cx| {
                if *change != Change::Navigation {
                    return;
                }
                let showing = session.read(cx).screen() == Some(Screen::Tags);
                if showing && !this.showing {
                    // What other screens changed shows when Tags opens again.
                    this.refresh(cx);
                }
                this.showing = showing;
            }),
        ];
        let focus = cx.focus_handle();
        let mut screen = Self {
            session,
            _thumbnails: thumbnails,
            focus: focus.clone(),
            toolbar,
            query: String::new(),
            namespaces: Vec::new(),
            unused: 0,
            preferences: Preferences::from_settings(crate::settings::current(cx)),
            tags: Vec::new(),
            cursor: None,
            loading: true,
            loading_more: false,
            generation: 0,
            summary_generation: 0,
            identity: None,
            error: None,
            busy: false,
            browse: Browse::All,
            pending_browse: None,
            fade: ViewFade::Idle,
            list_shown: Instant::now(),
            rows: Rows::default(),
            rows_key: None,
            tags_epoch: 0,
            scroll: ScrollHandle::new(),
            rail_scroll: ScrollHandle::new(),
            bounds: Bounds::default(),
            clock: Instant::now(),
            menu: menu::Host::deferred(30, menu::Return::To(focus)),
            menu_target: None,
            layers: Vec::new(),
            refocus: false,
            showing: false,
            _subscriptions: subscriptions,
        };
        screen.load_summary(cx);
        screen.load(cx);
        screen
    }

    /// The search field the titlebar shows while this screen is up.
    pub fn toolbar(&self) -> Entity<TagsToolbar> {
        self.toolbar.clone()
    }

    // ── Loading ─────────────────────────────────────────────────

    /// The groups and their counts, and the unused count.
    fn load_summary(&mut self, cx: &mut Context<Self>) {
        self.summary_generation += 1;
        let generation = self.summary_generation;
        let answer = self.session.read(cx).run(|library| {
            let namespaces = library.tag_namespaces().map_err(|e| e.to_string())?;
            let unused = library.unused_tag_count().map_err(|e| e.to_string())?;
            Ok::<_, String>((namespaces, unused))
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = answer.await else { return };
            let _ = this.update(cx, |this: &mut Self, cx| {
                if generation != this.summary_generation {
                    return;
                }
                match result {
                    Ok((namespaces, unused)) => {
                        let total = namespaces.iter().map(|ns| ns.tag_count).sum::<u64>();
                        this.namespaces = namespaces;
                        this.unused = unused;
                        // The sidebar's Tags row counts every tag.
                        this.session.update(cx, |session, cx| {
                            if let Some(entry) = session
                                .nav
                                .iter_mut()
                                .find(|entry| entry.screen == Some(Screen::Tags))
                                && entry.count != Some(total)
                            {
                                entry.count = Some(total);
                                cx.emit(Change::Navigation);
                            }
                        });
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The first page of what the list shows. When that is a different
    /// browse or search, the rows fade in once it arrives.
    fn load(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.loading_more = false;
        let identity = (self.browse.clone(), self.query.clone());
        let changed = self.identity.as_ref() != Some(&identity);
        self.identity = Some(identity);
        if changed {
            self.loading = true;
        }
        let query = self.query.trim().to_owned();
        let answer = match &self.browse {
            Browse::Starred => {
                let starred = self.preferences.starred.clone();
                self.session.read(cx).run(move |library| {
                    let tags = library.tags().map_err(|e| e.to_string())?;
                    let found = starred
                        .iter()
                        .filter_map(|name| {
                            tags.iter()
                                .find(|tag| crate::tags::record_name(tag) == *name)
                                .cloned()
                        })
                        .collect::<Vec<_>>();
                    Ok((
                        crate::text::rank_matches(found, &query, |tag| tag.subname.as_str()),
                        None,
                    ))
                })
            }
            browse => {
                let namespace = match browse {
                    Browse::Group(name) => Some(name.clone()),
                    _ => None,
                };
                self.session
                    .read(cx)
                    .run(move |library| page(library, namespace, query, 0))
            }
        };
        cx.spawn(async move |this, cx| {
            let Ok(result) = answer.await else { return };
            let _ = this.update(cx, |this: &mut Self, cx| {
                if generation != this.generation {
                    return;
                }
                match result {
                    Ok((tags, cursor)) => {
                        this.tags = merge_unique(Vec::new(), tags);
                        this.tags_epoch += 1;
                        this.cursor = cursor;
                    }
                    Err(error) => this.error = Some(error),
                }
                if changed {
                    // Shown even when the load failed, so the error is not
                    // left over a hidden list.
                    this.fade = ViewFade::Entering(Instant::now());
                    this.list_shown = Instant::now();
                }
                this.loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// The next page, once the list is scrolled near the end of what is
    /// loaded.
    fn load_more(&mut self, cx: &mut Context<Self>) {
        let Some(offset) = self.cursor.filter(|_| !self.loading_more && !self.loading) else {
            return;
        };
        self.loading_more = true;
        let generation = self.generation;
        let namespace = match &self.browse {
            Browse::Group(name) => Some(name.clone()),
            _ => None,
        };
        let query = self.query.trim().to_owned();
        let answer = self
            .session
            .read(cx)
            .run(move |library| page(library, namespace, query, offset));
        cx.spawn(async move |this, cx| {
            let Ok(result) = answer.await else { return };
            let _ = this.update(cx, |this: &mut Self, cx| {
                if generation != this.generation {
                    return;
                }
                match result {
                    Ok((tags, cursor)) => {
                        this.tags = merge_unique(std::mem::take(&mut this.tags), tags);
                        this.tags_epoch += 1;
                        // A page that does not move on would be fetched for ever.
                        this.cursor = cursor.filter(|next| *next != offset);
                    }
                    Err(error) => this.error = Some(error),
                }
                this.loading_more = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Fetches everything again after an edit, keeping what shows until
    /// the answers arrive.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load(cx);
        self.load_summary(cx);
    }

    /// How many tags the list will hold once loaded, for its placeholders.
    fn expected(&self) -> usize {
        let total = |namespaces: &[TagNamespaceRecord]| {
            namespaces.iter().map(|ns| ns.tag_count).sum::<u64>() as usize
        };
        if !self.query.trim().is_empty() {
            self.tags.len()
        } else {
            match &self.browse {
                Browse::Starred => self.preferences.starred.len(),
                Browse::All => total(&self.namespaces),
                Browse::Group(name) => self
                    .namespaces
                    .iter()
                    .find(|ns| ns.name == *name)
                    .map_or(self.tags.len(), |ns| ns.tag_count as usize),
            }
        }
    }

    // ── Browsing ────────────────────────────────────────────────

    /// `handleNamespaceChange` and `handleStarredChange`: the list fades out
    /// for 100 ms, then shows `browse`.
    fn show(&mut self, browse: Browse, cx: &mut Context<Self>) {
        if browse == self.browse && self.pending_browse.is_none() {
            return;
        }
        self.pending_browse = Some(browse);
        if !matches!(self.fade, ViewFade::Leaving(_)) {
            self.fade = ViewFade::Leaving(Instant::now());
        }
        cx.notify();
    }

    /// Applies the browse waiting on the fade-out, once it has run.
    fn advance_fade(&mut self, cx: &mut Context<Self>) {
        match self.fade {
            ViewFade::Leaving(since) if since.elapsed() >= VIEW_LEAVE => {
                self.fade = ViewFade::Hidden;
                if let Some(browse) = self.pending_browse.take() {
                    // `resetBrowse`
                    self.browse = browse;
                    self.tags.clear();
                    self.tags_epoch += 1;
                    self.cursor = None;
                    self.error = None;
                    self.close_editor(cx);
                    self.load(cx);
                }
            }
            ViewFade::Entering(since) if since.elapsed() >= VIEW_ENTER => {
                self.fade = ViewFade::Idle;
            }
            _ => {}
        }
    }

    // ── Edits ───────────────────────────────────────────────────

    /// `runMutation`: runs an edit on the library's thread, then reloads.
    /// On success the editor closes if `close_editor`, or else returns from
    /// a merge or delete step; `then` follows.
    fn mutate(
        &mut self,
        work: impl FnOnce(&Library) -> Result<(), String> + Send + 'static,
        close_editor: bool,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.error = None;
        let answer = self
            .session
            .update(cx, |session, cx| session.edit(work, Reread::NOTHING, cx));
        cx.spawn(async move |this, cx| {
            let result = answer
                .await
                .unwrap_or_else(|_| Err("The library stopped".into()));
            let _ = this.update(cx, |this: &mut Self, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        this.refresh(cx);
                        if close_editor {
                            this.close_editor(cx);
                        } else {
                            this.close_editor_step();
                        }
                        then(this, cx);
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The tag preferences changed, here or in the Settings window.
    fn follow_settings(&mut self, cx: &mut Context<Self>) {
        let next = Preferences::from_settings(crate::settings::current(cx));
        if next == self.preferences {
            return;
        }
        let starred_changed = next.starred != self.preferences.starred;
        self.preferences = next;
        if starred_changed && self.browse == Browse::Starred {
            self.load(cx);
        }
        cx.notify();
    }

    /// Stores changed tag preferences through the settings model, which owns
    /// the library's application settings and writes what changed.
    fn save_preferences(&mut self, next: Preferences, cx: &mut Context<Self>) {
        let colors = next.colors_json();
        let starred = next.starred.clone();
        crate::settings::model(cx).update(cx, |model, cx| {
            model.apply_now(
                |app| {
                    app.starred_tags = starred.clone();
                    app.tag_group_colors = colors.clone();
                },
                cx,
            )
        });
        // The model tells every follower, this screen too.
        self.follow_settings(cx);
    }

    fn set_starred(&mut self, name: String, starred: bool, cx: &mut Context<Self>) {
        crate::tags::set_starred(&name, starred, cx);
        self.follow_settings(cx);
    }

    /// Moves a starred tag's star to its new name after a rename.
    fn rename_star(&mut self, previous: &str, next_name: &str, cx: &mut Context<Self>) {
        crate::tags::rename_star(previous, next_name, cx);
        self.follow_settings(cx);
    }

    fn set_group_color(&mut self, group: &str, color: Option<&str>, cx: &mut Context<Self>) {
        let mut next = self.preferences.clone();
        next.colors = self.preferences.with_color(group, color);
        self.save_preferences(next, cx);
    }

    /// Renames a tag, or merges it into the tag that already has the name,
    /// as the application's `tags.rename_or_merge` does.
    fn rename_tag(&mut self, tag: &TagRecord, name: String, close: bool, cx: &mut Context<Self>) {
        let previous = crate::tags::record_name(tag);
        let id = tag.tag_id;
        let next_name = name.trim().to_owned();
        let star_name = next_name.clone();
        self.mutate(
            move |library| rename_or_merge(library, id, &next_name),
            close,
            move |this, cx| this.rename_star(&previous, &star_name, cx),
            cx,
        );
    }

    fn merge_tag(&mut self, tag: &TagRecord, target: &TagRecord, cx: &mut Context<Self>) {
        let (source, destination) = (tag.tag_id, target.tag_id);
        self.mutate(
            move |library| {
                library
                    .merge_tags(source, destination, crate::now_ms())
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            },
            true,
            |_, _| {},
            cx,
        );
    }

    fn delete_tag(&mut self, tag_id: TagId, cx: &mut Context<Self>) {
        self.mutate(
            move |library| {
                library
                    .delete_tag(tag_id, crate::now_ms())
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            },
            true,
            |_, _| {},
            cx,
        );
    }

    fn rename_group(&mut self, group: TagNamespaceRecord, name: String, cx: &mut Context<Self>) {
        let id = group.namespace_id;
        let target = name.clone();
        self.mutate(
            move |library| {
                library
                    .rename_or_merge_tag_namespace(id, &target)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            },
            false,
            move |this, cx| {
                if let Some(colors) = this.preferences.renamed_color(&group.name, &name) {
                    let mut next = this.preferences.clone();
                    next.colors = colors;
                    this.save_preferences(next, cx);
                }
                this.drop_layers(|dialog| matches!(dialog, Dialog::RenameGroup { .. }));
                if this.browse == Browse::Group(group.name.clone()) {
                    this.show(Browse::Group(name), cx);
                }
            },
            cx,
        );
    }

    fn delete_group(&mut self, group: TagNamespaceRecord, cx: &mut Context<Self>) {
        let id = group.namespace_id;
        self.mutate(
            move |library| {
                library
                    .delete_tag_namespace(id)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            },
            false,
            move |this, cx| {
                if this.preferences.colors.contains_key(&group.name) {
                    this.set_group_color(&group.name, None, cx);
                }
                this.drop_layers(|dialog| matches!(dialog, Dialog::DeleteGroup { .. }));
                if this.browse == Browse::Group(group.name.clone()) {
                    this.show(Browse::All, cx);
                }
            },
            cx,
        );
    }

    fn create_group(&mut self, name: String, cx: &mut Context<Self>) {
        let target = name.clone();
        self.mutate(
            move |library| {
                library
                    .create_tag_namespace(&target)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            },
            false,
            move |this, cx| {
                this.drop_layers(|dialog| matches!(dialog, Dialog::CreateGroup { .. }));
                this.show(Browse::Group(name), cx);
            },
            cx,
        );
    }

    fn delete_unused(&mut self, cx: &mut Context<Self>) {
        self.mutate(
            |library| {
                library
                    .delete_unused_tags()
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            },
            false,
            |this, cx| this.forget_deleted_stars(cx),
            cx,
        );
    }

    /// The library drops deleted tags from the starred list it stores; the
    /// settings model, which holds that list too, is told the same.
    fn forget_deleted_stars(&mut self, cx: &mut Context<Self>) {
        let answer = self.session.read(cx).run(|library| {
            library.tags().map(|tags| {
                tags.iter()
                    .map(crate::tags::record_name)
                    .collect::<std::collections::HashSet<_>>()
            })
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(names)) = answer.await else { return };
            let _ = this.update(cx, |this: &mut Self, cx| {
                let mut next = this.preferences.clone();
                next.starred.retain(|name| names.contains(name));
                if next != this.preferences {
                    this.save_preferences(next, cx);
                }
            });
        })
        .detach();
    }

    /// `showTagManagerItems`: the All grid, filtered to items with the tag.
    fn show_items(&mut self, tag_id: TagId, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            let Some(all) = session
                .nav
                .iter()
                .position(|entry| entry.screen.is_none() && !entry.random && entry.label == "All")
            else {
                return;
            };
            // The filter bar shows the tag, as `showTagItems` sets it.
            let name = session
                .tags
                .get(&tag_id.0)
                .map(crate::tags::record_name)
                .unwrap_or_default();
            let filters = crate::grid::filters::Filters {
                include_tags: vec![crate::grid::filters::TagChoice {
                    tag_id: tag_id.0,
                    name,
                }],
                ..Default::default()
            };
            session.navigate_with_filters(all, filters, cx);
        });
    }

    // ── Menus ───────────────────────────────────────────────────

    fn open_menu(
        &mut self,
        entries: Vec<menu::Entry>,
        target: MenuTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = cx.new(|cx| Menu::new(entries, position, cx));
        self.menu
            .open(menu, window, cx, |this, menu, action, window, cx| {
                let value = menu.read(cx).chosen_value();
                this.run_menu_action(action, value, window, cx);
            });
        self.menu_target = Some(target);
    }

    /// `openTagContextMenu`, with `buildCommonTagContextEntries`.
    fn open_tag_menu(
        &mut self,
        tag: TagRecord,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut entries = crate::tags::common_menu(&tag, &self.namespaces, true, cx);
        entries.extend([
            menu::Entry::Separator,
            menu::item("edit", "Edit Tag").icon("menu-edit").into(),
            menu::item("merge", "Merge Into…")
                .icon("menu-git-merge")
                .into(),
            menu::Entry::Separator,
            menu::item("delete", "Delete Tag")
                .icon("menu-trash")
                .danger(true)
                .into(),
        ]);
        self.open_menu(entries, MenuTarget::Tag(tag), position, window, cx);
    }

    /// `openGroupContextMenu`
    fn open_group_menu(
        &mut self,
        group: TagNamespaceRecord,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chosen = self.preferences.colors.get(&group.name).cloned();
        let entries = vec![
            menu::item("show-group", "Show Tags in Group")
                .icon("menu-bookmark")
                .into(),
            menu::Entry::Separator,
            menu::item("rename-group", "Rename Group…")
                .icon("menu-edit")
                .into(),
            ui::color_picker::menu_row("group-color", chosen.as_deref()).into(),
            menu::item("delete-group", "Delete Group")
                .icon("menu-trash")
                .into(),
        ];
        self.open_menu(entries, MenuTarget::Group(group), position, window, cx);
    }

    fn run_menu_action(
        &mut self,
        action: &str,
        value: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.menu_target.as_ref() {
            Some(MenuTarget::Tag(tag)) => {
                let tag = tag.clone();
                let name = crate::tags::record_name(&tag);
                match action {
                    "filter" => self.show_items(tag.tag_id, cx),
                    "star" => self.set_starred(name, true, cx),
                    "unstar" => self.set_starred(name, false, cx),
                    "move-to-group" => {
                        if let Some(group) = value {
                            let next = crate::tags::name_in_group(&tag.subname, Some(&group));
                            self.rename_tag(&tag, next, false, cx);
                        }
                    }
                    "remove-from-group" => {
                        let next = crate::tags::name_in_group(&tag.subname, None);
                        self.rename_tag(&tag, next, false, cx);
                    }
                    "copy" => cx.write_to_clipboard(ClipboardItem::new_string(name)),
                    "edit" => self.open_editor(tag, None, window, cx),
                    "merge" => {
                        let merge = dialogs::merge(&tag, cx);
                        self.open_editor(tag, Some(merge), window, cx);
                    }
                    "delete" => {
                        let delete = Dialog::DeleteTag { tag: tag.clone() };
                        self.open_editor(tag, Some(delete), window, cx);
                    }
                    _ => {}
                }
            }
            Some(MenuTarget::Group(group)) => {
                let group = group.clone();
                match action {
                    "show-group" => self.show(Browse::Group(group.name), cx),
                    "rename-group" => {
                        let dialog = dialogs::rename_group(group, cx);
                        self.push(dialog, cx);
                    }
                    "group-color" => {
                        let color = value.filter(|value| !value.is_empty());
                        self.set_group_color(&group.name, color.as_deref(), cx);
                    }
                    "delete-group" => self.push(Dialog::DeleteGroup { group }, cx),
                    _ => {}
                }
            }
            None => {}
        }
        cx.notify();
    }

    // ── Dialogs ─────────────────────────────────────────────────

    fn push(&mut self, dialog: Dialog, cx: &mut Context<Self>) {
        self.layers.push(Layer {
            dialog,
            presence: Presence::new(),
            focus: cx.focus_handle(),
        });
        self.refocus = true;
        cx.notify();
    }

    /// `setSelected(tag)`: the editor for `tag`, with a merge or delete step
    /// over it when one is given.
    fn open_editor(
        &mut self,
        tag: TagRecord,
        step: Option<Dialog>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_editor(cx);
        let editor = dialogs::editor(tag, cx);
        self.push(editor, cx);
        if let Some(step) = step {
            self.push(step, cx);
        }
    }

    /// `closeEditor`: the editor and any step over it go at once.
    fn close_editor(&mut self, cx: &mut Context<Self>) {
        let before = self.layers.len();
        self.drop_layers(Dialog::belongs_to_editor);
        if self.layers.len() != before {
            cx.notify();
        }
    }

    /// `setEditorAction(null)`: the merge or delete step goes, the editor
    /// stays.
    fn close_editor_step(&mut self) {
        self.drop_layers(|dialog| {
            matches!(dialog, Dialog::Merge { .. } | Dialog::DeleteTag { .. })
        });
    }

    fn drop_layers(&mut self, which: impl Fn(&Dialog) -> bool) {
        let before = self.layers.len();
        self.layers.retain(|layer| !which(&layer.dialog));
        if self.layers.len() != before {
            self.refocus = true;
        }
    }

    // ── Drawing ─────────────────────────────────────────────────

    /// `.namespaceItem`: one row of the rail.
    #[allow(clippy::too_many_arguments)]
    fn rail_item(
        &self,
        id: impl Into<ElementId>,
        icon: &'static str,
        icon_color: Rgba,
        label: SharedString,
        count: u64,
        active: bool,
        cx: &App,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        div()
            .id(id)
            .h(px(theme::CONTROL_SM))
            .mx(px(8.))
            .mb(px(1.))
            .px(px(6.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .rounded(px(theme::RADIUS_MD))
            .when(active, |row| row.bg(theme.surface_active()))
            // `.namespaceItem:hover` outranks `.namespaceItemActive`.
            .hover(move |style| style.bg(theme.surface_hover()))
            .cursor_pointer()
            .text(theme::TEXT_MD)
            .line_height(px(theme::CONTROL_SM))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::icon(icon, 16., icon_color).flex_none())
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.text())
                            .child(label),
                    ),
            )
            .child(count_label(count, cx))
    }

    fn rail(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let showing = |browse: Browse| self.browse == browse;
        let total = self.namespaces.iter().map(|ns| ns.tag_count).sum::<u64>();
        let general = self
            .namespaces
            .iter()
            .find(|ns| ns.name.is_empty())
            .map_or(0, |ns| ns.tag_count);
        let mut groups = self
            .namespaces
            .iter()
            .filter(|ns| !ns.name.is_empty() && ns.name != "general")
            .cloned()
            .collect::<Vec<_>>();
        // A stable sort: groups of one order keep the library's name order.
        groups.sort_by_key(|ns| crate::tags::group_order(&ns.name));

        let mut column = div()
            .pt(px(8.))
            .pb(px(8.))
            .pr(px(ui::scrollbar::GUTTER))
            .child(
                self.rail_item(
                    "tags-all",
                    "tags-bookmarks",
                    theme.text(),
                    "All".into(),
                    total,
                    showing(Browse::All),
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.show(Browse::All, cx))),
            )
            .child(
                self.rail_item(
                    "tags-general",
                    "tags-bookmark",
                    theme.text(),
                    "General".into(),
                    general,
                    showing(Browse::Group(String::new())),
                    cx,
                )
                .on_click(
                    cx.listener(|this, _, _, cx| this.show(Browse::Group(String::new()), cx)),
                ),
            )
            .child(
                self.rail_item(
                    "tags-starred",
                    "tags-star",
                    theme.text(),
                    "Starred".into(),
                    self.preferences.starred.len() as u64,
                    showing(Browse::Starred),
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.show(Browse::Starred, cx))),
            )
            .child(
                // `.railHeading`
                div()
                    .h(px(theme::CONTROL_SM))
                    .mt(px(12.))
                    .mx(px(8.))
                    .mb(px(3.))
                    .pl(px(3.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_color(theme.text_tertiary())
                    .text(theme::TEXT_SM)
                    .child(format!("Groups({})", groups.len()))
                    .child(
                        ui::icon_button("create-tag-group", "tags-plus")
                            .size(20.)
                            .glyph(15.)
                            .disabled(self.busy)
                            .tip(ui::tip("Create tag group"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let dialog = dialogs::create_group(cx);
                                this.push(dialog, cx);
                            })),
                    ),
            );
        for group in groups {
            let color = rgb(crate::tags::group_color(&group.name, &self.preferences));
            let active = showing(Browse::Group(group.name.clone()));
            let label = if group.name.is_empty() {
                "general".to_owned()
            } else {
                group.name.clone()
            };
            let (shown, opened) = (group.name.clone(), group.clone());
            column = column.child(
                self.rail_item(
                    ("tags-group", group.namespace_id.0 as usize),
                    "tags-bookmark-group",
                    color,
                    label.into(),
                    group.tag_count,
                    active,
                    cx,
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| this.show(Browse::Group(shown.clone()), cx)),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.open_group_menu(opened.clone(), event.position, window, cx);
                        cx.stop_propagation();
                    }),
                ),
            );
        }
        // `.cleanupTagsButton`
        let enabled = self.unused > 0 && !self.busy;
        let mut cleanup = div()
            .id("delete-unused-tags")
            .h(px(theme::CONTROL_SM))
            .mt(px(10.))
            .mx(px(8.))
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(theme::RADIUS_MD))
            .text_color(theme.text_secondary())
            .text(theme::TEXT_SM)
            .line_height(px(theme::CONTROL_SM))
            .group("cleanup")
            .child(
                ui::icon("tags-trash", 14., theme.text_secondary())
                    .flex_none()
                    .group_hover("cleanup", move |style| {
                        if enabled {
                            style.text_color(theme.negative())
                        } else {
                            style
                        }
                    }),
            )
            .child("Delete Unused")
            .child(count_label(self.unused, cx));
        cleanup = if enabled {
            cleanup
                .cursor_pointer()
                .hover(move |style| style.bg(theme.surface_hover()).text_color(theme.negative()))
                .on_click(cx.listener(|this, _, _, cx| this.push(Dialog::Cleanup, cx)))
        } else {
            cleanup.opacity(0.35)
        };
        column = column.child(cleanup);

        div()
            .relative()
            .w(px(RAIL_WIDTH))
            .flex_none()
            .h_full()
            .bg(theme.background())
            .border_r_1()
            .border_color(theme.border_secondary())
            .child(
                div()
                    .id("tag-rail")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.rail_scroll)
                    .child(column),
            )
            .child(ui::scrollbar(
                "tag-rail-scrollbar",
                &self.rail_scroll,
                window,
                cx,
            ))
    }

    /// `.tagCard`: a tag in the list, with its group's dot, a star when
    /// starred, and how many items carry it.
    fn tag_card(
        &self,
        tag: &TagRecord,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let name = crate::tags::record_name(tag);
        let starred = self.preferences.is_starred(&name);
        let color = rgb(crate::tags::group_color(&tag.namespace, &self.preferences));
        let count = format!("({})", tag.assignment_count);
        // The name keeps its own width beside the count unless it is too
        // long, when it takes what is left and ends in an ellipsis.
        let measure = |text: SharedString, font: &'static str, size: f32, window: &mut Window| {
            let mut style = window.text_style();
            style.font_family = font.into();
            let run = style.to_run(text.len());
            f32::from(
                window
                    .text_system()
                    .shape_line(text, px(size), &[run], None)
                    .width,
            )
        };
        let label: SharedString = tag.subname.clone().into();
        let room = width
            - 8.
            - 18.
            - if starred { 12. } else { 0. }
            - 6.
            - measure(
                count.clone().into(),
                theme::FONT_INSPECTOR_MONO,
                theme::TEXT_CAPTION,
                window,
            );
        let overflows = measure(label.clone(), theme::FONT_UI, theme::TEXT_MD, window) > room;
        let name_cell = div().min_w_0().child(label);
        let name_cell = if overflows {
            name_cell.flex_1().truncate()
        } else {
            name_cell.flex_none().whitespace_nowrap()
        };
        let (opened, menu_tag) = (tag.clone(), tag.clone());
        div()
            .id(("tag", tag.tag_id.0 as usize))
            .flex_1()
            .min_w_0()
            .h(px(model::ROW_HEIGHT))
            .px(px(4.))
            .flex()
            .items_center()
            .rounded(px(theme::RADIUS_MD))
            .cursor_pointer()
            .group("tag-card")
            .hover(move |style| style.bg(theme.surface_hover()))
            .text(theme::TEXT_MD)
            .line_height(px(model::ROW_HEIGHT))
            .child(
                div()
                    .size(px(5.))
                    .flex_none()
                    .ml(px(4.))
                    .mr(px(9.))
                    .rounded_full()
                    .bg(color)
                    .opacity(0.5)
                    .group_hover("tag-card", |style| style.opacity(1.)),
            )
            .child(name_cell)
            .children(starred.then(|| {
                // `color: var(--color-warning)` names no token, so the star
                // takes the label's colour, as it does in the Electron app.
                ui::icon("tags-star-solid", 12., theme.text())
                    .flex_none()
                    .ml_auto()
            }))
            .child(
                div()
                    .flex_none()
                    .ml(px(6.))
                    .opacity(0.5)
                    .font_family(theme::FONT_INSPECTOR_MONO)
                    .text(theme::TEXT_CAPTION)
                    .child(count),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_editor(opened.clone(), None, window, cx)
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_tag_menu(menu_tag.clone(), event.position, window, cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }

    /// `.tagSkeleton`: a placeholder with the moving shimmer.
    fn skeleton(&self, cx: &App) -> AnyElement {
        let theme = theme::theme(cx);
        let elapsed = self.clock.elapsed().as_secs_f32() % SHIMMER.as_secs_f32();
        let eased = motion::EASE_IN_OUT.ease(elapsed / SHIMMER.as_secs_f32());
        let (bend, values) = model::shimmer(eased);
        let strength = move |value: f32| theme.contrast(3. + 5. * value);
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let radius = px(theme::RADIUS_MD);
                let split = bounds.left() + bounds.size.width * bend;
                let piece = |from: Pixels, to: Pixels| {
                    Bounds::from_corners(point(from, bounds.top()), point(to, bounds.bottom()))
                };
                let gradient = |left: f32, right: f32| {
                    linear_gradient(
                        90.,
                        linear_color_stop(strength(left), 0.),
                        linear_color_stop(strength(right), 1.),
                    )
                };
                if split > bounds.left() {
                    window.paint_quad(
                        fill(piece(bounds.left(), split), gradient(values[0], values[1]))
                            .corner_radii(Corners {
                                top_left: radius,
                                bottom_left: radius,
                                ..Default::default()
                            }),
                    );
                }
                if split < bounds.right() {
                    window.paint_quad(
                        fill(piece(split, bounds.right()), gradient(values[1], values[2]))
                            .corner_radii(Corners {
                                top_right: radius,
                                bottom_right: radius,
                                ..Default::default()
                            }),
                    );
                }
            },
        )
        .flex_1()
        .min_w_0()
        .h(px(model::ROW_HEIGHT))
        .into_any_element()
    }

    fn list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let width = (f32::from(self.bounds.size.width) - ui::scrollbar::GUTTER).max(0.);
        let columns = model::column_count(width);
        let searching = !self.query.trim().is_empty();
        let virtual_count = if self.cursor.is_some() || self.loading {
            self.tags.len().max(self.expected())
        } else {
            self.tags.len()
        };
        let key = Some((self.tags_epoch, columns, virtual_count, searching));
        if key != self.rows_key {
            self.rows = Rows::build(&self.tags, searching, columns, virtual_count);
            self.rows_key = key;
        }
        let top = -f32::from(self.scroll.offset().y) - model::LIST_TOP;
        let viewport = f32::from(self.bounds.size.height);
        let visible = self.rows.visible(top, top + viewport);
        // Fetch at the boundary of what is loaded, not at the end of the
        // placeholders' estimate.
        if visible.end > 0
            && visible.end - 1 + LOAD_AHEAD >= self.rows.load_boundary()
            && self.cursor.is_some()
        {
            self.load_more(cx);
        }

        // The rows sit 12px in from each side of the content, which stops
        // short of the scrollbar's gutter.
        let row_width = width - 24.;
        let card_width = (row_width - 2. * (columns as f32 - 1.)) / columns as f32;
        let mut shimmering = false;
        let mut content = div()
            .relative()
            .w_full()
            .h(px(model::LIST_TOP + self.rows.total + model::LIST_BOTTOM))
            .opacity(
                motion::EASE_DEFAULT.ease(motion::progress(self.list_shown.elapsed(), LIST_FADE)),
            );
        for index in visible {
            let start = model::LIST_TOP + self.rows.starts[index];
            let place = |row: Div| {
                row.absolute()
                    .top(px(start))
                    .left(px(12.))
                    .right(px(12. + ui::scrollbar::GUTTER))
            };
            let row = match self.rows.rows[index].clone() {
                Row::Section { label, count } => place(div())
                    .h(px(model::SECTION_HEIGHT))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_color(theme.text())
                    .text(theme::TEXT_LG)
                    .line_height(px(model::SECTION_HEIGHT))
                    .child(label)
                    .child(
                        div()
                            .text_color(theme.text_tertiary())
                            .text(theme::TEXT_CAPTION)
                            .child(format!("({count})")),
                    ),
                Row::Tags { tags, section_end } => {
                    let shown = tags.len();
                    let mut row = place(div()).h(px(model::ROW_HEIGHT)).flex().gap(px(2.));
                    for position in tags {
                        let tag = self.rows.ordered[position].clone();
                        row = row.child(self.tag_card(&tag, card_width, window, cx));
                    }
                    // Each column keeps its share of the width.
                    for _ in shown..columns {
                        row = row.child(div().flex_1().min_w_0());
                    }
                    row.when(section_end, |row| {
                        row.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(1.))
                                .bg(theme.border_secondary()),
                        )
                    })
                }
                Row::Skeleton => {
                    shimmering = true;
                    let mut row = place(div()).h(px(model::ROW_HEIGHT)).flex().gap(px(2.));
                    for _ in 0..columns {
                        row = row.child(self.skeleton(cx));
                    }
                    row
                }
            };
            content = content.child(row);
        }
        if shimmering || self.list_shown.elapsed() < LIST_FADE {
            window.request_animation_frame();
        }

        let entity = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, _, cx| {
                let _ = entity.update(cx, |this, cx| {
                    if this.bounds != bounds {
                        this.bounds = bounds;
                        cx.notify();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let empty = !self.loading && self.tags.is_empty();
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .opacity(self.fade.opacity())
            .child(measure)
            .child(
                div()
                    .id("tag-list")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                    .child(content),
            )
            .children(empty.then(|| {
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.text_secondary())
                    .text(theme::TEXT_MD)
                    .child("No tags")
            }))
            .child(ui::scrollbar(
                "tag-list-scrollbar",
                &self.scroll,
                window,
                cx,
            ))
    }
}

/// `.namespaceCount`
fn count_label(count: u64, cx: &App) -> Div {
    div()
        .flex_none()
        .ml_auto()
        .text_color(theme::theme(cx).text_tertiary())
        .text(theme::TEXT_CAPTION)
        .child(count.to_string())
}

/// `mergeUniqueTags`: `incoming` after `current`, each tag once, a tag
/// already listed updated where it stands.
fn merge_unique(mut current: Vec<TagRecord>, incoming: Vec<TagRecord>) -> Vec<TagRecord> {
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

/// One page of tags from the library: the tags and where the next page
/// starts.
fn page(
    library: &Library,
    namespace: Option<String>,
    query: String,
    offset: usize,
) -> Result<(Vec<TagRecord>, Option<usize>), String> {
    let search = (!query.is_empty()).then_some(query.as_str());
    library
        .tag_page(namespace.as_deref(), search, offset, PAGE_SIZE)
        .map(|(tags, next, _)| (tags, next))
        .map_err(|error| error.to_string())
}

/// Renames a tag, or merges it into the tag that already has the name, as
/// the application's `tags.rename_or_merge` does.
fn rename_or_merge(library: &Library, id: TagId, name: &str) -> Result<(), String> {
    let target = library
        .projections()
        .snapshot()
        .tag_ids_by_name
        .get(name.trim())
        .copied();
    match target {
        Some(target) if target != id => library.merge_tags(id, target, crate::now_ms()),
        _ => library.rename_tag(id, name),
    }
    .map(|_| ())
    .map_err(|error| error.to_string())
}

impl Render for TagsScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        self.advance_fade(cx);
        if matches!(self.fade, ViewFade::Leaving(_) | ViewFade::Entering(_)) {
            window.request_animation_frame();
        }
        // Dialogs that have finished leaving go; the keyboard follows the
        // topmost one that is left.
        let before = self.layers.len();
        self.layers.retain(|layer| !layer.presence.finished());
        if self.layers.len() != before {
            self.refocus = true;
        }
        if std::mem::take(&mut self.refocus) {
            match self.layers.last() {
                Some(layer) => layer.focus_in(window, cx),
                None => window.focus(&self.focus, cx),
            }
        }
        // Until the last has gone, a dialog leaving keeps this repainting.
        if self.layers.iter().any(|layer| layer.presence.closing()) {
            window.request_animation_frame();
        }
        let menu = self.menu.layer(window, cx);

        let mut root = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.background())
            .text_color(theme.text())
            .track_focus(&self.focus);
        if let Some(error) = &self.error {
            root = root.child(dialogs::error_banner(error.clone(), cx));
        }
        let rail = self.rail(window, cx);
        let list = self.list(window, cx);
        root = root.child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .bg(theme.background())
                .child(rail)
                .child(list),
        );
        for (index, layer) in self.layers.iter().enumerate() {
            let element = self.layer(index, layer, window, cx);
            root = root.child(element);
        }
        root.children(menu)
    }
}

#[cfg(test)]
mod tests {
    use super::{page, rename_or_merge};
    use picto_library::Library;
    use std::path::PathBuf;

    #[test]
    #[ignore = "Requires PICTO_GPUI_TEST_LIBRARY naming a disposable copy made by run.sh"]
    fn real_snapshot_tags_browse_rename_and_groups() {
        let root = PathBuf::from(std::env::var("PICTO_GPUI_TEST_LIBRARY").unwrap());
        let root = crate::application::guard(&root).unwrap();
        let library = Library::open(root.join("library.sqlite")).unwrap();

        // The rail's counts add up to every tag, and General is the
        // namespace with no name.
        let namespaces = library.tag_namespaces().unwrap();
        let tags = library.tags().unwrap();
        let total = namespaces.iter().map(|ns| ns.tag_count).sum::<u64>();
        assert_eq!(total as usize, tags.len());
        let general = namespaces.iter().find(|ns| ns.name.is_empty()).unwrap();

        // Paging through General reaches exactly its tags.
        let mut seen = Vec::new();
        let mut offset = Some(0);
        while let Some(at) = offset {
            let (page, next) = page(&library, Some(String::new()), String::new(), at).unwrap();
            assert!(page.iter().all(|tag| tag.namespace.is_empty()));
            seen.extend(page);
            offset = next;
        }
        assert_eq!(seen.len() as u64, general.tag_count);

        // A search puts the exact name first.
        let probe = tags.iter().find(|tag| !tag.subname.is_empty()).unwrap();
        let (found, _) = page(&library, None, probe.subname.clone(), 0).unwrap();
        assert_eq!(
            found[0].subname.to_lowercase(),
            probe.subname.to_lowercase()
        );

        // A rename goes through and back.
        let name = crate::tags::record_name(&probe);
        let renamed = format!("{name}_gpui_check");
        rename_or_merge(&library, probe.tag_id, &renamed).unwrap();
        assert!(
            library
                .tags()
                .unwrap()
                .iter()
                .any(|tag| tag.tag_id == probe.tag_id && crate::tags::record_name(&tag) == renamed)
        );
        rename_or_merge(&library, probe.tag_id, &name).unwrap();

        // A group can be made, renamed and deleted.
        library.create_tag_namespace("gpui_check_group").unwrap();
        let group = library
            .tag_namespaces()
            .unwrap()
            .into_iter()
            .find(|ns| ns.name == "gpui_check_group")
            .unwrap();
        library
            .rename_or_merge_tag_namespace(group.namespace_id, "gpui_check_renamed")
            .unwrap();
        library.delete_tag_namespace(group.namespace_id).unwrap();
        assert!(
            library
                .tag_namespaces()
                .unwrap()
                .iter()
                .all(|ns| !ns.name.starts_with("gpui_check"))
        );
    }
}
