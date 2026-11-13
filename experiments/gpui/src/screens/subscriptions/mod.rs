//! The Subscriptions screen (`SubscriptionsScreen.tsx`): a grid of the
//! library's subscriptions, and one subscription's workspace when a card is
//! opened. Opening a subscription is a place of the screen, so Back returns
//! to the grid and the scope title names it.
mod data;
mod detail;
mod format;
mod home;

use crate::backend::Screen;
use crate::grid::thumbnails::Thumbnails;
use crate::session::{Change, Session};
use crate::ui::menu::{self, Menu};
use crate::ui::text_input::{Committed, TextInput};
use gpui::*;
use std::{cell::Cell, collections::HashSet, rc::Rc};

actions!(subscriptions, [Open, Deselect]);

/// The screen's own keys, over the browser's: Enter opens the selected
/// subscription and Escape clears the selection. Like the registry's keys,
/// they give way to a text field, a menu, a popover or a dialog.
pub fn bind(cx: &mut App) {
    let context = Some("Subscriptions && !TextInput && !Menu && !Modal && !Popover");
    cx.bind_keys([
        KeyBinding::new("enter", Open, context),
        KeyBinding::new("escape", Deselect, context),
    ]);
}

/// The tabs of a subscription's data plane.
const TABS: [&str; 3] = ["Sources", "History", "Problems"];
const SOURCES: usize = 0;
const HISTORY: usize = 1;

pub struct SubscriptionsScreen {
    session: Entity<Session>,
    /// Covers decode into a cache of their own, so scrolling the grid never
    /// evicts an image this screen still shows.
    covers: Entity<Thumbnails>,
    focus: FocusHandle,
    /// Every subscription, once the first read has arrived. A newer read
    /// replaces it only when it arrives, so nothing blanks in between.
    list: Option<Vec<data::Subscription>>,
    error: Option<String>,
    generation: u64,
    /// Selected cards, by subscription.
    selected: HashSet<i64>,
    /// The card a shift-click selects from, by position in the grid.
    anchor: Option<usize>,
    /// The open subscription's History and Problems, and which one they are
    /// for; another subscription's stay until the new ones arrive.
    detail: Option<data::Detail>,
    detail_generation: u64,
    loading_more: bool,
    tab: usize,
    /// The name field of a rename in place.
    rename: Entity<TextInput>,
    /// The subscription whose name is being edited.
    renaming: Option<i64>,
    /// The open context menu, which stays while it fades out.
    menu: Option<Entity<Menu>>,
    _menu: Option<Subscription>,
    /// The subscriptions the open menu acts on.
    menu_targets: Vec<i64>,
    /// Whether the screen showed at the last navigation, to read the list
    /// again each time it is opened.
    shown: bool,
    /// The place showing at the last navigation.
    place_seen: Option<i64>,
    /// The grid's content width and the data plane's width, which the
    /// container-relative CSS sizes are measured against.
    home_width: Rc<Cell<f32>>,
    plane_width: Rc<Cell<f32>>,
    /// Where the rail's "More actions" button is, to open its menu below it.
    overflow: Rc<Cell<Bounds<Pixels>>>,
    /// Where the name field of a rename is; a press anywhere else ends it.
    rename_bounds: Rc<Cell<Bounds<Pixels>>>,
    home_scroll: ScrollHandle,
    rail_scroll: ScrollHandle,
    table_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl SubscriptionsScreen {
    pub fn new(
        session: Entity<Session>,
        _thumbnails: Entity<Thumbnails>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let rename = cx.new(|cx| TextInput::rename(crate::theme::TEXT_MD, window, cx));
        let rename_focus = rename.read(cx).focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe(&session, |this, _, change: &Change, cx| {
                if *change == Change::Navigation {
                    this.navigated(cx);
                }
            }),
            // The field is keyed by the subscription it renames, so a commit
            // that arrives after the field closed still knows its target.
            cx.subscribe(&rename, |this, _, Committed(id, name), cx| {
                this.commit_rename(*id as i64, name.clone(), cx);
            }),
            cx.on_blur(&rename_focus, window, |this, window, cx| {
                this.renaming = None;
                window.focus(&this.focus, cx);
                cx.notify();
            }),
        ];
        let mut screen = Self {
            session,
            covers: cx.new(Thumbnails::new),
            focus: cx.focus_handle(),
            list: None,
            error: None,
            generation: 0,
            selected: HashSet::new(),
            anchor: None,
            detail: None,
            detail_generation: 0,
            loading_more: false,
            tab: SOURCES,
            rename,
            renaming: None,
            menu: None,
            _menu: None,
            menu_targets: Vec::new(),
            shown: false,
            place_seen: None,
            home_width: Rc::new(Cell::new(0.)),
            plane_width: Rc::new(Cell::new(0.)),
            overflow: Rc::new(Cell::new(Bounds::default())),
            rename_bounds: Rc::new(Cell::new(Bounds::default())),
            home_scroll: ScrollHandle::new(),
            rail_scroll: ScrollHandle::new(),
            table_scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        // Read the list at once, so it is there before the screen is opened.
        screen.load(cx);
        screen
    }

    /// Reads every subscription again. What shows stays until the answer
    /// arrives.
    fn load(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let blobs = self.session.read(cx).blobs().to_path_buf();
        let answer = self.session.read(cx).run(move |library| {
            data::subscriptions(library, &blobs).map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok(list) => {
                        this.error = None;
                        // A subscription that is gone is no longer selected.
                        this.selected
                            .retain(|id| list.iter().any(|entry| entry.id == *id));
                        this.list = Some(list);
                    }
                    Err(error) => this.error = Some(error),
                }
                this.name_place(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads the open subscription's runs and issues.
    fn load_detail(&mut self, id: i64, cx: &mut Context<Self>) {
        self.detail_generation += 1;
        let generation = self.detail_generation;
        self.loading_more = false;
        let answer = self
            .session
            .read(cx)
            .run(move |library| data::detail(library, id).map_err(|error| error.to_string()));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(detail)) = answer.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.detail_generation == generation {
                    this.detail = Some(detail);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Adds the next page of open issues to the Problems tab.
    fn load_more_issues(&mut self, cx: &mut Context<Self>) {
        let Some((id, after)) = self
            .detail
            .as_ref()
            .and_then(|detail| Some((detail.subscription, detail.issues.next.clone()?)))
        else {
            return;
        };
        if self.loading_more {
            return;
        }
        self.loading_more = true;
        let generation = self.detail_generation;
        let answer = self.session.read(cx).run(move |library| {
            data::more_issues(library, id, after).map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                this.loading_more = false;
                if this.detail_generation != generation {
                    return;
                }
                if let (Ok(Ok(page)), Some(detail)) = (result, this.detail.as_mut()) {
                    detail.issues.items.extend(page.items);
                    detail.issues.next = page.next;
                    detail.issues.total = page.total;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Follows the session's navigation: reads the list each time the
    /// screen is opened, and the detail when a subscription is.
    fn navigated(&mut self, cx: &mut Context<Self>) {
        let (showing, place) = {
            let session = self.session.read(cx);
            let showing = session
                .nav
                .get(session.scope)
                .is_some_and(|entry| entry.screen == Some(Screen::Subscriptions));
            (showing, session.place())
        };
        let entering = showing && !self.shown;
        if entering {
            self.load(cx);
        }
        if !showing {
            self.close_menu(cx);
        }
        self.shown = showing;
        if showing {
            if entering || place != self.place_seen {
                // The grid opens with nothing selected, as it does each time
                // the Electron grid mounts.
                self.selected.clear();
                self.anchor = None;
                if let Some(id) = place {
                    if self.detail.as_ref().map(|detail| detail.subscription) != Some(id) {
                        self.tab = SOURCES;
                    }
                    self.load_detail(id, cx);
                }
            }
            self.place_seen = place;
            self.name_place(cx);
        }
        cx.notify();
    }

    /// The opened subscription, when it is one the library has. The screen
    /// keeps showing the place it had while it fades out.
    fn opened(&self) -> Option<&data::Subscription> {
        let place = self.place_seen?;
        self.list.as_ref()?.iter().find(|entry| entry.id == place)
    }

    /// Gives the scope title the open subscription's name.
    fn name_place(&self, cx: &mut Context<Self>) {
        let label = self
            .opened()
            .map(|subscription| SharedString::from(subscription.name.clone()));
        self.session
            .update(cx, |session, cx| session.set_place_label(label, cx));
    }

    /// Opens a subscription's workspace, as a step of Back and Forward.
    fn open(&mut self, id: i64, cx: &mut Context<Self>) {
        self.close_menu(cx);
        self.session
            .update(cx, |session, cx| session.open_place(Some(id), cx));
    }

    // ── Rename in place ─────────────────────────────────────────

    /// Edits a name in place, in a field the size of the name it replaces.
    fn start_rename(&mut self, id: i64, size: f32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self
            .list
            .as_ref()
            .and_then(|list| list.iter().find(|entry| entry.id == id))
            .map(|entry| entry.name.clone())
        else {
            return;
        };
        self.renaming = Some(id);
        self.rename.update(cx, |input, cx| {
            input.set_rename_size(size, cx);
            input.set_field(id as u64, name, cx);
            input.select_everything(cx);
        });
        window.focus(&self.rename.focus_handle(cx), cx);
        cx.notify();
    }

    /// Shows the new name at once and writes it through the library.
    fn commit_rename(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return;
        }
        if let Some(entry) = self
            .list
            .as_mut()
            .and_then(|list| list.iter_mut().find(|entry| entry.id == id))
        {
            entry.name = name.clone();
        }
        self.name_place(cx);
        let answer = self.session.read(cx).run(move |library| {
            data::rename(library, id, &name).map_err(|error| error.to_string())
        });
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(Err(error)) = result {
                    this.error = Some(error);
                }
                // Read back what the library holds now.
                this.load(cx);
            });
        })
        .detach();
        cx.notify();
    }

    // ── Selection ───────────────────────────────────────────────

    /// Click, Cmd-click and Shift-click on the card at `index` of the grid's
    /// order, as `SubscriptionsGrid` handles them.
    fn click_card(
        &mut self,
        index: usize,
        order: &[i64],
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some(&id) = order.get(index) else {
            return;
        };
        if modifiers.shift
            && let Some(anchor) = self.anchor
        {
            let range = order[anchor.min(index)..=anchor.max(index).min(order.len() - 1)]
                .iter()
                .copied();
            if !modifiers.platform {
                self.selected.clear();
            }
            self.selected.extend(range);
        } else if modifiers.platform {
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
            self.anchor = Some(index);
        } else {
            self.selected = [id].into();
            self.anchor = Some(index);
        }
        cx.notify();
    }

    // ── Context menu ────────────────────────────────────────────

    /// Opens the menu for `targets` at `position`: the one subscription's
    /// menu, or the bulk menu for several (`subscriptionsContextMenu.tsx`).
    /// Rows this prototype cannot perform are shown and not offered.
    fn open_menu(
        &mut self,
        targets: Vec<i64>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(list) = self.list.as_ref() else {
            return;
        };
        let chosen: Vec<&data::Subscription> = targets
            .iter()
            .filter_map(|id| list.iter().find(|entry| entry.id == *id))
            .collect();
        let entries: Vec<menu::Entry> = match chosen.as_slice() {
            [] => return,
            [subscription] => {
                let active = subscription.active_run_id.is_some();
                let running = format::running(subscription);
                let paused = subscription.run_status.as_deref() == Some("paused");
                // TODO: run, pause, resume, stop and hold through the
                // subscription runner.
                let (run_label, run_icon) = match (active, running) {
                    (true, true) => ("Pause", "menu-player-pause"),
                    (true, false) if paused => ("Resume", "menu-player-play"),
                    (true, false) => ("Retry", "menu-refresh"),
                    (false, _) => ("Run Now", "menu-player-play"),
                };
                let (hold_label, hold_icon) = if active {
                    ("Stop", "menu-player-stop")
                } else if subscription.paused {
                    ("Release Hold", "menu-player-play")
                } else {
                    ("Hold", "menu-hand-stop")
                };
                vec![
                    menu::item("run", run_label)
                        .icon(run_icon)
                        .disabled(true)
                        .into(),
                    menu::item("hold", hold_label)
                        .icon(hold_icon)
                        .disabled(true)
                        .into(),
                    menu::Entry::Separator,
                    menu::item("rename", "Rename").icon("menu-rename").into(),
                    // TODO: the cover dialog, to frame a downloaded image.
                    menu::item("cover", "Set Cover Photo…")
                        .icon("menu-photo-edit")
                        .disabled(true)
                        .into(),
                    // TODO: a submenu of Manual, Daily, Weekly and Monthly.
                    menu::item("schedule", "Schedule")
                        .icon("menu-clock")
                        .disabled(true)
                        .into(),
                    menu::Entry::Separator,
                    // TODO: confirm, then clear the sync history.
                    menu::item("reset", "Reset Sync History…")
                        .icon("menu-refresh")
                        .disabled(true)
                        .into(),
                    menu::Entry::Separator,
                    // TODO: confirm, then delete the subscription; its files
                    // stay in the library.
                    menu::item("delete", "Delete Subscription…")
                        .icon("menu-trash")
                        .danger(true)
                        .disabled(true)
                        .into(),
                ]
            }
            several => {
                let total = several.len();
                // TODO: the bulk actions, through the subscription runner.
                vec![
                    menu::item("run", format!("Run {total} Now"))
                        .icon("menu-player-play")
                        .disabled(true)
                        .into(),
                    menu::item("hold", format!("Hold {total}"))
                        .icon("menu-hand-stop")
                        .disabled(true)
                        .into(),
                    menu::item("release", format!("Release Hold for {total}"))
                        .icon("menu-player-play")
                        .disabled(true)
                        .into(),
                    menu::item("schedule", "Schedule")
                        .icon("menu-clock")
                        .disabled(true)
                        .into(),
                    menu::Entry::Separator,
                    menu::item("delete", format!("Delete {total}…"))
                        .icon("menu-trash")
                        .danger(true)
                        .disabled(true)
                        .into(),
                ]
            }
        };
        let menu = cx.new(|cx| Menu::new(entries, position, cx));
        self._menu =
            Some(
                cx.subscribe_in(&menu, window, |this, _, event: &menu::Event, window, cx| {
                    let menu::Event::Chosen(action) = event;
                    if *action == "rename"
                        && let [id] = this.menu_targets.as_slice()
                    {
                        let id = *id;
                        // The rail shows the name larger than a card does.
                        let size = if this.place_seen.is_some() {
                            crate::theme::TEXT_XL
                        } else {
                            crate::theme::TEXT_MD
                        };
                        this.start_rename(id, size, window, cx);
                    }
                }),
            );
        self.menu_targets = targets;
        window.focus(&menu.focus_handle(cx), cx);
        self.menu = Some(menu);
        cx.notify();
    }

    /// The menu for a right-clicked card: the whole selection when the card
    /// is part of it, else the card alone, which becomes the selection.
    fn card_menu(
        &mut self,
        index: usize,
        order: &[i64],
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(&id) = order.get(index) else {
            return;
        };
        if !self.selected.contains(&id) {
            self.selected = [id].into();
            self.anchor = Some(index);
        }
        let targets = order
            .iter()
            .copied()
            .filter(|id| self.selected.contains(id))
            .collect();
        self.open_menu(targets, position, window, cx);
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = &self.menu {
            menu.update(cx, |menu, cx| menu.dismiss(cx));
        }
    }

    /// The open menu, laid over the whole window as the shell lays the
    /// grid's, until its fade-out is over.
    fn menu_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(menu) = self.menu.take_if(|menu| menu.read(cx).finished()) {
            self._menu = None;
            // Unless its action moved the keyboard, as Rename does. A menu
            // shown in pop-up windows can outlive this by a moment, still
            // holding the keyboard.
            if window.focused(cx).is_none() || menu.focus_handle(cx).is_focused(window) {
                window.focus(&self.focus, cx);
            }
        }
        let menu = self.menu.clone()?;
        let viewport = window.viewport_size();
        Some(
            deferred(
                anchored()
                    .position(point(px(0.), px(0.)))
                    .position_mode(AnchoredPositionMode::Window)
                    .child(div().w(viewport.width).h(viewport.height).child(menu)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

impl Render for SubscriptionsScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = crate::theme::theme(cx);
        let place = self.place_seen;
        let body = match (&self.list, place) {
            // Nothing is drawn until the first read arrives.
            (None, _) => div().into_any_element(),
            (Some(_), None) => self.home(window, cx).into_any_element(),
            (Some(_), Some(_)) => match self.opened().cloned() {
                Some(subscription) => self.workspace(&subscription, window, cx).into_any_element(),
                None => home::empty_state(cx).into_any_element(),
            },
        };
        let menu = self.menu_layer(window, cx);
        div()
            .id("subscriptions")
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .text_color(theme.text())
            .track_focus(&self.focus)
            .key_context("Subscriptions")
            // A press takes the keyboard to the screen, which ends a rename
            // unless it lands in the name field.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    if this.renaming.is_none()
                        || !this.rename_bounds.get().contains(&event.position)
                    {
                        window.focus(&this.focus, cx);
                    }
                }),
            )
            .on_action(cx.listener(|this, _: &Open, _, cx| {
                let order = this.card_order();
                let chosen = order
                    .iter()
                    .copied()
                    .filter(|id| this.selected.contains(id))
                    .collect::<Vec<_>>();
                if this.place_seen.is_none()
                    && let [id] = chosen.as_slice()
                {
                    let id = *id;
                    this.open(id, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &Deselect, _, cx| {
                if this.selected.is_empty() || this.place_seen.is_some() {
                    cx.propagate();
                } else {
                    this.selected.clear();
                    cx.notify();
                }
            }))
            .child(body)
            .children(menu)
    }
}

impl SubscriptionsScreen {
    /// The name field of a rename in place, which notes where it is.
    fn rename_field(&self, id: &'static str) -> Div {
        let bounds = self.rename_bounds.clone();
        div()
            .relative()
            .w_full()
            .child(crate::ui::field(
                id,
                &self.rename,
                crate::ui::field::Look::Rename,
            ))
            .child(
                canvas(move |at, _, _| bounds.set(at), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
    }
}

/// Keeps a measured width in `cell` and repaints `view` when it changes.
fn measure(cell: Rc<Cell<f32>>, window: &Window) -> impl IntoElement {
    let view = window.current_view();
    canvas(
        move |bounds, _, cx| {
            let width = f32::from(bounds.size.width);
            if (cell.replace(width) - width).abs() > 0.5 {
                cx.notify(view);
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
