//! Shows the members of a collection in one scrolling column, each as wide
//! as the column, one under the other. The media view, Quick Look and the
//! detail window use this when the item they show is a collection. The
//! Electron app does the same in `GroupSurface.tsx`, in its reader mode.
//!
//! A member is first drawn from its thumbnail. After it has been visible
//! for 200 ms, its full media is loaded. For an image, the original file is
//! decoded and fades in over the thumbnail. For anything else (a PDF, a
//! video and so on), a media surface is created under the thumbnail, and
//! the thumbnail fades out once that surface has drawn its first frame or
//! page. Only the 100 most recently loaded members stay loaded.
//!
//! A PDF, DjVu or comic member shows all of its pages, one under the other,
//! at the column's width. The page sizes of every such member are read when
//! the collection opens, so that the member already has its final height
//! before it is scrolled into view.
use super::Media;
use super::document::geometry::PageSize;
use super::document::kind::{RendererKind, renderer_kind};
use super::document::{paged, pages};
use super::surface::{self, MediaSurface};
use crate::i18n::t;
use crate::session::{Report, Reread, Session};
use crate::theme::Text as _;
use crate::{motion, theme, ui};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::{ContentSortField, Lifecycle, MediaId, RootDetails, RootId};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

/// The padding around the column and the gap between two members, in
/// points. The Electron CSS uses `--space-4` for both.
const SPACE: f32 = 16.;
/// The smallest height a member is given, in points, however flat its
/// picture is (`min-height: 180px` in the Electron CSS).
const MIN_HEIGHT: f32 = 180.;
/// How long a member must stay visible before its full media is loaded
/// (`FULL_MEDIA_VISIBILITY_DELAY_MS` in the Electron app).
const VISIBILITY_DELAY: Duration = Duration::from_millis(200);
/// How many members may have their full media loaded at the same time
/// (`MAX_WARM_GROUP_MEDIA` in the Electron app).
const WARM_LIMIT: usize = 100;

/// Sent when a member is right-clicked. `position` is where the pointer was,
/// in window coordinates, and `member` is the member's index.
pub struct MemberMenu {
    pub position: Point<Pixels>,
    pub member: usize,
}

impl EventEmitter<MemberMenu> for GroupReader {}

/// Sent when a member is double-clicked, with the member's index. The host
/// then shows that member on its own in the media view.
pub struct MemberOpened(pub usize);

/// Sent when the empty space between members is right-clicked, with the
/// pointer's position in window coordinates. The host then opens the menu
/// for the collection itself.
pub struct CollectionMenu(pub Point<Pixels>);

impl EventEmitter<MemberOpened> for GroupReader {}
impl EventEmitter<CollectionMenu> for GroupReader {}

/// The two ways the media view can show a collection: `Reader` shows the
/// members one under the other, for reading; `Editor` shows them as a grid,
/// for reordering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Reader,
    Editor,
}

/// Returns the members sorted by name or by size. Names are compared
/// ignoring case, and numbers inside a name are compared as numbers. Sizes
/// are sorted largest first. Members that compare equal are ordered by id.
pub fn sorted_members(members: &[(MediaId, &str, u64)], field: ContentSortField) -> Vec<MediaId> {
    let mut sorted = members.to_vec();
    sorted.sort_by(|left, right| {
        let first = match field {
            ContentSortField::Size => right.2.cmp(&left.2),
            _ => crate::text::natural_order(left.1, right.1),
        };
        first.then(left.0.cmp(&right.0))
    });
    sorted.into_iter().map(|(id, _, _)| id).collect()
}

/// Removes `members` from `collection` as one change that Undo reverses.
/// When `trash` is true the removed members are also moved to the Trash.
/// The returned task resolves to whether the change was saved.
pub fn detach(
    session: &Entity<Session>,
    collection: RootId,
    members: Vec<MediaId>,
    trash: bool,
    cx: &mut App,
) -> futures::channel::oneshot::Receiver<Result<(), picto_library::LibraryError>> {
    let lifecycle = trash.then_some(Lifecycle::Trash);
    session.update(cx, |session, cx| {
        session.edit_reporting(
            Report::announce("collections.detach"),
            move |library| {
                library
                    .detach_collection_members(collection, members, lifecycle, crate::now_ms())
                    .map(|_| ())
            },
            Reread::RESULTS,
            cx,
        )
    })
}

/// Saves `order` as the order of the collection's members, as one change
/// that Undo reverses.
pub fn reorder(
    session: &Entity<Session>,
    collection: RootId,
    order: Vec<MediaId>,
    cx: &mut App,
) -> futures::channel::oneshot::Receiver<Result<(), picto_library::LibraryError>> {
    session.update(cx, |session, cx| {
        session.edit_reporting(
            Report::announce("collections.reorder"),
            move |library| {
                library
                    .reorder_collection(collection, order, crate::now_ms())
                    .map(|_| ())
            },
            Reread::SELECTION,
            cx,
        )
    })
}

/// Sorts the members of the collection in `details` by `field` and saves
/// that as the collection's order.
pub fn sort(
    session: &Entity<Session>,
    details: &RootDetails,
    field: ContentSortField,
    cx: &mut App,
) -> futures::channel::oneshot::Receiver<Result<(), picto_library::LibraryError>> {
    let members: Vec<(MediaId, &str, u64)> = details
        .media
        .iter()
        .map(|media| {
            (
                media.media_id,
                media.media_name.as_str(),
                media.facts.size_bytes,
            )
        })
        .collect();
    let order = sorted_members(&members, field);
    reorder(session, details.root.root_id, order, cx)
}

/// A red strip shown above the members when a change to the collection
/// could not be saved. `message` is the reason.
pub fn inline_error(message: SharedString, cx: &App) -> Div {
    let theme = theme::theme(cx);
    div()
        .flex_none()
        .px(px(18.))
        .py(px(8.))
        .border_b_1()
        .border_color(theme.border_secondary())
        .bg(rgba(theme::GROUP_ERROR_BACKGROUND))
        .text(theme::TEXT_SM)
        .text_color(rgb(theme::GROUP_ERROR_TEXT))
        .child(message)
}

/// What decides a member's height in the column.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// One picture, with its width and height in pixels when they are known.
    Picture(Option<(f32, f32)>),
    /// A document, with the size of each page. The pages are drawn one under
    /// the other.
    Pages(Vec<PageSize>),
}

/// The height a member takes in a column `width` points wide. A picture is
/// scaled to the column's width; a document is the height of all its pages
/// at that width. The result is never less than `MIN_HEIGHT`.
pub fn frame_height(shape: &Shape, width: f32) -> f32 {
    match shape {
        Shape::Picture(size) => size
            .filter(|(w, h)| *w > 0. && *h > 0.)
            .map_or(0., |(w, h)| width * h / w),
        Shape::Pages(sizes) => {
            paged::inline_height(sizes, width.max(1.))
        }
    }
    .max(MIN_HEIGHT)
}

/// For each member, where it starts (measured from the top of the column's
/// content) and how tall it is, in a column `width` points wide.
pub fn layout(shapes: &[Shape], width: f32) -> Vec<(f32, f32)> {
    let mut top = SPACE;
    shapes
        .iter()
        .map(|shape| {
            let height = frame_height(shape, width);
            let place = (top, height);
            top += height + SPACE;
            place
        })
        .collect()
}

/// The member that the top edge of the visible area falls on when the
/// column is scrolled down by `offset`, and how far down that member the
/// edge is, as a fraction of the member's height. When the edge is in the
/// space above a member, the fraction is negative. `places` is the result
/// of [`layout`]. Returns `None` when there are no members.
pub fn top_member(places: &[(f32, f32)], offset: f32) -> Option<(usize, f32)> {
    let index = places
        .iter()
        .position(|(top, height)| top + height > offset)
        .or(places.len().checked_sub(1))?;
    let (top, height) = places[index];
    Some((index, (offset - top) / height.max(1.)))
}

/// The indices of the members that are at least partly visible when the
/// column is scrolled down by `offset` and the visible area is `height`
/// tall. `places` is the result of [`layout`].
pub fn in_view(places: &[(f32, f32)], offset: f32, height: f32) -> Vec<usize> {
    places
        .iter()
        .enumerate()
        .filter(|(_, (top, size))| top + size > offset && *top < offset + height)
        .map(|(index, _)| index)
        .collect()
}

/// Returns the list of loaded members after `member` is loaded: `member`
/// moves to the end as the most recent, and the oldest entries are dropped
/// so that at most `limit` remain.
pub fn retain_warm(current: &[usize], member: usize, limit: usize) -> Vec<usize> {
    let mut next: Vec<usize> = current.iter().copied().filter(|m| *m != member).collect();
    next.push(member);
    let drop = next.len().saturating_sub(limit);
    next.drain(..drop);
    next
}

struct Member {
    media: Media,
    /// True when the member is a still image, which this view draws itself.
    /// Any other kind of member is drawn by a media surface once it is
    /// loaded.
    image: bool,
    /// Set when the member is a PDF, DjVu or comic, which is shown as all of
    /// its pages. `pages` holds the page sizes once they have been read.
    paged: Option<RendererKind>,
    pages: Option<Vec<PageSize>>,
    /// The fade that hides the thumbnail once the member's media surface
    /// has drawn its first frame or page.
    uncover: motion::Fade,
    thumbnail: Option<Arc<RenderImage>>,
    full: Option<Arc<RenderImage>>,
    /// The 130 ms fade that shows the original image once it has been
    /// decoded.
    reveal: motion::Fade,
    surface: Option<Entity<MediaSurface>>,
    tasks: Vec<Task<()>>,
}

pub struct GroupReader {
    session: Entity<Session>,
    root: Option<RootId>,
    members: Vec<Member>,
    scroll: ScrollHandle,
    /// The size of the scrolling area, as measured in the last frame drawn.
    viewport: Size<Pixels>,
    warm: Vec<usize>,
    /// The visible members that are still inside their 200 ms wait and have
    /// not started loading yet.
    waiting: HashSet<usize>,
}

impl GroupReader {
    pub fn new(session: Entity<Session>) -> Self {
        Self {
            session,
            root: None,
            members: Vec::new(),
            scroll: ScrollHandle::new(),
            viewport: Size::default(),
            warm: Vec::new(),
            waiting: HashSet::new(),
        }
    }

    /// The collection shown, if any.
    pub fn root(&self) -> Option<RootId> {
        self.root
    }

    /// Shows `details`' members. The same collection read again keeps its
    /// place and what is loaded; another starts at the top.
    pub fn show(&mut self, details: &RootDetails, cx: &mut Context<Self>) {
        let root = details.root.root_id;
        let fresh = self.root != Some(root);
        if fresh {
            self.clear(cx);
            self.root = Some(root);
            self.scroll.set_offset(point(px(0.), px(0.)));
        }
        let session = self.session.read(cx);
        let media: Vec<Media> = (0..details.media.len())
            .filter_map(|index| Media::of(session, details, index))
            .collect();
        let same = !fresh
            && media.len() == self.members.len()
            && media
                .iter()
                .zip(&self.members)
                .all(|(media, member)| media.key == member.media.key);
        if same {
            return;
        }
        self.clear(cx);
        self.root = Some(root);
        for media in media {
            let kind = renderer_kind(&media.mime);
            let image = kind == RendererKind::Image;
            let paged = matches!(
                kind,
                RendererKind::Pdf | RendererKind::Djvu | RendererKind::Cbz
            )
            .then_some(kind);
            let index = self.members.len();
            let load = ImageAssetLoader::load(Resource::Path(media.thumbnail.clone().into()), cx);
            let load = cx.background_executor().spawn(load);
            let task = cx.spawn(async move |this, cx| {
                let result = load.await;
                let _ = this.update(cx, |this: &mut Self, cx| {
                    if let Some(member) = this.members.get_mut(index) {
                        member.thumbnail = result.ok();
                        cx.notify();
                    }
                });
            });
            let mut tasks = vec![task];
            if let Some(kind) = paged {
                let path = media.path.clone();
                let key = media.key.clone();
                let measure = cx
                    .background_executor()
                    .spawn(async move { pages::measure(kind, &path) });
                tasks.push(cx.spawn(async move |this, cx| {
                    let measured = measure.await;
                    let _ = this.update(cx, |this: &mut Self, cx| {
                        if let Some(member) = this.members.get_mut(index)
                            && member.media.key == key
                            && let Ok(sizes) = measured
                        {
                            member.pages = Some(sizes);
                            cx.notify();
                        }
                    });
                }));
            }
            self.members.push(Member {
                media,
                image,
                paged,
                pages: None,
                uncover: motion::Fade::default(),
                thumbnail: None,
                full: None,
                reveal: motion::Fade::default(),
                surface: None,
                tasks,
            });
        }
        cx.notify();
    }

    /// Removes every member and frees the images and media surfaces that
    /// were loaded for them.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        for member in self.members.drain(..) {
            for image in [member.thumbnail, member.full].into_iter().flatten() {
                crate::atlas::release(image, cx);
            }
            if let Some(surface) = member.surface {
                surface.update(cx, |surface, _| surface.stop());
            }
        }
        self.root = None;
        self.warm.clear();
        self.waiting.clear();
        cx.notify();
    }

    /// The width of the column: the scrolling area's width minus the padding
    /// on both sides and the space reserved for the scrollbar.
    fn column_width(&self) -> f32 {
        (f32::from(self.viewport.width) - 2. * SPACE - ui::scrollbar::GUTTER).max(0.)
    }

    fn shape(member: &Member) -> Shape {
        match (&member.pages, member.paged, member.media.natural) {
            (Some(pages), _, _) => Shape::Pages(pages.clone()),
            // The page sizes have not been read yet, so use the thumbnail's
            // size as the size of a single page for now.
            (None, Some(_), Some(natural)) => Shape::Pages(vec![natural]),
            _ => Shape::Picture(member.media.natural),
        }
    }

    fn places(&self) -> Vec<(f32, f32)> {
        let shapes: Vec<_> = self.members.iter().map(Self::shape).collect();
        layout(&shapes, self.column_width())
    }

    /// Records the scrolling area's new size. Members are as wide as the
    /// column, so a change of width changes every member's height. To keep
    /// the same part of the collection on screen, the member at the top of
    /// the visible area is found before the change, together with how far
    /// down it the top edge is (as a fraction of its height), and the scroll
    /// offset is then set so that the same point of that member is at the
    /// top edge again.
    fn resize_viewport(&mut self, size: Size<Pixels>) {
        let width_changed = self.viewport.width != size.width && self.viewport.width > px(0.);
        let anchor = width_changed
            .then(|| {
                let offset = -f32::from(self.scroll.offset().y);
                top_member(&self.places(), offset)
            })
            .flatten();
        self.viewport = size;
        if let Some((index, fraction)) = anchor
            && let Some((top, height)) = self.places().get(index).copied()
        {
            let offset = (top + fraction * height).max(0.);
            self.scroll.set_offset(point(px(0.), px(-offset)));
        }
    }

    fn visible(&self) -> Vec<usize> {
        let offset = -f32::from(self.scroll.offset().y);
        in_view(&self.places(), offset, f32::from(self.viewport.height))
    }

    /// Starts the 200 ms wait for every member that has just become
    /// visible. A member that is still visible when its wait ends is
    /// loaded. A member that is already loaded is only marked as the most
    /// recently used.
    fn watch(&mut self, cx: &mut Context<Self>) {
        if self.viewport.height <= px(0.) {
            return;
        }
        for index in self.visible() {
            if self.warm.contains(&index) {
                continue;
            }
            if !self.waiting.insert(index) {
                continue;
            }
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(VISIBILITY_DELAY).await;
                let _ = this.update(cx, |this: &mut Self, cx| {
                    this.waiting.remove(&index);
                    if this.visible().contains(&index) {
                        this.warm_up(index, cx);
                    }
                });
            })
            .detach();
        }
    }

    /// Loads the full media of member `index`, and unloads the members that
    /// no longer fit within `WARM_LIMIT`.
    fn warm_up(&mut self, index: usize, cx: &mut Context<Self>) {
        let next = retain_warm(&self.warm, index, WARM_LIMIT);
        for cold in self.warm.iter().filter(|member| !next.contains(member)) {
            if let Some(member) = self.members.get_mut(*cold) {
                if let Some(image) = member.full.take() {
                    crate::atlas::release(image, cx);
                }
                member.reveal = motion::Fade::default();
                member.uncover = motion::Fade::default();
                if let Some(surface) = member.surface.take() {
                    surface.update(cx, |surface, _| surface.stop());
                }
            }
        }
        self.warm = next;
        let Some(member) = self.members.get_mut(index) else {
            return;
        };
        if member.image {
            let load = ImageAssetLoader::load(Resource::Path(member.media.path.clone().into()), cx);
            let load = cx.background_executor().spawn(load);
            let key = member.media.key.clone();
            member.tasks.push(cx.spawn(async move |this, cx| {
                let result = load.await;
                let _ = this.update(cx, |this: &mut Self, cx| {
                    let warm = this.warm.contains(&index);
                    if let Some(member) = this.members.get_mut(index)
                        && member.media.key == key
                        && warm
                        && let Ok(image) = result
                    {
                        member.full = Some(image);
                        member.reveal.set(true, motion::FULL_IMAGE_FADE);
                        cx.notify();
                    }
                });
            }));
        } else if member.surface.is_none() {
            let media = Media {
                name: member.media.name.clone(),
                key: member.media.key.clone(),
                thumbnail: member.media.thumbnail.clone(),
                natural: member.media.natural,
                path: member.media.path.clone(),
                mime: member.media.mime.clone(),
            };
            let surface = cx.new(|cx| {
                let mut options = super::options(cx, false, true);
                options.member = true;
                // Members ignore the two preview settings, as they do in the
                // Electron app: no checkerboard behind transparent images,
                // and smooth scaling.
                options.transparency_grid = false;
                options.pixelated = false;
                let mut surface = MediaSurface::new(options, cx);
                media.open(&mut surface, cx);
                surface
            });
            let key = member.media.key.clone();
            cx.subscribe(
                &surface,
                move |this: &mut Self, _, event: &surface::Event, cx| {
                    if matches!(event, surface::Event::Progressed)
                        && this.members.get(index).is_some_and(|m| m.media.key == key)
                    {
                        cx.notify();
                    }
                },
            )
            .detach();
            member.surface = Some(surface);
        }
        cx.notify();
    }

    /// Draws member `index` at `height` points tall: its thumbnail, and its
    /// full media once that is loaded.
    fn frame(&self, index: usize, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let member = &self.members[index];
        let theme = theme::theme(cx);
        let radius = theme::RADIUS_LG;
        let picture = |image: Arc<RenderImage>, id: String| {
            img(image)
                .id(ElementId::Name(id.into()))
                .absolute()
                .inset_0()
                .size_full()
                .rounded(px(radius))
                .object_fit(ObjectFit::Fill)
        };
        // For a document, where each page goes at the column's width. Only
        // the first page has a thumbnail. The other pages are drawn as blank
        // sheets until the member's media surface draws them.
        let pages = match Self::shape(member) {
            Shape::Pages(sizes) if member.paged.is_some() => Some(
                paged::inline_column(&sizes, self.column_width().max(1.)).pages,
            ),
            _ => None,
        };
        let warm = member.surface.is_some() && !member.image;
        // `cover` is the thumbnail's opacity. For a member drawn by a media
        // surface it goes from 1 to 0 once the surface has drawn.
        let cover = if warm {
            1. - member.uncover.value_with(motion::EASE)
        } else {
            1.
        };
        let thumbnail = member
            .thumbnail
            .clone()
            .filter(|_| cover > 0.)
            .map(|thumbnail| {
                let id = format!("group-thumb-{}", member.media.key);
                let layer = match pages.as_ref().and_then(|pages| pages.first()) {
                    // A document's thumbnail is drawn where its first page
                    // is.
                    Some(first) => img(thumbnail)
                        .id(ElementId::Name(id.into()))
                        .absolute()
                        .left(px(first.0))
                        .top(px(first.1))
                        .w(px(first.2))
                        .h(px(first.3))
                        .object_fit(ObjectFit::Fill),
                    None => picture(thumbnail, id),
                };
                if warm { layer.opacity(cover) } else { layer }
            });
        let blank = pages.iter().flatten().skip(1).map(|page| {
            div()
                .absolute()
                .left(px(page.0))
                .top(px(page.1))
                .w(px(page.2))
                .h(px(page.3))
                .bg(rgb(theme::DOCUMENT_PAPER))
        });
        let (below, above) = if warm {
            (None, thumbnail)
        } else {
            (thumbnail, None)
        };
        let content = div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .rounded(px(radius))
            .children(blank)
            .children(below)
            .when_some(member.full.clone(), |content, full| {
                content.child(
                    picture(full, format!("group-full-{}", member.media.key))
                        .opacity(member.reveal.value_with(motion::EASE)),
                )
            })
            .when_some(member.surface.clone(), |content, surface| {
                content.child(div().absolute().inset_0().child(surface))
            })
            .children(above);
        // The member is drawn as the picture alone, with rounded corners
        // and no outline. Until its thumbnail has loaded, this faint fill
        // shows where it will be.
        div()
            .id(ElementId::NamedInteger("group-member".into(), index as u64))
            .rounded(px(radius))
            .bg(theme.subtle_overlay())
            .relative()
        .flex_none()
        .w_full()
        .h(px(height))
        .overflow_hidden()
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                cx.emit(MemberMenu {
                    position: event.position,
                    member: index,
                });
            }),
        )
        // Screen readers treat the member as a picture, not as a button, as
        // they do in the Electron app. A double-click asks the host to show
        // the member on its own.
        .a11y_presentational()
        .on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
            if event.click_count() == 2 {
                cx.emit(MemberOpened(index));
            }
        }))
        .child(content)
        .into_any_element()
    }
}

impl Render for GroupReader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Start fading out the thumbnail of each member whose media surface
        // has now drawn its first frame or page.
        for member in &mut self.members {
            if let Some(surface) = &member.surface
                && !member.image
                && !member.uncover.on()
                && surface.read(cx).settled(cx)
            {
                member.uncover.set(true, motion::FULL_IMAGE_FADE);
            }
        }
        let animating = self
            .members
            .iter()
            .any(|member| member.reveal.animating() || member.uncover.animating());
        if animating {
            window.request_animation_frame();
        }
        self.watch(cx);
        // Before the first frame has measured this view, the column's width
        // is unknown. Draw no members in that frame, because without a
        // width each one would be laid out at the minimum height.
        let measured = self.viewport.width > px(0.);
        let empty = self.root.is_some() && self.members.is_empty();
        let places = if measured { self.places() } else { Vec::new() };
        let frames: Vec<AnyElement> = places
            .iter()
            .enumerate()
            .map(|(index, (_, height))| self.frame(index, *height, cx))
            .collect();
        let entity = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, window, cx| {
                let _ = entity.update(cx, |this, cx| {
                    if this.viewport != bounds.size {
                        this.resize_viewport(bounds.size);
                        cx.notify();
                        // Ask for another frame as well, so that the members
                        // are loaded for the size just measured.
                        window.request_animation_frame();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        // The app's background colour fills the area behind the column.
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(theme::theme(cx).background())
            .child(measure)
            .child(
                div()
                    .id("group-reader")
                    .track_scroll(&self.scroll)
                    .overflow_y_scroll()
                    .size_full()
                    .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                    // A right-click that no member handled was on the empty
                    // space between members, so ask the host for the
                    // collection's menu.
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|_, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            cx.emit(CollectionMenu(event.position));
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(SPACE))
                            .pt(px(SPACE))
                            .pb(px(SPACE))
                            .pl(px(SPACE))
                            .pr(px(SPACE + ui::scrollbar::GUTTER))
                            .children(frames)
                            // A collection with no members shows a short
                            // message instead of an empty column.
                            .when(empty, |column| {
                                column.child(
                                    div()
                                        .w_full()
                                        .flex()
                                        .justify_center()
                                        .text(theme::TEXT_LG)
                                        .text_color(theme::theme(cx).text_secondary())
                                        .child(t("This group is empty.")),
                                )
                            }),
                    ),
            )
            .child(ui::scrollbar(
                "group-reader-scrollbar",
                &self.scroll,
                window,
                cx,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::{Shape, in_view, layout, retain_warm, sorted_members};
    use picto_library::{ContentSortField, MediaId};

    #[test]
    fn frames_take_the_member_shape_at_the_column_width() {
        let places = layout(
            &[
                Shape::Picture(Some((1000., 500.))),
                Shape::Picture(Some((100., 400.))),
                Shape::Picture(None),
            ],
            500.,
        );
        assert_eq!(places, vec![(16., 250.), (282., 2000.), (2298., 180.)]);
    }

    #[test]
    fn a_frame_is_never_under_180_tall() {
        assert_eq!(
            layout(&[Shape::Picture(Some((4000., 100.)))], 553.),
            vec![(16., 180.)]
        );
    }

    #[test]
    fn a_document_member_holds_all_its_pages() {
        // Two US Letter pages in a column 504 points wide, with the 8-point
        // gap the document layout puts between pages.
        let places = layout(&[Shape::Pages(vec![(612., 792.), (612., 792.)])], 504.);
        let height = 2. * 504. * 792. / 612. + 8.;
        assert!((places[0].1 - height).abs() < 0.01, "{places:?}");
        // Before the page sizes are read, a document is laid out as one page
        // with the thumbnail's proportions.
        let one = layout(&[Shape::Pages(vec![(2550., 3300.)])], 504.);
        assert!((one[0].1 - (504. * 3300. / 2550.)).abs() < 0.01);
    }

    #[test]
    fn members_partly_in_view_count() {
        let places = vec![(16., 250.), (282., 2000.), (2298., 180.)];
        assert_eq!(in_view(&places, 0., 300.), vec![0, 1]);
        assert_eq!(in_view(&places, 300., 300.), vec![1]);
        assert_eq!(in_view(&places, 2200., 400.), vec![1, 2]);
        // A member that only touches the edge of the visible area does not
        // count as visible.
        assert_eq!(in_view(&places, 266., 16.), Vec::<usize>::new());
    }

    #[test]
    fn the_warm_budget_keeps_the_latest() {
        assert_eq!(retain_warm(&[1, 2, 3], 2, 3), vec![1, 3, 2]);
        assert_eq!(retain_warm(&[1, 2, 3], 4, 3), vec![2, 3, 4]);
        assert_eq!(retain_warm(&[], 0, 100), vec![0]);
    }

    #[test]
    fn contents_sort_by_natural_name_or_largest_first() {
        let members = [
            (MediaId(4), "Chapter 10", 300),
            (MediaId(2), "chapter 2", 100),
            (MediaId(3), "Chapter 2", 300),
            (MediaId(1), "Epilogue", 50),
        ];
        // Numbers inside names are compared as numbers, case is ignored, and
        // members with equal names are ordered by id.
        assert_eq!(
            sorted_members(&members, ContentSortField::Name),
            [MediaId(2), MediaId(3), MediaId(4), MediaId(1)]
        );
        assert_eq!(
            sorted_members(&members, ContentSortField::Size),
            [MediaId(3), MediaId(4), MediaId(2), MediaId(1)]
        );
    }
}
