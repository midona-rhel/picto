//! The Tags screen (`TagManagerScreen.tsx`): a rail of tag groups beside the
//! tags of the chosen group, in letter sections. A tag opens its editor;
//! tags and groups have context menus; the titlebar holds the search.
mod dialogs;
mod menus;
pub mod model;
mod view;

use crate::backend::Screen;
use crate::grid::thumbnails::Thumbnails;
use crate::motion;
use crate::session::{Change, Report, Reread, Session};
use crate::tags::{Answer, Page, Preferences, TagPager, View as Browse};
use crate::ui::{
    menu::{self},
    modal::Presence,
    text_input::TextInput,
};
use dialogs::{Dialog, Layer};
use gpui::*;
use model::Rows;
use picto_library::query::ItemScope;
use picto_library::{Library, TagId, TagNamespaceRecord, TagRecord};
use std::time::Instant;

/// Tags fetched at a time.
const PAGE_SIZE: usize = 500;
/// `.namespaceRail`
const RAIL_WIDTH: f32 = 184.;
/// `.titlebarSearch`
const SEARCH_WIDTH: f32 = 125.;
/// A load near the end of what is loaded starts this many rows early.
const LOAD_AHEAD: usize = 4;

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
            ViewFade::Leaving(since) => 1. - eased(since, motion::TAG_VIEW_LEAVE),
            ViewFade::Hidden => 0.,
            ViewFade::Entering(since) => eased(since, motion::TAG_VIEW_ENTER),
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
    /// What the list shows: every tag, one group (`""` is General), or the
    /// starred tags, as loaded so far.
    pager: TagPager,
    loading: bool,
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

    /// The first page of what the list shows has arrived.
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
            // Shown even when the load failed, so the error is not left over
            // a hidden list.
            self.fade = ViewFade::Entering(Instant::now());
            self.list_shown = Instant::now();
        }
        self.loading = false;
        cx.notify();
    }

    /// The next page, once the list is scrolled near the end of what is
    /// loaded.
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
            ViewFade::Leaving(since) if since.elapsed() >= motion::TAG_VIEW_LEAVE => {
                self.fade = ViewFade::Hidden;
                if let Some(browse) = self.pending_browse.take() {
                    // `resetBrowse`
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

    // ── Edits ───────────────────────────────────────────────────

    /// `runMutation`: runs an edit on the library's thread, telling the user
    /// what `report` says (a delete is announced with an Undo button), then
    /// reloads. On success the editor closes if `close_editor`, or else
    /// returns from a merge or delete step; `then` follows.
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
            model.apply_now_reporting(
                |app| {
                    app.starred_tags = starred.clone();
                    app.tag_group_colors = colors.clone();
                },
                Some("Unable to save tag preferences"),
                cx,
            )
        });
        // The model tells every follower, this screen too.
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
            let Some(all) = session.nav.iter().position(|entry| {
                entry.screen.is_none() && !entry.random && entry.scope == ItemScope::All
            }) else {
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

        // The rail's counts add up to every tag, and General is the
        // namespace with no name.
        let namespaces = library.tag_namespaces().unwrap();
        let tags = library.tags().unwrap();
        let total = namespaces.iter().map(|ns| ns.tag_count).sum::<u64>();
        assert_eq!(total as usize, tags.len());
        let general = namespaces.iter().find(|ns| ns.name.is_empty()).unwrap();

        // Paging through General reaches exactly its tags.
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

        // A search puts the exact name first.
        let probe = tags.iter().find(|tag| !tag.subname.is_empty()).unwrap();
        let (found, _) = TagPager::new(super::PAGE_SIZE)
            .begin(&probe.subname, None)
            .fetch(&library)
            .unwrap();
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
