//! Draws the scrolling content of the inspector rail. The rail describes the
//! item that is selected or open in the viewer, or gives a summary of
//! several selected items, or, when nothing is selected, describes the
//! destination the grid shows.
mod colors;
mod edits;
pub mod folder_dialog;
pub mod folder_picker;
mod item;
mod membership;
mod scope;
mod selection;
mod sources;
mod stack;
pub mod tag_dialog;
pub mod tag_picker;
mod turned;

pub use folder_dialog::FolderDialog;
pub use folder_picker::FolderPicker;
pub use tag_dialog::TagDialog;
pub use tag_picker::TagPicker;

use crate::grid::{format, thumbnails::Thumbnails};
use crate::i18n::format::grouped;
use crate::i18n::t;
use crate::session::{Change, Session};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    field::Look,
    menu, modal,
    text_input::{self, Kind, TextInput},
};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::selection::SelectionTarget;
use picto_library::{FolderId, RootDetails, RootId, TagRecord};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Returns the id of the destination the grid shows. The id stays the same
/// when the list of destinations is rebuilt, which the destination's position
/// in the list does not.
fn displayed_id(session: &Session) -> Option<String> {
    session.nav.get(session.displayed).map(|entry| entry.id())
}

/// Says what the rail describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// The rail describes nothing, because the destination is a screen of its
    /// own and not a grid.
    Empty,
    /// Nothing is selected, and the rail describes the destination the grid
    /// shows.
    Scope,
    Item,
    /// Several items are selected, and the rail gives a summary of them.
    Selection,
}

/// Returns what the rail describes when it follows the selection: nothing on
/// a destination that is a screen of its own, otherwise the item that is open
/// in the viewer or is the only one selected, otherwise the summary of
/// several selected items, otherwise the grid's destination.
fn wanted(screen: bool, viewer: bool, selected: usize) -> Mode {
    if screen {
        Mode::Empty
    } else if viewer || selected == 1 {
        Mode::Item
    } else if selected > 1 {
        Mode::Selection
    } else {
        Mode::Scope
    }
}

/// The event the inspector emits when its preview is right-clicked. It holds
/// the position of the pointer in the window.
pub struct PreviewMenuRequested(pub Point<Pixels>);

impl EventEmitter<PreviewMenuRequested> for Inspector {}

/// Says what a click on a star rates: one item, or every selected item.
#[derive(Clone, Copy)]
enum Rate {
    Item(RootId),
    Selection,
}

/// The colour of the chip of a folder that has no colour of its own.
pub const FOLDER_COLOR: u32 = theme::FOLDER_DEFAULT;
/// The width and height, in pixels, of the square that the preview fits in.
const PREVIEW: f32 = 200.;

/// Returns the chip of a tag, in the colour of the tag's namespace. When the
/// setting `show_tag_prefixes` is off, the chip shows the tag without its
/// namespace.
pub(crate) fn tag_chip(tag: &picto_library::TagRecord, cx: &App) -> ui::Chip {
    let show_prefixes = crate::settings::current(cx).show_tag_prefixes;
    ui::chip(
        ("inspector-tag", tag.tag_id.0 as usize),
        tag.subname.clone(),
        crate::tags::chip_color(&tag.namespace),
    )
    .namespace(
        (show_prefixes && crate::tags::chip_shows_namespace(&tag.namespace))
            .then(|| tag.namespace.clone().into()),
    )
}

/// Returns the width and height of a preview: the media's dimensions scaled
/// down to fit the square of `PREVIEW` pixels, and never scaled up. Without
/// dimensions it is the whole square.
fn fitted(dimensions: Option<(u32, u32)>) -> (f32, f32) {
    match dimensions {
        Some((width, height)) if width > 0 && height > 0 => {
            let scale = (PREVIEW / width as f32)
                .min(PREVIEW / height as f32)
                .min(1.);
            (width as f32 * scale, height as f32 * scale)
        }
        _ => (PREVIEW, PREVIEW),
    }
}

/// Returns the time as `YYYY/MM/DD HH:MM` in local time.
fn format_date(milliseconds: i64) -> Option<String> {
    use chrono::TimeZone;
    let date = chrono::Local.timestamp_millis_opt(milliseconds).single()?;
    Some(date.format(crate::text::date_format::INSPECTOR).to_string())
}

/// Says what the open context menu belongs to.
#[derive(Clone)]
enum ChipMenu {
    Tag(TagRecord),
    Folder(FolderId),
    /// A palette swatch. The menu's handler holds the swatch's colour.
    Swatch,
}

/// Holds a change to every selected item that would replace notes or sources
/// that some of them already have. It waits here until the user confirms it.
enum Overwrite {
    Notes(String),
    Sources(Vec<String>),
}

struct Confirm {
    overwrite: Overwrite,
    count: u64,
}

pub struct Inspector {
    session: Entity<Session>,
    thumbnails: Entity<Thumbnails>,
    /// The picture of a torn page that is drawn for a cover whose thumbnail
    /// cannot be loaded.
    art: Option<crate::grid::art::Art>,
    name: Entity<TextInput>,
    notes: Entity<TextInput>,
    collapsed: HashSet<&'static str>,
    /// While this is true, the rail keeps what it shows and does not follow
    /// the selection.
    pinned: bool,
    /// After the rail is unpinned, it keeps what it shows until the selection
    /// or the destination next changes. This holds the position of the
    /// destination that was shown when the rail was unpinned, and `None` when
    /// the rail is not waiting for such a change.
    holding: Option<usize>,
    tag_picker: Entity<TagPicker>,
    folder_picker: Entity<FolderPicker>,
    /// The AI tagger's review, which opens beside the rail as the pickers do.
    ai_tagger: Entity<crate::ai_tagger::AiTagger>,
    /// The Add Tags dialog and the Folders dialog, which the grid's shortcuts
    /// and menu open.
    tag_dialog: Entity<TagDialog>,
    folder_dialog: Entity<FolderDialog>,
    /// Where the rail and its add buttons, sections and fields were last
    /// drawn, in window coordinates. The pickers open beside these places.
    places: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
    sources: sources::SourcesState,
    menu: menu::Host,
    menu_target: Option<ChipMenu>,
    /// The colour of the swatch that was copied last. Its tooltip says
    /// "Copied!" for 1.5 seconds.
    copied: Option<u32>,
    confirm: modal::Host<Confirm>,
    /// The id of the destination that was shown when the pickers were opened.
    /// The pickers close when the destination changes.
    picker_destination: Option<String>,
    /// The star the pointer is over. The stars up to it are drawn filled.
    hovered_star: Option<usize>,
    scroll: ScrollHandle,
    /// The item the rail describes, or the one it described last.
    item: Option<RootDetails>,
    /// What the rail shows. A new state replaces it only when the data of the
    /// new state has arrived, so that the rail is never blank in between.
    shown: Mode,
    scope: scope::ScopeState,
    selection: selection::SelectionState,
    _subscriptions: Vec<Subscription>,
}

impl Inspector {
    pub fn new(
        session: Entity<Session>,
        thumbnails: Entity<Thumbnails>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| {
            TextInput::new(Kind::Inspector, cx)
                .placeholder("Name")
                .commits_on_blur(window, cx)
                .expanding()
        });
        let notes = cx.new(|cx| {
            TextInput::new(Kind::Inspector, cx)
                .placeholder("Notes")
                .commits_on_blur(window, cx)
                .expanding()
        });
        let scope = scope::ScopeState::new(window, cx);
        let selection = selection::SelectionState::new(window, cx);
        let tag_picker = cx.new(|cx| TagPicker::new(session.clone(), cx));
        let folder_picker = cx.new(|cx| FolderPicker::new(session.clone(), cx));
        let ai_tagger =
            cx.new(|cx| crate::ai_tagger::AiTagger::new(session.clone(), thumbnails.clone(), cx));
        let tag_dialog = cx.new(|cx| TagDialog::new(session.clone(), cx));
        let folder_dialog = cx.new(|cx| FolderDialog::new(session.clone(), cx));
        let sources = sources::SourcesState::new(cx);
        let source_input = sources.input.clone();
        let subscriptions = vec![
            cx.subscribe(&tag_picker, |this, _, event: &tag_picker::Event, cx| {
                this.tag_picker_event(event, cx)
            }),
            cx.subscribe(
                &folder_picker,
                |this, _, event: &folder_picker::Event, cx| {
                    // The inspector opens the picker only to change the
                    // items' folders.
                    if let folder_picker::Event::Toggle { folder, add } = *event {
                        this.set_membership(folder, add, cx);
                    }
                },
            ),
            cx.subscribe(&tag_dialog, |this, _, event: &tag_dialog::Event, cx| {
                let tag_dialog::Event::Apply { add, remove } = event;
                this.apply_tag_dialog(add.clone(), remove.clone(), cx);
            }),
            cx.subscribe(
                &folder_dialog,
                |this, _, event: &folder_dialog::Event, cx| {
                    let folder_dialog::Event::Apply { add, remove } = event;
                    this.apply_folder_dialog(add.clone(), remove.clone(), cx);
                },
            ),
            // The address being edited is saved when the user presses Enter and
            // when its field loses the focus.
            cx.subscribe(&source_input, |this, _, _: &text_input::Submitted, cx| {
                this.save_source(cx)
            }),
            cx.on_blur(
                &source_input.read(cx).focus_handle(cx),
                window,
                |this, _, cx| this.save_source(cx),
            ),
            // When a thumbnail is written again, because it was regenerated or
            // a frame was captured, the preview is drawn again.
            cx.subscribe(&session, |_, _, event: &crate::application::Event, cx| {
                if matches!(event, crate::application::Event::Thumbnail(_)) {
                    cx.notify();
                }
            }),
            // The pickers close when the destination changes. The destination
            // is compared by its id, which stays the same when the list of
            // destinations is rebuilt.
            cx.subscribe_in(
                &session,
                window,
                |this, session, change: &Change, window, cx| {
                    if *change != Change::Navigation {
                        return;
                    }
                    let displayed = displayed_id(session.read(cx));
                    if displayed != this.picker_destination {
                        this.picker_destination = displayed;
                        this.close_pickers(window, cx);
                    }
                },
            ),
            cx.subscribe(&name, |this, _, event: &text_input::Committed, cx| {
                this.rename_item(RootId(event.0 as u32), event.1.clone(), cx);
            }),
            cx.subscribe(&notes, |this, _, event: &text_input::Committed, cx| {
                this.set_item_notes(RootId(event.0 as u32), event.1.clone(), cx);
            }),
            cx.subscribe(&scope.name, |this, _, event: &text_input::Committed, cx| {
                this.commit_scope(event.0, Some(event.1.clone()), None, cx);
            }),
            cx.subscribe(
                &scope.notes,
                |this, _, event: &text_input::Committed, cx| {
                    this.commit_scope(event.0, None, Some(event.1.clone()), cx);
                },
            ),
            cx.subscribe(
                &crate::settings::model(cx),
                |_, _, _: &crate::settings::Applied, cx| cx.notify(),
            ),
            cx.subscribe(
                &selection.notes,
                |this, _, event: &text_input::Committed, cx| {
                    this.commit_selection_notes(event.0, event.1.clone(), cx);
                },
            ),
            cx.subscribe(&session, |this, session, change: &Change, cx| {
                // After the rail is unpinned, it keeps what it shows until the
                // selection or the destination changes.
                let released = match (this.holding, change) {
                    (Some(_), Change::Selection | Change::Viewer) => true,
                    (Some(destination), Change::Navigation) => {
                        session.read(cx).displayed != destination
                    }
                    _ => false,
                };
                if released {
                    this.holding = None;
                }
                let held = this.holds();
                match change {
                    Change::Selection => {
                        // An edit that is not saved yet belongs to the item or
                        // items it was typed for, so it is saved before the
                        // fields change to the new selection.
                        for field in [&this.name, &this.notes, &this.selection.notes] {
                            field.update(cx, |field, cx| field.commit(cx));
                        }
                        this.sync_item(false, cx);
                        this.selection_changed(cx);
                        // The selection also changes when a subfolder tile is
                        // selected or deselected in the grid, and the rail
                        // then describes that subfolder.
                        if !held {
                            this.refresh_scope(cx);
                        }
                        cx.notify();
                    }
                    Change::Details => {
                        this.sync_item(true, cx);
                        cx.notify();
                    }
                    Change::Edited => this.selection_edited(cx),
                    Change::Viewer => {
                        this.sync_item(false, cx);
                        this.ensure_summary(cx);
                        cx.notify();
                    }
                    Change::Navigation if !held => {
                        // The folders changed, so the contents of a selected
                        // subfolder are read again.
                        this.forget_holdings(cx);
                        this.refresh_scope(cx);
                        cx.notify();
                    }
                    Change::Results if !held => this.refresh_scope(cx),
                    _ => {}
                }
                if released {
                    this.sync_item(false, cx);
                    this.ensure_summary(cx);
                    this.refresh_scope(cx);
                    cx.notify();
                }
            }),
        ];
        Self {
            session,
            thumbnails,
            art: None,
            name,
            notes,
            collapsed: HashSet::new(),
            hovered_star: None,
            scroll: ScrollHandle::new(),
            item: None,
            shown: Mode::Empty,
            scope,
            selection,
            pinned: false,
            holding: None,
            tag_picker,
            folder_picker,
            ai_tagger,
            tag_dialog,
            folder_dialog,
            places: Rc::new(RefCell::new(HashMap::new())),
            sources,
            menu: menu::Host::deferred(crate::ui::layer::MENU, menu::Return::Opener),
            menu_target: None,
            copied: None,
            confirm: modal::Host::default(),
            picker_destination: None,
            _subscriptions: subscriptions,
        }
    }

    /// Returns true if the rail keeps what it shows and does not follow the
    /// selection: while it is pinned, and after it is unpinned until the
    /// selection or the destination changes.
    fn holds(&self) -> bool {
        self.pinned || self.holding.is_some()
    }

    /// Returns the two pickers. The window draws them above everything else.
    pub fn pickers(&self) -> (Entity<TagPicker>, Entity<FolderPicker>) {
        (self.tag_picker.clone(), self.folder_picker.clone())
    }

    /// Returns the AI tagger's review. The window draws it above everything
    /// else.
    pub fn ai_tagger(&self) -> Entity<crate::ai_tagger::AiTagger> {
        self.ai_tagger.clone()
    }

    /// Returns the Add Tags dialog and the Folders dialog. The window draws
    /// them above everything else.
    pub fn dialogs(&self) -> (Entity<TagDialog>, Entity<FolderDialog>) {
        (self.tag_dialog.clone(), self.folder_dialog.clone())
    }

    /// Pins the rail to what it shows, or lets it follow the selection
    /// again. When the rail is unpinned while several items are selected, it
    /// reads their summary at once. Otherwise it keeps what it shows until
    /// the selection or the destination changes.
    pub fn set_pinned(&mut self, pinned: bool, cx: &mut Context<Self>) {
        if self.pinned == pinned {
            return;
        }
        self.pinned = pinned;
        if !pinned {
            let live = self.live_mode(cx);
            if live == Mode::Selection {
                self.holding = None;
                self.ensure_summary(cx);
            } else {
                self.holding = Some(self.session.read(cx).displayed);
            }
        }
        cx.notify();
    }

    fn wanted(&self, cx: &App) -> Mode {
        if self.holds() {
            return self.shown;
        }
        self.live_mode(cx)
    }

    /// Returns what the rail would describe if it followed the selection.
    fn live_mode(&self, cx: &App) -> Mode {
        let session = self.session.read(cx);
        wanted(
            session.screen().is_some(),
            session.viewer,
            session.selection_count() as usize,
        )
    }

    /// Copies the selected item's details into the rail, if they have arrived
    /// and the rail is to describe that item. When they are for the item
    /// already shown at the same revision, they are copied only if `edited`
    /// is true, which callers pass after an edit. Returns true if the details
    /// were copied, which also set the name field, the notes field and the
    /// open pickers.
    fn sync_item(&mut self, edited: bool, cx: &mut Context<Self>) -> bool {
        if self.holds() || self.wanted(cx) != Mode::Item {
            return false;
        }
        let Some(details) = self.session.read(cx).selected_details() else {
            return false;
        };
        let id = details.root.root_id;
        if !edited
            && self
                .item
                .as_ref()
                .is_some_and(|item| item.root.root_id == id && item.revision == details.revision)
        {
            return false;
        }
        let details = details.clone();
        let key = id.0 as u64;
        self.name.update(cx, |field, cx| {
            field.set_field(key, details.root.name.clone(), cx)
        });
        self.notes.update(cx, |field, cx| {
            field.set_field(key, details.root.notes.clone().unwrap_or_default(), cx)
        });
        self.item = Some(details);
        self.sync_pickers(cx);
        true
    }

    /// Returns the state to draw, and whether its data has arrived. It is the
    /// wanted state if its data has arrived, otherwise the state shown so
    /// far if the rail still has its data. Otherwise it is the wanted state
    /// with `false`, for which the skeleton is drawn.
    fn decide(&mut self, window: &mut Window, cx: &mut Context<Self>) -> (Mode, bool) {
        if self.holds() {
            let held = match self.shown {
                Mode::Empty => true,
                Mode::Scope => self.scope.view.is_some(),
                Mode::Item => self.item.is_some(),
                Mode::Selection => self.selection.shown.is_some(),
            };
            return (self.shown, held);
        }
        let wanted = self.wanted(cx);
        let ready = {
            let session = self.session.read(cx);
            match wanted {
                Mode::Empty => true,
                Mode::Scope => self.scope.ready(session),
                Mode::Item => session.selected_details().is_some(),
                Mode::Selection => self.selection.showable(),
            }
        };
        if ready {
            // When the session reports a change, the rail copies the details
            // in its handler for that change. When the rail finds out only
            // here that the details have arrived, for example when a screen
            // closes and an item is still selected, the fields and pickers
            // it changes are drawn in the next frame. Nothing else asks for
            // that frame, so it is asked for here.
            if wanted == Mode::Item && self.sync_item(false, cx) {
                window.request_animation_frame();
            }
            self.shown = wanted;
            return (wanted, true);
        }
        let held = match self.shown {
            Mode::Empty => false,
            Mode::Scope => self.scope.view.is_some(),
            Mode::Item => self.item.is_some(),
            Mode::Selection => self.selection.shown.is_some(),
        };
        if held {
            (self.shown, true)
        } else {
            (wanted, false)
        }
    }

    /// Puts the focus in the name field and selects the whole name. The
    /// Rename command calls this.
    pub fn edit_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.name.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        self.name.update(cx, |name, cx| name.select_everything(cx));
    }

    fn property(label: &'static str, value: String, _: &App) -> ui::property_row::PropertyRow {
        ui::property_row(label, value)
    }

    fn section(
        &self,
        title: &'static str,
        content: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> ui::PanelSection {
        ui::panel_section(title, title)
            .expanded(!self.collapsed.contains(title))
            .content(content)
            .on_toggle(cx.listener(move |this, _, _, cx| {
                if !this.collapsed.remove(title) {
                    this.collapsed.insert(title);
                }
                cx.notify();
            }))
    }

    /// Returns an element that records the bounds it is drawn at under `key`
    /// in `places`. It covers the element it is a child of.
    fn place(&self, key: &'static str) -> impl IntoElement + use<> {
        let places = self.places.clone();
        canvas(
            move |bounds, _, _| {
                places.borrow_mut().insert(key, bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    }

    /// Returns the point a picker opens at for the control recorded under
    /// `key`: the left edge of the rail and the top of the control. The
    /// picker opens beside the rail from there.
    fn anchor(&self, key: &'static str) -> Option<Point<f32>> {
        let places = self.places.borrow();
        let rail = places.get("rail")?;
        let control = places.get(key)?;
        Some(point(f32::from(rail.left()), f32::from(control.top())))
    }

    /// Returns the small "+" button that follows a row of chips.
    fn add_button(
        &self,
        id: &'static str,
        tip: ui::tooltip::Tip,
        open: fn(&mut Self, &'static str, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> ui::icon_button::IconButton {
        ui::icon_button(id, "inspector-add")
            .look(ui::icon_button::Look::TagAdd)
            .tip(tip)
            .child(self.place(id))
            .on_click(cx.listener(move |this, _, window, cx| open(this, id, window, cx)))
    }

    /// Returns the one wide "Add …" button of a section that has nothing in
    /// it.
    fn add_row(
        &self,
        id: &'static str,
        label: &'static str,
        tip: ui::tooltip::Tip,
        open: fn(&mut Self, &'static str, &mut Window, &mut Context<Self>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        ui::tooltip::attach(
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(t(label))
                .relative()
                .child(self.place(id))
                .child(ui::action_row(
                    SharedString::from(format!("{id}-action")),
                    "inspector-add",
                    label,
                ))
                .on_click(cx.listener(move |this, _, window, cx| open(this, id, window, cx))),
            SharedString::from(format!("{id}-tip")),
            tip,
            theme::CONTROL_SM,
            window,
            cx,
        )
        .into_any_element()
    }

    /// Returns the Auto Tag action that follows the properties. Its label is
    /// "Auto Tag N Images" for more than one item. When `enabled` is false,
    /// which is the case when none of the items is an image, the action is
    /// dimmed and its tooltip gives the reason.
    fn auto_tag(
        &self,
        id: &'static str,
        count: u64,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = if count > 1 {
            crate::tf!("Auto Tag {value0} Images", grouped(count))
        } else {
            t("Auto Tag").to_owned()
        };
        let row = ui::action_row(
            SharedString::from(format!("{id}-action")),
            "inspector-auto-tag",
            label,
        )
        .glyph(14.)
        .height(theme::CONTROL_MD)
        .disabled(!enabled)
        // A click opens the AI tagger's review for the selection, beside the
        // rail and at the height of the button.
        .on_click(cx.listener(move |this, _, window, cx| {
            let anchor = this.anchor(id);
            this.ai_tagger
                .update(cx, |tagger, cx| tagger.open(anchor, None, window, cx));
        }));
        let row = div().relative().child(row).child(self.place(id));
        // The tooltip of a disabled action says why it is disabled.
        let tip = if enabled {
            t("Suggest tags with AI")
        } else {
            t("AI tagging requires an explicit image selection")
        };
        ui::tooltip::attach(
            div().id(id).child(row),
            SharedString::from(format!("{id}-tip")),
            ui::tip(tip).shortcut_written("organize.autoTag", cx),
            theme::CONTROL_MD,
            window,
            cx,
        )
        .mt(px(12.))
        .into_any_element()
    }

    /// Returns the content of the Folders section: the chips and a "+" button
    /// after them, or one wide "Add to Folder" button when there are none.
    fn folders_row(
        &self,
        chips: Div,
        empty: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tip = ui::tip(t("Add to Folder")).shortcut("organize.addFolder", cx);
        if empty {
            self.add_row(
                "add-to-folder",
                t("Add to Folder"),
                tip,
                Self::open_folder_picker_at,
                window,
                cx,
            )
        } else {
            chips
                .child(self.add_button("add-folder", tip, Self::open_folder_picker_at, cx))
                .into_any_element()
        }
    }

    /// Returns the content of the Tags section: the chips and a "+" button
    /// after them, or one wide "Add Tags" button when there are none.
    fn tags_row(
        &self,
        chips: Div,
        empty: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tip = ui::tip(t("Add Tags")).shortcut("organize.addTag", cx);
        if empty {
            self.add_row(
                "add-tags",
                t("Add Tags"),
                tip,
                Self::open_tag_picker_at,
                window,
                cx,
            )
        } else {
            chips
                .child(self.add_button("add-tag", tip, Self::open_tag_picker_at, cx))
                .into_any_element()
        }
    }

    /// Returns the Folders section or the Tags section. `title` is the
    /// section's English title, which selects the section and is shown
    /// translated. A right-click in the section opens its picker.
    fn picker_section(
        &self,
        title: &'static str,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let key = if title == "Tags" {
            "section-tags"
        } else {
            "section-folders"
        };
        let open = if title == "Tags" {
            Self::open_tag_picker_at
        } else {
            Self::open_folder_picker_at
        };
        div().relative().child(self.place(key)).child(
            self.section(t(title), Some(content), cx).on_menu(
                cx.listener(move |this, _: &Point<Pixels>, window, cx| open(this, key, window, cx)),
            ),
        )
    }

    /// Returns the status line under the sections. It is a note in small
    /// grey text, or, when `error` is true, an error in the colour for
    /// errors with an alert icon before it.
    fn status(message: impl Into<SharedString>, error: bool, cx: &App) -> Div {
        let theme = theme::theme(cx);
        let status = div()
            .mt(px(10.))
            .min_h(px(20.))
            .flex()
            .items_center()
            .gap(px(6.));
        if error {
            status
                .child(ui::icon("alert-circle", 16., theme.negative()))
                .child(
                    div()
                        .text(theme::TEXT_XS)
                        .text_color(theme.negative())
                        .child(message.into()),
                )
        } else {
            status.child(
                div()
                    .text(theme::TEXT_SM)
                    .text_color(theme.text_tertiary())
                    .child(message.into()),
            )
        }
    }
}

impl Inspector {
    /// The inspector's scrolling area. Its content is 16 px from the left
    /// edge and 16 px from the right edge, 16 px from the top and 48 px
    /// from the bottom. The scrollbar is 8 px wide and is drawn next to the
    /// right edge, inside that 16 px, so the space beside the content is
    /// the same on both sides and the scrollbar never covers the content.
    fn scroller(&self) -> Stateful<Div> {
        div()
            .id("inspector")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .px(px(16.))
            .pt(px(16.))
            .pb(px(48.))
            .font_family(theme::FONT_INSPECTOR)
    }

    /// Returns the column that holds the fields and sections. It is as wide
    /// as the rail's content and as tall as what it holds.
    fn panel() -> Div {
        div().w_full().flex().flex_col()
    }

    /// Draws the scrolling column of fields and sections for the selected
    /// item.
    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let scroller = self.scroller();
        let mut panel = Self::panel();
        let (details, entries, tags, cover_thumbnail) = {
            let session = self.session.read(cx);
            let details = self.item.clone();
            let cover_thumbnail = details.as_ref().and_then(|details| {
                details
                    .media
                    .iter()
                    .find(|media| media.media_id == details.root.cover_media_id)
                    .or(details.media.first())
                    .map(|media| {
                        crate::grid::thumbnails::existing(
                            session.thumbnail(&media.facts.content_hash),
                        )
                    })
            });
            let tags = details.as_ref().map_or_else(Vec::new, |details| {
                details
                    .tag_ids
                    .iter()
                    .filter_map(|id| session.tags.get(&id.0).cloned())
                    .collect::<Vec<_>>()
            });
            (details, session.nav.clone(), tags, cover_thumbnail)
        };
        // The thumbnail cache keeps the preview's thumbnail decoded while the
        // grid scrolls, so that the preview does not have to be decoded again.
        let pinned = cover_thumbnail
            .clone()
            .map(|path| Resource::Path(path.into()));
        self.thumbnails.update(cx, |thumbnails, _| {
            thumbnails.pin("inspector-preview", pinned)
        });
        let Some(details) = details else {
            return scroller.child(panel);
        };
        let root = &details.root;
        let id = root.root_id;
        let cover = details
            .media
            .iter()
            .find(|media| media.media_id == root.cover_media_id)
            .or(details.media.first());
        if let (Some(media), Some(thumbnail)) = (cover, cover_thumbnail) {
            // The format badge has opaque white text on the same surface as
            // the grid's badges. A collection shows the group glyph in place
            // of a format.
            let label = if root.kind == picto_library::RootKind::Collection {
                ui::badge(0.)
                    .top(px(5.))
                    .left(px(5.))
                    .w(px(18.))
                    .justify_center()
                    .child(ui::icon("group", 14., rgb(theme::BADGE_STRONG_TEXT)))
            } else {
                ui::badge(3.)
                    .top(px(5.))
                    .left(px(5.))
                    .font_family(theme::FONT_UI)
                    .text_color(rgb(theme::BADGE_STRONG_TEXT))
                    .child(format::format_label(&media.facts.mime))
            };
            // The badge blurs the part of the preview that is behind it.
            let label = ui::GlassPanel::new(label.id("preview-type-label"), theme::TILE_RADIUS)
                .backdrop_blur(theme::preview_label_blur())
                .into_any_element();
            panel = panel.child(self.preview(
                thumbnail,
                media.facts.width.zip(media.facts.height),
                label,
                cx,
            ));
        }
        let palette = cover.map_or(&[][..], |media| &media.facts.palette[..]);
        panel = panel
            .child(self.palette(palette, window, cx))
            .child(
                div()
                    .mb(px(8.))
                    .child(ui::field("inspector-name", &self.name, Look::Inspector)),
            )
            .child(div().mb(px(8.)).child(ui::field(
                "inspector-notes",
                &self.notes,
                Look::Inspector,
            )))
            .child(self.source(Some(&root.source_urls), cx));

        // The Folders section holds a chip for each folder and then a "+"
        // button, or one wide button when the item is in no folder.
        let folders = self.folder_chips(&entries, &details.folder_ids, window, cx);
        // i18n: the section's English title, which picker_section translates
        panel = panel.child(self.picker_section("Folders", folders, cx));
        let tags = self.tag_chips(tags, true, window, cx);
        // i18n: the section's English title, which picker_section translates
        panel = panel.child(self.picker_section("Tags", tags, cx));

        // A property row whose value is missing is left out.
        let mut properties = div()
            .child(self.rating(Rate::Item(id), details.rating.bitmap_key() as usize, cx))
            .child(Self::property(
                t("Items"),
                grouped(root.media_count as u64),
                cx,
            ));
        if let Some((width, height)) = cover
            .and_then(|media| media.facts.width.zip(media.facts.height))
            .filter(|(width, height)| *width > 0 && *height > 0)
        {
            properties = properties.child(Self::property(
                t("Dimensions"),
                format!("{width} × {height}"),
                cx,
            ));
        }
        properties = properties.child(Self::property(
            t("Size"),
            crate::text::size_to_gigabytes(root.total_size_bytes),
            cx,
        ));
        let mut types = details
            .media
            .iter()
            .map(|media| media.facts.mime.as_str())
            .collect::<Vec<_>>();
        types.sort_unstable();
        types.dedup();
        if let Some(kind) = match types[..] {
            [] => None,
            [one] => Some(crate::text::inspector_type(one)),
            _ => Some("Mixed".into()),
        } {
            properties = properties.child(Self::property(t("Type"), kind, cx));
        }
        if let Some(duration) = cover
            .and_then(|media| media.facts.duration_ms)
            .filter(|duration| *duration > 0)
        {
            properties = properties.child(Self::property(
                t("Duration"),
                format::format_duration(duration),
                cx,
            ));
        }
        for (label, at) in [
            (t("Date Imported"), Some(root.imported_at_ms)),
            (t("Date Created"), root.captured_at_ms),
            (t("Date Modified"), Some(root.modified_at_ms)),
        ] {
            if let Some(date) = at.and_then(format_date) {
                properties = properties.child(Self::property(label, date, cx));
            }
        }
        let images = details
            .media
            .iter()
            .any(|media| media.facts.mime.starts_with("image/"));
        let auto_tag = self.auto_tag("auto-tag", details.media.len() as u64, images, window, cx);
        let export = Self::export_row(
            "export",
            SelectionTarget::Explicit {
                root_ids: vec![root.root_id],
            },
            details.media.len() as u64,
            cx,
        );
        scroller.child(
            panel
                .child(self.section(t("Properties"), Some(properties.into_any_element()), cx))
                .when(!details.media.is_empty(), |panel| {
                    panel.child(div().mt(px(12.)).child(export))
                })
                .child(auto_tag),
        )
    }
}

impl Inspector {
    /// Returns the Export action. A click opens the Export dialog for the
    /// `count` items of `target`.
    fn export_row(
        id: &'static str,
        target: SelectionTarget,
        count: u64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        ui::action_row(id, "inspector-export", t("Export"))
            .glyph(16.)
            .height(theme::CONTROL_MD)
            .on_click(cx.listener(move |_, _, _, cx| {
                if let Some(exports) = crate::export::get(cx) {
                    let target = target.clone();
                    exports.update(cx, |exports, cx| exports.export_as(target, count, cx));
                }
            }))
    }

    /// Draws the rows that a state will have, with dashes in place of the
    /// values, while the state's data has not arrived.
    fn skeleton(
        &mut self,
        mode: Mode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let error = self.session.read(cx).error.clone();
        let mut panel = Self::panel()
            .child(div().h(px(PREVIEW)).mb(px(8.)))
            .child(self.palette(&[], window, cx))
            .child(div().mb(px(8.)).child(ui::field::read_only("—", cx)))
            .child(div().mb(px(8.)).child(ui::field::read_only("—", cx)));
        if mode != Mode::Scope {
            panel = panel.child(self.source(None, cx));
        }
        panel = panel.child(self.section(t("Properties"), Some(div().into_any_element()), cx));
        // The skeleton of an item or a selection says "Loading...", because
        // their details have been asked for. The skeleton of a destination
        // has no status line unless there is an error.
        let status = match error {
            Some(error) => Some(Self::status(error, true, cx)),
            None if mode != Mode::Scope => Some(Self::status(t("Loading..."), false, cx)),
            None => None,
        };
        self.scroller().child(panel.children(status))
    }
}

impl Render for Inspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Inspector);
        let (mode, ready) = self.decide(window, cx);
        // A state that is not drawn releases the thumbnails that it kept
        // decoded.
        if (mode, ready) != (Mode::Item, true) {
            self.thumbnails.update(cx, |thumbnails, _| {
                thumbnails.pin("inspector-preview", None)
            });
        }
        if (mode, ready) != (Mode::Scope, true) {
            self.release_collage(cx);
        }
        if (mode, ready) != (Mode::Selection, true) {
            self.release_stack(cx);
        }
        // Before the first destination has loaded there is nothing to
        // describe, so the rail is empty and shows no skeleton.
        let launching = self.session.read(cx).results_epoch == 0;
        let body = match (mode, ready) {
            (Mode::Empty, _) => self.scroller(),
            (Mode::Scope, false) if launching => self.scroller(),
            (Mode::Item, true) => self.body(window, cx),
            (Mode::Scope, true) => self.scope_body(window, cx),
            (Mode::Selection, true) => self.selection_body(window, cx),
            (mode, false) => self.skeleton(mode, window, cx),
        };
        let menu = self.menu.layer(window, cx);
        let confirm = self.confirm_dialog(window, cx);
        div()
            .relative()
            .size_full()
            .child(self.place("rail"))
            .child(body)
            .child(ui::scrollbar(
                "inspector-scrollbar",
                &self.scroll,
                window,
                cx,
            ))
            .children(menu)
            .children(confirm)
    }
}

#[cfg(test)]
mod tests {
    use super::{Mode, fitted, wanted};

    #[test]
    fn the_rail_describes_the_viewer_then_the_selection_then_the_destination() {
        assert_eq!(wanted(true, false, 0), Mode::Empty);
        assert_eq!(wanted(false, true, 3), Mode::Item);
        assert_eq!(wanted(false, false, 1), Mode::Item);
        assert_eq!(wanted(false, false, 2), Mode::Selection);
        assert_eq!(wanted(false, false, 0), Mode::Scope);
    }

    #[test]
    fn previews_shrink_to_the_square_but_never_grow() {
        assert_eq!(fitted(Some((400, 200))), (200., 100.));
        assert_eq!(fitted(Some((100, 50))), (100., 50.));
        assert_eq!(fitted(None), (200., 200.));
    }
}
