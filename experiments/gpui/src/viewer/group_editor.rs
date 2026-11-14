//! A collection's editor (`GroupSurface.tsx` in its editor mode): the
//! members as a grid of tiles (`CanvasGrid` at 180px, names under them, no
//! extensions), to select, reorder by dragging, open, and act on through
//! their menu. The tiles are the library grid's own ([`grid::tile_art`]);
//! the editor keeps its own selection, which is of members, not of library
//! items.
use super::group::{CollectionMenu, MemberOpened};
use crate::grid::{
    self, art, drag, format, layout,
    layout::{Layout, ViewMode},
    marquee, preview, reveal,
    thumbnails::{self, Thumbnails},
};
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
        /// `group.removeMembers`: the selected members leave the collection.
        RemoveMembers,
        /// `view.quicklook`: Quick Look on the one selected member.
        QuickLookMember
    ]
);

/// The key context the editor's grid carries while it has the keyboard.
pub const CONTEXT: &str = "GroupEditor";

/// `targetSize={180}`.
const TARGET_SIZE: f32 = 180.;
/// "Return to Top" appears once the grid has scrolled this far.
const RETURN_TO_TOP_AFTER: f32 = 200.;

/// Members were right-clicked at this window position: `members` are the
/// ones the menu acts on (the selection when the tile is in it, else the
/// tile), by their place in the collection.
pub struct MemberMenu {
    pub position: Point<Pixels>,
    pub members: Vec<usize>,
}

/// Quick Look on the member at this place.
pub struct QuickLookRequested(pub usize);

/// The selected members are to leave the collection.
pub struct RemoveRequested(pub Vec<usize>);

/// A reorder by dragging has been answered: why it failed, or `None` when
/// it went through (`saveOrder` sets and clears the view's inline error).
pub struct Reordered(pub Option<SharedString>);

impl EventEmitter<MemberMenu> for GroupEditor {}
impl EventEmitter<QuickLookRequested> for GroupEditor {}
impl EventEmitter<RemoveRequested> for GroupEditor {}
impl EventEmitter<MemberOpened> for GroupEditor {}
impl EventEmitter<CollectionMenu> for GroupEditor {}
impl EventEmitter<Reordered> for GroupEditor {}

/// `groupSelectionForClick`: Shift selects from the anchor to the member
/// (the anchor stays), Cmd or Ctrl toggles it, a plain click selects it
/// alone. Members are named by id, `order` is the collection's order.
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

/// `memberToGridItem`: a member as the grid draws a tile, with its media's
/// id for the item's.
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

/// Members being dragged to a new place.
struct Dragging {
    /// By their place in the collection.
    members: Vec<usize>,
    ghost: drag::ItemDrag,
    /// The member they would go beside, and on which side.
    target: Option<(usize, drag::Side)>,
}

pub struct GroupEditor {
    session: Entity<Session>,
    /// The editor's own thumbnail cache, so the library grid's plan for its
    /// frame is not replaced by this one's.
    thumbnails: Entity<Thumbnails>,
    focus: FocusHandle,
    collection: Option<RootId>,
    members: Vec<RootSummary>,
    /// Each member's thumbnail and original file.
    thumbs: Vec<PathBuf>,
    files: Vec<PathBuf>,
    selected: BTreeSet<MediaId>,
    anchor: Option<MediaId>,
    scroll: ScrollHandle,
    bounds: Bounds<Pixels>,
    layout: Layout,
    /// The width, gap and member count the layout was made for.
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
    /// A press on a tile that becomes a drag after 5px.
    press: Option<(usize, Point<Pixels>)>,
    dragging: Option<Dragging>,
    swallow_tile_click: bool,
}

impl GroupEditor {
    pub fn new(session: Entity<Session>, cx: &mut Context<Self>) -> Self {
        // A member's thumbnail written again (regenerated, or a captured
        // frame) is read again.
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
        // A missing thumbnail that was made is drawn on the next frame.
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

    /// The collection shown, if any.
    pub fn collection(&self) -> Option<RootId> {
        self.collection
    }

    /// Shows `details`' members. The same collection read again keeps its
    /// place, and the selected members that are still in it.
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
        // The pointer stays over whichever tile is now where it was.
        self.hovered = self.hovered.filter(|index| *index < members.len());
        self.members = members;
        self.layout_key = None;
        self.marquee = None;
        cx.notify();
    }

    /// Lets go of the collection, its selection and its place.
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

    /// The selected members, in the collection's order.
    pub fn selected(&self) -> Vec<usize> {
        self.members
            .iter()
            .enumerate()
            .filter(|(_, item)| self.selected.contains(&item.cover_media_id))
            .map(|(index, _)| index)
            .collect()
    }

    /// Escape: a selection is cleared first; says whether there was one.
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

    /// `onSelectionChange`/`onMarqueeSelectionChange`: one member becomes
    /// the anchor, several leave none.
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

    /// `openMemberMenu` with the selection: a member outside the selection
    /// becomes it, and the anchor.
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

    // ── Geometry ────────────────────────────────────────────────

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

    /// The image under a window position: its member, and the position in
    /// layout pixels.
    fn image_at(&self, position: Point<Pixels>) -> Option<(usize, f32, f32)> {
        let (x, y) = self.local(position);
        let y = y + self.scroll_top();
        if x < 0. || x >= self.grid_width() {
            return None;
        }
        marquee::image_at(&self.layout, x, y, self.text_height()).map(|index| (index, x, y))
    }

    /// The editor's tiles look: the grid mode, names without extensions,
    /// nothing else (`CanvasGrid`'s defaults), at the library's spacing.
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

    // ── Pointer ─────────────────────────────────────────────────

    fn pointer_moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.pointer = Some(position);
        if self.marquee.is_some() || self.dragging.is_some() {
            return;
        }
        let hit = self.image_at(position);
        let hovered = hit.map(|(index, _, _)| index);
        if hovered != self.hovered {
            self.hovered = hovered;
            cx.notify();
        }
        let text_height = self.text_height();
        let magnifier = hit.filter(|&(index, x, y)| {
            marquee::on_magnifier(&self.layout.tiles[index], x, y, text_height)
        });
        if let Some((index, _, _)) = magnifier {
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
        self.pointer = None;
        if self.marquee.is_none() && self.hovered.take().is_some() {
            cx.notify();
        }
        self.preview.left(|this: &mut Self| &mut this.preview, cx);
    }

    /// A left press: on an image it may become a drag; on empty space it
    /// starts a rubber band, which Shift or Cmd adds to the selection.
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
        if let Some(marquee) = &mut self.marquee {
            marquee.pointer = local;
            marquee.speed = marquee::auto_scroll_speed(local.1, viewport);
            marquee.track(top, &self.layout, text_height);
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

    /// Past 5px a press on a tile drags the selection, or that tile when it
    /// is not selected; the ghost follows the pointer and the drop line
    /// shows where they would go.
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
            let ghost = drag::ItemDrag {
                roots: members
                    .iter()
                    .map(|index| self.members[*index].root_id)
                    .collect(),
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
        // The image under the pointer keeps its hover, as the canvas grid
        // goes on hit-testing while it drags.
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

    /// The button came up after a press on a tile: a drag over the grid puts
    /// the dragged members where the line showed and keeps them selected;
    /// the click it makes selects nothing.
    fn drag_released(&mut self, cx: &mut Context<Self>) {
        self.press = None;
        let Some(dragging) = self.dragging.take() else {
            return;
        };
        // The click the release makes, on whatever is under the pointer,
        // selects nothing.
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
                        // Only while the editor still shows that collection.
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme::theme(cx);
        // A rubber band near an edge scrolls each frame.
        if let Some(speed) = self
            .marquee
            .as_ref()
            .map(|marquee| marquee.speed)
            .filter(|speed| *speed != 0.)
        {
            let current = self.scroll_top();
            let limit = f32::from(self.scroll.max_offset().y).max(0.);
            let next = (current + speed).clamp(0., limit);
            if next != current {
                self.scroll.set_offset(point(px(0.), px(-next)));
            }
            window.request_animation_frame();
        }
        let prefs = self.prefs(cx);
        let width = self.grid_width();
        let viewport = f32::from(self.bounds.size.height);
        let top = self.scroll_top();
        // A scroll ends the hover and closes the preview; the hover comes
        // back once the scrolling has settled.
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
        // Thumbnails load from the middle of the viewport outward.
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
                    // A member with no thumbnail yet has one made, as the
                    // library's grid asks for its tiles'.
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
                    // The editor's grid has no Grayscale of its own.
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
                    div()
                        .id(("group-member-tile", index))
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
        // `CanvasGrid`'s drop indicator: a 2px line in the gap beside the
        // target's image, with a triangle at its top.
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

        let entity = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, window, cx| {
                let _ = entity.update(cx, |this, cx| {
                    if this.bounds != bounds {
                        this.bounds = bounds;
                        cx.notify();
                        // A notification made while a frame is being drawn
                        // does not by itself bring another frame.
                        window.request_animation_frame();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        let body = div()
            .id("group-editor-grid")
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
            // A right-click no image took is the collection's own menu.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.focus, cx);
                    cx.emit(CollectionMenu(event.position));
                }),
            )
            // A click no image took clears the selection, unless it ends a
            // rubber band.
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
            // `edit.selectAll`, from its key or the Edit menu: every member.
            .on_action(cx.listener(|this, _: &crate::shell::SelectAll, _, cx| this.select_all(cx)))
            .on_action(cx.listener(|this, _: &RemoveMembers, _, cx| {
                let members = this.selected();
                if !members.is_empty() {
                    cx.emit(RemoveRequested(members));
                }
            }))
            // `edit.copy` (`GroupSurface`): the selected members' files, or
            // every member's when none is selected.
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
                    crate::ui::notify::error("Could not copy selection", error, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &QuickLookMember, _, cx| {
                if let [member] = this.selected().as_slice() {
                    cx.emit(QuickLookRequested(*member));
                }
            }))
            .child(measure)
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
        // A rubber band or a press on a tile is followed anywhere in the
        // window until the button comes up, as pointer capture does.
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
        if let Some(shown) = &self.preview.shown {
            let (overlay, fading) = preview::overlay(shown, window);
            if fading {
                window.request_animation_frame();
            }
            root = root.children(overlay);
        }
        root
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
        // Without an anchor, Shift is a plain click.
        let (selected, anchor) =
            selection_for_click(&BTreeSet::new(), &ORDER, MediaId(20), None, false, true);
        assert_eq!(selected, ids(&[20]));
        assert_eq!(anchor, MediaId(20));
    }
}
