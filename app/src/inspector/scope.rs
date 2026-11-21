//! Draws the inspector when nothing is selected. It then describes the
//! destination the grid shows: a folder card with the first four covers, the
//! destination's name and notes, how many items it holds and their size, and
//! for a folder whether it adds tags automatically and whether it watches a
//! folder on disk.
use super::{Inspector, PREVIEW};
use crate::grid::format;
use crate::i18n::format::grouped;
use crate::i18n::t;
use crate::session::Session;
use crate::theme;
use crate::ui::{
    self,
    field::Look,
    text_input::{self, TextInput},
};
use gpui::*;
use picto_library::query::ItemScope;
use picto_library::{FolderId, FolderRecord, LibraryError, SmartFolderId, SmartFolderRecord};
use std::collections::HashMap;
use std::path::PathBuf;

/// The kind of destination. It decides what the rail lets the user change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Kind {
    /// All, Inbox, Trash and the other built-in destinations. Their name
    /// cannot be changed and they have no notes.
    System,
    Folder(FolderId),
    Smart(SmartFolderId),
}

impl Kind {
    fn of(scope: &ItemScope) -> Self {
        match scope {
            ItemScope::FolderTree { folder_id } | ItemScope::Folder { folder_id } => {
                Kind::Folder(*folder_id)
            }
            ItemScope::SmartFolder { smart_folder_id } => Kind::Smart(*smart_folder_id),
            _ => Kind::System,
        }
    }

    /// Returns the key that the name field and the notes field are set with.
    /// Every folder and every smart folder has a different key.
    fn key(self) -> u64 {
        match self {
            Kind::System => u64::MAX,
            Kind::Folder(id) => id.0 as u64,
            Kind::Smart(id) => 1 << 32 | id.0 as u64,
        }
    }
}

/// Holds the library's record of a folder or a smart folder. The record has
/// the notes and, for a folder, the watch setting, which the sidebar's list
/// of destinations does not have.
#[derive(Clone)]
pub(super) enum Record {
    Folder {
        record: FolderRecord,
        auto_tags: bool,
    },
    Smart(SmartFolderRecord),
}

/// Holds one cell of the folder card: the thumbnail of a cover and the colour
/// drawn behind it.
#[derive(Clone, PartialEq)]
struct Cover {
    thumbnail: PathBuf,
    background: Option<u32>,
}

/// Holds everything that is drawn when the rail describes a destination. It
/// is built whole and replaced whole, so that all of it changes in the same
/// frame.
#[derive(Clone, PartialEq)]
pub(super) struct ScopeView {
    /// The destination's position in the sidebar's list of destinations.
    index: usize,
    kind: Kind,
    name: String,
    notes: String,
    total: u64,
    size: u128,
    covers: Vec<Cover>,
    search: String,
    /// For a folder whose record has been read: whether it adds tags
    /// automatically, and whether it watches a folder on disk.
    folder: Option<(bool, bool)>,
}

/// The number of covers on the folder card. They are the destination's first
/// four items.
const COVERS: usize = 4;
const COVER_PINS: [&str; COVERS] = [
    "inspector-cover-0",
    "inspector-cover-1",
    "inspector-cover-2",
    "inspector-cover-3",
];

pub(super) struct ScopeState {
    pub(super) name: Entity<TextInput>,
    pub(super) notes: Entity<TextInput>,
    pub(super) view: Option<ScopeView>,
    /// The records that have been read, by destination. A value of `None`
    /// means that reading the record failed.
    records: HashMap<Kind, Option<Record>>,
    /// The position of the destination whose record was last asked for.
    asked: Option<usize>,
    /// Whether the thumbnail cache is keeping the card's covers decoded.
    pinned: bool,
    /// For each subfolder that was selected in the grid, the items that are
    /// directly in it: their total size and the first four of them. A value
    /// of `None` means that they are being read.
    subfolders: HashMap<FolderId, Option<Holding>>,
    /// The selected subfolder whose record is being read.
    reading: Option<Kind>,
}

/// Holds the total size and the first four of the items that are directly in
/// a folder. The rail shows them when it describes a subfolder that is
/// selected in the grid.
#[derive(Clone)]
struct Holding {
    size: u128,
    head: Vec<picto_library::query::RootSummary>,
}

/// Returns what the rail describes when nothing else is selected: the
/// subfolder whose tile is selected in the grid, or otherwise the destination
/// the grid shows. The first value is its position in the list of
/// destinations. The second is the folder's id when it is a subfolder.
fn described(session: &Session) -> (usize, Option<FolderId>) {
    session
        .selected_folder()
        .and_then(|node| {
            let index = session.nav.iter().position(|entry| entry.id() == node)?;
            let folder = crate::grid::subfolders::folder_of(node)?;
            Some((index, Some(folder)))
        })
        .unwrap_or((session.displayed, None))
}

impl ScopeState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Inspector>) -> Self {
        Self {
            name: cx.new(|cx| {
                TextInput::new(text_input::Kind::Inspector, cx)
                    .placeholder("Name")
                    .commits_on_blur(window, cx)
                    .expanding()
            }),
            notes: cx.new(|cx| {
                TextInput::new(text_input::Kind::Inspector, cx)
                    .placeholder("Notes")
                    .commits_on_blur(window, cx)
                    .expanding()
            }),
            view: None,
            records: HashMap::new(),
            asked: None,
            pinned: false,
            subfolders: HashMap::new(),
            reading: None,
        }
    }

    /// Returns true if the view is for what [`described`] returns now.
    pub(super) fn ready(&self, session: &Session) -> bool {
        self.view
            .as_ref()
            .is_some_and(|view| view.index == described(session).0)
    }
}

impl Inspector {
    /// Builds the view again for what the rail is to describe, and replaces
    /// the old view if the new one differs. The view of a folder or smart
    /// folder is not built until its record has been read, so the old view
    /// stays until the new one is complete.
    pub(super) fn refresh_scope(&mut self, cx: &mut Context<Self>) {
        // The record of the destination that is being opened is asked for
        // now, so that it has usually arrived when its grid has.
        let opening = {
            let session = self.session.read(cx);
            (self.scope.asked != Some(session.scope))
                .then(|| (session.scope, session.nav.get(session.scope)))
                .and_then(|(index, entry)| Some((index, Kind::of(&entry?.scope))))
        };
        if let Some((index, kind)) = opening {
            self.scope.asked = Some(index);
            if kind != Kind::System {
                self.read_record(kind, cx);
            }
        }

        // For a subfolder selected in the grid, its record and the items
        // directly in it are read, unless they are already known or being
        // read.
        let (index, subfolder) = described(self.session.read(cx));
        if let Some(folder) = subfolder {
            let kind = Kind::Folder(folder);
            if !self.scope.records.contains_key(&kind) && self.scope.reading != Some(kind) {
                self.scope.reading = Some(kind);
                self.read_record(kind, cx);
            }
            if !self.scope.subfolders.contains_key(&folder) {
                self.read_holding(folder, cx);
            }
        }
        let view = {
            let session = self.session.read(cx);
            // Before the first page of the first destination has loaded there
            // is nothing to describe.
            if session.results_epoch == 0 {
                return;
            }
            let Some(entry) = session.nav.get(index) else {
                return;
            };
            if entry.screen.is_some() {
                return;
            }
            let kind = Kind::of(&entry.scope);
            let record = match kind {
                Kind::System => None,
                kind => match self.scope.records.get(&kind) {
                    Some(record) => record.clone(),
                    None => return,
                },
            };
            let (notes, folder) = match &record {
                Some(Record::Folder { record, auto_tags }) => (
                    record.notes.clone(),
                    Some((*auto_tags, record.watch_enabled)),
                ),
                Some(Record::Smart(record)) => (record.notes.clone(), None),
                None => (None, None),
            };
            // A subfolder shows only the items that are directly in it: their
            // count from the sidebar, their total size and the first of them.
            let (total, size, head) = match subfolder {
                Some(folder) => match self.scope.subfolders.get(&folder) {
                    Some(Some(holding)) => (
                        entry.count.unwrap_or(0),
                        holding.size,
                        holding.head.as_slice(),
                    ),
                    _ => return,
                },
                None => (
                    session.total,
                    session.total_size_bytes,
                    session.head.as_slice(),
                ),
            };
            ScopeView {
                index,
                kind,
                name: entry.label.clone(),
                notes: notes.unwrap_or_default(),
                total,
                size,
                // The covers are the first items of the destination, also when
                // the grid has scrolled away from them.
                covers: head
                    .iter()
                    .take(COVERS)
                    .map(|item| Cover {
                        thumbnail: crate::grid::thumbnails::existing(
                            session.thumbnail(&item.content_hash),
                        ),
                        background: item
                            .palette
                            .first()
                            .map(|color| format::lab_to_rgb(color.l, color.a, color.b)),
                    })
                    .collect(),
                search: if subfolder.is_some() {
                    String::new()
                } else {
                    session.search_text().trim().to_owned()
                },
                folder,
            }
        };
        if self.scope.view.as_ref() == Some(&view) {
            return;
        }
        if view.kind != Kind::System {
            let key = view.kind.key();
            let (name, notes) = (view.name.clone(), view.notes.clone());
            self.scope
                .name
                .update(cx, |field, cx| field.set_field(key, name, cx));
            self.scope
                .notes
                .update(cx, |field, cx| field.set_field(key, notes, cx));
        }
        self.scope.view = Some(view);
        cx.notify();
    }

    /// Reads the total size and the first four of the items that are directly
    /// in `folder`, in the default sort order.
    fn read_holding(&mut self, folder: FolderId, cx: &mut Context<Self>) {
        self.scope.subfolders.insert(folder, None);
        let job = self.session.read(cx).run(move |library| {
            library.query(
                &picto_library::query::RootQuery {
                    scope: ItemScope::Folder { folder_id: folder },
                    view: Default::default(),
                },
                &picto_library::query::PageRequest {
                    limit: COVERS,
                    cursor: None,
                },
            )
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = job.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| match result {
                Ok(page) => {
                    this.scope.subfolders.insert(
                        folder,
                        Some(Holding {
                            size: page.total_size_bytes,
                            head: page.items,
                        }),
                    );
                    this.refresh_scope(cx);
                }
                Err(error) => {
                    eprintln!("inspector: reading folder {folder} failed: {error}");
                    this.scope.subfolders.remove(&folder);
                }
            });
        })
        .detach();
    }

    /// Drops what was read about the items of subfolders. This is called
    /// after an edit, so that the next subfolder that is selected is read
    /// again.
    pub(super) fn forget_holdings(&mut self, cx: &App) {
        self.scope.subfolders.clear();
        // The record of a selected subfolder is dropped too, so that it is
        // read again. The record of the destination the grid shows is kept.
        // It is read again when the destination is opened.
        let displayed = {
            let session = self.session.read(cx);
            session
                .nav
                .get(session.displayed)
                .map(|entry| Kind::of(&entry.scope))
        };
        self.scope
            .records
            .retain(|kind, _| Some(*kind) == displayed || *kind == Kind::System);
    }

    /// Reads the record of a folder or smart folder on the library's thread
    /// and then builds the view again.
    fn read_record(&mut self, kind: Kind, cx: &mut Context<Self>) {
        let job = self.session.read(cx).run(move |library| match kind {
            Kind::Folder(id) => {
                let record = library
                    .folders()?
                    .into_iter()
                    .find(|folder| folder.folder_id == id)
                    .ok_or_else(|| LibraryError::NotFound(format!("folder {id}")))?;
                let auto_tags = !library.folder_auto_tags(id)?.is_empty();
                Ok(Record::Folder { record, auto_tags })
            }
            Kind::Smart(id) => library
                .smart_folders()?
                .into_iter()
                .find(|folder| folder.smart_folder_id == id)
                .map(Record::Smart)
                .ok_or_else(|| LibraryError::NotFound(format!("smart folder {id}"))),
            Kind::System => Err(LibraryError::InvalidState(
                "a built-in destination has no record".into(),
            )),
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = job.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = &result {
                    eprintln!("inspector: reading {kind:?} failed: {error}");
                }
                this.scope.records.insert(kind, result.ok());
                if this.scope.reading == Some(kind) {
                    this.scope.reading = None;
                }
                this.refresh_scope(cx);
            });
        })
        .detach();
    }

    /// Saves a new name or new notes for the folder or smart folder that the
    /// rail describes, with the functions the sidebar uses (`rename_node`,
    /// `set_node_notes`). If the save fails, for example because the library
    /// refuses the name, the fields are set back to the saved values.
    pub(super) fn commit_scope(
        &mut self,
        key: u64,
        name: Option<String>,
        notes: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self
            .scope
            .view
            .as_ref()
            .filter(|view| view.kind.key() == key)
        else {
            return;
        };
        let (kind, index) = (view.kind, view.index);
        if self.scope.records.get(&kind).cloned().flatten().is_none() {
            return;
        }
        // The row is passed by its id, because the sidebar may rebuild the
        // list of destinations while the save runs.
        let Some(node) = self.session.read(cx).nav.get(index).map(|entry| entry.id()) else {
            return;
        };
        let (saved_name, saved_notes) = (name.clone(), notes.clone());
        let then = move |this: &mut Self, saved: bool, cx: &mut Context<Self>| {
            this.scope_saved(kind, saved.then_some((saved_name, saved_notes)), cx)
        };
        match (name, notes) {
            (Some(name), _) => {
                crate::shell::sidebar::rename_node(&self.session, node, name, cx, then)
            }
            (None, Some(notes)) => {
                crate::shell::sidebar::set_node_notes(&self.session, &node, notes, cx, then)
            }
            (None, None) => {}
        }
    }

    /// Handles the end of a save of the name or notes. `saved` holds the new
    /// values if the save succeeded. They are then written into the record
    /// that the rail holds. If the save failed, the fields are set back to
    /// the values of the view.
    fn scope_saved(
        &mut self,
        kind: Kind,
        saved: Option<(Option<String>, Option<String>)>,
        cx: &mut Context<Self>,
    ) {
        let Some((name, notes)) = saved else {
            if let Some(view) = self.scope.view.clone() {
                let key = view.kind.key();
                self.scope
                    .name
                    .update(cx, |field, cx| field.set_field(key, view.name, cx));
                self.scope
                    .notes
                    .update(cx, |field, cx| field.set_field(key, view.notes, cx));
            }
            return;
        };
        let notes = notes.map(|notes| (!notes.is_empty()).then_some(notes));
        let (record_name, record_notes) = match self.scope.records.get_mut(&kind) {
            Some(Some(Record::Folder { record, .. })) => (&mut record.name, &mut record.notes),
            Some(Some(Record::Smart(record))) => (&mut record.name, &mut record.notes),
            _ => return,
        };
        if let Some(name) = name {
            *record_name = name;
        }
        if let Some(notes) = notes {
            *record_notes = notes;
        }
        self.refresh_scope(cx);
    }

    pub(super) fn release_collage(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.scope.pinned) {
            return;
        }
        self.thumbnails.update(cx, |thumbnails, _| {
            for holder in COVER_PINS {
                thumbnails.pin(holder, None);
            }
        });
    }

    /// Draws the folder card: a front card that holds the first four covers
    /// in a grid of two by two, and above it the top edges of two cards that
    /// lie behind it.
    fn collage(&mut self, covers: &[Cover], cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        self.scope.pinned = true;
        self.thumbnails.update(cx, |thumbnails, _| {
            for (index, holder) in COVER_PINS.into_iter().enumerate() {
                let cover = covers
                    .get(index)
                    .map(|cover| Resource::Path(cover.thumbnail.clone().into()));
                thumbnails.pin(holder, cover);
            }
        });
        // The top edge of a card behind the front card is a strip 2px high
        // with rounded top corners.
        let peek = |inset: f32, top: f32, color: Rgba| {
            div()
                .absolute()
                .top(px(top))
                .left(px(inset))
                .right(px(inset))
                .h(px(2.))
                .rounded_t(px(1.))
                .bg(color)
        };
        let content = if covers.is_empty() {
            // A destination without items shows a folder icon at 20% opacity.
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(ui::icon("inspector-folder", 32., theme.text()).opacity(0.2))
        } else {
            // The cells have 8px gaps between them and corners of 8px. Each
            // cell shows its cover over the cover's colour.
            let cell = |cover: Option<&Cover>| {
                // The cover is positioned over the cell and is not part of its
                // layout, so that the cover's own size cannot stretch the grid.
                let mut cell = div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .h_full()
                    .overflow_hidden()
                    .rounded(px(theme::RADIUS_LG))
                    .bg(theme.contrast(5.));
                if let Some(cover) = cover {
                    if let Some(background) = cover.background {
                        cell = cell.bg(rgb(background));
                    }
                    cell = cell.child(
                        img(cover.thumbnail.clone())
                            .image_cache(&self.thumbnails)
                            .absolute()
                            .inset_0()
                            .size_full()
                            .rounded(px(theme::RADIUS_LG))
                            .object_fit(ObjectFit::Cover),
                    );
                }
                cell
            };
            let row = |first: usize| {
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(px(8.))
                    .child(cell(covers.get(first)))
                    .child(cell(covers.get(first + 1)))
            };
            div()
                .size_full()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(row(0))
                .child(row(2))
        };
        // The background of the front card is brighter at the top than at
        // the bottom.
        let front = div()
            .absolute()
            .top(px(10.))
            .left_0()
            .right_0()
            .bottom_0()
            .rounded(px(10.))
            .bg(linear_gradient(
                180.,
                linear_color_stop(theme.contrast(15.), 0.),
                linear_color_stop(theme.contrast(5.), 1.),
            ))
            // The front card has a border on its left and right sides. The
            // brighter border along the top is the child below, drawn over
            // them. The top padding is 9px and not 8px to make room for that
            // border's pixel.
            .border_l_1()
            .border_r_1()
            .border_color(theme.contrast(5.))
            .pt(px(9.))
            .px(px(8.))
            .pb(px(8.))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(-1.))
                    .right(px(-1.))
                    .h(px(10.))
                    .rounded_t(px(10.))
                    .border_t_1()
                    .border_color(theme.contrast(15.)),
            )
            .child(content);
        // The card is 200px wide and 150px high, centred in an area that is
        // at least 200px high.
        div()
            .w_full()
            .min_h(px(PREVIEW))
            .mb(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .relative()
                    .w(px(PREVIEW))
                    .h(px(PREVIEW * 3. / 4.))
                    .child(peek(20., 2., theme.contrast(10.)))
                    .child(peek(10., 6., theme.contrast(15.)))
                    .child(front),
            )
    }

    /// Draws the column for a destination: the folder card, the name, which
    /// cannot be edited for a built-in destination, the notes of a folder or
    /// smart folder, the item count and size, the Export action, and the
    /// Details section.
    pub(super) fn scope_body(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let Some(view) = self.scope.view.clone() else {
            return self.scroller();
        };
        let mut panel = Self::panel()
            .child(self.collage(&view.covers, cx))
            .child(self.palette(&[], window, cx));
        panel = if view.kind == Kind::System {
            panel.child(
                div()
                    .mb(px(8.))
                    .child(ui::field::read_only(view.name.clone(), cx)),
            )
        } else {
            panel
                .child(div().mb(px(8.)).child(ui::field(
                    "scope-name",
                    &self.scope.name,
                    Look::Inspector,
                )))
                .child(div().mb(px(8.)).child(ui::field(
                    "scope-notes",
                    &self.scope.notes,
                    Look::Inspector,
                )))
        };

        let mut properties = div()
            .child(Self::property(t("Items"), grouped(view.total), cx))
            .child(Self::property(
                t("Size"),
                crate::text::size_to_gigabytes(view.size as u64),
                cx,
            ));
        let scope = match view.kind {
            Kind::Folder(folder_id) => Some(ItemScope::Folder { folder_id }),
            Kind::Smart(smart_folder_id) => Some(ItemScope::SmartFolder { smart_folder_id }),
            Kind::System => None,
        };
        if let Some(scope) = scope.filter(|_| view.total > 0) {
            // The export takes every item the folder or smart folder holds,
            // in the default sort order, as the sidebar's Export command
            // does.
            let target = picto_library::selection::SelectionTarget::Query {
                query: picto_library::query::RootQuery {
                    scope,
                    view: Default::default(),
                },
                excluded_root_ids: Vec::new(),
            };
            properties = properties.child(div().mt(px(12.)).child(Self::export_row(
                "scope-export",
                target,
                view.total,
                cx,
            )));
        }
        panel = panel.child(self.section(t("Properties"), Some(properties.into_any_element()), cx));

        let mut details = Vec::new();
        if !view.search.is_empty() {
            details.push((t("Search"), view.search.clone()));
        }
        if let Some((auto_tags, watch)) = view.folder {
            let answer = |on: bool| if on { t("Yes") } else { t("No") }.to_owned();
            details.push((t("Auto tags"), answer(auto_tags)));
            details.push((t("Watch"), answer(watch)));
        }
        if !details.is_empty() {
            let rows = details.into_iter().fold(div(), |rows, (label, value)| {
                rows.child(Self::property(label, value, cx))
            });
            panel = panel.child(self.section(t("Details"), Some(rows.into_any_element()), cx));
        }
        self.scroller().child(panel)
    }
}
