//! The editor of a collection. It shows the collection's members as a grid
//! of tiles, with each member's name under its tile and without the file
//! extension. The user can select members, drag them to a new position in
//! the collection, double-click one to open it, and right-click for a menu
//! of actions.
//!
//! The tiles are drawn by the same code as the library grid's tiles
//! (`grid::tile::tile_art`). The editor keeps its own selection, which is a
//! set of members of the collection. It is separate from the library grid's
//! selection of items.
use super::group::{CollectionMenu, MemberOpened};
use crate::grid::{
    self, art, drag, format, layout,
    layout::{Layout, ViewMode},
    marquee, pointer, preview, reveal,
    thumbnails::{self, Thumbnails},
};
use crate::i18n::t;
use crate::session::Session;
use crate::{theme, ui};
use gpui::*;
use picto_library::query::RootSummary;
use picto_library::{MediaId, RootDetails, RootId, RootKind};
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::time::Instant;

actions!(
    group_editor,
    [
        /// Removes the selected members from the collection. Its key is the
        /// shortcut `group.removeMembers`.
        RemoveMembers,
        /// Opens Quick Look on the selected member, when exactly one member is
        /// selected. Its key is the shortcut `view.quicklook`.
        QuickLookMember
    ]
);

/// The key context of the editor's root element. Key bindings in this
/// context apply while the editor has keyboard focus.
pub const CONTEXT: &str = "GroupEditor";

/// The width, in pixels, that the layout aims at for a tile of the
/// collection editor's grid. The layout counts the columns from it.
const TARGET_SIZE: f32 = 200.;
/// The "Return to Top" button appears once the grid has been scrolled down
/// by at least this many pixels.
const RETURN_TO_TOP_AFTER: f32 = 200.;

/// Emitted when a member's tile is right-clicked, so that the viewer opens
/// the members' menu. `position` is where the pointer was, in window
/// coordinates. `members` are the members the menu acts on, as indexes into
/// the collection's order: the whole selection when the right-clicked member
/// is selected, and only that member otherwise.
pub struct MemberMenu {
    pub position: Point<Pixels>,
    pub members: Vec<usize>,
}

/// Emitted to ask the viewer to open Quick Look on one member. The number is
/// the member's index in the collection's order.
pub struct QuickLookRequested(pub usize);

/// Emitted to ask the viewer to remove members from the collection. The
/// numbers are the selected members' indexes in the collection's order.
pub struct RemoveRequested(pub Vec<usize>);

/// Emitted when the library has answered a reorder that was made by
/// dragging. It carries the error message when the reorder failed, and
/// `None` when it succeeded. The viewer shows the message above the members
/// on a failure, and removes it on a success.
pub struct Reordered(pub Option<SharedString>);

impl EventEmitter<MemberMenu> for GroupEditor {}
impl EventEmitter<QuickLookRequested> for GroupEditor {}
impl EventEmitter<RemoveRequested> for GroupEditor {}
impl EventEmitter<MemberOpened> for GroupEditor {}
impl EventEmitter<CollectionMenu> for GroupEditor {}
impl EventEmitter<Reordered> for GroupEditor {}

/// Returns the selection and the anchor after a click on `member`. With
/// `range` (Shift is held) and an anchor, every member from the anchor to
/// the clicked member is selected and the anchor stays the same. With
/// `toggle` (Cmd or Ctrl is held), the clicked member is added to the
/// selection, or removed from it when it was already selected, and it
/// becomes the anchor. Otherwise the clicked member becomes the only
/// selected member and the anchor. Members are identified by their media
/// id, and `order` is the collection's order.
pub fn selection_for_click(
    current: &BTreeSet<MediaId>,
    order: &[MediaId],
    member: MediaId,
    anchor: Option<MediaId>,
    toggle: bool,
    range: bool,
) -> (BTreeSet<MediaId>, MediaId) {
    if range && let Some(anchor) = anchor {
        let from = order.iter().position(|id| *id == anchor);
        let to = order.iter().position(|id| *id == member);
        if let (Some(from), Some(to)) = (from, to) {
            let (start, end) = (from.min(to), from.max(to));
            return (order[start..=end].iter().copied().collect(), anchor);
        }
    }
    if toggle {
        let mut selected = current.clone();
        if !selected.remove(&member) {
            selected.insert(member);
        }
        return (selected, member);
    }
    (BTreeSet::from([member]), member)
}

/// Makes the item summary that the grid's tile code needs from one member
/// of a collection. A member is a media file and has no item id of its own,
/// so its media id is used as the item id.
fn member_item(details: &RootDetails, media: &picto_library::MediaRecord) -> RootSummary {
    RootSummary {
        root_id: RootId(media.media_id.0),
        kind: RootKind::Media,
        lifecycle: details.lifecycle,
        name: media.media_name.clone(),
        cover_media_id: media.media_id,
        content_hash: media.facts.content_hash.clone(),
        mime: media.facts.mime.clone(),
        width: media.facts.width,
        height: media.facts.height,
        duration_ms: media.facts.duration_ms,
        frame_count: media.facts.frame_count,
        palette: media.facts.palette.clone(),
        imported_at_ms: details.root.imported_at_ms,
        captured_at_ms: details.root.captured_at_ms,
        modified_at_ms: details.root.modified_at_ms,
        media_count: 1,
        total_size_bytes: media.facts.size_bytes,
        rating: details.rating,
    }
}

/// The state of a drag that moves members to a new position in the
/// collection.
struct Dragging {
    /// The dragged members, as indexes into the collection's order.
    members: Vec<usize>,
    ghost: drag::ItemDrag,
    /// Where the dragged members would be put if the button were released
    /// now: the index of the member they would go next to, and on which
    /// side of it. `None` while the pointer is outside the editor or there
    /// is no member to put them next to.
    target: Option<(usize, drag::Side)>,
}

pub struct GroupEditor {
    session: Entity<Session>,
    /// The editor's own thumbnail cache. Each frame, a view tells its cache
    /// which thumbnails it wants (`Thumbnails::plan`). If the editor shared
    /// the library grid's cache, the editor's list would replace the grid's.
    thumbnails: Entity<Thumbnails>,
    focus: FocusHandle,
    collection: Option<RootId>,
    members: Vec<RootSummary>,
    /// The path of each member's thumbnail file, in the collection's order.
    thumbs: Vec<PathBuf>,
    /// The path of each member's original file, in the collection's order.
    files: Vec<PathBuf>,
    selected: BTreeSet<MediaId>,
    anchor: Option<MediaId>,
    scroll: ScrollHandle,
    /// The editor's position and size in window coordinates in the frame
    /// that was built last. `build` sets it at its start. The handlers of
    /// pointer events read it between frames.
    bounds: Bounds<Pixels>,
    layout: Layout,
    /// The grid's width, the gap between tiles and the number of members
    /// that `layout` was computed for. The layout is computed again when
    /// any of the three changes.
    layout_key: Option<(u32, u32, usize)>,
    reveal: reveal::RevealTracker,
    clock: Instant,
    art: Option<art::Art>,
    hovered: Option<usize>,
    pointer: Option<Point<Pixels>>,
    scrolled: Instant,
    last_top: f32,
    settle: Option<Task<()>>,
    preview: preview::HoverPreview,
    marquee: Option<marquee::Marquee>,
    marquee_keep: bool,
    swallow_click: bool,
    /// Set while the left button is held down after a press on a tile: the
    /// member's index and where the press was. Once the pointer has moved
    /// 5px from there, the press becomes a drag.
    press: Option<(usize, Point<Pixels>)>,
    dragging: Option<Dragging>,
    swallow_tile_click: bool,
}

impl GroupEditor {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        // When a member's thumbnail file has been written again (it was
        // regenerated, or a video frame was captured as the new thumbnail),
        // the cached picture is dropped so that the new file is read.
        cx.subscribe(
            &session,
            |this, session, event: &crate::application::Event, cx| {
                if let crate::application::Event::Thumbnail(hash) = event {
                    let jpeg = session.read(cx).thumbnail(hash);
                    this.thumbnails
                        .update(cx, |thumbnails, cx| thumbnails.forget(&jpeg, cx));
                    cx.notify();
                }
            },
        )
        .detach();
        // The editor is drawn again whenever its thumbnail cache changes,
        // for example when a thumbnail that was missing has been made.
        let thumbnails = cx.new(Thumbnails::new);
        cx.observe(&thumbnails, |_, _, cx| cx.notify()).detach();
        Self {
            session,
            thumbnails,
            focus: cx.focus_handle(),
            collection: None,
            members: Vec::new(),
            thumbs: Vec::new(),
            files: Vec::new(),
            selected: BTreeSet::new(),
            anchor: None,
            scroll: ScrollHandle::new(),
            bounds: Bounds::default(),
            layout: Layout::default(),
            layout_key: None,
            reveal: reveal::RevealTracker::default(),
            clock: Instant::now(),
            art: None,
            hovered: None,
            pointer: None,
            scrolled: Instant::now(),
            last_top: 0.,
            settle: None,
            preview: preview::HoverPreview::default(),
            marquee: None,
            marquee_keep: false,
            swallow_click: false,
            press: None,
            dragging: None,
            swallow_tile_click: false,
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The collection the editor shows, if it shows one.
    pub fn collection(&self) -> Option<RootId> {
        self.collection
    }

    /// Shows the members in `details`. When `details` is for the collection
    /// the editor already shows (it was read again after a change), the
    /// scroll position is kept, and so is the selection, except for members
    /// that are no longer in the collection.
    pub fn show(&mut self, details: &RootDetails, cx: &mut Context<Self>) {
        let root = details.root.root_id;
        if self.collection != Some(root) {
            self.clear(cx);
            self.collection = Some(root);
        }
        let members: Vec<RootSummary> = details
            .media
            .iter()
            .map(|media| member_item(details, media))
            .collect();
        if members == self.members {
            return;
        }
        let session = self.session.read(cx);
        self.thumbs = members
            .iter()
            .map(|item| thumbnails::existing(session.thumbnail(&item.content_hash)))
            .collect();
        self.files = details
            .media
            .iter()
            .map(|media| PathBuf::from(&media.file_path))
            .collect();
        let ids: HashSet<MediaId> = details.media.iter().map(|media| media.media_id).collect();
        self.selected.retain(|id| ids.contains(id));
        if self.anchor.is_some_and(|anchor| !ids.contains(&anchor)) {
            self.anchor = None;
        }
        // The hovered tile is kept by its index, so after the members change
        // the hover is on whichever member is now at that index. It is
        // dropped when the index is past the end of the new list.
        self.hovered = self.hovered.filter(|index| *index < members.len());
        self.members = members;
        self.layout_key = None;
        self.marquee = None;
        cx.notify();
    }

    /// Empties the editor: it shows no collection, nothing is selected, the
    /// grid is scrolled back to the top, and a rubber band, a drag or a
    /// hover preview that was under way is ended.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.collection = None;
        self.members.clear();
        self.thumbs.clear();
        self.files.clear();
        self.selected.clear();
        self.anchor = None;
        self.layout = Layout::default();
        self.layout_key = None;
        self.hovered = None;
        self.marquee = None;
        self.press = None;
        self.dragging = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
        self.preview.close(cx);
        cx.notify();
    }

    fn ids(&self) -> Vec<MediaId> {
        self.members
            .iter()
            .map(|item| item.cover_media_id)
            .collect()
    }

    /// The indexes of the selected members, in the collection's order.
    pub fn selected(&self) -> Vec<usize> {
        self.members
            .iter()
            .enumerate()
            .filter(|(_, item)| self.selected.contains(&item.cover_media_id))
            .map(|(index, _)| index)
            .collect()
    }

    /// Clears the selection and returns whether anything was selected. The
    /// viewer calls this for Escape: when it returns true, Escape has only
    /// cleared the selection and does not close the viewer.
    pub fn deselect(&mut self, cx: &mut Context<Self>) -> bool {
        if self.selected.is_empty() {
            return false;
        }
        self.selected.clear();
        self.anchor = None;
        cx.notify();
        true
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        if self.members.is_empty() {
            return;
        }
        self.selected = self.ids().into_iter().collect();
        self.anchor = self.members.first().map(|item| item.cover_media_id);
        cx.notify();
    }

    /// Replaces the selection. When exactly one member is selected, it
    /// becomes the anchor for a later Shift-click. Otherwise there is no
    /// anchor.
    fn set_selection(&mut self, selected: BTreeSet<MediaId>, cx: &mut Context<Self>) {
        self.anchor = if selected.len() == 1 {
            selected.first().copied()
        } else {
            None
        };
        self.selected = selected;
        cx.notify();
    }

    fn click_member(&mut self, index: usize, modifiers: Modifiers, cx: &mut Context<Self>) {
        let Some(member) = self.members.get(index).map(|item| item.cover_media_id) else {
            return;
        };
        let (selected, anchor) = selection_for_click(
            &self.selected,
            &self.ids(),
            member,
            self.anchor,
            modifiers.platform || modifiers.control,
            modifiers.shift,
        );
        self.selected = selected;
        self.anchor = Some(anchor);
        cx.notify();
    }

    /// Handles a right-click on the member at `index`. If that member is not
    /// selected, it becomes the only selected member and the anchor. Then
    /// [`MemberMenu`] is emitted for the selected members.
    fn open_member_menu(&mut self, index: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(member) = self.members.get(index).map(|item| item.cover_media_id) else {
            return;
        };
        if !self.selected.contains(&member) {
            self.selected = BTreeSet::from([member]);
            self.anchor = Some(member);
        }
        cx.notify();
        cx.emit(MemberMenu {
            position,
            members: self.selected(),
        });
    }

    // The functions below convert between window coordinates and the grid's
    // layout.

    fn grid_width(&self) -> f32 {
        (f32::from(self.bounds.size.width) - ui::scrollbar::GUTTER).max(0.)
    }

    fn scroll_top(&self) -> f32 {
        -f32::from(self.scroll.offset().y)
    }

    fn local(&self, position: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(position.x - self.bounds.origin.x),
            f32::from(position.y - self.bounds.origin.y),
        )
    }

    fn text_height(&self) -> f32 {
        layout::TEXT_ROW_HEIGHT
    }

    /// The tile image under `position`, which is in window coordinates.
    /// Returns the member's index and the same position in the layout's
    /// coordinates: pixels from the grid's left edge and from the top of
    /// its scrolled content. Returns `None` when the position is not on a
    /// tile's image.
    fn image_at(&self, position: Point<Pixels>) -> Option<(usize, f32, f32)> {
        let (x, y) = self.local(position);
        let y = y + self.scroll_top();
        if x < 0. || x >= self.grid_width() {
            return None;
        }
        marquee::image_at(&self.layout, x, y, self.text_height()).map(|index| (index, x, y))
    }

    /// The view preferences the editor's tiles are drawn with. They are the
    /// library grid's preferences, which keeps the library's gap between
    /// tiles, with these values fixed: the grid layout, tiles of
    /// [`TARGET_SIZE`], the name shown without its extension, no resolution,
    /// no extension label, no item count, and `fit_cover` off.
    fn prefs(&self, cx: &App) -> grid::prefs::ViewPrefs {
        let mut prefs = self.session.read(cx).prefs.clone();
        prefs.mode = ViewMode::Grid;
        prefs.target_size = TARGET_SIZE;
        prefs.show_name = true;
        prefs.show_resolution = false;
        prefs.show_extension_label = false;
        prefs.show_extension = false;
        prefs.show_item_count = false;
        prefs.fit_cover = false;
        prefs
    }

    // The functions below handle the pointer: hover, press, rubber band and
    // drag.

    fn pointer_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.pointer = Some(position);
        if self.marquee.is_some() || self.dragging.is_some() {
            return;
        }
        let hit = self.image_at(position);
        if pointer::hover(&mut self.hovered, hit) {
            cx.notify();
        }
        if let Some(index) = pointer::magnifier(hit, &self.layout, self.text_height()) {
            let target = self
                .members
                .get(index)
                .filter(|item| !item.mime.starts_with("video/"))
                .map(|item| (item.content_hash.clone(), self.files[index].clone()));
            let (hash, path) = target.unzip();
            self.preview.rest_on(
                hash,
                move |_| async move { path },
                |this: &mut Self| &mut this.preview,
                cx,
            );
        } else {
            self.preview.away(|this: &mut Self| &mut this.preview, cx);
        }
    }

    fn pointer_left(&mut self, cx: &mut Context<Self>) {
        let banding = self.marquee.is_some();
        if pointer::left(
            &mut self.pointer,
            &mut self.hovered,
            banding,
            &mut self.preview,
            |this: &mut Self| &mut this.preview,
            cx,
        ) {
            cx.notify();
        }
    }

    /// Handles a press of the left mouse button in the grid. A press on a
    /// tile's image is remembered in `press`, and becomes a drag if the
    /// pointer then moves 5px. A press anywhere else starts a rubber band
    /// selection. With Shift, Cmd or Ctrl held, the rubber band adds to the
    /// current selection. Otherwise it replaces the selection.
    fn pressed(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.swallow_click = false;
        self.swallow_tile_click = false;
        let (x, y) = self.local(event.position);
        if x < 0. || x >= self.grid_width() {
            return;
        }
        if let Some((index, _, _)) = self.image_at(event.position) {
            self.press = Some((index, event.position));
            cx.notify();
            return;
        }
        let modifiers = event.modifiers;
        let keep = modifiers.shift || modifiers.platform || modifiers.control;
        let base = if keep {
            self.selected().into_iter().collect()
        } else {
            BTreeSet::new()
        };
        self.marquee_keep = keep;
        self.marquee = Some(marquee::Marquee::new(
            (x, y + self.scroll_top()),
            (x, y),
            base,
        ));
        cx.notify();
    }

    fn marquee_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let local = self.local(position);
        let top = self.scroll_top();
        let viewport = f32::from(self.bounds.size.height);
        let text_height = self.text_height();
        if pointer::band_moved(
            self.marquee.as_mut(),
            local,
            top,
            viewport,
            &self.layout,
            text_height,
        ) {
            cx.notify();
        }
    }

    fn marquee_released(&mut self, cx: &mut Context<Self>) {
        let Some(marquee) = self.marquee.take() else {
            return;
        };
        if let Some(chosen) = marquee.committed() {
            self.swallow_click = true;
            let selected = chosen
                .into_iter()
                .filter_map(|index| self.members.get(index))
                .map(|item| item.cover_media_id)
                .collect();
            self.set_selection(selected, cx);
        }
        cx.notify();
    }

    /// Handles a pointer move while the left button is held after a press
    /// on a tile. Once the pointer is 5px from the press, a drag starts. It
    /// drags all the selected members when the pressed member is selected,
    /// and only the pressed member otherwise. During the drag, the ghost (a
    /// small stack of the dragged members' thumbnails) follows the pointer,
    /// and `target` is updated to where the members would be put, which the
    /// drop line shows.
    fn drag_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.dragging.is_none() {
            let Some((index, from)) = self.press else {
                return;
            };
            if !drag::past_threshold(from, position) {
                return;
            }
            let Some(item) = self.members.get(index) else {
                self.press = None;
                return;
            };
            let members = if self.selected.contains(&item.cover_media_id) {
                self.selected()
            } else {
                vec![index]
            };
            let theme = theme::theme(cx);
            let roots: Vec<_> = members
                .iter()
                .map(|index| self.members[*index].root_id)
                .collect();
            let ghost = drag::ItemDrag {
                count: roots.len() as u64,
                target: picto_library::selection::SelectionTarget::Explicit {
                    root_ids: roots.clone(),
                },
                roots,
                source: picto_library::query::ItemScope::All,
                covers: members
                    .iter()
                    .take(3)
                    .map(|index| drag::Cover {
                        thumbnail: self.thumbs[*index].clone(),
                        matte: self.members[*index]
                            .palette
                            .first()
                            .map(|color| rgb(format::lab_to_rgb(color.l, color.a, color.b)))
                            .unwrap_or(theme.background()),
                    })
                    .collect(),
                position,
            };
            for (holder, cover) in ["group-ghost-0", "group-ghost-1", "group-ghost-2"]
                .into_iter()
                .zip(ghost.covers.iter().map(Some).chain(std::iter::repeat(None)))
            {
                let resource = cover.map(|cover| Resource::Path(cover.thumbnail.clone().into()));
                self.thumbnails
                    .update(cx, |thumbnails, _| thumbnails.pin(holder, resource));
            }
            self.dragging = Some(Dragging {
                members,
                ghost,
                target: None,
            });
            self.pointer_left(cx);
        }
        let (x, y) = self.local(position);
        let y = y + self.scroll_top();
        let text_height = self.text_height();
        let over = x >= 0.
            && x < f32::from(self.bounds.size.width)
            && y - self.scroll_top() >= 0.
            && y - self.scroll_top() < f32::from(self.bounds.size.height);
        // The tile under the pointer keeps its hover highlight during a
        // drag.
        self.hovered = self.image_at(position).map(|(index, _, _)| index);
        self.pointer = Some(position);
        if let Some(dragging) = &mut self.dragging {
            dragging.ghost.position = position;
            let skip: HashSet<usize> = dragging.members.iter().copied().collect();
            dragging.target = over
                .then(|| drag::reorder_target(&self.layout.tiles, x, y, text_height, &skip))
                .flatten();
        }
        cx.notify();
    }

    /// Handles the release of the left button after a press on a tile. If a
    /// drag was under way and it had a target, the dragged members are
    /// moved to where the drop line showed and the new order is saved. The
    /// dragged members then become the selection, whether or not they
    /// moved. The release also produces a click event, which is ignored
    /// (see the comment below).
    fn drag_released(&mut self, cx: &mut Context<Self>) {
        self.press = None;
        let Some(dragging) = self.dragging.take() else {
            return;
        };
        // The mouse release that ends the drag also produces a click event
        // on whatever is under the pointer: a tile, or the empty part of the
        // grid. These two flags make both click handlers ignore that click,
        // so that it does not change the selection.
        self.swallow_tile_click = true;
        self.swallow_click = true;
        for holder in ["group-ghost-0", "group-ghost-1", "group-ghost-2"] {
            self.thumbnails
                .update(cx, |thumbnails, _| thumbnails.pin(holder, None));
        }
        let ids = self.ids();
        let dragged: Vec<MediaId> = dragging.members.iter().map(|index| ids[*index]).collect();
        if let (Some((target, side)), Some(collection)) = (dragging.target, self.collection) {
            let order = drag::plan_reorder(&ids, &dragged, target, side);
            if !order.is_empty() {
                let answer = super::group::reorder(&self.session, collection, order, cx);
                cx.spawn(async move |this, cx| {
                    let Ok(result) = answer.await else {
                        return;
                    };
                    let _ = this.update(cx, |this, cx| {
                        // The result is reported only if the editor still
                        // shows the collection that was reordered.
                        if this.collection == Some(collection) {
                            cx.emit(Reordered(
                                result.err().map(|error| error.to_string().into()),
                            ));
                        }
                    });
                })
                .detach();
            }
        }
        self.set_selection(dragged.into_iter().collect(), cx);
    }
}

impl Render for GroupEditor {
    /// The tiles' layout depends on the editor's width, so `render` returns
    /// an element that fills the editor's area and builds the editor for
    /// that element's bounds in the frame that lays it out
    /// (`ui::sized_view`).
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::sized_view(cx.weak_entity(), Self::build).size_full()
    }
}

impl GroupEditor {
    /// Builds one frame of the editor for `bounds`, the editor's position
    /// and size in window coordinates in this frame. It stores them in
    /// `self.bounds` first, and everything it calls reads them from there.
    fn build(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.bounds = bounds;
        let theme = theme::theme(cx);
        // While the pointer of a rubber band is near the top or the bottom
        // edge, the grid scrolls a little each frame. Another frame is
        // requested for as long as it scrolls.
        if pointer::autoscroll(self.marquee.as_ref(), &self.scroll, self.scroll_top()) {
            window.request_animation_frame();
        }
        let prefs = self.prefs(cx);
        let width = self.grid_width();
        let viewport = f32::from(self.bounds.size.height);
        let top = self.scroll_top();
        // When the grid has scrolled since the last frame, the hover
        // highlight is removed and the hover preview is closed. The hover is
        // found again 80 ms (`MAGNIFIER_RESUME`) after the last scroll, from
        // where the pointer is then.
        if top != self.last_top {
            self.last_top = top;
            self.hovered = None;
            self.preview.close(cx);
            self.scrolled = Instant::now();
            self.settle = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(crate::motion::MAGNIFIER_RESUME)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(pointer) = this.pointer {
                        this.pointer_moved(pointer, cx);
                    }
                    cx.notify();
                });
            }));
        }
        let key = Some((width as u32, prefs.gap as u32, self.members.len()));
        if key != self.layout_key {
            let aspects: Vec<f64> = self
                .members
                .iter()
                .map(|item| match (item.width, item.height) {
                    (Some(w), Some(h)) if w > 0 && h > 0 => f64::from(w) / f64::from(h),
                    _ if item.mime.starts_with("audio/") => 2.,
                    _ => 1.5,
                })
                .collect();
            self.layout = Layout::compute(
                &aspects,
                width,
                TARGET_SIZE,
                prefs.gap,
                ViewMode::Grid,
                prefs.text_height(),
            );
            self.layout_key = key;
        }
        let text_height = prefs.text_height();
        if let Some(marquee) = &mut self.marquee {
            marquee.track(top, &self.layout, text_height);
        }
        let selected_now: BTreeSet<usize> = self.selected().into_iter().collect();
        let visible = self
            .layout
            .visible(top - reveal::MARGIN, top + viewport + reveal::MARGIN);
        // The thumbnails to load are listed in the order they should load:
        // those of the tiles nearest the vertical middle of the viewport
        // first.
        let spans: Vec<(f32, f32)> = visible
            .iter()
            .map(|index| (self.layout.tiles[*index].y, self.layout.tiles[*index].h))
            .collect();
        let wanted = thumbnails::load_order(&spans, top, viewport)
            .into_iter()
            .map(|at| Resource::Path(self.thumbs[visible[at]].clone().into()))
            .collect();
        self.thumbnails
            .update(cx, |thumbnails, cx| thumbnails.plan(wanted, window, cx));
        let mut missing = Vec::new();
        let states: Vec<thumbnails::Thumb> = {
            let thumbnails = self.thumbnails.read(cx);
            visible
                .iter()
                .map(|index| {
                    let resource = Resource::Path(self.thumbs[*index].clone().into());
                    let thumb = thumbnails.thumb(&resource, &self.members[*index].mime);
                    // A member whose thumbnail could not be loaded is
                    // added to `missing`, and a thumbnail is made for it
                    // below. The library grid does the same for its tiles.
                    if thumb == thumbnails::Thumb::Failed {
                        missing.push(thumbnails::Missing {
                            hash: self.members[*index].content_hash.clone(),
                            mime: self.members[*index].mime.clone(),
                            jpeg: self.thumbs[*index].clone(),
                        });
                    }
                    thumb
                })
                .collect()
        };
        let session = self.session.clone();
        self.thumbnails.update(cx, |thumbnails, cx| {
            thumbnails.make_missing(&session, missing, cx)
        });
        let now = self.clock.elapsed().as_secs_f64() * 1000.;
        let decoded: Vec<(u32, bool)> = visible
            .iter()
            .zip(&states)
            .map(|(index, state)| {
                (
                    self.members[*index].root_id.0,
                    *state == thumbnails::Thumb::Loaded,
                )
            })
            .collect();
        self.reveal.update(&decoded, now);
        if self.reveal.active(now) {
            window.request_animation_frame();
        }

        let mut content = div().relative().w_full().h(px(self.layout.total_height));
        for (index, thumb) in visible.iter().copied().zip(states) {
            let tile = self.layout.tiles[index];
            let item = self.members[index].clone();
            let (name, count) = (SharedString::from(item.name.clone()), self.members.len());
            let selected = match &self.marquee {
                Some(marquee) => marquee.shows_selected(index, &selected_now),
                None => selected_now.contains(&index),
            };
            let alphas = reveal::alphas(self.reveal.progress(item.root_id.0, now));
            let magnifier = self.hovered == Some(index)
                && !item.mime.starts_with("video/")
                && self.scrolled.elapsed() >= crate::motion::MAGNIFIER_RESUME;
            let cell = grid::tile::tile_art(
                grid::tile::TileLook {
                    tile,
                    item,
                    thumbnail: self.thumbs[index].clone(),
                    selected,
                    thumb,
                    original: None,
                    alphas,
                    magnifier,
                    // The Grayscale view option applies to the library
                    // grid only. The editor always shows its tiles in
                    // colour.
                    grayscale: false,
                },
                &prefs,
                &mut self.art,
                &self.thumbnails,
                window,
                cx,
            );
            let image_height = tile.h - text_height;
            content = content.child(
                cell.child(
                    // For assistive technology, each tile is one option
                    // of the list box that the grid is, as a library grid
                    // tile is.
                    div()
                        .id(("group-member-tile", index))
                        .role(Role::ListBoxOption)
                        .aria_label(name)
                        .aria_selected(selected)
                        .aria_position_in_set(index + 1)
                        .aria_size_of_set(count)
                        .absolute()
                        .left_0()
                        .top_0()
                        .w(px(tile.w))
                        .h(px(image_height))
                        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.focus, cx);
                            if std::mem::take(&mut this.swallow_tile_click) {
                                return;
                            }
                            this.click_member(index, event.modifiers(), cx);
                            if event.click_count() == 2 {
                                cx.emit(MemberOpened(index));
                            }
                        }))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                window.focus(&this.focus, cx);
                                this.open_member_menu(index, event.position, cx);
                            }),
                        ),
                ),
            );
        }
        // The drop line of a drag is a 2px vertical line in the gap next to
        // the target tile's image, on the side the members would go, with a
        // triangle at its top.
        if let Some((target, side)) = self.dragging.as_ref().and_then(|d| d.target)
            && let Some(tile) = self.layout.tiles.get(target)
        {
            content = content.child(drag::reorder_marker(tile, side, prefs.gap, text_height));
        }
        if let Some(rect) = self.marquee.as_ref().and_then(|marquee| marquee.rect) {
            content = content.child(
                div()
                    .absolute()
                    .left(px(rect.left))
                    .top(px(rect.top))
                    .w(px(rect.width))
                    .h(px(rect.height))
                    .bg(rgba(theme::MARQUEE_FILL))
                    .border_1()
                    .border_color(rgba(theme::MARQUEE_BORDER)),
            );
        }

        let body = div()
            .id("group-editor-grid")
            .role(Role::ListBox)
            .aria_multiselectable(true)
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.pointer_moved(event.position, cx)
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered {
                    this.pointer_left(cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.pressed(event, window, cx)
                }),
            )
            // A right-click that is not on a tile's image opens the
            // collection's own menu. A right-click on an image is handled by
            // the tile, which stops it from reaching this handler.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.focus, cx);
                    cx.emit(CollectionMenu(event.position));
                }),
            )
            // A click that is not on a tile's image clears the selection.
            // The exception is the click event that the mouse release at the
            // end of a rubber band or of a drag produces: `swallow_click` is
            // set then, and that click is ignored.
            .on_click(cx.listener(|this, _, window, cx| {
                window.focus(&this.focus, cx);
                if std::mem::take(&mut this.swallow_click) {
                    return;
                }
                this.deselect(cx);
            }))
            .child(content);

        let mut root = div()
            .id("group-editor")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(theme.background())
            // The shortcut `edit.selectAll`, from its key or from the Edit
            // menu, selects every member.
            .on_action(cx.listener(|this, _: &crate::shell::SelectAll, _, cx| this.select_all(cx)))
            .on_action(cx.listener(|this, _: &RemoveMembers, _, cx| {
                let members = this.selected();
                if !members.is_empty() {
                    cx.emit(RemoveRequested(members));
                }
            }))
            // The shortcut `edit.copy` puts the files of the selected members
            // on the clipboard, or the files of all the members when none is
            // selected.
            .on_action(cx.listener(|this, _: &crate::keymap::CopyFiles, _, cx| {
                let selected = this.selected();
                let files: Vec<PathBuf> = if selected.is_empty() {
                    this.files.clone()
                } else {
                    selected
                        .iter()
                        .filter_map(|index| this.files.get(*index).cloned())
                        .collect()
                };
                let copied = if files.is_empty() {
                    Err("The selection has no physical files to copy.".to_owned())
                } else {
                    crate::desktop::copy_files(&files, cx)
                };
                if let Err(error) = copied {
                    crate::ui::notify::error(t("Could not copy selection"), error, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &QuickLookMember, _, cx| {
                if let [member] = this.selected().as_slice() {
                    cx.emit(QuickLookRequested(*member));
                }
            }))
            .child(body)
            .child(ui::scrollbar(
                "group-editor-scrollbar",
                &self.scroll,
                window,
                cx,
            ))
            .children(ui::scroll_to_top(
                "group-editor-return-to-top",
                &self.scroll,
                RETURN_TO_TOP_AFTER,
                12.,
                window,
                cx,
            ));
        // While a rubber band is out, or the button is held after a press on
        // a tile, pointer moves and the button's release are handled
        // wherever in the window they happen, not only over the grid.
        if self.marquee.is_some() || self.press.is_some() || self.dragging.is_some() {
            let entity = cx.entity().downgrade();
            root = root.child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let moved = entity.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            let _ = moved.update(cx, |this, cx| {
                                let held = event.pressed_button == Some(MouseButton::Left);
                                if this.marquee.is_some() {
                                    if held {
                                        this.marquee_moved(event.position, cx);
                                    } else {
                                        this.marquee_released(cx);
                                    }
                                } else if held {
                                    this.drag_moved(event.position, cx);
                                } else {
                                    this.drag_released(cx);
                                }
                            });
                        });
                        let released = entity.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture && event.button == MouseButton::Left
                            {
                                let _ = released.update(cx, |this, cx| {
                                    if this.marquee.is_some() {
                                        this.marquee_released(cx);
                                    } else {
                                        this.drag_released(cx);
                                    }
                                });
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            );
        }
        if let Some(dragging) = &self.dragging {
            root = root.child(drag::ghost(&dragging.ghost, &self.thumbnails, cx));
        }
        root.children(pointer::preview_overlay(&self.preview, window))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::selection_for_click;
    use picto_library::MediaId;
    use std::collections::BTreeSet;

    fn ids(values: &[u32]) -> BTreeSet<MediaId> {
        values.iter().map(|value| MediaId(*value)).collect()
    }

    const ORDER: [MediaId; 4] = [MediaId(10), MediaId(20), MediaId(30), MediaId(40)];

    #[test]
    fn a_plain_click_selects_the_member_alone() {
        let (selected, anchor) =
            selection_for_click(&ids(&[10, 20]), &ORDER, MediaId(30), None, false, false);
        assert_eq!(selected, ids(&[30]));
        assert_eq!(anchor, MediaId(30));
    }

    #[test]
    fn cmd_toggles_the_member_and_moves_the_anchor() {
        let (selected, anchor) = selection_for_click(
            &ids(&[10, 20]),
            &ORDER,
            MediaId(20),
            Some(MediaId(10)),
            true,
            false,
        );
        assert_eq!(selected, ids(&[10]));
        assert_eq!(anchor, MediaId(20));
        let (selected, _) =
            selection_for_click(&ids(&[10]), &ORDER, MediaId(40), None, true, false);
        assert_eq!(selected, ids(&[10, 40]));
    }

    #[test]
    fn shift_selects_from_the_anchor_either_way_and_keeps_it() {
        let (selected, anchor) = selection_for_click(
            &ids(&[30]),
            &ORDER,
            MediaId(10),
            Some(MediaId(30)),
            false,
            true,
        );
        assert_eq!(selected, ids(&[10, 20, 30]));
        assert_eq!(anchor, MediaId(30));
        // With Shift held but no anchor, the click selects only the clicked
        // member, as a plain click does.
        let (selected, anchor) =
            selection_for_click(&BTreeSet::new(), &ORDER, MediaId(20), None, false, true);
        assert_eq!(selected, ids(&[20]));
        assert_eq!(anchor, MediaId(20));
    }
}
