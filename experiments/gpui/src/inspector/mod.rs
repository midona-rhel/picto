//! The inspector rail's scrolling body (`Inspector.tsx`): the destination
//! showing when nothing is selected, the selected item, or a summary of
//! several. Layout follows InspectorSkeleton, InspectorField, TagChip and
//! PropertyRow.
mod edits;
mod folder_picker;
mod membership;
mod scope;
mod selection;
mod sources;
mod stack;
mod tag_picker;

pub use folder_picker::FolderPicker;
pub use tag_picker::TagPicker;

use crate::grid::{format, thumbnails::Thumbnails};
use crate::session::{Change, Session};
use crate::theme;
use crate::theme::Text as _;
use crate::ui::{
    self,
    field::Look,
    menu::Menu,
    modal::Presence,
    text_input::{self, TextInput},
};
use gpui::*;
use picto_library::predicate::FilterClause;
use picto_library::{FolderId, RootDetails, RootId, TagRecord};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// What the rail describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// A destination that is a screen of its own: nothing.
    Empty,
    /// Nothing selected: the destination showing.
    Scope,
    Item,
    /// Several items selected.
    Selection,
}

/// What the rail should describe (`liveInspectorTargetAtom`): an item open in
/// the viewer, else what the grid has selected, else the grid's destination.
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

/// Where a click on a star applies its rating.
#[derive(Clone, Copy)]
enum Rate {
    Item(RootId),
    Selection,
}

/// A chip's colour and sort order for a tag namespace (`NS_COLORS` in
/// `TagChip.tsx`, `tagGroupOrder`). General tags come last.
pub fn tag_style(namespace: &str) -> (u32, u8) {
    match namespace {
        "creator" => (0xff6667, 0),
        "studio" => (0xff6667, 10),
        "series" => (0xc499ff, 1),
        "character" => (0x30d159, 2),
        "person" => (0x30d159, 10),
        "species" => (0x00aaff, 3),
        "rating" => (0xffd60a, 4),
        "system" => (0xff9f0a, 10),
        "" | "general" => (0xbdbec0, 99),
        _ => (0xbdbec0, 10),
    }
}

/// A folder chip without a colour of its own.
pub const FOLDER_COLOR: u32 = 0x868e96;
/// The preview fits this square.
const PREVIEW: f32 = 200.;

fn is_general(tag: &picto_library::TagRecord) -> bool {
    matches!(tag.namespace.as_str(), "" | "general" | "default")
}

/// A tag's full name (`namespace:subtag`, or the subtag alone for a general
/// tag), as the library adds and removes it by.
fn tag_raw(tag: &picto_library::TagRecord) -> String {
    if is_general(tag) {
        tag.subname.clone()
    } else {
        format!("{}:{}", tag.namespace, tag.subname)
    }
}

/// A tag's chip in its namespace's colour (`TagChip`). With the
/// `showTagPrefixes` setting off, it shows the tag without its group.
fn tag_chip(tag: &picto_library::TagRecord, cx: &App) -> ui::Chip {
    let show_prefixes = crate::settings::current(cx).show_tag_prefixes;
    ui::chip(
        ("inspector-tag", tag.tag_id.0 as usize),
        tag.subname.clone(),
        tag_style(&tag.namespace).0,
    )
    .namespace((show_prefixes && !is_general(tag)).then(|| tag.namespace.clone().into()))
}

/// A preview's size: the media scaled down to fit the square, never up.
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

fn format_size(bytes: u64) -> String {
    let bytes = bytes as f64;
    if bytes < 1024. {
        format!("{bytes} B")
    } else if bytes < 1048576. {
        format!("{:.1} KB", bytes / 1024.)
    } else if bytes < 1073741824. {
        format!("{:.1} MB", bytes / 1048576.)
    } else {
        format!("{:.2} GB", bytes / 1073741824.)
    }
}

/// `YYYY/MM/DD HH:MM` in local time.
fn format_date(milliseconds: i64) -> Option<String> {
    use chrono::TimeZone;
    let date = chrono::Local.timestamp_millis_opt(milliseconds).single()?;
    Some(date.format("%Y/%m/%d %H:%M").to_string())
}

/// The short type name the Type row shows (`fmtExt`).
fn format_type(mime: &str) -> String {
    let subtype = mime.split_once('/').map_or("", |(_, subtype)| subtype);
    match subtype {
        "jpeg" => "JPG".into(),
        "svg+xml" => "SVG".into(),
        "quicktime" => "MOV".into(),
        "x-matroska" => "MKV".into(),
        "epub+zip" => "EPUB".into(),
        other => other.trim_start_matches("x-").to_uppercase(),
    }
}

/// What a chip's context menu was opened for.
#[derive(Clone)]
enum ChipMenu {
    Tag(TagRecord),
    Folder(FolderId),
}

/// A change to every selected item that would replace what some of them
/// already have, waiting for the confirmation (`confirmSelectionOverwrite`).
enum Overwrite {
    Notes(String),
    Sources(Vec<String>),
}

struct Confirm {
    overwrite: Overwrite,
    count: u64,
    presence: Presence,
    focus: FocusHandle,
}

pub struct Inspector {
    session: Entity<Session>,
    thumbnails: Entity<Thumbnails>,
    name: Entity<TextInput>,
    notes: Entity<TextInput>,
    collapsed: HashSet<&'static str>,
    /// While pinned the rail keeps what it shows instead of following the
    /// selection (`inspectorPinnedAtom`).
    pinned: bool,
    /// After unpinning, what the rail shows stays until the selection or the
    /// destination next changes, as the Electron inspector waits for its
    /// live target to change; the destination it was unpinned on.
    holding: Option<usize>,
    tag_picker: Entity<TagPicker>,
    folder_picker: Entity<FolderPicker>,
    /// Where the rail and its add buttons, sections and fields were last
    /// drawn, in window coordinates, to open the pickers beside them.
    places: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
    sources: sources::SourcesState,
    menu: Option<Entity<Menu>>,
    menu_target: Option<ChipMenu>,
    _menu: Option<Subscription>,
    confirm: Option<Confirm>,
    /// The destination the pickers were opened on; they close when it changes.
    picker_destination: usize,
    /// The star under the pointer, which previews that rating.
    hovered_star: Option<usize>,
    scroll: ScrollHandle,
    /// The item the rail describes, or last described.
    item: Option<RootDetails>,
    /// What the rail shows. A new state takes its place only once that
    /// state's data is there, so the rail never goes blank in between.
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
        let name = cx.new(|cx| TextInput::expanding("Name", window, cx));
        let notes = cx.new(|cx| TextInput::expanding("Notes", window, cx));
        let scope = scope::ScopeState::new(window, cx);
        let selection = selection::SelectionState::new(window, cx);
        let tag_picker = cx.new(|cx| TagPicker::new(session.clone(), cx));
        let folder_picker = cx.new(|cx| FolderPicker::new(session.clone(), cx));
        let sources = sources::SourcesState::new(cx);
        let source_input = sources.input.clone();
        let subscriptions = vec![
            cx.subscribe(&tag_picker, |this, _, event: &tag_picker::Event, cx| {
                this.tag_picker_event(event, cx)
            }),
            cx.subscribe(
                &folder_picker,
                |this, _, event: &folder_picker::Event, cx| {
                    let folder_picker::Event::Toggle { folder, add } = *event;
                    this.set_membership(folder, add, cx);
                },
            ),
            // Enter, or leaving the address being edited, saves it.
            cx.subscribe(&source_input, |this, _, _: &text_input::Submitted, cx| {
                this.save_source(cx)
            }),
            cx.on_blur(
                &source_input.read(cx).focus_handle(cx),
                window,
                |this, _, cx| this.save_source(cx),
            ),
            // The pickers close when the destination changes.
            cx.subscribe_in(
                &session,
                window,
                |this, session, change: &Change, window, cx| {
                    if *change == Change::Navigation
                        && session.read(cx).displayed != this.picker_destination
                    {
                        this.picker_destination = session.read(cx).displayed;
                        this.close_pickers(window, cx);
                    }
                },
            ),
            cx.subscribe(&name, |this, _, event: &text_input::Committed, cx| {
                this.session
                    .read(cx)
                    .rename(RootId(event.0 as u32), event.1.clone());
            }),
            cx.subscribe(&notes, |this, _, event: &text_input::Committed, cx| {
                this.session
                    .read(cx)
                    .set_notes(RootId(event.0 as u32), event.1.clone());
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
                // After unpinning, the rail lets go of what it holds once the
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
                        // A pending edit belongs to what it was typed for.
                        for field in [&this.name, &this.notes, &this.selection.notes] {
                            field.update(cx, |field, cx| field.commit(cx));
                        }
                        this.sync_item(false, cx);
                        this.selection_changed(cx);
                        cx.notify();
                    }
                    Change::Details => {
                        this.sync_item(true, cx);
                        cx.notify();
                    }
                    Change::Viewer => {
                        this.sync_item(false, cx);
                        this.ensure_summary(cx);
                        cx.notify();
                    }
                    Change::Navigation if !held => {
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
            places: Rc::new(RefCell::new(HashMap::new())),
            sources,
            menu: None,
            menu_target: None,
            _menu: None,
            confirm: None,
            picker_destination: 0,
            _subscriptions: subscriptions,
        }
    }

    /// Whether the rail keeps what it shows rather than following the
    /// selection: while pinned, and for a moment after.
    fn holds(&self) -> bool {
        self.pinned || self.holding.is_some()
    }

    /// The pickers, which the window draws above everything.
    pub fn pickers(&self) -> (Entity<TagPicker>, Entity<FolderPicker>) {
        (self.tag_picker.clone(), self.folder_picker.clone())
    }

    /// Pins the rail to what it shows, or lets it follow the selection
    /// again. A multi-selection is read again at once (its summary request
    /// follows the pin); anything else stays until the selection changes.
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

    /// What the rail would describe if it followed the selection.
    fn live_mode(&self, cx: &App) -> Mode {
        let session = self.session.read(cx);
        if session.demo.is_some() {
            return Mode::Item;
        }
        wanted(
            session.screen().is_some(),
            session.viewer,
            session.selection.len(),
        )
    }

    /// Takes the selected item's details once they are there and the rail
    /// is to describe it. `edited` takes them even when they are for the
    /// item already shown, after an edit.
    fn sync_item(&mut self, edited: bool, cx: &mut Context<Self>) {
        if self.holds() || self.wanted(cx) != Mode::Item {
            return;
        }
        let Some(details) = self.session.read(cx).selected_details() else {
            return;
        };
        let id = details.root.root_id;
        if !edited
            && self
                .item
                .as_ref()
                .is_some_and(|item| item.root.root_id == id && item.revision == details.revision)
        {
            return;
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
    }

    /// What to draw: the state wanted once its data is there, else the one
    /// shown so far, else (`None`) the wanted state's skeleton.
    fn decide(&mut self, cx: &mut Context<Self>) -> (Mode, bool) {
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
                Mode::Item => session.demo.is_some() || session.selected_details().is_some(),
                Mode::Selection => self.selection.ready(),
            }
        };
        if ready {
            if wanted == Mode::Item {
                self.sync_item(false, cx);
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

    /// Puts the caret in the name field, as Rename does.
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

    /// Records where an element was drawn, under `key`.
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

    /// `openPortal`: a picker opens beside the rail, its top at the
    /// control's top.
    fn anchor(&self, key: &'static str) -> Option<Point<f32>> {
        let places = self.places.borrow();
        let rail = places.get("rail")?;
        let control = places.get(key)?;
        Some(point(f32::from(rail.left()), f32::from(control.top())))
    }

    /// The small "+" that ends a row of chips (`.tagAddBtn`).
    fn add_button(
        &self,
        id: &'static str,
        tip: ui::tooltip::Tip,
        open: fn(&mut Self, &'static str, &mut Window, &mut Context<Self>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = theme::theme(cx);
        let button = div()
            .id(id)
            .group(id)
            .w(px(26.))
            .h(px(theme::CONTROL_SM))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme::RADIUS_MD))
            .cursor_pointer()
            .child(self.place(id))
            .child(
                ui::icon("inspector-add", 15., theme.text())
                    .opacity(0.5)
                    .group_hover(id, |style| style.opacity(1.)),
            )
            .on_click(cx.listener(move |this, _, window, cx| open(this, id, window, cx)));
        ui::tooltip::attach(button, (id, 1usize), tip, theme::CONTROL_SM, window, cx)
    }

    /// One wide "Add …" button for a section with nothing in it
    /// (`.emptySectionActionBtn`).
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

    /// The Folders section's content: the chips and a "+" after them, or one
    /// wide "Add to Folder" button when there are none.
    fn folders_row(
        &self,
        chips: Div,
        empty: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tip = ui::tip("Add to Folder").keys(&["F"]);
        if empty {
            self.add_row(
                "add-to-folder",
                "Add to Folder",
                tip,
                Self::open_folder_picker_at,
                window,
                cx,
            )
        } else {
            chips
                .child(self.add_button("add-folder", tip, Self::open_folder_picker_at, window, cx))
                .into_any_element()
        }
    }

    /// The Tags section's content: the chips and a "+" after them, or one
    /// wide "Add Tags" button when there are none.
    fn tags_row(
        &self,
        chips: Div,
        empty: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tip = ui::tip("Add Tags").keys(&["T"]);
        if empty {
            self.add_row(
                "add-tags",
                "Add Tags",
                tip,
                Self::open_tag_picker_at,
                window,
                cx,
            )
        } else {
            chips
                .child(self.add_button("add-tag", tip, Self::open_tag_picker_at, window, cx))
                .into_any_element()
        }
    }

    /// The Folders and Tags sections; a right-click in one opens its picker
    /// (`onContextMenu`).
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
            self.section(title, Some(content), cx).on_menu(
                cx.listener(move |this, _: &Point<Pixels>, window, cx| open(this, key, window, cx)),
            ),
        )
    }

    /// The status line under the sections (`.status`): a quiet note, or an
    /// error with its icon.
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

    /// The cover in its fitted frame, with the format badge
    /// (`InspectorPreview`).
    fn preview(
        &self,
        thumbnail: std::path::PathBuf,
        dimensions: Option<(u32, u32)>,
        label: AnyElement,
    ) -> Div {
        let (width, height) = fitted(dimensions);
        div()
            .h(px(PREVIEW))
            .mb(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .relative()
                    .w(px(width.round()))
                    .h(px(height.round()))
                    .rounded(px(theme::RADIUS_MD))
                    .overflow_hidden()
                    .shadow(vec![
                        BoxShadow::new(px(0.), px(0.), rgba(0x0000001a).into()).blur_radius(px(5.)),
                    ])
                    .child(
                        img(thumbnail)
                            .image_cache(&self.thumbnails)
                            .size_full()
                            .rounded(px(theme::RADIUS_MD))
                            .object_fit(ObjectFit::Cover),
                    )
                    // The glass edge: a 1px ring over the image.
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(theme::RADIUS_MD))
                            .border_1()
                            .border_color(rgba(0x0000001a)),
                    )
                    .child(label),
            )
    }

    /// The colour pill (`ColorPalette`). It keeps its height when there are
    /// no colours, so the fields below never move.
    fn palette(&self, colors: &[picto_library::LabColor], cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let mut swatches = div()
            .h(px(theme::CONTROL_SM))
            .max_w_full()
            .px(px(6.))
            .rounded_full()
            .border_1()
            .border_color(theme.border_secondary())
            .bg(theme.control_surface())
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(2.));
        if colors.is_empty() {
            swatches = swatches.invisible();
        }
        for (index, color) in colors.iter().enumerate() {
            let color = color.clone();
            let group = SharedString::from(format!("swatch-{index}"));
            swatches = swatches.child(
                div()
                    .id(("palette", index))
                    .group(group.clone())
                    .relative()
                    .size(px(16.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    // The hover ring sits one pixel outside the swatch's cell.
                    .child(
                        div()
                            .absolute()
                            .inset(px(-1.))
                            .rounded_full()
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .group_hover(group, move |style| {
                                style.border_color(theme.text_tertiary())
                            }),
                    )
                    .child(
                        div()
                            .size(px(14.))
                            .rounded_full()
                            .bg(rgb(format::lab_to_rgb(color.l, color.a, color.b)))
                            .border_1()
                            .border_color(theme.contrast(10.)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let clause = FilterClause::Color {
                            color: color.clone(),
                            delta_e: 20.,
                        };
                        this.session
                            .update(cx, |session, cx| session.add_filter(clause, cx));
                    })),
            );
        }
        div()
            .h(px(theme::CONTROL_SM))
            .mb(px(8.))
            .flex()
            .justify_center()
            .child(swatches)
    }

    /// The rating row (`StarRating`): the stars up to the one under the
    /// pointer preview that rating, and a click on the current rating clears it.
    fn rating(&self, target: Rate, current: usize, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let shown = self.hovered_star.unwrap_or(current);
        let mut row = div()
            .h(px(20.))
            .flex()
            .items_center()
            .text(theme::TEXT_CAPTION)
            .child(div().flex_1().child("Rating"));
        for value in 1..=5 {
            let star = if value <= shown {
                ui::icon("star-filled", 12., rgb(theme::RATING))
            } else {
                ui::icon("star", 12., theme.text_secondary()).opacity(0.25)
            };
            row = row.child(
                div()
                    .id(("rating", value))
                    .w(px(16.))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(star)
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.hovered_star = Some(value);
                        } else if this.hovered_star == Some(value) {
                            this.hovered_star = None;
                        }
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let next =
                            picto_library::Rating::ALL[if value == current { 0 } else { value }];
                        match target {
                            Rate::Item(id) => {
                                this.session.read(cx).rate(id, next);
                                // A pinned item is not the selection, whose
                                // details the session reads back.
                                if this.pinned {
                                    this.reread(membership::Touches::Text, cx);
                                }
                            }
                            Rate::Selection => this.rate_selection(next, cx),
                        }
                    })),
            );
        }
        row
    }
}

impl Inspector {
    /// `.scrollContent`: 16px all round, 48 below, with the scrollbar's
    /// gutter reserved on the right.
    fn scroller(&self) -> Stateful<Div> {
        div()
            .id("inspector")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .pl(px(16.))
            .pr(px(16. + ui::scrollbar::GUTTER))
            .pt(px(16.))
            .pb(px(48.))
            .font_family(theme::FONT_INSPECTOR)
    }

    /// `.contentStack`: a flex column as tall as its content, so the fields'
    /// and sections' margins add up instead of collapsing.
    fn panel() -> Div {
        div().w_full().flex().flex_col()
    }

    /// The scrolling column of fields and sections for the selected item.
    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let scroller = self.scroller();
        let mut panel = Self::panel();
        let (demo, details, entries, tags, cover_thumbnail) = {
            let session = self.session.read(cx);
            let details = self.item.clone();
            let cover_thumbnail = details.as_ref().and_then(|details| {
                details
                    .media
                    .iter()
                    .find(|media| media.media_id == details.root.cover_media_id)
                    .or(details.media.first())
                    .map(|media| session.thumbnail(&media.facts.content_hash))
            });
            let tags = details.as_ref().map_or_else(Vec::new, |details| {
                details
                    .tag_ids
                    .iter()
                    .filter_map(|id| session.tags.get(&id.0).cloned())
                    .collect::<Vec<_>>()
            });
            (
                session.demo,
                details,
                session.nav.clone(),
                tags,
                cover_thumbnail,
            )
        };
        // The preview stays decoded while the grid scrolls past it.
        let pinned = cover_thumbnail
            .clone()
            .map(|path| Resource::Path(path.into()));
        self.thumbnails.update(cx, |thumbnails, _| {
            thumbnails.pin("inspector-preview", pinned)
        });
        if let Some((name, mime)) = demo {
            return scroller.child(
                panel
                    .child(Self::property("Example", name.to_owned(), cx))
                    .child(Self::property("Type", mime.to_owned(), cx)),
            );
        }
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
            // `.formatBadge`: opaque white on the grid's badge surface. A
            // collection shows the group glyph instead of a format.
            let label = if root.kind == picto_library::RootKind::Collection {
                ui::badge(0.)
                    .top(px(5.))
                    .left(px(5.))
                    .w(px(18.))
                    .justify_center()
                    .child(ui::icon("group", 14., white()))
                    .into_any_element()
            } else {
                ui::badge(3.)
                    .top(px(5.))
                    .left(px(5.))
                    .font_family(theme::FONT_UI)
                    .text_color(white())
                    .child(format::format_label(&media.facts.mime))
                    .into_any_element()
            };
            panel = panel.child(self.preview(
                thumbnail,
                media.facts.width.zip(media.facts.height),
                label,
            ));
        }
        let palette = cover.map_or(&[][..], |media| &media.facts.palette[..]);
        panel = panel
            .child(self.palette(palette, cx))
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
            .child(self.source(Some(&root.source_urls), window, cx));

        // Folders: a chip each, then "+"; or one wide button when there are none.
        let folders = self.folder_chips(&entries, &details.folder_ids, window, cx);
        panel = panel.child(self.picker_section("Folders", folders, cx));
        let tags = self.tag_chips(tags, true, window, cx);
        panel = panel.child(self.picker_section("Tags", tags, cx));

        // `normalizedCoreProperties`: a row with nothing to say is left out.
        let mut properties = div()
            .child(self.rating(Rate::Item(id), details.rating.bitmap_key() as usize, cx))
            .child(Self::property(
                "Items",
                format::grouped(root.media_count as u64),
                cx,
            ));
        if let Some((width, height)) = cover
            .and_then(|media| media.facts.width.zip(media.facts.height))
            .filter(|(width, height)| *width > 0 && *height > 0)
        {
            properties = properties.child(Self::property(
                "Dimensions",
                format!("{width} × {height}"),
                cx,
            ));
        }
        properties = properties.child(Self::property(
            "Size",
            format_size(root.total_size_bytes),
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
            [one] => Some(format_type(one)),
            _ => Some("Mixed".into()),
        } {
            properties = properties.child(Self::property("Type", kind, cx));
        }
        if let Some(duration) = cover
            .and_then(|media| media.facts.duration_ms)
            .filter(|duration| *duration > 0)
        {
            properties = properties.child(Self::property(
                "Duration",
                format::format_duration(duration),
                cx,
            ));
        }
        for (label, at) in [
            ("Date Imported", Some(root.imported_at_ms)),
            ("Date Created", root.captured_at_ms),
            ("Date Modified", Some(root.modified_at_ms)),
        ] {
            if let Some(date) = at.and_then(format_date) {
                properties = properties.child(Self::property(label, date, cx));
            }
        }
        // TODO: Export opens the export dialog for this item.
        scroller.child(
            panel
                .child(self.section("Properties", Some(properties.into_any_element()), cx))
                .child(
                    div().mt(px(12.)).child(
                        ui::action_row("export", "inspector-export", "Export")
                            .glyph(16.)
                            .height(theme::CONTROL_MD),
                    ),
                ),
        )
    }
}

impl Inspector {
    /// `UnavailableInspectorSkeleton`: the rows a state will have, with
    /// dashes for values, while there is nothing to show yet.
    fn skeleton(
        &mut self,
        mode: Mode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let error = self.session.read(cx).error.clone();
        let mut panel = Self::panel()
            .child(div().h(px(PREVIEW)).mb(px(8.)))
            .child(self.palette(&[], cx))
            .child(div().mb(px(8.)).child(ui::field::read_only("—", cx)))
            .child(div().mb(px(8.)).child(ui::field::read_only("—", cx)));
        if mode != Mode::Scope {
            panel = panel.child(self.source(None, window, cx));
        }
        panel = panel.child(self.section("Properties", Some(div().into_any_element()), cx));
        // An item's or a selection's details are on their way; a
        // destination's skeleton says nothing unless something failed.
        let status = match error {
            Some(error) => Some(Self::status(error, true, cx)),
            None if mode != Mode::Scope => Some(Self::status("Loading...", false, cx)),
            None => None,
        };
        self.scroller().child(panel.children(status))
    }
}

impl Render for Inspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::stats::rendered(crate::stats::View::Inspector);
        let (mode, ready) = self.decide(cx);
        // A state that is not drawn gives back the thumbnails it kept.
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
        // describe, and the Electron rail is empty too.
        let launching = self.session.read(cx).results_epoch == 0;
        let body = match (mode, ready) {
            (Mode::Empty, _) => self.scroller(),
            (Mode::Scope, false) if launching => self.scroller(),
            (Mode::Item, true) => self.body(window, cx),
            (Mode::Scope, true) => self.scope_body(cx),
            (Mode::Selection, true) => self.selection_body(window, cx),
            (mode, false) => self.skeleton(mode, window, cx),
        };
        // A dismissed menu stays until its fade is over.
        if self
            .menu
            .as_ref()
            .is_some_and(|menu| menu.read(cx).finished())
        {
            self.menu = None;
            self._menu = None;
        }
        let menu = self.menu.clone().map(|menu| {
            let view = window.viewport_size();
            deferred(
                anchored()
                    .position(point(px(0.), px(0.)))
                    .child(div().w(view.width).h(view.height).child(menu)),
            )
            .with_priority(30)
        });
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
    use super::{Mode, fitted, format_size, format_type, tag_style, wanted};

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

    #[test]
    fn sizes_roll_over_to_the_next_unit() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1048576), "5.0 MB");
        assert_eq!(format_size(3 * 1073741824 / 2), "1.50 GB");
    }

    #[test]
    fn the_type_row_shows_a_short_name() {
        assert_eq!(format_type("image/jpeg"), "JPG");
        assert_eq!(format_type("video/quicktime"), "MOV");
        assert_eq!(format_type("video/x-matroska"), "MKV");
        assert_eq!(format_type("image/svg+xml"), "SVG");
        assert_eq!(
            format_type("application/x-shockwave-flash"),
            "SHOCKWAVE-FLASH"
        );
        assert_eq!(format_type("image/png"), "PNG");
    }

    #[test]
    fn general_tags_sort_after_every_named_group() {
        let order = |namespace| tag_style(namespace).1;
        assert!(order("creator") < order("series"));
        assert!(order("rating") < order("somethingelse"));
        assert!(order("somethingelse") < order("general"));
        assert_eq!(order(""), order("general"));
    }
}
