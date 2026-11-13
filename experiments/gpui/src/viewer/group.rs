//! A collection's members in one continuous column (`GroupSurface.tsx` in
//! its reader mode), as the media view, Quick Look and the detail window
//! show a collection. Each member sits in a glass frame of its own shape,
//! the column's full width, one under the other; the column scrolls.
//!
//! A member shows its thumbnail first. Once it has been in view for 200 ms
//! its full media is asked for: an image's original fades in over the
//! thumbnail, and any other member opens in a media surface of its own (the
//! PDF viewer, the media player, ...). The last 100 members asked for stay
//! loaded (`useWarmGroupMediaBudget`).
use super::Media;
use super::document::kind::{RendererKind, renderer_kind};
use super::surface::MediaSurface;
use crate::session::Session;
use crate::{motion, theme, ui};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use picto_library::{RootDetails, RootId};
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

/// Where each member's frame lies in the column: its top, from the top of
/// the scrolled content, and its height, for a column `width` wide. A frame
/// takes the member's shape at that width, but is never under 180px tall.
pub fn layout(shapes: &[Option<(f32, f32)>], width: f32) -> Vec<(f32, f32)> {
    let mut top = SPACE;
    shapes
        .iter()
        .map(|shape| {
            let height = shape
                .filter(|(w, h)| *w > 0. && *h > 0.)
                .map_or(0., |(w, h)| width * h / w)
                .max(MIN_HEIGHT);
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
            let image = renderer_kind(&media.mime) == RendererKind::Image;
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
            self.members.push(Member {
                media,
                image,
                thumbnail: None,
                full: None,
                reveal: motion::Fade::default(),
                surface: None,
                tasks: vec![task],
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

    fn places(&self) -> Vec<(f32, f32)> {
        let shapes: Vec<_> = self
            .members
            .iter()
            .map(|member| member.media.natural)
            .collect();
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
                surface.set_options(options, cx);
                media.open(&mut surface, cx);
                surface
            });
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
        let content = div()
            .absolute()
            .inset(px(BORDER))
            .overflow_hidden()
            .rounded(px(inner))
            .when_some(
                member
                    .thumbnail
                    .clone()
                    .filter(|_| member.image || member.surface.is_none()),
                |content, thumbnail| {
                    content.child(picture(
                        thumbnail,
                        format!("group-thumb-{}", member.media.key),
                    ))
                },
            )
            .when_some(member.full.clone(), |content, full| {
                content.child(
                    picture(full, format!("group-full-{}", member.media.key))
                        .opacity(member.reveal.value_with(motion::EASE)),
                )
            })
            .when_some(member.surface.clone(), |content, surface| {
                content.child(div().absolute().inset_0().child(surface))
            });
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
        // TODO: a double-click opens the member alone in the media view,
        // "Back to group" returning here (`setViewerIndex`).
        .child(content)
        .into_any_element()
    }
}

impl Render for GroupReader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let animating = self.members.iter().any(|member| member.reveal.animating());
        if animating {
            window.request_animation_frame();
        }
        self.watch(cx);
        let places = self.places();
        let frames: Vec<AnyElement> = places
            .iter()
            .enumerate()
            .map(|(index, (_, height))| self.frame(index, *height, cx))
            .collect();
        let entity = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, _, cx| {
                let _ = entity.update(cx, |this, cx| {
                    if this.viewport != bounds.size {
                        this.viewport = bounds.size;
                        cx.notify();
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
                    // TODO: a right-click between members opens the
                    // collection's menu: Sort Contents (name, size) and
                    // Ungroup... (`openEmptyMenu`).
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(SPACE))
                            .pt(px(SPACE))
                            .pb(px(SPACE))
                            .pl(px(SPACE))
                            .pr(px(SPACE + ui::scrollbar::GUTTER))
                            .children(frames),
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
    use super::{in_view, layout, retain_warm};

    #[test]
    fn frames_take_the_member_shape_at_the_column_width() {
        let places = layout(&[Some((1000., 500.)), Some((100., 400.)), None], 500.);
        assert_eq!(places, vec![(16., 250.), (282., 2000.), (2298., 180.)]);
    }

    #[test]
    fn a_frame_is_never_under_180_tall() {
        assert_eq!(layout(&[Some((4000., 100.))], 553.), vec![(16., 180.)]);
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
}
