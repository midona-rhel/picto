//! The Tags screen. On the left is a rail with one row for each tag group.
//! On the right is the list of the tags of the chosen row, under a heading
//! for each first letter of the tags' names. A click on a tag opens the
//! tag's editor. Tags and groups have context menus. The titlebar holds the
//! field that searches the tags.
mod dialogs;
mod menus;
pub mod model;
mod view;

use crate::application;
use crate::backend::Screen;
use crate::grid::thumbnails::Thumbnails;
use crate::i18n::t;
use crate::motion;
use crate::session::{Change, Report, Reread, Session};
use crate::tags::{Answer, Page, Preferences, TagPager, View as Browse};
use crate::ui::{
    menu::{self},
    modal::Presence,
    text_input::{Kind, TextInput},
};
use dialogs::{Dialog, Layer};
use gpui::*;
use model::Rows;
use picto_library::query::ItemScope;
use picto_library::{Library, TagId, TagNamespaceRecord, TagRecord};
use std::time::{Duration, Instant};

/// The shortest time between two reads of the tags that changes to the
/// library's tags cause.
const INVALIDATION_SPACING: Duration = Duration::from_millis(500);

/// The number of tags that the screen asks the library for at a time.
const PAGE_SIZE: usize = 500;
/// The width in pixels of the rail of tag groups.
const RAIL_WIDTH: f32 = 184.;
/// The width in pixels of the search field in the titlebar.
const SEARCH_WIDTH: f32 = 125.;
/// The next page of tags is asked for when the last row that the list builds
/// is this many rows or fewer before the first row of tags that are not
/// loaded yet.
const LOAD_AHEAD: usize = 4;

/// The stage of the fade that the list of tags goes through when the user
/// chooses another row of the rail.
#[derive(Clone, Copy)]
enum ViewFade {
    Idle,
    /// The list has been fading out since this time. When the fade-out
    /// ends, the tags of the chosen row are asked for.
    Leaving(Instant),
    /// The list is not visible. It stays so until the first page of the
    /// tags of the chosen row arrives.
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
            ViewFade::Leaving(since) => 1. - eased(since, motion::TAG_VIEW_LEAVE),
            ViewFade::Hidden => 0.,
            ViewFade::Entering(since) => eased(since, motion::TAG_VIEW_ENTER),
        }
    }
}

/// The tag or the group that the open context menu belongs to.
enum MenuTarget {
    Tag(TagRecord),
    Group(TagNamespaceRecord),
}

/// The view that the titlebar shows while the Tags screen is open. It holds
/// the field that searches the tags.
pub struct TagsToolbar {
    search: Entity<TextInput>,
    _observe: Subscription,
}

impl TagsToolbar {
    /// Returns the focus handle of the search field. The shell gives it the
    /// keyboard focus when the user presses the `nav.search` shortcut on the
    /// Tags screen.
    pub fn search_focus(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

pub struct TagsScreen {
    session: Entity<Session>,
    _thumbnails: Entity<Thumbnails>,
    focus: FocusHandle,
    /// The view that the titlebar shows while this screen is open.
    toolbar: Entity<TagsToolbar>,
    query: String,

    // The fields from `namespaces` to `busy` hold what the screen has read
    // from the library, and the state of its reads and edits.
    namespaces: Vec<TagNamespaceRecord>,
    unused: u64,
    preferences: Preferences,
    /// The tags of the list that are loaded so far. The list shows every
    /// tag, the tags of one group (the group with the empty name is
    /// General), or the starred tags.
    pager: TagPager,
    loading: bool,
    summary_generation: u64,
    /// The row of the rail and the search text that tags were last asked
    /// for.
    identity: Option<(Browse, String)>,
    error: Option<String>,
    /// True while an edit of the library is running. The controls that
    /// start an edit are disabled during that time.
    busy: bool,

    browse: Browse,
    /// The row of the rail that the user chose. Its tags are asked for when
    /// the list has faded out.
    pending_browse: Option<Browse>,
    fade: ViewFade,
    /// The time at which the first page of tags last arrived for another
    /// row of the rail or another search text. The rows fade in from this
    /// time.
    list_shown: Instant,
    rows: Rows,
    rows_key: Option<(u64, usize, usize, bool)>,
    /// A number that goes up every time the loaded tags change. It is part
    /// of `rows_key`, so the rows are built again when it changes.
    tags_epoch: u64,

    scroll: ScrollHandle,
    rail_scroll: ScrollHandle,
    bounds: Bounds<Pixels>,
    clock: Instant,

    menu: menu::Host,
    menu_target: Option<MenuTarget>,
    /// The open dialogs. The last one is drawn over the others.
    layers: Vec<Layer>,
    /// True when the keyboard focus has to move in the next frame: to the
    /// topmost dialog, or to the screen when no dialog is open.
    refocus: bool,
    /// True while the session shows this screen. When it becomes true, the
    /// screen reads the tags again.
    showing: bool,
    /// The task that waits to read the tags again after a change to the
    /// library's tags, made on this screen or anywhere else.
    /// `invalidated_at` is the time at which the last such read started.
    invalidation: Option<Task<()>>,
    invalidated_at: Option<Instant>,
    _subscriptions: Vec<Subscription>,
}

impl TagsScreen {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            TextInput::new(Kind::Form, cx)
                .placeholder("Search tags")
                .submits()
        });
        let toolbar = cx.new(|cx| TagsToolbar {
            _observe: cx.observe(&search, |_, _, cx| cx.notify()),
            search: search.clone(),
        });
        let subscriptions = vec![
            cx.observe(&search, |this, search, cx| {
                let query = search.read(cx).value();
                if query != this.query {
                    this.query = query;
                    // A changed search text closes the tag editor.
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
                    // The tags may have changed while this screen was not
                    // shown, so they are read again.
                    this.refresh(cx);
                }
                this.showing = showing;
            }),
            cx.subscribe(&session, |this, _, event: &application::Event, cx| {
                if let application::Event::Changed(changed) = event
                    && application::touches(changed, "tags")
                {
                    this.invalidated(cx);
                }
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
            pager: TagPager::new(PAGE_SIZE),
            loading: true,
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
            menu: menu::Host::deferred(crate::ui::layer::MENU, menu::Return::To(focus)),
            menu_target: None,
            layers: Vec::new(),
            refocus: false,
            showing: false,
            invalidation: None,
            invalidated_at: None,
            _subscriptions: subscriptions,
        };
        screen.load_summary(cx);
        screen.load(cx);
        screen
    }

    /// Returns the view that the titlebar shows while this screen is open.
    pub fn toolbar(&self) -> Entity<TagsToolbar> {
        self.toolbar.clone()
    }

    // The functions below read the groups and the tags from the library.

    /// Reads the tag groups with the number of tags in each, and the number
    /// of tags that are assigned to no item.
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
                        // The Tags row of the sidebar shows the number of
                        // all tags.
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

    /// Asks for the first page of the tags of the chosen row of the rail
    /// and the search text. When the row or the search text is not the one
    /// that tags were last asked for, the rows fade in once the page
    /// arrives.
    fn load(&mut self, cx: &mut Context<Self>) {
        let identity = (self.browse.clone(), self.query.clone());
        let changed = self.identity.as_ref() != Some(&identity);
        self.identity = Some(identity);
        if changed {
            self.loading = true;
        }
        let page = self.pager.begin(self.query.trim(), self.browse.group());
        let then = move |this: &mut Self, page, answer, cx: &mut Context<Self>| {
            this.loaded(page, answer, changed, cx)
        };
        if self.browse != Browse::Starred {
            crate::tags::fetch_page(&self.session, page, None, cx, then).detach();
            return;
        }
        let (starred, query) = (self.preferences.starred.clone(), page.query.clone());
        let answer = self.session.read(cx).run(move |library| {
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
        });
        cx.spawn(async move |this, cx| {
            let Ok(answer) = answer.await else { return };
            let _ = this.update(cx, |this, cx| then(this, page, answer, cx));
        })
        .detach();
    }

    /// Runs when the first page of tags arrives. `changed` is true when the
    /// page is for another row of the rail or another search text than the
    /// tags that were asked for before it.
    fn loaded(&mut self, page: Page, answer: Answer, changed: bool, cx: &mut Context<Self>) {
        if !self.pager.current(&page) {
            return;
        }
        match answer {
            Ok((tags, cursor)) => {
                self.pager.first(&page, tags, cursor);
                self.tags_epoch += 1;
            }
            Err(error) => self.error = Some(error),
        }
        if changed {
            // The list fades in also when the load failed, so that the
            // error message is not shown above a list that stays invisible.
            self.fade = ViewFade::Entering(Instant::now());
            self.list_shown = Instant::now();
        }
        self.loading = false;
        cx.notify();
    }

    /// Asks for the next page of tags. The list calls this when it is
    /// scrolled near the end of the loaded tags.
    fn load_more(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.pager.next().filter(|_| !self.loading) else {
            return;
        };
        crate::tags::fetch_page(&self.session, page, None, cx, |this, page, answer, cx| {
            if !this.pager.current(&page) {
                return;
            }
            match &answer {
                Ok(_) => this.tags_epoch += 1,
                Err(error) => this.error = Some(error.clone()),
            }
            this.pager.more(&page, answer);
            cx.notify();
        })
        .detach();
    }

    /// Reads the tags and the groups again. What the screen shows stays
    /// until the answers arrive.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load(cx);
        self.load_summary(cx);
    }

    /// Runs when the library's tags changed. While the screen is shown, it
    /// reads them again, at most once every `INVALIDATION_SPACING`. While
    /// the screen is not shown, it does nothing, and the tags are read when
    /// the screen is shown again.
    fn invalidated(&mut self, cx: &mut Context<Self>) {
        if !self.showing || self.invalidation.is_some() {
            return;
        }
        let delay = self.invalidated_at.map_or(Duration::ZERO, |at| {
            INVALIDATION_SPACING.saturating_sub(at.elapsed())
        });
        self.invalidation = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |this, cx| {
                this.invalidation = None;
                this.invalidated_at = Some(Instant::now());
                if this.showing {
                    this.refresh(cx);
                }
            });
        }));
    }

    /// Returns the number of tags that the list will hold when all of them
    /// are loaded. The list draws placeholders for the tags that are not
    /// loaded yet.
    fn expected(&self) -> usize {
        let total = |namespaces: &[TagNamespaceRecord]| {
            namespaces.iter().map(|ns| ns.tag_count).sum::<u64>() as usize
        };
        if !self.query.trim().is_empty() {
            self.pager.tags.len()
        } else {
            match &self.browse {
                Browse::Starred => self.preferences.starred.len(),
                Browse::Group(name) => self
                    .namespaces
                    .iter()
                    .find(|ns| ns.name == *name)
                    .map_or(self.pager.tags.len(), |ns| ns.tag_count as usize),
                _ => total(&self.namespaces),
            }
        }
    }

    // The functions below change which row of the rail the list shows.

    /// Makes the list show the tags of another row of the rail. The list
    /// first fades out for 100 ms (`motion::TAG_VIEW_LEAVE`), and then the
    /// tags of `browse` are asked for.
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

    /// Moves the fade of the list to its next stage when the present stage
    /// has lasted its full time. When the fade-out has ended, it asks for
    /// the tags of the row that the user chose.
    fn advance_fade(&mut self, cx: &mut Context<Self>) {
        match self.fade {
            ViewFade::Leaving(since) if since.elapsed() >= motion::TAG_VIEW_LEAVE => {
                self.fade = ViewFade::Hidden;
                if let Some(browse) = self.pending_browse.take() {
                    // The loaded tags of the row shown before are dropped
                    // and the tag editor is closed, and then the tags of the
                    // chosen row are asked for.
                    self.browse = browse;
                    self.pager.tags.clear();
                    self.tags_epoch += 1;
                    self.pager.cursor = None;
                    self.error = None;
                    self.close_editor(cx);
                    self.load(cx);
                }
            }
            ViewFade::Entering(since) if since.elapsed() >= motion::TAG_VIEW_ENTER => {
                self.fade = ViewFade::Idle;
            }
            _ => {}
        }
    }

    // The functions below change tags, groups and the tag preferences.

    /// Runs the edit `work` on the library's thread and notifies the user
    /// as `report` says. A delete is reported with a notification that has
    /// an Undo button. When the edit succeeds, the screen reads the tags
    /// again. It then closes the tag editor if `close_editor` is true, and
    /// otherwise only the merge or delete dialog that is open over the
    /// editor. Last, it calls `then`. When the edit fails, the error is
    /// shown at the top of the screen.
    fn mutate(
        &mut self,
        report: Report,
        work: impl FnOnce(&Library) -> Result<(), String> + Send + 'static,
        close_editor: bool,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.error = None;
        crate::session::edit_then(
            &self.session,
            report,
            Reread::NOTHING,
            work,
            cx,
            move |this, result, cx| {
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
            },
        );
    }

    /// Runs when the application settings were changed, on this screen or
    /// in the Settings window. It takes over the tag preferences. When the
    /// starred tags changed and the list shows them, it reads them again.
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

    /// Stores the tag preferences `next` through the settings model, which
    /// holds the library's application settings and writes the ones that
    /// changed.
    fn save_preferences(&mut self, next: Preferences, cx: &mut Context<Self>) {
        let colors = next.colors_json();
        let starred = next.starred.clone();
        crate::settings::model(cx).update(cx, |model, cx| {
            model.apply_now_reporting(
                |app| {
                    app.starred_tags = starred.clone();
                    app.tag_group_colors = colors.clone();
                },
                Some(t("Unable to save tag preferences")),
                cx,
            )
        });
        // The screen takes over the new preferences at once and does not
        // wait for the settings model to report the change.
        self.follow_settings(cx);
    }

    /// After a tag was renamed, puts `next_name` in place of `previous` in
    /// the list of starred tags, if `previous` is in it.
    fn rename_star(&mut self, previous: &str, next_name: &str, cx: &mut Context<Self>) {
        crate::tags::rename_star(previous, next_name, cx);
        self.follow_settings(cx);
    }

    fn set_group_color(&mut self, group: &str, color: Option<&str>, cx: &mut Context<Self>) {
        let mut next = self.preferences.clone();
        next.colors = self.preferences.with_color(group, color);
        self.save_preferences(next, cx);
    }

    /// Renames a tag, or merges it into the tag that already has the name
    /// (`crate::tags::rename_or_merge`). The tag editor is closed afterwards
    /// if `close` is true.
    fn rename_tag(&mut self, tag: &TagRecord, name: String, close: bool, cx: &mut Context<Self>) {
        let previous = crate::tags::record_name(tag);
        let id = tag.tag_id;
        let next_name = name.trim().to_owned();
        let star_name = next_name.clone();
        self.mutate(
            Report::default(),
            move |library| crate::tags::rename_or_merge(library, id, &next_name),
            close,
            move |this, cx| this.rename_star(&previous, &star_name, cx),
            cx,
        );
    }

    fn merge_tag(&mut self, tag: &TagRecord, target: &TagRecord, cx: &mut Context<Self>) {
        let (source, destination) = (tag.tag_id, target.tag_id);
        self.mutate(
            Report::default(),
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
            Report::announce("tags.delete"),
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
            Report::default(),
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
            Report::announce("tags.group.delete"),
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
            Report::default(),
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
            Report::announce("tags.delete_unused"),
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

    /// Removes from the starred tags every name that no tag of the library
    /// has any more. The library has already removed the deleted tags from
    /// the list of starred tags that it stores. The settings model holds a
    /// copy of that list, and this function removes them from the copy too.
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

    /// Shows the All grid with a filter that leaves only the items that
    /// have the tag.
    fn show_items(&mut self, tag_id: TagId, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            let Some(all) = session.nav.iter().position(|entry| {
                entry.screen.is_none() && !entry.random && entry.scope == ItemScope::All
            }) else {
                return;
            };
            // The filter holds the tag's name, which the filter bar shows.
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

    // The functions below open and close the dialogs.

    fn push(&mut self, dialog: Dialog, cx: &mut Context<Self>) {
        self.layers.push(Layer {
            dialog,
            presence: Presence::new(),
            focus: cx.focus_handle(),
        });
        self.refocus = true;
        cx.notify();
    }

    /// Opens the editor of `tag`. When `step` is given, that merge or delete
    /// dialog is opened over the editor. An editor that is already open is
    /// closed first.
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

    /// Closes the tag editor and the merge or delete dialog that is open
    /// over it. They do not fade out.
    fn close_editor(&mut self, cx: &mut Context<Self>) {
        let before = self.layers.len();
        self.drop_layers(Dialog::belongs_to_editor);
        if self.layers.len() != before {
            cx.notify();
        }
    }

    /// Closes the merge or delete dialog that is open over the tag editor.
    /// The editor stays open.
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
}

#[cfg(test)]
mod tests {
    use crate::tags::{TagPager, rename_or_merge};
    use picto_library::Library;
    use std::path::PathBuf;

    #[test]
    #[ignore = "Requires PICTO_GPUI_TEST_LIBRARY naming a disposable copy made by run.sh"]
    fn real_snapshot_tags_browse_rename_and_groups() {
        let root = PathBuf::from(std::env::var("PICTO_GPUI_TEST_LIBRARY").unwrap());
        let root = crate::application::guard(&root).unwrap();
        let library = Library::open(root.join("library.sqlite")).unwrap();

        // The counts of the groups add up to the number of all tags, and
        // General is the group with the empty name.
        let namespaces = library.tag_namespaces().unwrap();
        let tags = library.tags().unwrap();
        let total = namespaces.iter().map(|ns| ns.tag_count).sum::<u64>();
        assert_eq!(total as usize, tags.len());
        let general = namespaces.iter().find(|ns| ns.name.is_empty()).unwrap();

        // Reading the General group page by page loads all of its tags and
        // no tag of another group.
        let mut pager = TagPager::new(super::PAGE_SIZE);
        let first = pager.begin("", Some(String::new()));
        let (page, next) = first.fetch(&library).unwrap();
        assert!(pager.first(&first, page, next));
        while let Some(page) = pager.next() {
            let answer = page.fetch(&library);
            assert!(pager.more(&page, answer));
        }
        assert!(pager.tags.iter().all(|tag| tag.namespace.is_empty()));
        assert_eq!(pager.tags.len() as u64, general.tag_count);

        // A search for a tag's exact name lists that tag first.
        let probe = tags.iter().find(|tag| !tag.subname.is_empty()).unwrap();
        let (found, _) = TagPager::new(super::PAGE_SIZE)
            .begin(&probe.subname, None)
            .fetch(&library)
            .unwrap();
        assert_eq!(
            found[0].subname.to_lowercase(),
            probe.subname.to_lowercase()
        );

        // A tag can be renamed, and then renamed back to its old name.
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
