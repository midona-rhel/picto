//! The subfolder tiles a folder's grid opens with (`SubfolderGrid.tsx`):
//! "Folders (n)" with its disclosure, a card for each child folder with its
//! cover, name and count, then "Content (n)" above the folder's items. A
//! tile is selected with a click, opened with a double-click, has the
//! folder's menu on a right-click, is renamed in place, and takes items
//! dragged onto it.
use super::GridView;
use super::selection::Press;
use crate::backend::Navigation;
use crate::theme::{self, Text as _};
use crate::ui::{self, text_input::Kind, text_input::Renamed, text_input::TextInput};
use gpui::prelude::FluentBuilder;
use gpui::*;
use picto_library::FolderId;
use picto_library::query::ItemScope;
use std::collections::HashMap;
use std::path::PathBuf;

/// A subfolder tile was right-clicked: the folder's menu, for this node (or
/// the tiles selected with it) at this window position.
pub struct FolderMenuRequested {
    pub node: String,
    pub selected: Vec<String>,
    pub position: Point<Pixels>,
}

/// One child folder, as its tile shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Child {
    /// `folder:5`.
    pub node: String,
    /// Its place in the destination list, which opening it navigates to.
    pub index: usize,
    pub folder: FolderId,
    pub label: String,
    pub icon: Option<String>,
    pub tint: Option<u32>,
    pub count: u64,
}

/// `gridChildFoldersAtom`: a folder's children, or, when it shows its
/// subfolders' items, all its descendants depth first, each level in the
/// sidebar's order. Anything that is not a folder has none.
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
    // Depth first: each child, then its own children, before its next
    // sibling.
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

/// The folder a `folder:5` node is.
pub fn folder_of(node: &str) -> Option<FolderId> {
    node.strip_prefix("folder:")?.parse().ok().map(FolderId)
}

/// `.container`'s side padding and `.list`'s gap: 16px, or 8px with tight
/// spacing.
fn padding(gap: f32) -> f32 {
    if gap <= 8. { 8. } else { 16. }
}

/// `.sectionLabel`: 4px above, 6px below, on a 12px × 1.55 line.
const LABEL_TOP: f32 = 4.;
const LABEL_HEIGHT: f32 = 18.6;
const LABEL_BELOW: f32 = 6.;
/// `.list`: 6px above, 30px below.
const LIST_TOP: f32 = 6.;
const LIST_BELOW: f32 = 30.;
/// `.contentLabel`: 4px above (taken up by the list's 30px when it shows),
/// 6px below, which also takes up the container's own 4px.
const CONTENT_TOP: f32 = 4.;
const CONTENT_BELOW: f32 = 6.;
/// `.container`'s `margin-bottom`.
const CONTAINER_BELOW: f32 = 4.;
/// `.thumbnail`: `padding-bottom: calc(75% + 10px)`, 12px above the name.
const CARD_EXTRA: f32 = 10.;
const CARD_BELOW: f32 = 12.;
/// `.nameRow` sits on a line of the tile's 13px × 1.55 text, which the 19px
/// glyph makes 24.1px tall; a name on two 15px lines adds 11px to it.
const NAME_LINE: f32 = 24.1;
const NAME_SECOND_LINE: f32 = 11.;
/// `.metas`: 10px on a 1.3 line.
const METAS: f32 = 13.;
/// `.nameRow`'s glyph and the gap after it.
const NAME_ICON: f32 = 19.;
const NAME_GAP: f32 = 6.;
/// `FolderRenameInput`'s width in the reference.
const RENAME_WIDTH: f32 = 161.6;
/// `.name`: 15px lines, at most two.
const NAME_LINE_HEIGHT: f32 = 15.;

/// Where everything in the header lies, from the top of the grid's content.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub padding: f32,
    pub gap: f32,
    pub columns: usize,
    pub tile_width: f32,
    /// Each tile's top-left corner and height.
    pub tiles: Vec<(f32, f32, f32)>,
    /// The "Content (n)" label's top, when the folder holds items.
    pub content: Option<f32>,
    /// The height the item tiles start below.
    pub height: f32,
}

/// `.list`'s `repeat(auto-fill, minmax(size, 1fr))` with the zoom's size
/// clamped to 150–400px: as many columns as fit, sharing the width.
pub fn columns(width: f32, target: f32, gap: f32) -> (usize, f32) {
    let size = target.clamp(150., 400.);
    let columns = (((width + gap) / (size + gap)).floor() as usize).max(1);
    let tile = ((width - (columns - 1) as f32 * gap) / columns as f32).max(0.);
    (columns, tile)
}

/// The tile's card height: three quarters of its width and 10px.
pub fn card_height(tile_width: f32) -> f32 {
    tile_width * 0.75 + CARD_EXTRA
}

/// Lays the header out for the grid's width. `two_lines` says which names
/// wrap onto a second line. A row is as tall as its tallest tile.
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
    // The list's bottom margin takes up the content label's top margin, and
    // the content label's bottom margin the container's.
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

/// A folder's cover: its chosen item, else its first in its own order.
#[derive(Clone, Debug, PartialEq)]
pub struct Cover {
    pub thumbnail: PathBuf,
    pub font: bool,
}

/// `getFolderCover`, on the library's thread.
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

/// A tile's name being edited in place (`FolderRenameInput`).
pub struct Renaming {
    pub node: String,
    original: String,
    input: Entity<TextInput>,
    _subscription: Subscription,
}

/// The header's state, which the grid keeps across frames.
pub struct Header {
    /// The destination the children were listed for.
    shown_for: Option<usize>,
    pub children: Vec<Child>,
    /// What the children were listed for: the destination, whether it shows
    /// its subfolders' items, and whether the viewer covers the grid.
    listed: Option<(usize, bool, bool)>,
    /// The destination list changed: the children are listed again.
    pub relist: bool,
    /// Which names take two lines, and the room a line has they were
    /// measured for.
    lines: Option<(f32, Vec<bool>)>,
    covers: HashMap<FolderId, Option<Cover>>,
    /// The folders whose covers were last asked for; asked again when the
    /// list changes or a folder changed.
    asked: Vec<FolderId>,
    /// The latest ask: an earlier one's covers arrive late and are dropped.
    request: super::Latest,
    pub stale: bool,
    /// The disclosure, open until it is closed; it opens again for the
    /// next folder that has children.
    pub expanded: bool,
    pub geometry: Geometry,
    /// The tile items dragged from the grid would go into.
    pub drop: Option<String>,
    pub renaming: Option<Renaming>,
    /// A folder made or duplicated from a tile's menu, renamed once its
    /// tile appears.
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
    /// The header's height, nothing when there are no subfolders.
    pub fn height(&self) -> f32 {
        if self.children.is_empty() {
            0.
        } else {
            self.geometry.height
        }
    }

    /// The covers the header wants loaded, when it is in view.
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
    /// Lists the destination's subfolders, asks for their covers when the
    /// list or a folder changed, and lays the header out for `width`.
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
            // Listed again only when the destination, the list or what it
            // shows changed, not every frame.
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
                // The covers are asked for again when the folders differ.
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
        // A folder just made from a tile's menu is renamed once it shows.
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

    /// The subfolder tile under a window position, by its place in the
    /// list, when the header shows.
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

    /// Renames a folder's tile once it shows (`setRenamingSubfolderId` for
    /// a folder just made).
    pub fn rename_subfolder_when_shown(&mut self, node: String, cx: &mut Context<Self>) {
        self.subfolders.rename_next = Some(node);
        cx.notify();
    }

    /// The subfolder tiles a rubber band covers; the band is in the item
    /// layout's coordinates, which start below the header.
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

    /// `FolderRenameInput`: the tile's name in a field, selected.
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
                // An empty name cancels; an unchanged one writes nothing.
                if let Renamed::Commit(value) = event
                    && !value.is_empty()
                    && *value != open.original
                    && let Some(folder) = folder_of(&open.node)
                {
                    let value = value.clone();
                    this.session.update(cx, |session, cx| {
                        let _ = session.edit_reporting(
                            crate::session::Report::failure("Unable to rename folder"),
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

    /// The header, drawn at the top of the grid's content.
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
        // A rubber band selects the tiles it covers when it is let go, as
        // the reference's does; until then they show no ring.
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
        // `.sectionLabel`: the disclosure, brighter under the pointer.
        root = root.child(
            div()
                .id("subfolders-label")
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
                .child(format!("Folders ({})", header.children.len())),
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
                    .child(format!("Content ({total})")),
            );
        }
        if !expanded {
            return root;
        }
        // Only what is in view is drawn.
        let raw = self.raw_scroll();
        let viewport = f32::from(self.bounds.size.height);
        let width = geometry.tile_width;
        let card_h = card_height(width);
        let mut tiles = Vec::new();
        for (index, child) in header.children.iter().enumerate() {
            let Some(&(x, y, height)) = geometry.tiles.get(index) else {
                continue;
            };
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
        // `.pic1`: the card, 10px below the tile's top, with a 4px inset.
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
            // The top edge is brighter than the sides.
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
                // `.coverImage`: at most the card less 8px, never larger
                // than the thumbnail, centred.
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
                // `.folderPlaceholder`: the folder glyph at a fifth of its
                // colour, a quarter of the tile wide.
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
                            "subfolder-tile-placeholder",
                            size,
                            Rgba {
                                a: color.a * 0.2,
                                ..color
                            },
                        )),
                );
            }
        }
        // `.pic2` and `.pic3`: the edges of the cards stacked behind.
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
                "subfolder-tile-name",
                NAME_ICON,
                tint.unwrap_or(theme.text_tertiary()),
            ),
        };
        let name_room = (width - NAME_ICON - NAME_GAP).max(0.);
        let name: AnyElement = match renaming {
            Some(input) => div()
                // `width: min(180px, 100%)`, which the row's own sizing
                // makes the input's default width (measured in the
                // reference).
                .w(px(RENAME_WIDTH.min(name_room)))
                .min_w_0()
                // A press in the field edits the name; it is not a click
                // on the tile.
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
        let metas = format!("{count} {}", if count == 1 { "item" } else { "items" });
        let node = child.node.clone();
        let (menu_node, open_index) = (child.node.clone(), child.index);
        let mut tile = div()
            .id(ElementId::Name(format!("subfolder-{}", child.node).into()))
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(width))
            .cursor_pointer()
            .flex()
            .flex_col()
            .items_center();
        if ringed {
            // `.tileSelected::after`: a 2px ring 3px outside the card.
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
                // A secondary click (Control on macOS) is a right-click.
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
                // `onOpenFolder`: the folder's own grid.
                this.session
                    .update(cx, |session, cx| session.navigate(open_index, cx));
                return;
            }
            // `onSelectFolder`: the platform's selection key toggles.
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

    /// A subfolder tile's menu, at a window position. The tile joins the
    /// selection first unless it is in it.
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
        // 569px of list at a 260px zoom and 8px gaps: two 280.5px columns.
        assert_eq!(columns(569., 260., 8.), (2, 280.5));
        // The zoom is clamped to 150–400px.
        assert_eq!(columns(569., 80., 8.).0, 3);
        assert_eq!(columns(569., 900., 8.).0, 1);
        assert_eq!(columns(100., 260., 16.), (1, 100.));
    }

    #[test]
    fn the_header_matches_the_reference_layout() {
        // The reference, 585px wide with tight spacing: four tiles in two
        // rows, then "Content (15)"; the items start 636.2px down.
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
        // Closed: the two labels alone.
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
