//! The media surface (`MediaSurface`): the part of a viewer that shows the
//! open item (the media area of production's `MediaView.tsx`). The media
//! view, Quick Look and the detail window each have one, and the collection
//! reader has one for each member that is not a still picture. A view that
//! has a surface is called its host. The surface fills the rectangle its
//! host lays it out in, which is called the pane.
//!
//! [`document::kind::renderer_kind`] decides from the item's media type how
//! the item is shown:
//!
//! - A picture, including a JPEG XL picture, is drawn with a [`Camera`],
//!   which holds its zoom and pan (production's `useImageZoom.ts`). Its
//!   thumbnail is drawn first, and its original is drawn in place of the
//!   thumbnail once it has been decoded (production's
//!   `ImageCrossfadeFrame`). A picture that has several frames, such as a
//!   GIF, is played as an animation. While the picture is larger than the
//!   pane, the navigator is shown: a square of 120 px in the pane's
//!   bottom-right corner that shows the whole picture and marks the part
//!   that is on screen.
//! - A Flash movie is played by Ruffle. Its frames are fitted to the pane.
//! - Video and audio play in the media player (`video/`).
//! - A document, a font, and a file of a type that cannot be shown get a
//!   [`Document`].
//!
//! When another item is opened, what was on screen stays there until the
//! new item has something of its own to show, and for at most 1,500 ms
//! ([`motion::MEDIA_HOLD`]).
//!
//! This file holds the surface's state, the zoom and the pan, the navigator,
//! and `MediaSurface::build`, which builds one frame of the surface.
//! `load.rs` opens an item and loads its thumbnail and its original.
mod load;

use super::camera::{self, Camera, Glide, Size};
use super::document::{self, Document, kind::RendererKind};
use super::scaled;
use crate::{motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::{path::PathBuf, sync::Arc, time::Instant};

/// The length in pixels of each side of the navigator, which is a square
/// (production's `NAV_SIZE`).
const NAVIGATOR: f32 = 120.;
/// The distance in pixels between the navigator and the pane's right edge,
/// and between the navigator and the pane's bottom edge.
const NAVIGATOR_INSET: f32 = 12.;

/// The events the surface emits. The viewer toolbar and the collection
/// reader subscribe to them.
pub enum Event {
    /// The zoom may have changed. It is emitted when the camera's scale
    /// changed, when a document was opened, when the open document's zoom
    /// changed, and when the item that is opened is shown in another way
    /// than the item before it (for example a video after a picture). The
    /// viewer toolbar then reads the zoom percentage again.
    Zoom,
    /// The item that is opening has loaded more. It is emitted when the
    /// item's thumbnail has been decoded or has failed to decode, when a
    /// picture's original has been decoded, when a Flash movie's first
    /// frame has arrived, when a paged document has become ready to show or
    /// has drawn its first page, and when a video's first frame has
    /// arrived. The collection reader then asks [`MediaSurface::settled`]
    /// again.
    Progressed,
}

/// What was on screen when another item was opened. The surface draws it
/// over the item that is opening until that item has something of its own
/// to show, and for at most 1,500 ms ([`motion::MEDIA_HOLD`]). The purpose
/// is that the first thing drawn in its place is a video's first frame or
/// a document's first page.
enum Held {
    /// A picture and the rectangle it was drawn in. `rect` is the
    /// rectangle's left edge, top edge, width and height, in pixels from
    /// the pane's top-left corner. `pixelated` is true when the picture was
    /// drawn with nearest-neighbour sampling (see [`Options::pixelated`]).
    Image {
        image: Arc<RenderImage>,
        rect: (f32, f32, f32, f32),
        pixelated: bool,
    },
    /// A document, with the view that shows it.
    Document(Document),
    /// A media player, which the surface paused when it took it. A video's
    /// player keeps drawing the frame it was paused on.
    Player(Entity<super::video::VideoPlayer>),
}

/// How a host wants its surface to show media. `navigator`,
/// `follows_resize` and `member` are the host's own choices. The other
/// options are copied from the preview settings ([`super::options`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// Whether the navigator is shown while a picture is larger than the
    /// pane. Quick Look and the collection reader do not show it.
    pub navigator: bool,
    /// True when the setting `imageDefaultZoom` is `actual`. A picture then
    /// opens at 100%. When it is false, a picture opens fitted to the pane.
    pub opens_actual: bool,
    /// The setting `viewerTrackpadGestures`. When it is on, on macOS, a
    /// scroll over a picture pans the picture, and the picture is zoomed
    /// with a pinch or with a scroll while Control is held. On Windows and
    /// Linux the option has no effect, and a scroll zooms the picture.
    pub scroll_pans: bool,
    /// When this is true and the pane changes size, the camera is changed
    /// with [`Camera::resized`]: a picture that was fitted to the pane
    /// stays fitted, and a zoomed picture keeps showing the same part. The
    /// detail window and the collection reader set it.
    pub follows_resize: bool,
    /// True for the surface of a collection's member in the collection
    /// reader (production's `GroupDeferredRenderer`). A video or audio
    /// member does not start to play by itself, does not loop, and starts
    /// with its sound on (a video that is opened elsewhere starts muted).
    /// A paged document's pages are laid out for the collection reader.
    pub member: bool,
    /// The setting `showTransparencyGrid`. When it is on, a checkerboard of
    /// white and light grey squares is drawn behind a picture, and it can
    /// be seen through the picture's transparent parts.
    pub transparency_grid: bool,
    /// True when the setting `imageRendering` is `pixelated`. A picture's
    /// thumbnail and its original are then drawn with nearest-neighbour
    /// sampling, which keeps the edges of enlarged image pixels sharp. The
    /// picture in the navigator is always drawn smoothed.
    pub pixelated: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            navigator: true,
            opens_actual: false,
            scroll_pans: false,
            follows_resize: false,
            member: false,
            transparency_grid: false,
            pixelated: false,
        }
    }
}

/// How the open item is shown. `open` in `load.rs` chooses it from the
/// item's media type.
#[derive(Clone, Copy, PartialEq, Default)]
enum Mode {
    /// A picture. It is drawn where the camera places it, and it can be
    /// zoomed and panned.
    #[default]
    Image,
    /// A Flash movie. Ruffle draws its frames, and each frame is drawn
    /// fitted to the pane.
    Flash,
    /// A [`Document`]: a paged document, a text document, a font, or the
    /// view for a file that cannot be shown. Every media type that no other
    /// mode shows gets this mode (in production these are the documents,
    /// the fonts and the unsupported view of `DetailMediaRenderer`).
    Document,
    /// Video or audio, which plays in the media player.
    Media,
}

/// A drag in the navigator, from a press of the left button in the
/// navigator until the button is released. Both fields are points of the
/// picture, given as fractions of the picture's width and height.
#[derive(Clone, Copy)]
struct NavigatorDrag {
    /// The point of the picture that is at the centre of the pane now. In
    /// each frame it moves 30% of the remaining distance towards `target`
    /// ([`camera::follow`]).
    current: (f32, f32),
    /// The point of the picture that is under the pointer in the
    /// navigator.
    target: (f32, f32),
}

#[derive(Default)]
pub struct MediaSurface {
    /// The key of the item that `show` or `open` was last called for. The
    /// hosts use the content hash of the item's file as the key.
    key: String,
    /// The width and height of the item `key` in image pixels, as the
    /// library records them. It is `None` when the library has no size for
    /// the item.
    natural: Option<Size>,
    /// How the item `key` is shown.
    mode: Mode,
    /// The key of the item whose picture is on screen. While the thumbnail
    /// of a new item is being decoded, this is still the item before it.
    /// `present_key` sets it to `key` when the thumbnail or the original of
    /// `key` has been decoded.
    shown: String,
    /// The width and height of the item `shown` in image pixels, as the
    /// library records them.
    shown_natural: Option<Size>,
    /// The thumbnail that was decoded last. When another item is shown, the
    /// thumbnail of the item before it stays here until the new one has
    /// been decoded.
    thumbnail: Option<Arc<RenderImage>>,
    /// The decoded original of the item `full_key`: a picture's original
    /// file, or the frame that a Flash movie drew last. A large JPEG may be
    /// here at a reduced size (see `whole`). When the thumbnail of another
    /// item has been decoded, the original of the item before it is removed
    /// from here.
    full: Option<Arc<RenderImage>>,
    /// The key of the item whose original was last decoded into `full`. It
    /// is also set when that decode failed; `full` is then `None`.
    full_key: String,
    /// The task that decodes the thumbnail of the item `key`. Showing
    /// another item replaces it, which cancels it.
    thumbnail_task: Option<Task<()>>,
    /// The task that decodes the original of the item `key`. Showing
    /// another item cancels it.
    full_task: Option<Task<()>>,
    /// The timer that `open` starts. When it ends, after 200 ms
    /// ([`motion::ORIGINAL_DELAY`]), the surface starts to load what the
    /// item needs beyond its thumbnail: the original picture, the media
    /// player, the document or the Flash movie. Showing another item
    /// cancels it.
    full_wait: Option<Task<()>>,
    /// True from the moment `open` starts that timer until the timer ends
    /// and the player, the document or the Flash movie is made. It is false
    /// again when the surface is stopped or shows another item.
    starting: bool,
    /// The Flash movie that is open. It is `None` for any other item, and
    /// during the 200 ms before the movie is loaded.
    flash: Option<super::flash::Flash>,
    /// The task that copies the Flash movie's newest frame into `full`
    /// every 16 ms ([`motion::FLASH_FRAME`]).
    flash_task: Option<Task<()>>,
    /// The opacity of the Flash movie's controls, which fade in and out
    /// over 300 ms (production's `useMediaControlsVisibility`).
    flash_controls: motion::Fade,
    /// The number of times the Flash movie's controls have been shown. Each
    /// time starts a timer that hides them 2,500 ms later. A timer hides
    /// them only if this number is still the one it started with, so only
    /// the timer that was started last hides them.
    flash_reveal: u64,
    /// The open document. It is `None` for any other item, and during the
    /// 200 ms before the document is opened.
    document: Option<Document>,
    /// The subscription that calls `document_zoom_changed` when the zoom of
    /// the open document changes. Only a paged document has one.
    _document_zoom: Option<Subscription>,
    /// The subscription that emits [`Event::Progressed`] when the open
    /// paged document has become ready to show or has drawn its first
    /// page.
    _document_progress: Option<Subscription>,
    /// The subscription that emits [`Event::Progressed`] when the media
    /// player has received a video's first frame.
    _player_progress: Option<Subscription>,
    /// What was on screen before the item that is opening. It is drawn over
    /// that item until `release_hold` removes it.
    held: Option<Held>,
    /// When `held` was set. `release_hold` removes `held` at the latest
    /// 1,500 ms after this moment.
    held_since: Option<Instant>,
    /// True from the moment `open` opens an item until `rest` is called. A
    /// host calls `rest` when it closes and when it shows a collection in
    /// place of an item. Only while this is true does `capture` keep what
    /// is on screen to draw it over the next item.
    live: bool,
    /// What the open document last reported about its zoom. The first value
    /// is true when the toolbar's zoom controls apply to the document,
    /// which is the case for a PDF only. The second value is the document's
    /// zoom in percent.
    document_zoom: (bool, f32),
    /// The item's name. The font viewer shows it on its page.
    display_name: SharedString,
    /// The media player of the open video or audio item. It is `None` for
    /// any other item, and during the 200 ms before the player is made.
    player: Option<Entity<super::video::VideoPlayer>>,
    /// The key of the item that `open` last opened. When `open` is called
    /// again for this item, it does nothing, unless the item is shown with
    /// a media player, a document or a Flash movie and the surface has none
    /// at that moment.
    opened: String,
    /// The reason a Flash movie or a JPEG XL picture could not be shown. It
    /// is drawn in a bar along the pane's bottom edge.
    error: Option<String>,
    /// The pane's position and size in window coordinates, in the frame
    /// that was built last.
    bounds: Bounds<Pixels>,
    /// The surface's focus handle, which `build` makes in the first frame.
    /// The surface's element tracks it, so a click on the surface gives the
    /// surface the keyboard focus.
    focus: Option<FocusHandle>,
    /// The zoom and the pan of the picture on screen.
    camera: Camera,
    /// Set by `refit`, which is called when another picture is shown and
    /// when a host opens. The next frame in which the pane's size and the
    /// picture's size are known gives the picture the camera it opens with,
    /// and clears this. The picture is not drawn while this is set.
    needs_fit: bool,
    /// The animation of the camera that is in progress. A zoom step
    /// (`step_zoom`) and one notch of the mouse wheel each start one, and
    /// it lasts 150 ms.
    glide: Option<Glide>,
    /// The scale that the last notch of the mouse wheel zooms to, and the
    /// moment that notch arrived. A notch that arrives within 200 ms
    /// ([`camera::NOTCH_RUN`]) of that moment zooms from this scale, and
    /// not from the scale the animation has reached.
    notches: Option<(f32, Instant)>,
    /// The pan in progress, from a press of the left button on a picture
    /// until the button is released. It holds the pointer's position at the
    /// press, in window coordinates, and the camera at the press.
    drag: Option<(Point<Pixels>, Camera)>,
    /// The drag in the navigator that is in progress.
    navigator_drag: Option<NavigatorDrag>,
    /// The moment the picture became larger than the pane. The navigator is
    /// shown once 80 ms ([`motion::NAVIGATOR_DELAY`]) have passed since
    /// then. It is `None` while the whole picture fits in the pane and
    /// while the host shows no navigator.
    overflowing_since: Option<Instant>,
    /// How the host wants media shown.
    options: Options,
    /// The subscription that builds `options` again from the preview
    /// settings each time the settings change. Only a surface that was made
    /// with `for_host` has one.
    _settings: Option<Subscription>,
    /// Pictures that another picture has replaced since the last frame.
    /// `release_retired` removes each of them from GPUI's sprite atlas once
    /// the surface no longer draws it.
    retired: Vec<Arc<RenderImage>>,
    /// Set while `full` holds a JPEG that was decoded at a reduced size. It
    /// is the path of the original file. `decode_whole_when_needed` decodes
    /// that file at its full size when the picture is drawn larger than the
    /// reduced decode, and clears this.
    whole: Option<PathBuf>,
    /// The window's scale factor in the frame that was built last: the
    /// number of screen pixels that one pixel of the layout takes, which is
    /// 2 on a high-density display.
    scale_factor: f32,
}

impl EventEmitter<Event> for MediaSurface {}

impl MediaSurface {
    /// Makes a surface with a host's options. It also registers what is
    /// done when the surface is released: every picture the surface still
    /// holds (the thumbnail, the original, a held picture and the retired
    /// pictures) is removed from GPUI's sprite atlas.
    pub fn new(options: Options, cx: &mut Context<Self>) -> Self {
        cx.on_release(|this, cx| {
            let held = match this.held.take() {
                Some(Held::Image { image, .. }) => Some(image),
                _ => None,
            };
            let images = this.retired.drain(..);
            for image in images.chain(
                [this.thumbnail.take(), this.full.take(), held]
                    .into_iter()
                    .flatten(),
            ) {
                crate::atlas::release(image, cx);
            }
        })
        .detach();
        Self {
            options,
            ..Self::default()
        }
    }

    /// Makes the surface of a host that follows the preview settings
    /// (production's `usePreviewPreferences`). [`super::options`] builds
    /// the options from the settings and from the host's two own choices,
    /// `navigator` and `follows_resize`. The options are built again each
    /// time the saved settings change.
    pub fn for_host(navigator: bool, follows_resize: bool, cx: &mut Context<Self>) -> Self {
        let mut surface = Self::new(super::options(cx, navigator, follows_resize), cx);
        surface._settings = Some(cx.subscribe(
            &crate::settings::model(cx),
            move |this, _, _: &crate::settings::Applied, cx| {
                let options = super::options(cx, navigator, follows_resize);
                this.set_options(options, cx);
            },
        ));
        surface
    }

    /// Returns the key context that a host gives its own element. It holds
    /// `base`, which is the host's name for its context. It also holds
    /// "Media" while the surface has a media player, so that the player's
    /// keys work. When the host passes true for `pages`, it also holds
    /// "Pages" ([`document::PAGES_CONTEXT`]) while a paged document is
    /// open, so that Home, End, Page Up, Page Down and Space scroll the
    /// document's pages.
    pub fn key_context(&self, base: &'static str, pages: bool) -> KeyContext {
        let mut context = super::video::key_context(base, self.player.is_some());
        if pages && self.shows_pages() {
            context.add(document::PAGES_CONTEXT);
        }
        context
    }

    /// Records a picture that another picture has just replaced, so that
    /// `release_retired` removes it from the sprite atlas once the surface
    /// no longer draws it. With `None` it does nothing.
    fn retire(&mut self, image: Option<Arc<RenderImage>>) {
        self.retired.extend(image);
    }

    /// Removes from the sprite atlas of every window each retired picture
    /// that the surface no longer draws. A retired picture that is still
    /// the thumbnail, the original or the held picture stays in `retired`
    /// for a later frame. `build` calls this in each frame.
    fn release_retired(&mut self, cx: &mut App) {
        let held = match &self.held {
            Some(Held::Image { image, .. }) => Some(image),
            _ => None,
        };
        let shown = [self.thumbnail.as_ref(), self.full.as_ref(), held];
        let (kept, released): (Vec<_>, Vec<_>) = self.retired.drain(..).partition(|image| {
            shown
                .iter()
                .flatten()
                .any(|shown| Arc::ptr_eq(shown, image))
        });
        self.retired = kept;
        for image in released {
            crate::atlas::release(image, cx);
        }
    }

    /// Drops the Flash movie, the document and the media player of the
    /// open item, with their tasks and subscriptions, and what is held from
    /// the item before. A movie, a video or audio that was playing stops.
    /// The thumbnail and the decoded original are kept. The collection
    /// reader calls this for a member's surface before it drops that
    /// surface.
    pub fn stop(&mut self) {
        self.flash = None;
        self.flash_task = None;
        self.document = None;
        self._document_zoom = None;
        self._document_progress = None;
        self._player_progress = None;
        self.player = None;
        self.held = None;
        self.held_since = None;
        // A start that is still waiting is cancelled, so that no player is
        // made for a surface that has been stopped.
        self.full_wait = None;
        self.starting = false;
    }

    /// Does what `stop` does, and records that no item is on screen
    /// (`live` becomes false). A host calls this when it closes and when it
    /// shows a collection in place of an item. The item's thumbnail and its
    /// decoded original stay loaded, so a picture that is opened again is
    /// shown from them immediately. Because no item is on screen, nothing
    /// of this item is drawn over the next item that is opened.
    pub fn rest(&mut self) {
        self.stop();
        self.live = false;
    }

    /// Returns true when the item `key` has something of its own to draw,
    /// so that what is held from the item before can be removed.
    ///
    /// - A picture or a Flash movie is ready when its thumbnail has been
    ///   decoded or has failed to decode, or its original has been decoded.
    /// - A document is ready when [`Document::presentable`] returns true. A
    ///   paged document is presentable when its pages have been measured
    ///   and laid out and its first page has a thumbnail or a drawing, or
    ///   when it cannot be shown. Every other document is presentable at
    ///   once.
    /// - Video or audio is ready when its player's `presentable` returns
    ///   true. For audio that is at once. For a video it is when the first
    ///   frame has arrived, when the video cannot be played, or when
    ///   1,500 ms have passed since the player was made.
    ///
    /// During the 200 ms that `open` waits before it makes a document or a
    /// player, none exists yet. The item is then ready when its thumbnail
    /// has been decoded or has failed to decode, because the thumbnail is
    /// what the surface draws during that wait. A video is also ready from
    /// then on while its player has no frame yet, because the thumbnail
    /// stays drawn under the player until the first frame arrives.
    fn ready(&self, cx: &App) -> bool {
        let thumbnail_shown = self.shown == self.key;
        match self.mode {
            Mode::Image | Mode::Flash => thumbnail_shown,
            Mode::Document => match &self.document {
                Some(document) => document.presentable(cx),
                None => thumbnail_shown,
            },
            Mode::Media => match &self.player {
                Some(player) => thumbnail_shown || player.read(cx).presentable(),
                None => thumbnail_shown,
            },
        }
    }

    /// Returns true when the item `key` has drawn the first thing it shows
    /// in full, or cannot be shown. The collection reader draws a member's
    /// thumbnail over the member's surface until this returns true.
    ///
    /// - It is true when the item has an error message.
    /// - For a picture, it is true when the decode of its original has
    ///   ended.
    /// - For a Flash movie, it is true when the movie's first frame has
    ///   arrived.
    /// - For a document, it is true when [`Document::settled`] returns
    ///   true: for a paged document when the first page in view has been
    ///   drawn or the document cannot be shown, and for every other
    ///   document at once.
    /// - For video or audio, it is true when the player has a video frame
    ///   or its `presentable` returns true.
    ///
    /// During the 200 ms that `open` waits before it makes a document or a
    /// player, it is false (`starting`). When no document or player exists
    /// and none is about to be made, it is true.
    pub fn settled(&self, cx: &App) -> bool {
        if self.error.is_some() {
            return true;
        }
        match self.mode {
            Mode::Image => self.full_key == self.key,
            Mode::Flash => self.full.is_some() && self.full_key == self.key,
            Mode::Document => match &self.document {
                Some(document) => document.settled(cx),
                None => !self.starting,
            },
            Mode::Media => match &self.player {
                Some(player) => {
                    let player = player.read(cx);
                    player.has_picture() || player.presentable()
                }
                None => !self.starting,
            },
        }
    }

    /// Takes what is on screen now, so that `show` can keep it and draw it
    /// over the item that is about to open. Returns `None` when there is
    /// nothing to keep.
    ///
    /// - When something is already held, the item that was asked for last
    ///   has not become ready, and what the user sees is the held item. The
    ///   held item is returned, so that it is kept for the new item too.
    /// - When no item is on screen (`live` is false), nothing is returned.
    ///   The surface then still holds the pictures of the item that was
    ///   open before the host closed, or the thumbnail of the grid's
    ///   selection, and neither of them may be drawn over the item that
    ///   opens.
    /// - A picture is returned with the rectangle the camera draws it in
    ///   now.
    /// - A Flash movie's last frame, or its thumbnail when no frame has
    ///   arrived, is returned with the rectangle it has when it is fitted
    ///   to the pane.
    /// - A document is moved out of the surface and returned.
    /// - A media player is moved out of the surface, paused and returned.
    fn capture(&mut self, cx: &mut Context<Self>) -> Option<Held> {
        if self.held.is_some() {
            // Something is still held, so what the user sees is the held
            // item and not the item that was asked for last.
            return self.held.take();
        }
        if !self.live {
            return None;
        }
        let pane = self.pane();
        match self.mode {
            Mode::Image => {
                let image = self
                    .image_size()
                    .filter(|_| pane.0 > 0. && !self.needs_fit)?;
                let picture = self
                    .full
                    .clone()
                    .filter(|_| self.full_key == self.shown)
                    .or_else(|| self.thumbnail.clone())?;
                Some(Held::Image {
                    image: picture,
                    rect: self.camera.image_rect(image, pane),
                    pixelated: self.options.pixelated,
                })
            }
            Mode::Flash => {
                let picture = self.full.clone().or_else(|| self.thumbnail.clone())?;
                let size = picture.size(0);
                let natural = (size.width.0 as f32, size.height.0 as f32);
                Some(Held::Image {
                    image: picture,
                    rect: camera::contain(natural, pane)?,
                    pixelated: false,
                })
            }
            Mode::Document => self.document.take().map(Held::Document),
            Mode::Media => self.player.take().map(|player| {
                // A player that is shown over its whole window stops being
                // shown that way at the next frame, because its item is
                // being replaced.
                super::video::release_fullscreen(&player, cx);
                player.update(cx, |player, _| player.pause());
                Held::Player(player)
            }),
        }
    }

    /// Removes what is held from the item before when the item that is
    /// opening is ready ([`Self::ready`]), or when 1,500 ms
    /// ([`motion::MEDIA_HOLD`]) have passed since it was held. `build`
    /// calls this in each frame.
    fn release_hold(&mut self, cx: &mut Context<Self>) {
        let expired = self
            .held_since
            .is_some_and(|since| since.elapsed() >= motion::MEDIA_HOLD);
        if self.held.is_some() && (expired || self.ready(cx)) {
            self.held = None;
            self.held_since = None;
        }
    }

    /// Returns the media player of the open video or audio item. Returns
    /// `None` for any other item, and while the player has not been made
    /// yet.
    pub fn player(&self) -> Option<Entity<super::video::VideoPlayer>> {
        self.player.clone()
    }

    /// Returns true when the viewer toolbar's zoom controls apply to the
    /// open item. That is the case for a picture and for a PDF. It is not
    /// the case for a Flash movie, for video and audio, and for any other
    /// document.
    pub fn zoomable(&self) -> bool {
        match self.mode {
            Mode::Image => true,
            Mode::Document => self.document_zoom.0,
            _ => false,
        }
    }

    /// Returns true when the picture on screen can be made the item's
    /// thumbnail with the menu command "Set as Thumbnail". That is the case
    /// when a Flash movie is open, and when the media player plays a video
    /// and not audio.
    pub fn can_capture(&self, cx: &App) -> bool {
        self.flash.is_some()
            || self
                .player
                .as_ref()
                .is_some_and(|player| player.read(cx).is_video())
    }

    /// Returns the picture that "Set as Thumbnail" saves. For a Flash movie
    /// it is the frame the movie drew last, or an error when no frame has
    /// arrived yet. For a video it is the frame the player shows, or an
    /// error when the player has no frame yet. Returns `None` when the open
    /// item is neither a Flash movie nor a video.
    pub fn current_picture(&self, cx: &App) -> Option<Result<super::capture::Picture, String>> {
        if self.flash.is_some() {
            return Some(match &self.full {
                Some(image) => super::capture::Picture::from_image(image),
                None => Err("The Flash frame is not ready yet.".into()),
            });
        }
        self.player.as_ref()?.read(cx).capture_frame()
    }

    /// Returns two values for the open Flash movie: whether it is playing,
    /// and whether it is muted. Returns `None` when no Flash movie is open.
    pub fn flash_playback(&self) -> Option<(bool, bool)> {
        self.flash
            .as_ref()
            .map(|flash| (flash.playback.playing, flash.playback.muted))
    }

    /// Replaces the host's options. When they differ from the options
    /// before, it asks for the surface to be drawn again.
    pub fn set_options(&mut self, options: Options, cx: &mut Context<Self>) {
        if self.options != options {
            self.options = options;
            cx.notify();
        }
    }

    /// Returns true when the open item is shown as a picture and the size
    /// of the picture on screen is known. Only then does the camera's zoom
    /// percentage describe something on screen.
    pub fn has_image(&self) -> bool {
        self.mode == Mode::Image && self.image_size().is_some()
    }

    /// Returns true when a host shows its zoom controls for the open item:
    /// the zoom percentage and the buttons "Actual size" and "Fit to
    /// window". That is the case for a picture once its size is known, and
    /// for a PDF, whose viewer does the zooming itself. The detail window
    /// uses it (production's `supportsZoom` and `usesRendererZoom` in
    /// `DetailWindow.tsx`).
    pub fn shows_zoom(&self) -> bool {
        match self.mode {
            Mode::Image => self.has_image(),
            Mode::Document => self.document_zoom.0,
            _ => false,
        }
    }

    /// Returns the camera that a picture opens with: at 100% when the
    /// option `opens_actual` is set, and fitted to the pane otherwise.
    /// `image` is the picture's size in image pixels and `pane` is the
    /// pane's size in pixels.
    fn opening(&self, image: Size, pane: Size) -> Camera {
        if self.options.opens_actual {
            Camera::actual()
        } else {
            Camera::fit(image, pane)
        }
    }

    /// Returns the zoom in percent, as a toolbar shows it. For a document
    /// it is the zoom the open document last reported. For any other item
    /// it is the camera's scale as a percentage, rounded to a whole number.
    pub fn percent(&self) -> f32 {
        match self.mode {
            Mode::Document => self.document_zoom.1,
            _ => self.camera.percent(),
        }
    }

    /// Copies the open document's zoom into `document_zoom` and emits
    /// [`Event::Zoom`], so that the viewer toolbar shows the new
    /// percentage. It is called when a document has been opened and each
    /// time a paged document's zoom changes.
    fn document_zoom_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = &self.document {
            self.document_zoom = (document.zoomable(cx), document.percent(cx));
        }
        cx.emit(Event::Zoom);
        // The detail window draws the zoom percentage in its own toolbar.
        // It observes the surface, so this makes it draw the percentage
        // again.
        cx.notify();
    }

    /// Runs `control` on the open Flash movie, for example to pause it, to
    /// stop it or to mute it, and shows the movie's controls again
    /// (production's `revealControls`). The buttons of the movie's controls
    /// and the Flash rows of the context menu call it. When no Flash movie
    /// is open, it does nothing.
    pub fn control_flash(
        &mut self,
        control: Box<dyn Fn(&mut super::flash::Flash)>,
        cx: &mut Context<Self>,
    ) {
        if let Some(flash) = &mut self.flash {
            control(flash);
        }
        self.reveal_flash_controls(cx);
    }

    /// Shows the Flash movie's controls, which fade in over 300 ms
    /// ([`motion::MEDIA_CONTROLS_FADE`]), and starts a timer that hides
    /// them 2,500 ms later (`CONTROLS_HIDE_DELAY`, the delay the media
    /// player's controls also use). It is called when the movie is opened,
    /// for each move of the pointer over the pane, and after each use of a
    /// control. Each call starts a new timer, and only the timer of the
    /// last call hides the controls, so they stay shown until 2,500 ms
    /// have passed without a call. When no Flash movie is open, it does
    /// nothing.
    fn reveal_flash_controls(&mut self, cx: &mut Context<Self>) {
        if self.flash.is_none() {
            return;
        }
        // The surface is asked to be drawn again only when the controls
        // were hidden. When they are already shown, a move of the pointer
        // only starts a new timer and does not draw the surface again.
        let hidden = !self.flash_controls.on();
        self.flash_controls.set(true, motion::MEDIA_CONTROLS_FADE);
        self.flash_reveal += 1;
        let reveal = self.flash_reveal;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(super::video::model::CONTROLS_HIDE_DELAY)
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.flash_reveal == reveal {
                    this.flash_controls.set(false, motion::MEDIA_CONTROLS_FADE);
                    cx.notify();
                }
            });
        })
        .detach();
        if hidden {
            cx.notify();
        }
    }

    /// Sets the name of the item that is about to be opened. A host calls
    /// it before `open`. The font viewer shows the name on its page.
    pub fn set_display_name(&mut self, name: SharedString) {
        self.display_name = name;
    }

    /// Returns the open document. Returns `None` when the open item is not
    /// shown as a document, and while the document has not been opened
    /// yet.
    fn document(&self) -> Option<&Document> {
        self.document
            .as_ref()
            .filter(|_| self.mode == Mode::Document)
    }

    /// Goes to the previous page of the open document. Its key is J unless
    /// the user has changed the shortcut `document.previousPage`. A paged
    /// document scrolls to the top of the page that is at the top of the
    /// view, or to the top of the page before it when it is already there.
    /// A text document or an EPUB loads the page before the one it shows.
    /// For any other item it does nothing.
    pub fn previous_page(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.previous_page(cx);
        }
    }

    /// Goes to the next page of the open document. Its key is L unless the
    /// user has changed the shortcut `document.nextPage`. A paged document
    /// scrolls to the top of the next page. A text document or an EPUB
    /// loads the page after the one it shows. For any other item it does
    /// nothing.
    pub fn next_page(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.next_page(cx);
        }
    }

    /// Scrolls the pages of the open paged document for one of the keys
    /// Home, End, Page Up, Page Down and Space. `scroll` says where to: to
    /// the start, to the end, or up or down by 87.5% of the view's height
    /// (and by at least 40 px). Space scrolls down and Shift+Space scrolls
    /// up. For any other item it does nothing.
    pub fn scroll_pages(&mut self, scroll: document::Scroll, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.scroll(scroll, cx);
        }
    }

    /// Returns true when the open item is a paged document: a PDF, a DjVu
    /// file or a comic archive. The keys Home, End, Page Up, Page Down and
    /// Space then scroll its pages.
    pub fn shows_pages(&self) -> bool {
        self.document().is_some_and(Document::paged)
    }

    /// Returns true when the Actual Size shortcut applies to the open item
    /// itself. The shortcut's default key is Cmd+0 on macOS and Ctrl+0 on
    /// Windows and Linux. It applies to a picture, a paged document and a
    /// font preview. When this returns false, the surface does not handle
    /// the key, and the key goes on to the window, where it resets the
    /// zoom of the interface. Production's `MediaView` does the same: it
    /// leaves the key to `AppShell` when the open item has no zoom.
    pub fn has_actual_size(&self) -> bool {
        match self.mode {
            Mode::Image => true,
            Mode::Document => matches!(
                self.document(),
                Some(Document::Paged(_) | Document::Font(_))
            ),
            Mode::Flash | Mode::Media => false,
        }
    }

    /// Returns true when the keys that change the size of a font preview
    /// apply to the open item, which is the case when a font is open. The
    /// keys are Cmd+= and Cmd++ for larger and Cmd+− for smaller, with
    /// Ctrl in place of Cmd on Windows and Linux. On macOS the same keys
    /// with Ctrl also work, and Ctrl+0 sets the preview back to its own
    /// size. Production's `FontViewer.tsx` handles these keys itself. When
    /// this returns false, the surface does not handle the keys, and they
    /// go on to the window, where they zoom the interface.
    pub fn has_preview_size(&self) -> bool {
        matches!(self.document(), Some(Document::Font(_)))
    }

    /// Makes the open font preview larger (`larger` is true) or smaller by
    /// one step of 10%. For any other item it does nothing.
    pub fn preview_size(&mut self, larger: bool, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.preview_size(larger, cx);
        }
    }

    /// Returns the pane's width and height in pixels, in the frame that
    /// was built last.
    fn pane(&self) -> Size {
        (
            f32::from(self.bounds.size.width),
            f32::from(self.bounds.size.height),
        )
    }

    /// Returns the width and the height, in image pixels, of the picture on
    /// screen. It is the size the library records for the item `shown`.
    /// When the library has none, it is the size of the decoded original,
    /// if `full` holds the original of the item `shown`, and otherwise the
    /// size of the decoded thumbnail. Returns `None` when none of them is
    /// known, and when the size has no width or no height.
    fn image_size(&self) -> Option<Size> {
        let decoded = |image: &Arc<RenderImage>| {
            let size = image.size(0);
            (size.width.0 as f32, size.height.0 as f32)
        };
        self.shown_natural
            .or_else(|| {
                self.full
                    .as_ref()
                    .filter(|_| self.full_key == self.shown)
                    .map(decoded)
            })
            .or_else(|| self.thumbnail.as_ref().map(decoded))
            .filter(|size| size.0 > 0. && size.1 > 0.)
    }

    /// Moves the camera to `camera` and emits [`Event::Zoom`] when the
    /// scale changed. It does not ask for the surface to be drawn again:
    /// `build` calls it while it builds a frame, and that frame already
    /// draws the picture with the new camera. A handler of a pointer or key
    /// event calls [`Self::set_camera`] instead.
    fn place_camera(&mut self, camera: Camera, cx: &mut Context<Self>) {
        let rescaled = camera.scale != self.camera.scale;
        self.camera = camera;
        if rescaled {
            cx.emit(Event::Zoom);
        }
    }

    /// Moves the camera to `camera`, emits [`Event::Zoom`] when the scale
    /// changed, and asks for the surface to be drawn again.
    fn set_camera(&mut self, camera: Camera, cx: &mut Context<Self>) {
        self.place_camera(camera, cx);
        cx.notify();
    }

    /// Makes the next frame give the picture on screen the camera a picture
    /// opens with ([`Self::opening`]): fitted to the pane, or at 100% when
    /// the setting says so. It also ends the zoom animation, the pan and
    /// the drag in the navigator that are in progress. It is called when
    /// another picture is shown, and a host calls it when it opens.
    pub fn refit(&mut self) {
        self.needs_fit = true;
        self.glide = None;
        self.drag = None;
        self.navigator_drag = None;
    }

    /// Fits the open item to the pane (production's `fitToWindow`). A paged
    /// document fits its page to its view, and any other document does
    /// nothing. Otherwise, when the size of the picture on screen is known,
    /// the camera is set so that the whole picture is shown, centred in the
    /// pane, and the zoom animation in progress ends.
    pub fn fit(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.fit(cx);
            return;
        }
        if let Some(image) = self.image_size() {
            self.glide = None;
            let fitted = Camera::fit(image, self.pane());
            self.set_camera(fitted, cx);
        }
    }

    /// Shows the open item at its actual size (production's `fitActual`). A
    /// paged document shows its pages at 100%, a font preview goes back to
    /// its own size, and any other document does nothing. Otherwise the
    /// camera is set to 100%, at which one image pixel takes one pixel of
    /// the layout, with the picture centred in the pane, and the zoom
    /// animation in progress ends.
    pub fn actual_size(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.actual_size(cx);
            return;
        }
        self.glide = None;
        self.set_camera(Camera::actual(), cx);
    }

    /// Sets the zoom to `percent`. The zoom slider and the zoom menu of the
    /// viewer toolbar call it. A paged document sets its own zoom, and any
    /// other document does nothing. Otherwise the camera's scale becomes
    /// `percent` / 100, limited to the range from 5% to 800%. The camera's
    /// offset is not changed, and the zoom animation in progress ends.
    pub fn set_percent(&mut self, percent: f32, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.set_percent(percent, cx);
            return;
        }
        self.glide = None;
        let camera = self.camera.zoomed(percent / 100., None);
        self.set_camera(camera, cx);
    }

    /// Zooms in (`zoom_in` is true) or out by one step. A paged document
    /// changes its own zoom, and any other document does nothing. For a
    /// picture, the scale is multiplied or divided by 1.25
    /// ([`camera::STEP`]), and the camera moves to the new scale in an
    /// animation of 150 ms. The part of the picture at the pane's centre
    /// stays there. When a zoom animation is already in progress, the step
    /// starts from the scale that animation is moving to, so each of
    /// several quick steps counts in full. For a Flash movie, video and
    /// audio it does nothing.
    pub fn step_zoom(&mut self, zoom_in: bool, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.step_zoom(zoom_in, cx);
            return;
        }
        if self.mode != Mode::Image {
            return;
        }
        let base = self.glide.map_or(self.camera, |glide| glide.to);
        let factor = if zoom_in {
            camera::STEP
        } else {
            1. / camera::STEP
        };
        self.glide_to(base.zoomed_about_centre(base.scale * factor), cx);
    }

    /// Starts an animation of the camera from where it is now to `to`. The
    /// animation lasts 150 ms ([`motion::ZOOM_GLIDE`]). `animate` moves
    /// the camera along it in each frame.
    fn glide_to(&mut self, to: Camera, cx: &mut Context<Self>) {
        self.glide = Some(Glide {
            from: self.camera,
            to,
            started: Instant::now(),
        });
        cx.notify();
    }

    /// Handles a scroll of the mouse wheel or of the trackpad over a
    /// picture. When the open item is not a picture, or the picture's size
    /// is not known, it does nothing.
    ///
    /// When the option `scroll_pans` is set, on macOS, and Control is not
    /// held, the scroll pans the picture by the distance scrolled.
    ///
    /// Otherwise the scroll zooms the picture, and the point of the picture
    /// under the pointer stays under the pointer. Scrolling down zooms out.
    /// One notch of a mouse wheel multiplies or divides the scale by 1.25
    /// in an animation of 150 ms. A scroll of the trackpad changes the
    /// scale at once, by a factor that depends on the distance scrolled
    /// ([`camera::wheel_factor`]).
    fn scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        if self.mode != Mode::Image || self.image_size().is_none() {
            return;
        }
        let centre = self.bounds.center();
        let focal = (
            f32::from(event.position.x - centre.x),
            f32::from(event.position.y - centre.y),
        );
        if self.options.scroll_pans && cfg!(target_os = "macos") && !event.modifiers.control {
            // The picture moves by the distance scrolled. When the distance
            // is given in lines, each line counts as 40 px
            // (`camera::NOTCH`).
            let (dx, dy) = match event.delta {
                ScrollDelta::Lines(lines) => (lines.x * camera::NOTCH, lines.y * camera::NOTCH),
                ScrollDelta::Pixels(pixels) => (f32::from(pixels.x), f32::from(pixels.y)),
            };
            self.glide = None;
            let camera = self.camera.panned(dx, dy);
            self.set_camera(camera, cx);
            cx.stop_propagation();
            return;
        }
        // `delta` is given the sign of a browser's `deltaY`, which
        // production reads: positive when the user scrolls down. GPUI's
        // delta is negative then, so its sign is reversed. A distance in
        // lines counts as a notch of a mouse wheel. A distance in pixels
        // counts as a notch when it is at least 40 px (`camera::NOTCH`),
        // and as a scroll of the trackpad when it is smaller.
        let (notch, delta) = match event.delta {
            ScrollDelta::Lines(lines) => (true, -lines.y),
            ScrollDelta::Pixels(pixels) => {
                let delta = -f32::from(pixels.y);
                (delta.abs() >= camera::NOTCH, delta)
            }
        };
        if delta == 0. {
            return;
        }
        if notch {
            // One notch multiplies or divides the scale by 1.25, and the
            // camera moves to the new scale in an animation. A notch that
            // arrives within 200 ms (`camera::NOTCH_RUN`) of the notch
            // before it starts from the scale that notch was zooming to,
            // so each of several quick notches adds a full step.
            let base = match self.notches {
                Some((target, at)) if at.elapsed() < camera::NOTCH_RUN => target,
                _ => self.camera.scale,
            };
            let target = (if delta > 0. {
                base / camera::STEP
            } else {
                base * camera::STEP
            })
            .clamp(camera::MIN_SCALE, camera::MAX_SCALE);
            self.notches = Some((target, Instant::now()));
            let to = self.camera.zoomed(target, Some(focal));
            self.glide_to(to, cx);
        } else {
            self.glide = None;
            let camera = self
                .camera
                .zoomed(self.camera.scale * camera::wheel_factor(delta), Some(focal));
            self.set_camera(camera, cx);
        }
        cx.stop_propagation();
    }

    /// Handles a pinch on the trackpad over a picture. It zooms the
    /// picture, and the point of the picture at the centre of the pinch
    /// stays where it is. `event.delta` is the change of zoom that the
    /// event reports, for example 0.1 for 10% larger. Production receives a
    /// pinch from Chromium as a wheel event whose `deltaY` is
    /// −100 × ln(1 + delta). The same `deltaY` is computed here and is
    /// handled as a scroll of the trackpad is, so the scale is multiplied
    /// by (1 + delta) to the power 0.4. When the open item is not a
    /// picture, or the picture's size is not known, it does nothing.
    fn pinch(&mut self, event: &PinchEvent, cx: &mut Context<Self>) {
        if self.mode != Mode::Image || self.image_size().is_none() {
            return;
        }
        let centre = self.bounds.center();
        let focal = (
            f32::from(event.position.x - centre.x),
            f32::from(event.position.y - centre.y),
        );
        let delta = -100. * (1. + event.delta).max(0.01).ln();
        self.glide = None;
        let camera = self
            .camera
            .zoomed(self.camera.scale * camera::wheel_factor(delta), Some(focal));
        self.set_camera(camera, cx);
    }

    /// Returns the navigator's square in window coordinates. Its sides are
    /// 120 px long, and it is 12 px from the pane's right edge and 12 px
    /// from the pane's bottom edge.
    fn navigator_bounds(&self) -> Bounds<Pixels> {
        let corner = self.bounds.bottom_right();
        Bounds::new(
            point(
                corner.x - px(NAVIGATOR_INSET + NAVIGATOR),
                corner.y - px(NAVIGATOR_INSET + NAVIGATOR),
            ),
            size(px(NAVIGATOR), px(NAVIGATOR)),
        )
    }

    /// Returns the point of the picture that the navigator shows at
    /// `position`, which is a position in window coordinates. The point is
    /// given as fractions of the picture's width and height, each limited
    /// to the range from 0 to 1. Returns `None` when the picture's size is
    /// not known.
    fn navigator_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let image = self.image_size()?;
        let origin = self.navigator_bounds().origin;
        Some(camera::navigator_point(
            (
                f32::from(position.x - origin.x),
                f32::from(position.y - origin.y),
            ),
            image,
            NAVIGATOR,
        ))
    }

    /// Moves the camera, without changing its scale, so that `point` of the
    /// picture is at the centre of the pane. `point` is given as fractions
    /// of the picture's width and height.
    fn centre_on(&mut self, point: (f32, f32), cx: &mut Context<Self>) {
        if let Some(image) = self.image_size() {
            let camera = self.camera.centred_on(point.0, point.1, image);
            self.set_camera(camera, cx);
        }
    }

    /// Moves the camera by one frame of the animations in progress. Returns
    /// true when an animation has not ended, so that `build` requests
    /// another frame. A zoom animation (`glide`) places the camera where
    /// the animation is at this moment, and ends 150 ms after it started.
    /// A drag in the navigator moves the point of the picture at the
    /// pane's centre 30% of the remaining distance towards the point under
    /// the pointer, until it has reached that point.
    fn animate(&mut self, cx: &mut Context<Self>) -> bool {
        let mut more = false;
        if let Some(glide) = self.glide {
            let (camera, done) = glide.at(glide.started.elapsed());
            if done {
                self.glide = None;
            }
            more |= !done;
            self.place_camera(camera, cx);
        }
        if let Some(mut drag) = self.navigator_drag
            && drag.current != drag.target
        {
            drag.current = camera::follow(drag.current, drag.target);
            self.navigator_drag = Some(drag);
            if let Some(image) = self.image_size() {
                let camera = self
                    .camera
                    .centred_on(drag.current.0, drag.current.1, image);
                self.place_camera(camera, cx);
            }
            more = true;
        }
        more
    }

    /// Returns the elements that draw the picture, in the order in which
    /// they are drawn. Each of them fills the rectangle in which the
    /// camera places the picture. The first is the checkerboard, when the
    /// option `transparency_grid` is set. The second is the original, when
    /// `full` holds the original of the item on screen, and the thumbnail
    /// otherwise. `image` is the picture's size in image pixels.
    fn image_layers(&self, image: Size) -> Vec<AnyElement> {
        let (left, top, width, height) = self.camera.image_rect(image, self.pane());
        let key = self.shown.clone();
        let pixelated = self.options.pixelated;
        let place = |source: Arc<RenderImage>| {
            // GPUI plays a picture that has several frames (a GIF or an
            // animated WebP) only when its element has an id, because it
            // keeps the number of the frame it shows under that id from
            // one draw to the next.
            let id = ElementId::Name(format!("viewer-image-{key}").into());
            img(source)
                .id(id)
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(width))
                .h(px(height))
                .object_fit(ObjectFit::Fill)
                .pixelated(pixelated)
                .into_any_element()
        };
        let full = self.full.clone().filter(|_| self.full_key == self.shown);
        let mut layers = Vec::new();
        if self.options.transparency_grid {
            let scale = self.camera.scale;
            layers.push(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| paint_checkerboard(bounds, scale, window),
                )
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(width))
                .h(px(height))
                .into_any_element(),
            );
        }
        match full {
            Some(full) => layers.push(place(full)),
            None => layers.extend(self.thumbnail.clone().map(place)),
        }
        layers
    }

    /// Builds the navigator (production's `.navigator`): a square of
    /// 120 px, 12 px from the pane's right and bottom edges, with a 1 px
    /// border and corners rounded by 6 px. It shows the item's thumbnail,
    /// or the original when there is no thumbnail, fitted into the square.
    /// A frame with a 1 px border marks the part of the picture that is
    /// visible in the pane, and the rest of the square is darkened. A
    /// press of the left button in the navigator moves the camera so that
    /// the point of the picture that was pressed is at the centre of the
    /// pane, and starts a drag in the navigator.
    ///
    /// `part` is the visible part of the picture as its left edge, top
    /// edge, width and height, in fractions of the picture's width and
    /// height. `image` is the picture's size in image pixels. Returns
    /// `None` when neither the thumbnail nor the original has been decoded.
    fn navigator(
        &self,
        part: (f32, f32, f32, f32),
        image: Size,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let source = self.thumbnail.clone().or_else(|| self.full.clone())?;
        let (offset_x, offset_y, shown_width, shown_height) =
            camera::navigator_image(image, NAVIGATOR);
        let frame = (
            offset_x + part.0 * shown_width,
            offset_y + part.1 * shown_height,
            part.2 * shown_width,
            part.3 * shown_height,
        );
        // Production darkens everything around the frame with one shadow
        // (`box-shadow: 0 0 0 999px`). Here the dark area is drawn as four
        // rectangles: one above the frame, one below it, one to its left
        // and one to its right. The area inside the navigator's border is
        // 118 px wide and high (`inner`). A rectangle that reaches a
        // corner of that area has that corner rounded by 5 px, which is
        // the navigator's radius of 6 px less the 1 px border.
        let inner = NAVIGATOR - 2.;
        let (left, top) = (frame.0.max(0.), frame.1.max(0.));
        let (right, bottom) = (
            (frame.0 + frame.2).min(inner),
            (frame.1 + frame.3).min(inner),
        );
        let radius = theme::RADIUS_MD - 1.;
        let band = |x: f32, y: f32, w: f32, h: f32| {
            let mut band = div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w.max(0.)))
                .h(px(h.max(0.)))
                .bg(rgba(theme::NAVIGATOR_SHADE));
            let (at_left, at_top) = (x <= 0., y <= 0.);
            let (at_right, at_bottom) = (x + w >= inner, y + h >= inner);
            if at_left && at_top {
                band = band.rounded_tl(px(radius));
            }
            if at_right && at_top {
                band = band.rounded_tr(px(radius));
            }
            if at_left && at_bottom {
                band = band.rounded_bl(px(radius));
            }
            if at_right && at_bottom {
                band = band.rounded_br(px(radius));
            }
            band
        };
        Some(
            div()
                .id("navigator")
                .occlude()
                .absolute()
                .right(px(NAVIGATOR_INSET))
                .bottom(px(NAVIGATOR_INSET))
                .size(px(NAVIGATOR))
                .overflow_hidden()
                .rounded(px(theme::RADIUS_MD))
                .bg(rgba(theme::NAVIGATOR_BACKGROUND))
                .border_1()
                .border_color(rgba(theme::NAVIGATOR_BORDER))
                .cursor(CursorStyle::Crosshair)
                .child(img(source).size_full().object_fit(ObjectFit::Contain))
                .child(band(0., 0., inner, top))
                .child(band(0., bottom, inner, inner - bottom))
                .child(band(0., top, left, bottom - top))
                .child(band(right, top, inner - right, bottom - top))
                .child(
                    div()
                        .absolute()
                        .left(px(frame.0))
                        .top(px(frame.1))
                        .w(px(frame.2))
                        .h(px(frame.3))
                        .rounded(px(2.))
                        .border_1()
                        .border_color(rgba(theme::NAVIGATOR_VIEWPORT_BORDER))
                        .bg(rgba(theme::NAVIGATOR_VIEWPORT_FILL)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        if let Some(point) = this.navigator_point(event.position) {
                            this.glide = None;
                            this.navigator_drag = Some(NavigatorDrag {
                                current: point,
                                target: point,
                            });
                            this.centre_on(point, cx);
                        }
                    }),
                )
                .into_any_element(),
        )
    }
}

impl Render for MediaSurface {
    /// Where the picture is drawn depends on the size of the pane, so
    /// `render` returns an element that fills the pane and builds the
    /// surface for that element's bounds in the frame that lays it out
    /// (`ui::sized_view`). Every frame is therefore built for the size the
    /// pane has in that frame.
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::ui::sized_view(cx.weak_entity(), Self::build).size_full()
    }
}

impl MediaSurface {
    /// Takes `bounds` as the pane's position and size in window
    /// coordinates. When the size changed and the host's option
    /// `follows_resize` is set, the camera is changed so that the picture
    /// keeps its framing in the pane's new size ([`Camera::resized`]). A
    /// picture that has not yet been given the camera it opens with is left
    /// alone, because `build` gives it that camera for the new size next.
    fn resize(&mut self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        if self.bounds == bounds {
            return;
        }
        let from = self.pane();
        self.bounds = bounds;
        if self.options.follows_resize
            && !self.needs_fit
            && let Some(image) = self.image_size()
        {
            let camera = self.camera.resized(image, from, self.pane());
            self.place_camera(camera, cx);
        }
    }

    /// Builds one frame of the surface for `bounds`, the pane's position
    /// and size in window coordinates in this frame.
    fn build(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let camera_before = self.camera;
        self.resize(bounds, cx);
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let entity = cx.entity().downgrade();
        let image_mode = self.mode == Mode::Image;
        let image = self.image_size().filter(|_| image_mode);
        let pane = self.pane();
        // A picture that has just been shown is given the camera it opens
        // with: fitted to the pane, or at 100% when the setting says so.
        if self.needs_fit
            && pane.0 > 0.
            && let Some(image) = image
        {
            self.needs_fit = false;
            self.glide = None;
            let opening = self.opening(image, pane);
            self.place_camera(opening, cx);
        }
        self.scale_factor = window.scale_factor();
        self.decode_whole_when_needed(image, cx);
        // This frame draws the picture with the camera as it is now. The
        // detail window shows the zoom percentage in its toolbar, which is
        // built before the surface, so it has drawn the percentage from
        // before a change made here. Another frame is requested when the
        // camera changed in this frame, so that the percentage is drawn
        // again, and while a glide or a drag of the navigator is still
        // moving the camera.
        if self.animate(cx) || self.camera != camera_before {
            window.request_animation_frame();
        }
        // The navigator is shown 80 ms (`motion::NAVIGATOR_DELAY`) after
        // the picture has become larger than the pane. `part` is the part
        // of the picture that is visible in the pane. It is `None` when
        // the whole picture fits in the pane, when the host shows no
        // navigator, and when the open item is not a picture. The first
        // frame in which `part` is not `None` records the time and starts
        // a timer that draws the surface again when the 80 ms have passed.
        let part = image
            .filter(|_| self.options.navigator)
            .and_then(|image| camera::visible_part(self.camera, image, pane));
        let navigator = match (part, self.overflowing_since) {
            (None, _) => {
                self.overflowing_since = None;
                None
            }
            (Some(_), None) => {
                self.overflowing_since = Some(Instant::now());
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(motion::NAVIGATOR_DELAY)
                        .await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
                None
            }
            (Some(part), Some(since)) if since.elapsed() >= motion::NAVIGATOR_DELAY => {
                image.and_then(|image| self.navigator(part, image, cx))
            }
            _ => None,
        };

        let dragging = self.drag.is_some() || self.navigator_drag.is_some();
        let listeners = entity;
        let mut surface = div()
            .id("media-surface")
            .track_focus(&focus)
            // While a Flash movie is open, the surface has the key context
            // "Flash". The shortcuts of the viewer and of the detail
            // window are bound so that they do not run while the keyboard
            // focus is in that context, so the keys go to the movie. For
            // any other item the surface adds no key context, and the
            // host's shortcuts keep working after a click has given the
            // surface the keyboard focus.
            .when(self.flash.is_some(), |surface| surface.key_context("Flash"))
            .relative()
            .size_full()
            .overflow_hidden()
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        if !dragging {
                            return;
                        }
                        // While a pan or a drag in the navigator is in
                        // progress, the surface listens for moves of the
                        // pointer and for the release of the button over
                        // the whole window. The drag therefore goes on
                        // when the pointer leaves the pane, and it ends
                        // wherever the button is released.
                        let surface = listeners.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = surface.update(cx, |this, cx| {
                                    if let Some((from, start)) = this.drag {
                                        let delta = event.position - from;
                                        let camera =
                                            start.panned(f32::from(delta.x), f32::from(delta.y));
                                        this.set_camera(camera, cx);
                                    } else if this.navigator_drag.is_some()
                                        && let Some(target) = this.navigator_point(event.position)
                                        && let Some(drag) = &mut this.navigator_drag
                                    {
                                        drag.target = target;
                                        cx.notify();
                                    }
                                });
                            }
                        });
                        let surface = listeners.clone();
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = surface.update(cx, |this, cx| {
                                    // The release ends the pan. It also
                                    // ends a drag in the navigator, and the
                                    // camera then goes straight to the
                                    // point that was under the pointer.
                                    this.drag = None;
                                    if let Some(drag) = this.navigator_drag.take() {
                                        this.centre_on(drag.target, cx);
                                    }
                                    cx.notify();
                                });
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.reveal_flash_controls(cx);
                if let Some(flash) = &this.flash {
                    let (x, y) = this.flash_position(event.position);
                    let _ = flash
                        .input
                        .send(ruffle_core::events::PlayerEvent::MouseMove { x, y });
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    if this.mode == Mode::Image {
                        // A press on a picture starts a pan. The position
                        // of the press and the camera at the press are
                        // recorded, and each move of the pointer pans the
                        // picture from them.
                        this.glide = None;
                        this.drag = Some((event.position, this.camera));
                        cx.notify();
                        return;
                    }
                    // A press on a Flash movie gives the surface the
                    // keyboard focus, so that the movie receives the keys.
                    // The press is passed to the movie at the matching
                    // position on its stage, and it is not passed on to
                    // the host.
                    if let Some(flash) = &this.flash {
                        if let Some(focus) = &this.focus {
                            window.focus(focus, cx);
                        }
                        cx.stop_propagation();
                        let (x, y) = this.flash_position(event.position);
                        let _ = flash
                            .input
                            .send(ruffle_core::events::PlayerEvent::MouseDown {
                                x,
                                y,
                                button: ruffle_core::events::MouseButton::Left,
                                index: None,
                            });
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, _| {
                    if let Some(flash) = &this.flash {
                        let (x, y) = this.flash_position(event.position);
                        let _ = flash.input.send(ruffle_core::events::PlayerEvent::MouseUp {
                            x,
                            y,
                            button: ruffle_core::events::MouseButton::Left,
                        });
                    }
                }),
            )
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| this.scroll(event, cx)))
            .on_pinch(cx.listener(|this, event: &PinchEvent, _, cx| this.pinch(event, cx)));
        if image_mode {
            // Over a picture the cursor is an open hand, and a closed hand
            // while the picture is being panned (the cursors `grab` and
            // `grabbing` of production's `.zoomContainer`).
            surface = surface.cursor(if self.drag.is_some() {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            });
        }
        surface = surface
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                // A key that is pressed is passed to the open Flash movie.
                // Escape is not passed, and neither is a key pressed with
                // Command on macOS or with Control on Windows and Linux.
                // Those are left to the app's own shortcuts.
                if event.keystroke.key == "escape" || event.keystroke.modifiers.secondary(cx) {
                    return;
                }
                if let Some(flash) = &this.flash {
                    cx.stop_propagation();
                    let key = super::flash::key_descriptor(&event.keystroke.key);
                    let logical = key.logical_key;
                    let _ = flash
                        .input
                        .send(ruffle_core::events::PlayerEvent::KeyDown { key });
                    if let Some(codepoint) = logical.character() {
                        let _ = flash
                            .input
                            .send(ruffle_core::events::PlayerEvent::TextInput { codepoint });
                    }
                }
            }))
            .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| {
                if let Some(flash) = &this.flash {
                    cx.stop_propagation();
                    let _ = flash.input.send(ruffle_core::events::PlayerEvent::KeyUp {
                        key: super::flash::key_descriptor(&event.keystroke.key),
                    });
                }
            }));
        if image_mode {
            // The picture is drawn only when the pane's size is known and
            // the picture has been given the camera it opens with. It is
            // therefore never drawn with the camera of the picture that
            // was shown before it.
            if let Some(image) = image.filter(|_| pane.0 > 0. && !self.needs_fit) {
                surface = surface.children(self.image_layers(image));
            }
        } else if let Some(document) = self.document() {
            surface = surface.child(document.element());
        } else if let Some(player) = self.player.clone().filter(|_| self.mode == Mode::Media) {
            // A video's player draws nothing until its first frame has
            // arrived. Until then the item's thumbnail stays drawn under
            // the player, fitted to the pane as it was during the wait, so
            // the pane is never empty between the thumbnail and the first
            // frame. While the player is shown over its whole window, the
            // host draws it there (`video::fullscreen_layer`), and the pane
            // is filled with black in its place.
            if self.shown == self.key
                && !player.read(cx).presentable()
                && let Some(thumbnail) = &self.thumbnail
            {
                surface = surface.child(
                    img(thumbnail.clone())
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                );
            }
            surface = surface.child(if super::video::is_fullscreen(&player, cx) {
                div().absolute().inset_0().bg(rgba(theme::MEDIA_BACKGROUND))
            } else {
                div().absolute().inset_0().child(player)
            });
        } else {
            // This branch draws a Flash movie, and an item whose document
            // or media player has not been made yet. The thumbnail is
            // drawn fitted to the pane. The picture in `full` is drawn
            // over it, also fitted to the pane. For a Flash movie that is
            // the frame the movie drew last.
            if let Some(thumbnail) = &self.thumbnail {
                surface = surface.child(
                    img(thumbnail.clone())
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                );
            }
            if let Some(full) = &self.full {
                surface = surface.child(
                    img(full.clone())
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                );
            }
        }
        surface = surface.children(navigator);
        self.release_hold(cx);
        self.release_retired(cx);
        // What is still held from the item before is drawn over the new
        // item and over the navigator: a picture in the rectangle it was
        // drawn in before, and a document or a media player over the whole
        // pane.
        if let Some(held) = &self.held {
            let layer = div().absolute().inset_0();
            surface = surface.child(match held {
                Held::Image {
                    image,
                    rect,
                    pixelated,
                } => layer.child(
                    img(image.clone())
                        .absolute()
                        .left(px(rect.0))
                        .top(px(rect.1))
                        .w(px(rect.2))
                        .h(px(rect.3))
                        .object_fit(ObjectFit::Fill)
                        .pixelated(*pixelated),
                ),
                Held::Document(document) => layer.child(document.element()),
                Held::Player(player) => layer.child(player.clone()),
            });
        }
        if let Some(error) = &self.error {
            surface = surface.child(
                div()
                    .absolute()
                    .bottom_0()
                    .w_full()
                    .p_3()
                    .bg(crate::theme::theme(cx).glass())
                    .text_size(px(12.))
                    .child(error.clone()),
            );
        }
        // The Flash movie's controls are part of the frame only while they
        // are shown or are still fading out. Controls that are hidden are
        // left out of the frame, so that they receive no clicks.
        if let Some(flash) = &self.flash
            && (self.flash_controls.on() || self.flash_controls.animating())
        {
            if self.flash_controls.animating() {
                window.request_animation_frame();
            }
            surface = surface.child(super::flash_controls::controls(
                cx.entity().downgrade(),
                &flash.playback,
                // The buttons are disabled until the movie's first frame
                // has arrived, because there is nothing to control before
                // that (production's `disabled={!controller}`).
                self.full.is_some() && self.full_key == self.key,
                self.flash_controls.value_with(motion::EASE),
                self.flash_controls.on(),
            ));
        }
        surface.into_any_element()
    }
}

/// Registers on `element` the handlers of the actions that every host
/// passes on to its surface, and returns the element. The actions are Fit
/// to Window, Actual Size, Zoom In and Zoom Out, the previous and the next
/// page of a document, the three actions that change the size of a font
/// preview, the four actions that scroll a paged document's pages, and the
/// media player's actions. A host calls this for its own element, the one
/// that has the host's key context. The shortcuts' actions are dispatched
/// to the element that has the keyboard focus and then to the elements
/// that contain it, so they reach these handlers while the focus is on the
/// host's element or on an element inside it, such as the surface.
pub fn host_actions<E: InteractiveElement>(element: E, surface: &Entity<MediaSurface>) -> E {
    use super::{ActualSize, FitToWindow, ZoomIn, ZoomOut};
    use document::{Scroll, *};
    type Run = fn(&mut MediaSurface, &mut Context<MediaSurface>);
    /// Registers a handler on `element` that runs `run` on the surface when
    /// the action `A` is dispatched.
    fn forward<A: Action, E: InteractiveElement>(
        element: E,
        surface: &Entity<MediaSurface>,
        run: Run,
    ) -> E {
        let surface = surface.downgrade();
        element.on_action(move |_: &A, _, cx| {
            let _ = surface.update(cx, run);
        })
    }
    let element = forward::<FitToWindow, _>(element, surface, |this, cx| this.fit(cx));
    /// Does what `forward` does, but runs `run` only when `applies` returns
    /// true for the surface. When `applies` returns false, or the surface
    /// no longer exists, the action is passed on, so that the next binding
    /// of the same key runs. For Actual Size and for the keys of the font
    /// preview, the next binding is the zoom of the interface.
    fn forward_if<A: Action, E: InteractiveElement>(
        element: E,
        surface: &Entity<MediaSurface>,
        applies: fn(&MediaSurface) -> bool,
        run: Run,
    ) -> E {
        let surface = surface.downgrade();
        element.on_action(move |_: &A, _, cx| {
            let handled = surface
                .update(cx, |this, cx| {
                    let applies = applies(this);
                    if applies {
                        run(this, cx);
                    }
                    applies
                })
                .unwrap_or(false);
            if !handled {
                cx.propagate();
            }
        })
    }
    let element = forward_if::<ActualSize, _>(
        element,
        surface,
        MediaSurface::has_actual_size,
        |this, cx| this.actual_size(cx),
    );
    let element = forward::<ZoomIn, _>(element, surface, |this, cx| this.step_zoom(true, cx));
    let element = forward::<ZoomOut, _>(element, surface, |this, cx| this.step_zoom(false, cx));
    let element = forward::<PreviousPage, _>(element, surface, |this, cx| this.previous_page(cx));
    let element = forward::<NextPage, _>(element, surface, |this, cx| this.next_page(cx));
    let element = forward_if::<PreviewLarger, _>(
        element,
        surface,
        MediaSurface::has_preview_size,
        |this, cx| this.preview_size(true, cx),
    );
    let element = forward_if::<PreviewReset, _>(
        element,
        surface,
        MediaSurface::has_preview_size,
        |this, cx| this.actual_size(cx),
    );
    let element = forward_if::<PreviewSmaller, _>(
        element,
        surface,
        MediaSurface::has_preview_size,
        |this, cx| this.preview_size(false, cx),
    );
    let element = forward::<ScrollToStart, _>(element, surface, |this, cx| {
        this.scroll_pages(Scroll::Start, cx)
    });
    let element = forward::<ScrollToEnd, _>(element, surface, |this, cx| {
        this.scroll_pages(Scroll::End, cx)
    });
    let element = forward::<ScrollPageUp, _>(element, surface, |this, cx| {
        this.scroll_pages(Scroll::PageUp, cx)
    });
    let element = forward::<ScrollPageDown, _>(element, surface, |this, cx| {
        this.scroll_pages(Scroll::PageDown, cx)
    });
    let surface = surface.downgrade();
    super::video::on_actions(element, move |cx| {
        surface
            .upgrade()
            .and_then(|surface| surface.read(cx).player())
    })
}

impl MediaSurface {
    /// Converts `position`, which is in window coordinates, to a position
    /// on the Flash movie's stage, in the stage's own pixels from its
    /// top-left corner. The stage is 960 × 720 px
    /// ([`super::flash::STAGE`]) and is drawn fitted to the pane and
    /// centred in it.
    fn flash_position(&self, position: Point<Pixels>) -> (f64, f64) {
        let stage = (super::flash::STAGE.0 as f32, super::flash::STAGE.1 as f32);
        let (left, top, width, _) =
            camera::contain(stage, self.pane()).unwrap_or((0., 0., stage.0, stage.1));
        let scale = width / stage.0;
        let local = position - self.bounds.origin;
        (
            ((f32::from(local.x) - left) / scale) as f64,
            ((f32::from(local.y) - top) / scale) as f64,
        )
    }
}

/// Paints the checkerboard that is drawn behind a picture (production's
/// `.transparencyGrid`). `bounds` is the picture's rectangle in window
/// coordinates and `scale` is the camera's scale. White and light grey
/// squares alternate. Each square is 8 image pixels wide, which is
/// 8 × `scale` px on screen. Only the part of `bounds` that is inside the
/// area the window is drawing is painted. When that part would hold more
/// than 40,000 squares, no squares are painted, and the part is filled
/// with the one colour that is halfway between the white and the grey.
fn paint_checkerboard(bounds: Bounds<Pixels>, scale: f32, window: &mut Window) {
    let visible = window.content_mask().bounds.intersect(&bounds);
    if visible.size.width <= px(0.) || visible.size.height <= px(0.) {
        return;
    }
    let rect = |bounds: Bounds<Pixels>| {
        (
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        )
    };
    let side = 8. * scale;
    let (_, _, width, height) = rect(visible);
    if width * height / (side * side) > 40_000. {
        let light = rgb(theme::TRANSPARENCY_GRID_LIGHT);
        let dark = rgb(theme::TRANSPARENCY_GRID_DARK);
        let blend = Rgba {
            r: (light.r + dark.r) / 2.,
            g: (light.g + dark.g) / 2.,
            b: (light.b + dark.b) / 2.,
            a: 1.,
        };
        window.paint_quad(fill(visible, blend));
        return;
    }
    window.paint_quad(fill(visible, rgb(theme::TRANSPARENCY_GRID_LIGHT)));
    for (x, y, w, h) in camera::checker_squares(rect(bounds), rect(visible), side) {
        window.paint_quad(fill(
            Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
            rgb(theme::TRANSPARENCY_GRID_DARK),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaSurface, Options};
    use gpui::{AnyWindowHandle, AppContext as _, RenderImage, TestAppContext};
    use std::sync::Arc;

    /// Opens a 4000 × 3000 JPEG (12 megapixels) fifty times, each time
    /// under a different key, and calls `rest` after each opening. This is
    /// what happens when the viewer opens and closes fifty items. At the
    /// end, the thumbnail and the original that were decoded for the last
    /// opening must still be in GPUI's sprite atlas, and the pictures of
    /// the 49 openings before it must not be. A decoded original of this
    /// size takes about 48 MB. If the originals stayed in the atlas, the
    /// app would hold another 48 MB for every item the user has viewed.
    #[test]
    fn originals_opened_and_closed_leave_the_sprite_atlas() {
        let folder = tempfile::tempdir().unwrap();
        let photo = folder.path().join("photo.jpg");
        image::RgbImage::from_fn(4000, 3000, |x, y| {
            image::Rgb([x as u8, y as u8, (x ^ y) as u8])
        })
        .save(&photo)
        .unwrap();
        let thumbnail = folder.path().join("thumbnail.jpg");
        image::RgbImage::from_fn(400, 300, |x, _| image::Rgb([x as u8, 90, 200]))
            .save(&thumbnail)
            .unwrap();
        let mut cx = TestAppContext::single();
        let window = cx.add_window(|_, cx| MediaSurface::new(Options::default(), cx));
        let handle: AnyWindowHandle = window.into();
        let draw = |cx: &mut TestAppContext| {
            cx.executor().advance_clock(crate::motion::ORIGINAL_DELAY);
            cx.run_until_parked();
            cx.update_window(handle, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })
            .unwrap();
            cx.run_until_parked();
        };
        let mut shown: Vec<Arc<RenderImage>> = Vec::new();
        for item in 0..50 {
            let key = format!("item-{item}");
            window
                .update(&mut cx, |surface, _, cx| {
                    let natural = Some((4000., 3000.));
                    let (thumbnail, photo) = (thumbnail.clone(), photo.clone());
                    surface.open(key.clone(), thumbnail, natural, photo, "image/jpeg", cx)
                })
                .unwrap();
            draw(&mut cx);
            let pictures = window
                .read_with(&cx, |surface, _| {
                    assert_eq!(surface.full_key, key);
                    [surface.thumbnail.clone(), surface.full.clone()]
                })
                .unwrap();
            shown.extend(
                pictures
                    .into_iter()
                    .map(|picture| picture.expect("decoded")),
            );
            // The host closes. `rest` keeps the item's pictures loaded, in
            // case the same item is opened again.
            window
                .update(&mut cx, |surface, _, _| surface.rest())
                .unwrap();
            draw(&mut cx);
        }
        let in_atlas: Vec<bool> = cx
            .update_window(handle, |_, window, _| {
                shown
                    .iter()
                    .map(|image| window.has_image_atlas_entry(image))
                    .collect()
            })
            .unwrap();
        let (earlier, last) = in_atlas.split_at(in_atlas.len() - 2);
        assert_eq!(last, [true, true]);
        assert!(earlier.iter().all(|kept| !kept), "{in_atlas:?}");
    }

    /// An 8000 × 6000 JPEG that is shown fitted to the pane, at less than
    /// half its own size, is decoded at half its size (4000 × 3000). When
    /// the view then goes to 100% with `actual_size`, the JPEG is decoded
    /// again at its full size.
    #[test]
    fn a_large_jpeg_decodes_at_the_size_shown_until_the_view_needs_more() {
        let folder = tempfile::tempdir().unwrap();
        let photo = folder.path().join("photo.jpg");
        image::RgbImage::from_fn(8000, 6000, |x, y| image::Rgb([x as u8, y as u8, 7]))
            .save(&photo)
            .unwrap();
        let mut cx = TestAppContext::single();
        let window = cx.add_window(|_, cx| MediaSurface::new(Options::default(), cx));
        let handle: AnyWindowHandle = window.into();
        let draw = |cx: &mut TestAppContext| {
            for _ in 0..2 {
                cx.executor().advance_clock(crate::motion::ORIGINAL_DELAY);
                cx.run_until_parked();
                cx.update_window(handle, |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear(cx);
                })
                .unwrap();
            }
            cx.run_until_parked();
        };
        // A frame is drawn before the item is opened, so that the surface
        // knows the pane's size and the window's scale factor, as it does
        // in the app once a viewer has been shown. The test window is
        // 1920 × 1080 with a scale factor of 2. The photo fitted to it is
        // 1440 × 1080, which is 2880 × 2160 screen pixels. That is less
        // than half of 8000 × 6000, so the JPEG is decoded at half its
        // size, which is the smallest reduced size that covers
        // 2880 × 2160.
        draw(&mut cx);
        let decoded = |cx: &mut TestAppContext| {
            window
                .read_with(cx, |surface, _| {
                    let size = surface.full.as_ref().unwrap().size(0);
                    (size.width.0, size.height.0)
                })
                .unwrap()
        };
        window
            .update(&mut cx, |surface, _, cx| {
                surface.open(
                    "photo".into(),
                    photo.clone(),
                    Some((8000., 6000.)),
                    photo.clone(),
                    "image/jpeg",
                    cx,
                )
            })
            .unwrap();
        draw(&mut cx);
        let reduced = decoded(&mut cx);
        assert_eq!(reduced, (4000, 3000));
        window
            .update(&mut cx, |surface, _, cx| surface.actual_size(cx))
            .unwrap();
        draw(&mut cx);
        assert_eq!(decoded(&mut cx), (8000, 6000));
    }
}
