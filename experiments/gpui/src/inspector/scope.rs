//! The inspector with nothing selected: the destination the grid shows
//! (the scope state of `Inspector.tsx`). A folder card of its first covers,
//! its name and notes, how many items it holds and their size, and for a
//! folder whether it tags and watches.
use super::{Inspector, PREVIEW};
use crate::grid::format;
use crate::session::Session;
use crate::theme;
use crate::ui::{self, field::Look, text_input::TextInput};
use gpui::*;
use picto_library::query::ItemScope;
use picto_library::{FolderId, FolderRecord, LibraryError, SmartFolderId, SmartFolderRecord};
use std::collections::HashMap;
use std::path::PathBuf;

/// The kind of destination, which decides what the rail lets change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Kind {
    /// All, Inbox, Trash and the other built-in destinations: read-only.
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

    /// The key the name and notes fields are set with, distinct for every
    /// folder and smart folder.
    fn key(self) -> u64 {
        match self {
            Kind::System => u64::MAX,
            Kind::Folder(id) => id.0 as u64,
            Kind::Smart(id) => 1 << 32 | id.0 as u64,
        }
    }
}

/// A folder's or smart folder's own record, which the sidebar list does not
/// carry.
#[derive(Clone)]
pub(super) enum Record {
    Folder {
        record: FolderRecord,
        auto_tags: bool,
    },
    Smart(SmartFolderRecord),
}

/// One cell of the folder card: a cover and the colour behind it.
#[derive(Clone, PartialEq)]
struct Cover {
    thumbnail: PathBuf,
    background: Option<u32>,
}

/// Everything the scope state draws, taken together so it changes at once.
#[derive(Clone, PartialEq)]
pub(super) struct ScopeView {
    /// The destination's place in the sidebar list.
    index: usize,
    kind: Kind,
    name: String,
    notes: String,
    total: u64,
    size: u128,
    covers: Vec<Cover>,
    search: String,
    /// A folder's auto-tags and watch, once its record has been read.
    folder: Option<(bool, bool)>,
}

/// `.preview`'s collage holds the first four covers.
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
    /// Records by destination; `None` when reading one failed.
    records: HashMap<Kind, Option<Record>>,
    /// The destination whose record was last asked for.
    asked: Option<usize>,
    /// Whether the card's covers are pinned in the thumbnail cache.
    pinned: bool,
    /// What a subfolder selected in the grid holds of its own: its size and
    /// first items, by folder; `None` while being read.
    subfolders: HashMap<FolderId, Option<Holding>>,
    /// The selected subfolder whose record is being read.
    reading: Option<Kind>,
}

/// A folder's own items, as the scope view of a selected subfolder shows
/// them.
#[derive(Clone)]
struct Holding {
    size: u128,
    head: Vec<picto_library::query::RootSummary>,
}

/// What the scope view describes: a subfolder tile selected in the grid
/// alone (`selectedSubfolderNodeIdAtom`), else the destination showing.
/// Its place in the destination list, and the subfolder when it is one.
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
            name: cx.new(|cx| TextInput::expanding("Name", window, cx)),
            notes: cx.new(|cx| TextInput::expanding("Notes", window, cx)),
            view: None,
            records: HashMap::new(),
            asked: None,
            pinned: false,
            subfolders: HashMap::new(),
            reading: None,
        }
    }

    /// Whether the view is the destination the grid shows.
    pub(super) fn ready(&self, session: &Session) -> bool {
        self.view
            .as_ref()
            .is_some_and(|view| view.index == described(session).0)
    }
}

impl Inspector {
    /// Brings the scope view up to date with the destination showing. A
    /// folder's view waits for its record, so the old view stays up until
    /// the new one is whole.
    pub(super) fn refresh_scope(&mut self, cx: &mut Context<Self>) {
        // The record of the destination being opened is asked for now, so it
        // is usually there by the time its grid is.
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

        // A subfolder selected in the grid: its record, and what it holds.
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
            // Before the first page there is nothing to describe.
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
            // A subfolder shows what it holds of its own: its count in the
            // sidebar, and the size and first items of those.
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
                // The destination's first items, wherever the grid's window
                // is (`queryHeadItems`).
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

    /// Reads the size and first items of what a subfolder holds of its own,
    /// in the order production opens a folder in (newest first).
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

    /// Forgets what subfolders hold, after an edit, so the next one
    /// selected is read afresh.
    pub(super) fn forget_holdings(&mut self, cx: &App) {
        self.scope.subfolders.clear();
        // The selected subfolder's record is read again too; the
        // destination's own is read again when it is opened.
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

    /// Reads a folder's or smart folder's record on the library's thread.
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

    /// Saves a new name or notes for the folder or smart folder showing,
    /// as the sidebar does (`rename_node`, `set_node_notes`). A name the
    /// library refuses puts the fields back as they were.
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
        // The row is found by id: the sidebar may rebuild the destination
        // list while the save runs.
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

    /// A save of the destination's name or notes is done: the record shown
    /// takes them, or the fields go back to what it has.
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

    /// The folder card (`.thumbnail`): two cards peeking out behind a front
    /// card that holds the first four covers in a 2×2 grid.
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
        // `.pic2`, `.pic3`: 2px strips with rounded tops.
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
            // `.folderPlaceholder`
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(ui::icon("inspector-folder", 32., theme.text()).opacity(0.2))
        } else {
            // `.collage`: 8px gaps between cells with 8px corners, each
            // showing its cover over the cover's colour.
            let cell = |cover: Option<&Cover>| {
                // The cover is laid over the cell, so its own size cannot
                // stretch the grid.
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
        // `.pic1`: the front card, lit from the top. CSS lights it with a
        // radial gradient from the top centre; this is the vertical line
        // through that centre.
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
            // The sides' borders; the brighter top one is drawn over them,
            // and the padding below it stands for its pixel.
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
        // `.preview` around a 4:3 card at most 200px wide.
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

    /// The scope state's column: the folder card, the name (read-only for a
    /// built-in destination), the notes of a folder or smart folder, the
    /// item count and size, Export, and the Details section.
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
            .child(Self::property("Items", format::grouped(view.total), cx))
            .child(Self::property(
                "Size",
                crate::text::size_to_gigabytes(view.size as u64),
                cx,
            ));
        let scope = match view.kind {
            Kind::Folder(folder_id) => Some(ItemScope::Folder { folder_id }),
            Kind::Smart(smart_folder_id) => Some(ItemScope::SmartFolder { smart_folder_id }),
            Kind::System => None,
        };
        if let Some(scope) = scope.filter(|_| view.total > 0) {
            // Everything the folder or smart folder holds, in its query's
            // order, as the sidebar's export takes it.
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
        panel = panel.child(self.section("Properties", Some(properties.into_any_element()), cx));

        let mut details = Vec::new();
        if !view.search.is_empty() {
            details.push(("Search", view.search.clone()));
        }
        if let Some((auto_tags, watch)) = view.folder {
            let answer = |on: bool| if on { "Yes" } else { "No" }.to_owned();
            details.push(("Auto tags", answer(auto_tags)));
            details.push(("Watch", answer(watch)));
        }
        if !details.is_empty() {
            let rows = details.into_iter().fold(div(), |rows, (label, value)| {
                rows.child(Self::property(label, value, cx))
            });
            panel = panel.child(self.section("Details", Some(rows.into_any_element()), cx));
        }
        self.scroller().child(panel)
    }
}
