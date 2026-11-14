//! A collection's members in one continuous column (`GroupSurface.tsx` in
//! its reader mode), as the media view, Quick Look and the detail window
//! show a collection. Each member sits in a glass frame of its own shape,
//! the column's full width, one under the other; the column scrolls.
//!
//! A member shows its thumbnail first. Once it has been in view for 200 ms
//! its full media is asked for: an image's original fades in over the
//! thumbnail, and any other member opens in a media surface of its own (the
//! PDF viewer, the media player, ...), under its thumbnail, which fades out
//! the same way once the surface has drawn its first frame or page. The
//! last 100 members asked for stay loaded (`useWarmGroupMediaBudget`).
//!
//! A PDF, DjVu or comic member shows all its pages, one under the other at
//! the column's width, in its frame: the reader is where a collection of
//! documents is read, so its pages scroll with the rest. Each such member is
//! measured as the collection opens, so its frame has its final height
//! before it is in view.
use super::Media;
use super::document::geometry::PageSize;
use super::document::kind::{RendererKind, renderer_kind};
use super::document::{paged, pages};
use super::surface::{self, MediaSurface};
use crate::session::{Report, Reread, Session};
use crate::theme::Text as _;
use crate::{motion, theme, ui};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::{ContentSortField, Lifecycle, MediaId, RootDetails, RootId};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

/// `.reader`'s padding and `.memberStack`'s gap (`--space-4`).
const SPACE: f32 = 16.;
/// `.imageFrame` / `.mediaFrame`: `min-height: 180px`.
const MIN_HEIGHT: f32 = 180.;
/// The frames' 1px glass border, inside which the media is drawn.
const BORDER: f32 = 1.;
/// `FULL_MEDIA_VISIBILITY_DELAY_MS`: how long a member must stay in view
/// before its full media is asked for.
const VISIBILITY_DELAY: Duration = Duration::from_millis(200);
/// `MAX_WARM_GROUP_MEDIA`.
const WARM_LIMIT: usize = 100;

/// A member was right-clicked at this window position.
pub struct MemberMenu {
    pub position: Point<Pixels>,
    pub member: usize,
}

impl EventEmitter<MemberMenu> for GroupReader {}

/// A member was double-clicked: it opens by itself in the media view
/// (`setViewerIndex`).
pub struct MemberOpened(pub usize);

/// The space between the members was right-clicked at this window position:
/// the collection's own menu (`openEmptyMenu`).
pub struct CollectionMenu(pub Point<Pixels>);

impl EventEmitter<MemberOpened> for GroupReader {}
impl EventEmitter<CollectionMenu> for GroupReader {}

/// How the media view shows a collection (`GroupSurface`'s `mode`): its
/// members one under the other to read, or as a grid to arrange.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Reader,
    Editor,
}

/// `sortCollectionContents`: the members' order by name (natural, case
/// ignored) or by size (largest first), ties by id.
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

/// `detachMembers`: takes `members` out of `collection`, into the Trash when
/// `trash`, as one undoable change. The answer says whether it went through.
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

/// `saveOrder`: the members of `collection` in this order, as one undoable
/// change.
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

/// The members of `details` sorted by `field`, saved as the collection's
/// order.
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

/// `.inlineError`: why an edit of the collection failed, in a band over its
/// members.
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

/// What a member's frame is shaped by.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// A picture of this size, if known.
    Picture(Option<(f32, f32)>),
    /// Pages of these natural sizes, inline one under the other.
    Pages(Vec<PageSize>),
}

/// A member's frame height for a column `width` wide: a picture at that
/// width, pages at the width inside the frame's border; never under 180px.
pub fn frame_height(shape: &Shape, width: f32) -> f32 {
    match shape {
        Shape::Picture(size) => size
            .filter(|(w, h)| *w > 0. && *h > 0.)
            .map_or(0., |(w, h)| width * h / w),
        Shape::Pages(sizes) => {
            paged::inline_height(sizes, (width - 2. * BORDER).max(1.)) + 2. * BORDER
        }
    }
    .max(MIN_HEIGHT)
}

/// Where each member's frame lies in the column: its top, from the top of
/// the scrolled content, and its height, for a column `width` wide.
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

/// The members any part of which is in the viewport, scrolled `offset`
/// down, `height` tall.
pub fn in_view(places: &[(f32, f32)], offset: f32, height: f32) -> Vec<usize> {
    places
        .iter()
        .enumerate()
        .filter(|(_, (top, size))| top + size > offset && *top < offset + height)
        .map(|(index, _)| index)
        .collect()
}

/// `retainWarmGroupMedia`: `member` becomes the most recent, and only the
/// latest `limit` stay.
pub fn retain_warm(current: &[usize], member: usize, limit: usize) -> Vec<usize> {
    let mut next: Vec<usize> = current.iter().copied().filter(|m| *m != member).collect();
    next.push(member);
    let drop = next.len().saturating_sub(limit);
    next.drain(..drop);
    next
}

struct Member {
    media: Media,
    /// Shown as an image (`detailRendererKind` `image`); anything else
    /// opens in a media surface once warm.
    image: bool,
    /// A PDF, DjVu or comic: shown as its pages, measured as the
    /// collection opens.
    paged: Option<RendererKind>,
    pages: Option<Vec<PageSize>>,
    /// The thumbnail over a member's surface fading out once the surface
    /// has drawn.
    uncover: motion::Fade,
    thumbnail: Option<Arc<RenderImage>>,
    full: Option<Arc<RenderImage>>,
    /// `.fullImage`'s 130 ms fade in once the original has decoded.
    reveal: motion::Fade,
    surface: Option<Entity<MediaSurface>>,
    tasks: Vec<Task<()>>,
}

pub struct GroupReader {
    session: Entity<Session>,
    root: Option<RootId>,
    members: Vec<Member>,
    scroll: ScrollHandle,
    /// The scroller's size, as last laid out.
    viewport: Size<Pixels>,
    warm: Vec<usize>,
    /// Members in view whose 200 ms wait is running.
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

    /// Lets go of every member and what it loaded.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        for member in self.members.drain(..) {
            for image in [member.thumbnail, member.full].into_iter().flatten() {
                cx.drop_image(image, None);
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

    /// The column's width: the scroller less its padding and the scrollbar's
    /// gutter (`scrollbar-gutter: stable`).
    fn column_width(&self) -> f32 {
        (f32::from(self.viewport.width) - 2. * SPACE - ui::scrollbar::GUTTER).max(0.)
    }

    fn shape(member: &Member) -> Shape {
        match (&member.pages, member.paged, member.media.natural) {
            (Some(pages), _, _) => Shape::Pages(pages.clone()),
            // Until measured, the first page has the thumbnail's shape.
            (None, Some(_), Some(natural)) => Shape::Pages(vec![natural]),
            _ => Shape::Picture(member.media.natural),
        }
    }

    fn places(&self) -> Vec<(f32, f32)> {
        let shapes: Vec<_> = self.members.iter().map(Self::shape).collect();
        layout(&shapes, self.column_width())
    }

    fn visible(&self) -> Vec<usize> {
        let offset = -f32::from(self.scroll.offset().y);
        in_view(&self.places(), offset, f32::from(self.viewport.height))
    }

    /// Starts the wait of each member that came into view; a member still
    /// in view when it ends is loaded, one already loaded is only touched.
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

    /// `request`: loads `index`'s full media, and lets go of the members
    /// that fell out of the budget.
    fn warm_up(&mut self, index: usize, cx: &mut Context<Self>) {
        let next = retain_warm(&self.warm, index, WARM_LIMIT);
        for cold in self.warm.iter().filter(|member| !next.contains(member)) {
            if let Some(member) = self.members.get_mut(*cold) {
                if let Some(image) = member.full.take() {
                    cx.drop_image(image, None);
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
                let mut surface = MediaSurface::default();
                let mut options = super::options(cx, false, true);
                options.member = true;
                // `GroupImage` and `GroupDeferredRenderer` follow neither
                // preview setting: no checkerboard, smooth scaling.
                options.transparency_grid = false;
                options.pixelated = false;
                surface.set_options(options, cx);
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

    /// One member's frame: `.imageFrame` or `.mediaFrame`, a glass panel of
    /// the member's shape holding its thumbnail, then its full media.
    fn frame(&self, index: usize, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let member = &self.members[index];
        let inner = theme::RADIUS_LG - BORDER;
        let picture = |image: Arc<RenderImage>, id: String| {
            img(image)
                .id(ElementId::Name(id.into()))
                .absolute()
                .inset_0()
                .size_full()
                .rounded(px(inner))
                .object_fit(ObjectFit::Fill)
        };
        // A paged member: its pages at the width inside the border. The
        // thumbnail is the first page's; the rest are blank sheets until
        // the member's surface draws them.
        let pages = match Self::shape(member) {
            Shape::Pages(sizes) if member.paged.is_some() => Some(
                paged::inline_column(&sizes, (self.column_width() - 2. * BORDER).max(1.)).pages,
            ),
            _ => None,
        };
        let warm = member.surface.is_some() && !member.image;
        // Over a warm member's surface the thumbnail fades out once the
        // surface has drawn.
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
                    // The first page's place: the frame's top, its width.
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
            .inset(px(BORDER))
            .overflow_hidden()
            .rounded(px(inner))
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
        ui::glass_panel(
            ElementId::NamedInteger("group-member".into(), index as u64),
            theme::RADIUS_LG,
            cx,
        )
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
        // A double-click opens the member by itself (`setViewerIndex`).
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
        // A warm member's thumbnail starts fading once its surface has
        // drawn its first frame or page.
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
        // Until the pane has been measured there is no column width to
        // shape the frames by: nothing is drawn, rather than every member
        // squashed into the minimum height.
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
                        this.viewport = bounds.size;
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
        // `.surface`: the app background under the column.
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
                    // A right-click that no member took is between them:
                    // the collection's own menu.
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
                            // `.empty`: a collection with no members says so.
                            .when(empty, |column| {
                                column.child(
                                    div()
                                        .w_full()
                                        .flex()
                                        .justify_center()
                                        .text(theme::TEXT_LG)
                                        .text_color(theme::theme(cx).text_secondary())
                                        .child("This group is empty."),
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
        // Two letter pages at 502 inside a 1px border: 649.6 each, 8 apart.
        let places = layout(&[Shape::Pages(vec![(612., 792.), (612., 792.)])], 504.);
        let height = 2. * 502. * 792. / 612. + 8. + 2.;
        assert!((places[0].1 - height).abs() < 0.01, "{places:?}");
        // Before it is measured, the first page alone has the thumbnail's
        // shape, which the measured first page keeps.
        let one = layout(&[Shape::Pages(vec![(2550., 3300.)])], 504.);
        assert!((one[0].1 - (502. * 3300. / 2550. + 2.)).abs() < 0.01);
    }

    #[test]
    fn members_partly_in_view_count() {
        let places = vec![(16., 250.), (282., 2000.), (2298., 180.)];
        assert_eq!(in_view(&places, 0., 300.), vec![0, 1]);
        assert_eq!(in_view(&places, 300., 300.), vec![1]);
        assert_eq!(in_view(&places, 2200., 400.), vec![1, 2]);
        // Touching the edge is not in view.
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
        // Numbers in names compare as numbers; case is ignored, ties by id.
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
