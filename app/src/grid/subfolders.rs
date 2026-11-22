//! The subfolder header: the block at the top of a folder's grid that lists
//! the folder's subfolders. From top to bottom it shows the label
//! "Folders (n)" with a chevron that collapses and expands the list, one tile
//! for each subfolder, and the label "Content (n)" above the folder's own
//! items. A tile is a card that shows the subfolder's cover picture, with the
//! subfolder's name and item count below the card.
//!
//! A click selects a tile, a double-click opens that folder in the grid, and a
//! right-click opens the folder's context menu. A tile's name can be edited in
//! place, and items dragged from the grid can be dropped on a tile to put them
//! into that folder.
use super::GridView;
use super::selection::Press;
use crate::backend::Navigation;
use crate::i18n::t;
use crate::theme::{self, Text as _};
use crate::ui::{self, text_input::Kind, text_input::Renamed, text_input::TextInput};
use gpui::prelude::FluentBuilder;
use gpui::*;
use picto_library::FolderId;
use picto_library::query::ItemScope;
use std::collections::HashMap;
use std::path::PathBuf;

/// The event the grid emits when a subfolder tile is right-clicked, or clicked
/// with Control held on macOS. The shell responds by opening the folder's
/// context menu at `position`, which is a position in the window. `node` is
/// the node id of the tile that was clicked. `selected` holds the node ids of
/// all the subfolder tiles that are selected, and `node` is always one of
/// them. When more than one tile is selected, the menu acts on all of them.
pub struct FolderMenuRequested {
    pub node: String,
    pub selected: Vec<String>,
    pub position: Point<Pixels>,
}

/// One subfolder of the folder the grid shows, with the values its tile
/// displays.
#[derive(Clone, Debug, PartialEq)]
pub struct Child {
    /// The folder's node id, for example `folder:5`. The sidebar uses the same
    /// id for the folder (`Navigation::id`).
    pub node: String,
    /// The folder's index in the session's destination list (`Session::nav`).
    /// Double-clicking the tile navigates to this index.
    pub index: usize,
    pub folder: FolderId,
    pub label: String,
    pub icon: Option<String>,
    pub tint: Option<u32>,
    pub count: u64,
}

/// Returns the subfolders to show as tiles for the destination at index
/// `displayed` of the destination list `nav`. When `descendants` is false,
/// these are the folder's direct children. When it is true, which is when the
/// folder shows its subfolders' items too, these are all of its descendants,
/// depth first: each child is followed by its own children before its next
/// sibling. Siblings keep the order they have in `nav`, which is the sidebar's
/// order. The result is empty when the destination is not a folder or is a
/// screen of its own.
pub fn children(nav: &[Navigation], displayed: usize, descendants: bool) -> Vec<Child> {
    let Some(entry) = nav.get(displayed) else {
        return Vec::new();
    };
    if entry.screen.is_some()
        || !matches!(
            entry.scope,
            ItemScope::Folder { .. } | ItemScope::FolderTree { .. }
        )
    {
        return Vec::new();
    }
    let mut found = Vec::new();
    // Adds the child folders of the destination at index `parent` to `found`.
    // When `deep` is true, each child is followed at once by all of its own
    // descendants, before its next sibling.
    fn walk(nav: &[Navigation], parent: usize, deep: bool, found: &mut Vec<Child>) {
        for (index, entry) in nav.iter().enumerate() {
            if entry.parent != Some(parent) {
                continue;
            }
            let (ItemScope::Folder { folder_id } | ItemScope::FolderTree { folder_id }) =
                entry.scope
            else {
                continue;
            };
            found.push(Child {
                node: entry.id(),
                index,
                folder: folder_id,
                label: entry.label.clone(),
                icon: entry.icon.clone(),
                tint: entry.tint,
                count: entry.count.unwrap_or(0),
            });
            if deep {
                walk(nav, index, deep, found);
            }
        }
    }
    walk(nav, displayed, descendants, &mut found);
    found
}

/// Returns the folder id contained in a node id such as `folder:5`. Returns
/// `None` when the node id does not have that form.
pub fn folder_of(node: &str) -> Option<FolderId> {
    node.strip_prefix("folder:")?.parse().ok().map(FolderId)
}

/// Returns the padding in pixels on the left and right of the header: 16px,
/// or 8px when the grid's spacing `gap` is 8px or less. The gap between two
/// subfolder tiles is the grid's spacing `gap` itself.
fn padding(gap: f32) -> f32 {
    if gap <= 8. { 8. } else { 16. }
}

/// The "Folders (n)" label: a 4px margin above it, a line 18.6px tall (12px
/// text at a line height of 1.55), and a 6px margin below it.
const LABEL_TOP: f32 = 4.;
const LABEL_HEIGHT: f32 = 18.6;
const LABEL_BELOW: f32 = 6.;
/// The list of subfolder tiles: a 6px margin above it and a 30px margin below
/// it.
const LIST_TOP: f32 = 6.;
const LIST_BELOW: f32 = 30.;
/// The "Content (n)" label: a 4px margin above it and a 6px margin below it.
/// When the list of tiles is shown, the list's 30px bottom margin is the
/// space above the label and the 4px is not added to it. In the same way the
/// 6px below the label is not added to the header's own 4px bottom margin
/// (`CONTAINER_BELOW`).
const CONTENT_TOP: f32 = 4.;
const CONTENT_BELOW: f32 = 6.;
/// The margin in pixels below the whole header. It is used only when the list
/// of tiles is collapsed and there is no "Content (n)" label.
const CONTAINER_BELOW: f32 = 4.;
/// A tile's card. Its height is 75% of the tile's width plus 10px, and there
/// are 12px between the card and the name below it.
const CARD_EXTRA: f32 = 10.;
const CARD_BELOW: f32 = 12.;
/// The height in pixels of the row that holds the folder's icon and name. The
/// row is one line of the tile's text, which is 13px at a line height of 1.55,
/// and the 19px icon in the row makes that line 24.1px tall. A name that wraps
/// onto a second 15px line makes the row 11px taller (`NAME_SECOND_LINE`).
const NAME_LINE: f32 = 24.1;
const NAME_SECOND_LINE: f32 = 11.;
/// The height in pixels of the line that shows the folder's item count: 10px
/// text at a line height of 1.3.
const METAS: f32 = 13.;
/// The width in pixels of the folder icon in the name row, and the gap between
/// the icon and the name.
const NAME_ICON: f32 = 19.;
const NAME_GAP: f32 = 6.;
/// The width in pixels of the text field in which a subfolder's name is
/// edited.
const RENAME_WIDTH: f32 = 161.6;
/// The line height in pixels of a folder's name. A name shows at most two
/// lines.
const NAME_LINE_HEIGHT: f32 = 15.;

/// The positions and sizes of the parts of the subfolder header, in pixels.
/// Vertical positions are measured from the top of the grid's scrolled
/// content, which is where the header starts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub padding: f32,
    pub gap: f32,
    pub columns: usize,
    pub tile_width: f32,
    /// For each subfolder tile, in the order of `Header::children`: the x and
    /// y of its top-left corner, and its height. Every tile is `tile_width`
    /// wide. The list is empty when the list of tiles is collapsed.
    pub tiles: Vec<(f32, f32, f32)>,
    /// The y of the top of the "Content (n)" label. It is `None` when the
    /// folder holds no items, and the label is then not shown.
    pub content: Option<f32>,
    /// The height of the whole header. The folder's item tiles start this far
    /// below the top of the grid's scrolled content.
    pub height: f32,
}

/// Returns the number of columns of subfolder tiles and the width of one tile,
/// for a list `width` pixels wide with `gap` pixels between tiles. `target` is
/// the zoom's tile size. It is clamped to between 150px and 400px, and the
/// result is the narrowest a tile may be. The list has as many columns as fit
/// at that width, and at least one. The columns then share the whole width
/// equally.
pub fn columns(width: f32, target: f32, gap: f32) -> (usize, f32) {
    let size = target.clamp(150., 400.);
    let columns = (((width + gap) / (size + gap)).floor() as usize).max(1);
    let tile = ((width - (columns - 1) as f32 * gap) / columns as f32).max(0.);
    (columns, tile)
}

/// Returns the height of a tile's card: three quarters of the tile's width,
/// plus 10px.
pub fn card_height(tile_width: f32) -> f32 {
    tile_width * 0.75 + CARD_EXTRA
}

/// Computes the header's layout for a grid `width` pixels wide. `target` is
/// the zoom's tile size and `gap` is the grid's spacing. `count` is the number
/// of subfolders. `expanded` says whether the list of tiles is shown.
/// `content` says whether the folder holds items, which decides whether the
/// "Content (n)" label is shown. `two_lines(index)` says whether the name of
/// the subfolder at `index` wraps onto a second line. Every tile in a row gets
/// the height of the tallest tile in that row.
pub fn geometry(
    width: f32,
    target: f32,
    gap: f32,
    count: usize,
    expanded: bool,
    content: bool,
    two_lines: impl Fn(usize) -> bool,
) -> Geometry {
    let padding = padding(gap);
    let list_width = (width - padding * 2.).max(0.);
    let (columns, tile_width) = columns(list_width, target, gap);
    let label_block = LABEL_TOP + LABEL_HEIGHT + LABEL_BELOW;
    let mut tiles = Vec::new();
    let mut bottom = label_block;
    if expanded && count > 0 {
        let top = label_block + LIST_TOP;
        let mut y = top;
        for row in 0..count.div_ceil(columns) {
            let indices = row * columns..(row * columns + columns).min(count);
            let height = indices
                .clone()
                .map(|index| {
                    card_height(tile_width)
                        + CARD_BELOW
                        + NAME_LINE
                        + if two_lines(index) {
                            NAME_SECOND_LINE
                        } else {
                            0.
                        }
                        + METAS
                })
                .fold(0., f32::max);
            for (column, _) in indices.enumerate() {
                tiles.push((padding + column as f32 * (tile_width + gap), y, height));
            }
            y += height + gap;
        }
        bottom = y - gap;
    }
    // The space below the last part of the header depends on which parts are
    // shown. When the list of tiles is shown, its 30px bottom margin is the
    // only space below it, and the 4px top margin of the "Content (n)" label
    // is not added. When the label is shown, its 6px bottom margin is the only
    // space below it, and the header's own 4px bottom margin is not added.
    let shown = expanded && count > 0;
    let (content_top, height) = match (content, shown) {
        (true, true) => {
            let top = bottom + LIST_BELOW;
            (Some(top), top + LABEL_HEIGHT + CONTENT_BELOW)
        }
        (true, false) => {
            let top = bottom + CONTENT_TOP;
            (Some(top), top + LABEL_HEIGHT + CONTENT_BELOW)
        }
        (false, true) => (None, bottom + LIST_BELOW),
        (false, false) => (None, bottom + CONTAINER_BELOW),
    };
    Geometry {
        padding,
        gap,
        columns,
        tile_width,
        tiles,
        content: content_top,
        height,
    }
}

/// The picture shown on a subfolder tile's card. It is the item set as the
/// folder's cover (`Library::folder_cover`). When no cover is set, it is the
/// first item of the folder in the folder's own order. `thumbnail` is the path
/// of that item's thumbnail, and `font` says that the item is a font file.
#[derive(Clone, Debug, PartialEq)]
pub struct Cover {
    pub thumbnail: PathBuf,
    pub font: bool,
}

/// Reads the cover of `folder` from the library. It is called on the library's
/// thread, not on the thread that draws. `blobs` is the library's blob store
/// folder, which the path of the thumbnail is built from. Returns `None` when
/// the folder has no cover set and either holds no items of its own or the
/// query for its first item fails.
fn read_cover(
    library: &picto_library::Library,
    folder: FolderId,
    blobs: &std::path::Path,
) -> Option<Cover> {
    use picto_library::predicate::{ItemSort, SortDirection, SortField, ViewQuerySpec};
    use picto_library::query::{PageRequest, RootQuery};
    let (hash, mime) = match library.folder_cover(folder).ok().flatten() {
        Some(cover) => (cover.content_hash, cover.mime),
        None => {
            let page = library
                .query(
                    &RootQuery {
                        scope: ItemScope::Folder { folder_id: folder },
                        view: ViewQuerySpec {
                            filter: Default::default(),
                            sort: ItemSort {
                                field: SortField::FolderOrder,
                                direction: SortDirection::Ascending,
                                random_seed: None,
                            },
                        },
                    },
                    &PageRequest {
                        limit: 1,
                        cursor: None,
                    },
                )
                .ok()?;
            let item = page.items.into_iter().next()?;
            (item.content_hash, item.mime)
        }
    };
    Some(Cover {
        thumbnail: crate::session::thumbnail_in(blobs, &hash),
        font: mime.starts_with("font/"),
    })
}

/// A rename in progress: the name of the subfolder tile `node` is being edited
/// in a text field on the tile.
pub struct Renaming {
    pub node: String,
    original: String,
    input: Entity<TextInput>,
    _subscription: Subscription,
}

/// The state of the subfolder header. The grid keeps it from one frame to the
/// next in `GridView::subfolders`.
pub struct Header {
    /// The index in the destination list of the destination the header was
    /// last brought up to date for. When the grid shows a different
    /// destination, the list of tiles is expanded again, and the drop target
    /// and any rename in progress are cleared.
    shown_for: Option<usize>,
    pub children: Vec<Child>,
    /// The values `children` was last computed from: the index of the
    /// displayed destination, whether the folder shows its subfolders' items
    /// too (the `show_subfolders` view preference), and whether the viewer is
    /// open. `children` is computed again when any of the three changes.
    listed: Option<(usize, bool, bool)>,
    /// Set when the session reports that its destination list changed
    /// (`Change::Navigation`). The next frame then computes `children` again.
    pub relist: bool,
    /// For each entry of `children`, whether its name is too wide for one line
    /// and wraps onto a second line, together with the width in pixels a name
    /// had available when that was measured. It is measured again when that
    /// width changes and when `children` is computed again.
    lines: Option<(f32, Vec<bool>)>,
    covers: HashMap<FolderId, Option<Cover>>,
    /// The folders the most recent cover request was made for, in the order of
    /// `children`. A new request is made when `stale` is set. That happens
    /// when `children` is computed again and no longer lists these same
    /// folders in this order, and when the session reports
    /// `Change::Navigation`.
    asked: Vec<FolderId>,
    /// Numbers the cover requests (`Latest`). When the covers of a request
    /// arrive after a newer request has been made, they are discarded.
    request: super::Latest,
    pub stale: bool,
    /// Whether the list of subfolder tiles is shown. A click on the
    /// "Folders (n)" label switches it. It is set back to `true` when the grid
    /// shows a different destination, and whenever the destination has no
    /// subfolders.
    pub expanded: bool,
    pub geometry: Geometry,
    /// While items are dragged over the grid, the node id of the subfolder
    /// tile under the pointer. Releasing the drag there puts the items into
    /// that folder. The tile is drawn with the same ring as a selected tile.
    pub drop: Option<String>,
    pub renaming: Option<Renaming>,
    /// The node id of a folder that was just created or duplicated from a
    /// tile's context menu. Its tile does not exist until the destination list
    /// has been read again. When the tile is in `children`, its name is opened
    /// for editing and this is cleared.
    pub rename_next: Option<String>,
}

impl Default for Header {
    fn default() -> Self {
        Self {
            shown_for: None,
            children: Vec::new(),
            listed: None,
            relist: false,
            lines: None,
            covers: HashMap::new(),
            asked: Vec::new(),
            request: super::Latest::default(),
            stale: false,
            expanded: true,
            geometry: Geometry::default(),
            drop: None,
            renaming: None,
            rename_next: None,
        }
    }
}

impl Header {
    /// Returns the height of the header in pixels. It is 0 when the
    /// destination has no subfolders, and the header is then not drawn.
    pub fn height(&self) -> f32 {
        if self.children.is_empty() {
            0.
        } else {
            self.geometry.height
        }
    }

    /// Returns the cover thumbnails the header needs loaded, in the order of
    /// `children`: one for each subfolder whose cover has been read, except a
    /// cover that is a font file, because a font file has no thumbnail.
    /// Returns nothing while the list of tiles is collapsed. The grid asks for
    /// these only while the header is inside the viewport.
    pub fn wanted(&self) -> Vec<Resource> {
        if !self.expanded {
            return Vec::new();
        }
        self.children
            .iter()
            .filter_map(|child| self.covers.get(&child.folder)?.as_ref())
            .filter(|cover| !cover.font)
            .map(|cover| Resource::Path(cover.thumbnail.clone().into()))
            .collect()
    }
}

impl GridView {
    /// Brings the header's state up to date for this frame, for a grid `width`
    /// pixels wide. It does four things, in this order:
    ///
    /// 1. It computes the list of subfolders again when the displayed
    ///    destination, the destination list, the `show_subfolders` preference
    ///    or whether the viewer is open has changed.
    ///
    /// 2. It requests the subfolders' covers from the library when the list of
    ///    folders is different or the session reported a change to the
    ///    destination list.
    ///
    /// 3. It opens the name of a folder that was just created or duplicated
    ///    for editing, once that folder's tile exists.
    ///
    /// 4. It computes the header's layout.
    pub(super) fn sync_subfolders(
        &mut self,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (displayed, gap, target, content) = {
            let session = self.session.read(cx);
            let listing = (
                session.displayed,
                session.prefs.show_subfolders,
                session.viewer,
            );
            // Compute `children` again only when one of the values it is
            // computed from has changed (`listed`) or the destination list has
            // changed (`relist`). Computing it goes through the whole
            // destination list, so it is not done on every frame.
            let header = &mut self.subfolders;
            if header.listed != Some(listing) || header.relist {
                header.listed = Some(listing);
                header.relist = false;
                header.lines = None;
                header.children = if session.viewer {
                    Vec::new()
                } else {
                    children(
                        &session.nav,
                        session.displayed,
                        session.prefs.show_subfolders,
                    )
                };
                // Request the covers again when the folders listed now are not
                // the same folders, in the same order, as the ones the last
                // cover request was made for.
                header.stale |= header
                    .children
                    .iter()
                    .map(|child| child.folder)
                    .ne(header.asked.iter().copied());
            }
            (
                session.displayed,
                session.prefs.gap,
                session.tile,
                session.total > 0,
            )
        };
        let header = &mut self.subfolders;
        if header.shown_for != Some(displayed) {
            header.shown_for = Some(displayed);
            header.expanded = true;
            header.drop = None;
            header.renaming = None;
        }
        if header.children.is_empty() {
            header.expanded = true;
        }
        if header.stale && !header.children.is_empty() {
            let ids = header
                .children
                .iter()
                .map(|child| child.folder)
                .collect::<Vec<_>>();
            header.asked = ids.clone();
            header.stale = false;
            let request = header.request.next();
            let blobs = self.session.read(cx).blobs().to_path_buf();
            let job = self.session.read(cx).run(move |library| {
                ids.iter()
                    .map(|id| (*id, read_cover(library, *id, &blobs)))
                    .collect::<Vec<_>>()
            });
            cx.spawn(async move |this, cx| {
                let Ok(covers) = job.await else {
                    return;
                };
                let _ = this.update(cx, |this, cx| {
                    let header = &mut this.subfolders;
                    if !header.request.is(request) {
                        return;
                    }
                    header.covers.retain(|id, _| header.asked.contains(id));
                    header.covers.extend(covers);
                    cx.notify();
                });
            })
            .detach();
        }
        // A folder that was just created or duplicated from a tile's context
        // menu is waiting to be renamed (`rename_next`). Once its tile is in
        // `children`, open its name for editing.
        if let Some(node) = header.rename_next.clone()
            && let Some(child) = header.children.iter().find(|child| child.node == node)
        {
            header.rename_next = None;
            let name = child.label.clone();
            self.rename_subfolder(node, name, window, cx);
        }
        let header = &mut self.subfolders;
        let tile_width = {
            let padding = padding(gap);
            columns((width - padding * 2.).max(0.), target, gap).1
        };
        let name_room = (tile_width - NAME_ICON - NAME_GAP).max(0.);
        if header
            .lines
            .as_ref()
            .is_none_or(|(room, _)| *room != name_room)
        {
            let two_lines = header
                .children
                .iter()
                .map(|child| super::tile::name_width(&child.label, window) > name_room)
                .collect();
            header.lines = Some((name_room, two_lines));
        }
        let two_lines = header
            .lines
            .as_ref()
            .map(|(_, lines)| lines.as_slice())
            .unwrap_or_default();
        header.geometry = geometry(
            width,
            target,
            gap,
            header.children.len(),
            header.expanded,
            content,
            |index| two_lines.get(index).copied().unwrap_or(false),
        );
    }

    /// Returns the index in `Header::children` of the subfolder tile under
    /// `position`, which is a position in the window. Returns `None` when no
    /// tile is there, when the destination has no subfolders, and when the
    /// list of tiles is collapsed.
    pub(super) fn subfolder_at(&self, position: Point<Pixels>) -> Option<usize> {
        let header = &self.subfolders;
        if header.children.is_empty() || !header.expanded {
            return None;
        }
        let (x, y) = self.local(position);
        let y = y + self.raw_scroll();
        if x < 0. || y < 0. || x >= f32::from(self.bounds.size.width) {
            return None;
        }
        let width = header.geometry.tile_width;
        header
            .geometry
            .tiles
            .iter()
            .position(|&(left, top, height)| {
                x >= left && x < left + width && y >= top && y < top + height
            })
            .filter(|index| *index < header.children.len())
    }

    /// Opens the name of the subfolder tile with node id `node` for editing as
    /// soon as that tile exists. It is called for a folder that was just
    /// created or duplicated, whose tile appears only after the destination
    /// list has been read again.
    pub fn rename_subfolder_when_shown(&mut self, node: String, cx: &mut Context<Self>) {
        self.subfolders.rename_next = Some(node);
        cx.notify();
    }

    /// Returns the node ids of the subfolder tiles that the rubber band
    /// rectangle `rect` overlaps. `rect` is in the coordinates of the item
    /// layout (`GridView::layout`), whose y = 0 lies `placement.top` below the
    /// bottom of the header. The subfolder tiles are positioned from the top
    /// of the header, so `placement.top` and the header's height are added to
    /// `rect.top` before the two are compared.
    pub(super) fn subfolders_in(&self, rect: super::marquee::Rect) -> Vec<String> {
        let header = &self.subfolders;
        if header.children.is_empty() || !header.expanded {
            return Vec::new();
        }
        let top = rect.top + self.placement.top + header.height();
        let width = header.geometry.tile_width;
        header
            .children
            .iter()
            .zip(&header.geometry.tiles)
            .filter(|(_, (x, y, height))| {
                *x + width > rect.left
                    && *x < rect.left + rect.width
                    && *y + *height > top
                    && *y < top + rect.height
            })
            .map(|(child, _)| child.node.clone())
            .collect()
    }

    /// Starts renaming the subfolder tile with node id `node`. The tile shows
    /// a text field in place of its name. The field holds `name` with all of
    /// it selected, and takes the keyboard focus. Enter, or moving the focus
    /// out of the field, commits the new name. Escape cancels.
    pub fn rename_subfolder(
        &mut self,
        node: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| TextInput::new(Kind::Form, cx).renaming(&name, window, cx));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &Renamed, window, cx| {
                let header = &mut this.subfolders;
                if header
                    .renaming
                    .as_ref()
                    .is_none_or(|open| open.input.entity_id() != input.entity_id())
                {
                    return;
                }
                let Some(open) = header.renaming.take() else {
                    return;
                };
                // Rename the folder in the library only when the rename was
                // committed, the new name is not empty, and the new name
                // differs from the old one. An empty name is treated like a
                // cancel.
                if let Renamed::Commit(value) = event
                    && !value.is_empty()
                    && *value != open.original
                    && let Some(folder) = folder_of(&open.node)
                {
                    let value = value.clone();
                    this.session.update(cx, |session, cx| {
                        let _ = session.edit_reporting(
                            crate::session::Report::failure(t("Unable to rename folder")),
                            move |library| library.rename_folder(folder, &value).map(|_| ()),
                            crate::session::Reread::NAVIGATION,
                            cx,
                        );
                    });
                }
                if window.focused(cx).is_none() {
                    window.focus(&this.focus, cx);
                }
                cx.notify();
            },
        );
        window.focus(&input.focus_handle(cx), cx);
        self.subfolders.renaming = Some(Renaming {
            node,
            original: name,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Builds the element for the subfolder header. The element is placed at
    /// the top of the grid's scrolled content, above the item tiles.
    pub(super) fn subfolder_header(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = theme::theme(cx);
        let card = if theme.is_light() {
            theme::FOLDER_CARD_LIGHT
        } else {
            theme::FOLDER_CARD_DARK
        };
        let header = &self.subfolders;
        let geometry = header.geometry.clone();
        let total = self.session.read(cx).total;
        // The subfolder tiles that are selected. A rubber band adds the tiles
        // it covers to this selection only when the mouse button is released.
        // While the band is being dragged, those tiles are drawn without the
        // selection ring.
        let selected = self.session.read(cx).folder_selection.clone();
        let expanded = header.expanded;
        let padding = geometry.padding;
        let mut root = div()
            .absolute()
            .left_0()
            .top_0()
            .w_full()
            .h(px(geometry.height))
            .font_family(theme::FONT_UI);
        // The "Folders (n)" label with its chevron. A click on it collapses or
        // expands the list of tiles. Its text changes from the secondary text
        // colour to the primary text colour while the pointer is over it.
        root = root.child(
            div()
                .id("subfolders-label")
                .role(Role::Button)
                .aria_label(format!("{}{})", t("Folders ("), header.children.len()))
                .aria_expanded(expanded)
                .absolute()
                .left(px(padding + 2.))
                .top(px(LABEL_TOP))
                .h(px(LABEL_HEIGHT))
                .flex()
                .items_center()
                .gap(px(2.))
                .text(theme::TEXT_SM)
                .line_height(px(LABEL_HEIGHT))
                .text_color(theme.text_secondary())
                .cursor_pointer()
                .hover(move |style| style.text_color(theme.text()))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.subfolders.expanded = !this.subfolders.expanded;
                    cx.notify();
                }))
                .child(
                    div()
                        .size(px(14.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path("chevron-right")
                                .size(px(11.))
                                .text_color(theme.text_secondary())
                                .with_transformation(Transformation::rotate(radians(
                                    if expanded {
                                        std::f32::consts::FRAC_PI_2
                                    } else {
                                        0.
                                    },
                                ))),
                        ),
                )
                .child(format!("{}{})", t("Folders ("), header.children.len())),
        );
        if let Some(top) = geometry.content {
            root = root.child(
                div()
                    .absolute()
                    .left(px(padding + 2.))
                    .top(px(top))
                    .h(px(LABEL_HEIGHT))
                    .text(theme::TEXT_SM)
                    .line_height(px(LABEL_HEIGHT))
                    .text_color(theme.text_secondary())
                    .child(format!("{}{total})", t("Content ("))),
            );
        }
        if !expanded {
            return root;
        }
        // Build elements only for the subfolder tiles that are inside the
        // viewport or within 8px of it. Tiles further away are skipped to save
        // work.
        let raw = self.raw_scroll();
        let viewport = f32::from(self.bounds.size.height);
        // During a zoom animation each subfolder tile is drawn at its
        // position and size for this moment, like the item tiles below.
        let now = self.clock.elapsed().as_secs_f64() * 1000.;
        let mut tiles = Vec::new();
        for (index, child) in header.children.iter().enumerate() {
            let Some(&(x, y, height)) = geometry.tiles.get(index) else {
                continue;
            };
            let mut place = super::layout::Tile {
                x,
                y,
                w: geometry.tile_width,
                h: height,
            };
            if let Some(animation) = &self.zoom_animation {
                place = animation.subfolder_tile_at(index, place, now);
            }
            let (x, y, width, height) = (place.x, place.y, place.w, place.h);
            let card_h = card_height(width);
            if y + height < raw - 8. || y > raw + viewport + 8. {
                continue;
            }
            let ringed = selected.contains(&child.node)
                || header.drop.as_deref() == Some(child.node.as_str());
            let cover = header.covers.get(&child.folder).cloned().flatten();
            let renaming = header
                .renaming
                .as_ref()
                .filter(|open| open.node == child.node)
                .map(|open| open.input.clone());
            tiles.push(self.subfolder_tile(
                child.clone(),
                (x, y, width, card_h),
                cover,
                renaming,
                ringed,
                card,
                window,
                cx,
            ));
        }
        root.children(tiles)
    }

    #[allow(clippy::too_many_arguments)]
    fn subfolder_tile(
        &self,
        child: Child,
        (x, y, width, card_h): (f32, f32, f32, f32),
        cover: Option<Cover>,
        renaming: Option<Entity<TextInput>>,
        ringed: bool,
        card: theme::FolderCard,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = theme::theme(cx);
        let tint = child.tint.map(rgb);
        // The front card of the tile. Its top is 10px below the top of the
        // tile.
        let card_top = 10.;
        let face_h = card_h - card_top;
        let mut face = div()
            .absolute()
            .left_0()
            .top(px(card_top))
            .w(px(width))
            .h(px(face_h))
            .rounded(px(10.))
            .bg(linear_gradient(
                180.,
                linear_color_stop(rgba(card.fill.0), 0.),
                linear_color_stop(rgba(card.fill.1), 1.),
            ))
            .border_t_1()
            .border_l_1()
            .border_r_1()
            .border_color(rgba(card.side))
            // A 1px line along the top of the card in the colour `card.top`.
            // That colour is more opaque than the colour of the card's border,
            // `card.side`: 15% against 5% white in the dark themes, and 20%
            // against 10% black in the light themes. The line starts and ends
            // 10px from the card's left and right edges.
            .child(
                div()
                    .absolute()
                    .top(px(-1.))
                    .left(px(10.))
                    .right(px(10.))
                    .h(px(1.))
                    .bg(rgba(card.top)),
            );
        let resource = cover
            .as_ref()
            .filter(|cover| !cover.font)
            .map(|cover| Resource::Path(cover.thumbnail.clone().into()));
        let size = resource
            .as_ref()
            .and_then(|resource| self.thumbnails.read(cx).size(resource));
        match (cover.as_ref(), resource, size) {
            (Some(cover), Some(_), Some((natural_w, natural_h))) => {
                // The cover picture. It is scaled down to fit inside the card
                // with 4px free on every side, it is never drawn larger than
                // the thumbnail's own size, and it is centred in the card.
                let (room_w, room_h) = (width - 2. - 8., face_h - 1. - 8.);
                let scale = (room_w / natural_w).min(room_h / natural_h).min(1.);
                let (w, h) = (natural_w * scale, natural_h * scale);
                face = face.child(
                    img(cover.thumbnail.clone())
                        .image_cache(&self.thumbnails)
                        .absolute()
                        .left(px((width - 2. - w) / 2.))
                        .top(px((face_h - 1. - h) / 2.))
                        .w(px(w))
                        .h(px(h))
                        .rounded(px(4.)),
                );
            }
            (Some(_), _, _) => {}
            (None, _, _) => {
                // A folder with no cover: draw the folder icon in the centre
                // of the card. The icon is a quarter as wide as the tile, with
                // the tile's width taken as no less than 150px and no more
                // than 400px. It is drawn at 20% opacity in the folder's tint
                // colour, or in the primary text colour when the folder has no
                // tint.
                let size = (width.clamp(150., 400.) * 0.25).round().max(24.);
                let color = tint.unwrap_or(theme.text());
                face = face.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(ui::icon(
                            "pin-folder",
                            size,
                            Rgba {
                                a: color.a * 0.2,
                                ..color
                            },
                        )),
                );
            }
        }
        // Builds one of the two bars, each 2px high, that are drawn above the
        // front card. They look like the top edges of two more cards stacked
        // behind it. `top` is the bar's distance from the top of the tile, and
        // `inset` is its distance from the tile's left and right edges.
        let stack = |top: f32, inset: f32, color: u32| {
            div()
                .absolute()
                .top(px(top))
                .left(px(inset))
                .right(px(inset))
                .h(px(2.))
                .rounded_t(px(1.))
                .bg(rgba(color))
        };
        let glyph = match &child.icon {
            Some(icon) if crate::assets::has_icon(icon) => {
                ui::icon(icon.clone(), NAME_ICON, tint.unwrap_or(theme.text()))
            }
            _ => ui::icon(
                "picker-folder",
                NAME_ICON,
                tint.unwrap_or(theme.text_tertiary()),
            ),
        };
        let name_room = (width - NAME_ICON - NAME_GAP).max(0.);
        let name: AnyElement = match renaming {
            Some(input) => div()
                // The rename field is `RENAME_WIDTH` wide, or as wide as the
                // room the name has when that is less.
                .w(px(RENAME_WIDTH.min(name_room)))
                .min_w_0()
                // A mouse press inside the rename field is for editing the
                // name. Stop it here so that the tile does not also handle it
                // as a press on the tile.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(ui::field::field(
                    ElementId::Name(format!("subfolder-rename-{}", child.node).into()),
                    &input,
                    ui::field::Look::TileRename,
                ))
                .into_any_element(),
            None => {
                let wraps = super::tile::name_width(&child.label, window) > name_room;
                div()
                    .max_w(px(name_room))
                    .min_w_0()
                    .text(theme::TEXT_MD)
                    .line_height(px(NAME_LINE_HEIGHT))
                    .max_h(px(NAME_LINE_HEIGHT * 2.))
                    .overflow_hidden()
                    .text_color(theme.text())
                    .when(wraps, |name| name.text_center())
                    .child(child.label.clone())
                    .into_any_element()
            }
        };
        let count = child.count;
        let metas = format!(
            "{count} {}",
            if count == 1 { t("item") } else { t("items") }
        );
        let node = child.node.clone();
        let (menu_node, open_index) = (child.node.clone(), child.index);
        // The tile itself. To assistive technology it is a button whose label
        // is the folder's name followed by its item count, for example
        // "Art 12 items".
        let mut tile = div()
            .id(ElementId::Name(format!("subfolder-{}", child.node).into()))
            .role(Role::Button)
            .aria_label(format!("{} {metas}", child.label))
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(width))
            .cursor_pointer()
            .flex()
            .flex_col()
            .items_center();
        if ringed {
            // The ring drawn around a tile that is selected or is the drop
            // target: a border 2px thick in the theme's primary colour, with
            // 3px between it and the card.
            tile = tile.child(
                div()
                    .absolute()
                    .left(px(-5.))
                    .top(px(-5.))
                    .w(px(width + 10.))
                    .h(px(width * 0.75 + 16. + 4.))
                    .rounded(px(10.))
                    .border_2()
                    .border_color(theme.primary()),
            );
        }
        tile.child(
            div()
                .relative()
                .w(px(width))
                .h(px(card_h))
                .flex_none()
                .child(stack(2., 20., card.back))
                .child(stack(6., 10., card.middle))
                .child(face),
        )
        .child(
            div()
                .mt(px(CARD_BELOW))
                .h(px(NAME_LINE))
                .max_w_full()
                .flex()
                .items_start()
                .justify_center()
                .gap(px(NAME_GAP))
                .child(
                    div()
                        .h(px(NAME_ICON))
                        .flex()
                        .items_center()
                        .gap(px(NAME_GAP))
                        .max_w_full()
                        .child(glyph.flex_none())
                        .child(name),
                ),
        )
        .child(
            div()
                .w_full()
                .h(px(METAS))
                .flex()
                .justify_center()
                .text(theme::TEXT_XS)
                .line_height(px(METAS))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_tertiary())
                .truncate()
                .child(metas),
        )
        .on_mouse_down(MouseButton::Left, {
            let menu_node = menu_node.clone();
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                // On macOS a left press with Control held is a secondary
                // click. Open the folder's context menu for it, as a
                // right-click does.
                if Press::here(event.modifiers) == Press::Menu {
                    this.folder_menu(&menu_node, event.position, window, cx);
                }
            })
        })
        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            cx.stop_propagation();
            let press = Press::here(event.modifiers());
            if press == Press::Menu {
                return;
            }
            if event.click_count() == 2 {
                // A double-click opens the folder: the grid navigates to it.
                this.session
                    .update(cx, |session, cx| session.navigate(open_index, cx));
                return;
            }
            // A single click selects the tile. With the platform's selection
            // key held (Command on macOS, Control on Windows and Linux), the
            // click adds the tile to the selected tiles, or removes it if it
            // was already selected. Without that key the tile becomes the only
            // selected tile.
            let toggle = matches!(press, Press::Toggle | Press::Range { keep: true });
            this.session.update(cx, |session, cx| {
                session.select_folders(vec![node.clone()], toggle, cx)
            });
            window.focus(&this.focus, cx);
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.folder_menu(&menu_node, event.position, window, cx);
            }),
        )
        .into_any_element()
    }

    /// Asks for the context menu of the subfolder tile with node id `node` to
    /// be opened at `position`, which is a position in the window, by emitting
    /// [`FolderMenuRequested`]. If the tile is not selected, it first becomes
    /// the only selected tile.
    fn folder_menu(
        &mut self,
        node: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = self
            .session
            .read(cx)
            .folder_selection
            .iter()
            .any(|chosen| chosen == node);
        if !selected {
            self.session.update(cx, |session, cx| {
                session.select_folders(vec![node.to_owned()], false, cx)
            });
        }
        let selected = self.session.read(cx).folder_selection.clone();
        cx.emit(FolderMenuRequested {
            node: node.to_owned(),
            selected,
            position,
        });
        window.focus(&self.focus, cx);
    }
}

impl EventEmitter<FolderMenuRequested> for GridView {}

#[cfg(test)]
mod tests {
    use super::{children, columns, geometry};
    use crate::backend::Navigation;
    use picto_library::FolderId;
    use picto_library::query::ItemScope;

    fn folder(id: u32, label: &str, parent: Option<usize>) -> Navigation {
        Navigation {
            label: label.into(),
            count: Some(id as u64),
            scope: ItemScope::FolderTree {
                folder_id: FolderId(id),
            },
            screen: None,
            random: false,
            icon: None,
            tint: None,
            group: false,
            watched: false,
            depth: 0,
            parent,
        }
    }

    #[test]
    fn a_folder_lists_its_children_or_all_its_descendants() {
        let mut all = folder(0, "All", None);
        all.scope = ItemScope::All;
        let nav = vec![
            all,
            folder(1, "Art", None),
            folder(2, "Polls", Some(1)),
            folder(3, "Sketches", Some(1)),
            folder(5, "Night", Some(3)),
            folder(4, "Empty", Some(1)),
            folder(6, "Comics", None),
        ];
        let names = |deep| {
            children(&nav, 1, deep)
                .into_iter()
                .map(|child| child.label)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(false), ["Polls", "Sketches", "Empty"]);
        assert_eq!(names(true), ["Polls", "Sketches", "Night", "Empty"]);
        assert!(children(&nav, 0, true).is_empty());
        assert!(children(&nav, 6, true).is_empty());
        assert_eq!(children(&nav, 3, true)[0].node, "folder:5");
    }

    #[test]
    fn tiles_fill_columns_of_at_least_the_clamped_zoom() {
        // A list 569px wide, at a zoom of 260px with 8px gaps, has two columns
        // that are each 280.5px wide.
        assert_eq!(columns(569., 260., 8.), (2, 280.5));
        // The zoom's tile size is clamped to between 150px and 400px. 80px is
        // treated as 150px, which gives three columns, and 900px is treated as
        // 400px, which gives one column.
        assert_eq!(columns(569., 80., 8.).0, 3);
        assert_eq!(columns(569., 900., 8.).0, 1);
        assert_eq!(columns(100., 260., 16.), (1, 100.));
    }

    #[test]
    fn the_header_matches_the_reference_layout() {
        // The expected numbers are for a grid 585px wide with 8px spacing:
        // four subfolder tiles in two rows, then the "Content (15)" label,
        // and the item tiles start 636.2px below the top of the header.
        let laid = geometry(585., 260., 8., 4, true, true, |_| false);
        assert_eq!(laid.padding, 8.);
        assert_eq!(laid.tiles.len(), 4);
        let close = |a: f32, b: f32| (a - b).abs() < 0.05;
        assert!(close(laid.tiles[0].1, 34.6));
        assert!(close(laid.tiles[0].2, 269.5));
        assert!(close(laid.tiles[2].1, 34.6 + 269.5 + 8.));
        assert!(close(laid.tiles[1].0, 8. + 280.5 + 8.));
        assert!(close(laid.content.unwrap(), 611.6));
        assert!(close(laid.height, 636.2));
        // With the list of tiles collapsed, the header holds only the two
        // labels.
        let closed = geometry(585., 260., 8., 4, false, true, |_| false);
        assert!(closed.tiles.is_empty());
        assert!(close(closed.content.unwrap(), 32.6));
        assert!(close(closed.height, 57.2));
        // A folder with no items of its own has no "Content" label.
        let bare = geometry(585., 260., 8., 1, true, false, |_| false);
        assert_eq!(bare.content, None);
        assert!(close(bare.height, 34.6 + 269.5 + 30.));
        // A name on two lines makes its whole row taller.
        let wrapped = geometry(585., 260., 8., 3, true, true, |index| index == 1);
        assert!(close(wrapped.tiles[0].2, 280.5));
        assert!(close(wrapped.tiles[2].1, 34.6 + 280.5 + 8.));
    }
}
