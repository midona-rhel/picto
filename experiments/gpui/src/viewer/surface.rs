//! The media area of the viewer (`MediaView.tsx`): full-bleed, the previous
//! item kept until the next one's thumbnail has decoded, the original laid
//! over the thumbnail once decoded (`ImageCrossfadeFrame`). Images zoom and
//! pan (`useImageZoom.ts`) and show the navigator while they overflow, and
//! animated ones play; Flash plays through Ruffle and fills the pane; video
//! and audio play in the media player (`video/`). What each item gets is
//! decided by [`document::kind::renderer_kind`]: JPEG XL decodes onto the
//! image camera, and documents, fonts and unviewable types show a
//! [`Document`].
use super::camera::{self, Camera, Glide, Size};
use super::document::{self, Document, kind::RendererKind};
use crate::{motion, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::{path::PathBuf, sync::Arc, time::Instant};

/// `NAV_SIZE`: the navigator's side.
const NAVIGATOR: f32 = 120.;
/// The navigator's inset from the pane's bottom-right corner.
const NAVIGATOR_INSET: f32 = 12.;

/// What the surface tells the viewer toolbar and the collection reader.
pub enum Event {
    /// The zoom, or whether the shown item zooms at all, changed.
    Zoom,
    /// The item opening got further: it can be shown, or it has drawn its
    /// first picture or page ([`MediaSurface::settled`]).
    Progressed,
}

/// What was on screen before the item now opening, kept over it until the
/// new item has something of its own to show, so a video's first frame or
/// a document's first page is the first thing drawn in its place.
enum Held {
    /// A picture at its place in the pane: left, top, width, height, and
    /// whether it was drawn pixelated.
    Image {
        image: Arc<RenderImage>,
        rect: (f32, f32, f32, f32),
        pixelated: bool,
    },
    Document(Document),
    /// A video, paused, its last picture still drawn.
    Player(Entity<super::video::VideoPlayer>),
}

/// How a host shows media: what the media view, Quick Look and the detail
/// window each draw, and the preview settings they all follow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// The navigator square while the image overflows. Quick Look has none.
    pub navigator: bool,
    /// `imageDefaultZoom` `actual`: a new image opens at 100%, not fitted.
    pub opens_actual: bool,
    /// `viewerTrackpadGestures` on macOS: scrolling pans and only a pinch
    /// zooms.
    pub scroll_pans: bool,
    /// The detail window's: a resized pane keeps the image's framing.
    pub follows_resize: bool,
    /// A collection member in the reader (`GroupDeferredRenderer`): video
    /// and audio wait to be played, do not loop and start with sound.
    pub member: bool,
    /// `showTransparencyGrid`: a checkerboard behind images, which shows
    /// through their transparent parts.
    pub transparency_grid: bool,
    /// `imageRendering: pixelated`: images (the thumbnail and the original,
    /// not the navigator) sampled nearest-neighbour.
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

#[derive(Clone, Copy, PartialEq, Default)]
enum Mode {
    /// Zoomed and panned on the camera.
    #[default]
    Image,
    /// Ruffle's frames, fitted to the pane.
    Flash,
    /// A document viewer of its own (`DetailMediaRenderer`'s documents,
    /// fonts and the unsupported view).
    Document,
    /// Video and audio, in the media player.
    Media,
    /// No renderer yet: the thumbnail, fitted, with a note.
    Other,
}

/// A drag of the navigator: where the view is and where it is heading, as
/// fractions of the image.
#[derive(Clone, Copy)]
struct NavigatorDrag {
    current: (f32, f32),
    target: (f32, f32),
}

#[derive(Default)]
pub struct MediaSurface {
    /// The item asked for, and its size in image pixels from the library.
    key: String,
    natural: Option<Size>,
    mode: Mode,
    /// The item whose image is on screen, and its size. It trails `key`
    /// until the new thumbnail has decoded.
    shown: String,
    shown_natural: Option<Size>,
    thumbnail: Option<Arc<RenderImage>>,
    full: Option<Arc<RenderImage>>,
    full_key: String,
    thumbnail_task: Option<Task<()>>,
    full_task: Option<Task<()>>,
    flash: Option<super::flash::Flash>,
    flash_task: Option<Task<()>>,
    /// Flash's controls (`useMediaControlsVisibility`): their opacity, and a
    /// count that lets only the latest reveal's timer hide them.
    flash_controls: motion::Fade,
    flash_reveal: u64,
    document: Option<Document>,
    _document_zoom: Option<Subscription>,
    _document_progress: Option<Subscription>,
    _player_progress: Option<Subscription>,
    /// The previous item's picture, over the opening one until it is ready.
    held: Option<Held>,
    held_since: Option<Instant>,
    /// Whether an opened item is on screen, rather than the surface resting
    /// while its host is closed or shows a collection; only then is the
    /// item held over the next.
    live: bool,
    /// Whether the open document takes the toolbar's zoom, and its zoom in
    /// percent, as it last reported them.
    document_zoom: (bool, f32),
    /// The item's name, which the font viewer shows as its title.
    display_name: SharedString,
    player: Option<Entity<super::video::VideoPlayer>>,
    opened: String,
    error: Option<String>,
    bounds: Bounds<Pixels>,
    focus: Option<FocusHandle>,
    camera: Camera,
    /// Set when a new image is shown, until it has been fitted to the pane.
    needs_fit: bool,
    glide: Option<Glide>,
    /// Where a run of wheel notches is heading, and when the last one came.
    notches: Option<(f32, Instant)>,
    /// A pan in progress: where the pointer went down and the camera then.
    drag: Option<(Point<Pixels>, Camera)>,
    navigator_drag: Option<NavigatorDrag>,
    /// Since when the image has overflowed the pane, for the navigator's
    /// short delay before it appears.
    overflowing_since: Option<Instant>,
    options: Options,
}

impl EventEmitter<Event> for MediaSurface {}

impl MediaSurface {
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
    }

    /// Stops what plays while the host shows no item (it closed, or shows a
    /// collection's reader). The item stays loaded, so the same one opens
    /// again at once, but nothing of it is held over the next item.
    pub fn rest(&mut self) {
        self.stop();
        self.live = false;
    }

    /// Whether the item asked for has something of its own to show: an
    /// image or Flash movie its thumbnail, a video its first frame, a paged
    /// document its measured pages with the first one's thumbnail.
    fn ready(&self, cx: &App) -> bool {
        match self.mode {
            Mode::Image | Mode::Flash | Mode::Other => self.shown == self.key,
            Mode::Document => self
                .document
                .as_ref()
                .is_none_or(|document| document.presentable(cx)),
            Mode::Media => self
                .player
                .as_ref()
                .is_none_or(|player| player.read(cx).presentable()),
        }
    }

    /// Whether the item has drawn what it opened to: a video its first
    /// frame, a document its first page, a Flash movie its first frame. The
    /// collection reader shows a member's thumbnail until then.
    pub fn settled(&self, cx: &App) -> bool {
        if self.error.is_some() {
            return true;
        }
        match self.mode {
            Mode::Image => self.full_key == self.key,
            Mode::Flash => self.full.is_some() && self.full_key == self.key,
            Mode::Other => true,
            Mode::Document => self
                .document
                .as_ref()
                .is_none_or(|document| document.settled(cx)),
            Mode::Media => self.player.as_ref().is_none_or(|player| {
                let player = player.read(cx);
                player.has_picture() || player.presentable()
            }),
        }
    }

    /// What is on screen now, to keep over the item about to open. Nothing
    /// while the surface rests (its host closed or showed a collection), so
    /// an item from before, or the grid's selection, never flashes up.
    fn capture(&mut self, cx: &mut Context<Self>) -> Option<Held> {
        if self.held.is_some() {
            // Still waiting on the last item: what is on screen is still
            // the one before it.
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
            Mode::Flash | Mode::Other => {
                let picture = self.full.clone().or_else(|| self.thumbnail.clone())?;
                let size = picture.size(0);
                let natural = (size.width.0 as f32, size.height.0 as f32);
                let fitted = document::geometry::fit_page(pane, natural)?;
                Some(Held::Image {
                    image: picture,
                    rect: (
                        (pane.0 - fitted.0) / 2.,
                        (pane.1 - fitted.1) / 2.,
                        fitted.0,
                        fitted.1,
                    ),
                    pixelated: false,
                })
            }
            Mode::Document => self.document.take().map(Held::Document),
            Mode::Media => self.player.take().map(|player| {
                player.update(cx, |player, _| player.pause());
                Held::Player(player)
            }),
        }
    }

    /// Lets the held picture go once the opening item is ready, or the wait
    /// has run out.
    fn release_hold(&mut self, cx: &mut Context<Self>) {
        let expired = self
            .held_since
            .is_some_and(|since| since.elapsed() >= motion::MEDIA_HOLD);
        if self.held.is_some() && (expired || self.ready(cx)) {
            self.held = None;
            self.held_since = None;
        }
    }

    /// The media player, while a video or audio item is shown.
    pub fn player(&self) -> Option<Entity<super::video::VideoPlayer>> {
        self.player.clone()
    }

    /// Whether the shown item has zoom controls: images and PDF pages do,
    /// Flash and the other documents do not.
    pub fn zoomable(&self) -> bool {
        match self.mode {
            Mode::Image => true,
            Mode::Document => self.document_zoom.0,
            _ => false,
        }
    }

    /// Whether a Flash movie is playing in the pane.
    pub fn flash(&self) -> bool {
        self.flash.is_some()
    }

    /// Whether the Flash movie plays and is muted, while one is shown.
    pub fn flash_playback(&self) -> Option<(bool, bool)> {
        self.flash
            .as_ref()
            .map(|flash| (flash.playback.playing, flash.playback.muted))
    }

    pub fn set_options(&mut self, options: Options, cx: &mut Context<Self>) {
        if self.options != options {
            self.options = options;
            cx.notify();
        }
    }

    /// Whether an image is on screen with a known size, so its zoom means
    /// something.
    pub fn has_image(&self) -> bool {
        self.mode == Mode::Image && self.image_size().is_some()
    }

    /// Whether a host shows the zoom (percentage, actual size, fit): an
    /// image once its size is known, or a document the viewer zooms itself
    /// (`supportsZoom` with `usesRendererZoom` in `DetailWindow.tsx`).
    pub fn shows_zoom(&self) -> bool {
        match self.mode {
            Mode::Image => self.has_image(),
            Mode::Document => self.document_zoom.0,
            _ => false,
        }
    }

    /// The camera a newly shown image opens with.
    fn opening(&self, image: Size, pane: Size) -> Camera {
        if self.options.opens_actual {
            Camera::actual()
        } else {
            Camera::fit(image, pane)
        }
    }

    /// The zoom as the toolbar shows it, in percent.
    pub fn percent(&self) -> f32 {
        match self.mode {
            Mode::Document => self.document_zoom.1,
            _ => self.camera.percent(),
        }
    }

    /// Takes the open document's zoom and tells the toolbar.
    fn document_zoom_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = &self.document {
            self.document_zoom = (document.zoomable(cx), document.percent(cx));
        }
        cx.emit(Event::Zoom);
        // The detail window observes the surface for its percentage.
        cx.notify();
    }

    /// Runs a control on the playing movie, and shows the controls again
    /// (`revealControls`).
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

    /// Shows Flash's controls, and hides them once the pointer has rested
    /// as long as the media player's do.
    fn reveal_flash_controls(&mut self, cx: &mut Context<Self>) {
        if self.flash.is_none() {
            return;
        }
        // Already up, only the timer restarts, without a repaint for every
        // pointer move.
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

    /// The name of the item about to be opened, for viewers that show it.
    pub fn set_display_name(&mut self, name: SharedString) {
        self.display_name = name;
    }

    /// The open document, while a document is shown.
    fn document(&self) -> Option<&Document> {
        self.document
            .as_ref()
            .filter(|_| self.mode == Mode::Document)
    }

    /// J: the previous page of a document.
    pub fn previous_page(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.previous_page(cx);
        }
    }

    /// L: the next page of a document.
    pub fn next_page(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.next_page(cx);
        }
    }

    /// Home, End, Page Up, Page Down and Space on a document's pages.
    pub fn scroll_pages(&mut self, scroll: document::Scroll, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.scroll(scroll, cx);
        }
    }

    /// Whether a document's pages are shown, whose keys scroll them.
    pub fn shows_pages(&self) -> bool {
        self.document().is_some_and(Document::paged)
    }

    /// Cmd+= and Cmd+−: the font preview's size.
    pub fn preview_size(&mut self, larger: bool, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.preview_size(larger, cx);
        }
    }

    fn pane(&self) -> Size {
        (
            f32::from(self.bounds.size.width),
            f32::from(self.bounds.size.height),
        )
    }

    /// The shown image's size in image pixels: the library's, else the
    /// decoded original's, else the thumbnail's.
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

    fn set_camera(&mut self, camera: Camera, cx: &mut Context<Self>) {
        let rescaled = camera.scale != self.camera.scale;
        self.camera = camera;
        if rescaled {
            cx.emit(Event::Zoom);
        }
        cx.notify();
    }

    /// Fits the shown image again on the next frame, dropping any drag.
    pub fn refit(&mut self) {
        self.needs_fit = true;
        self.glide = None;
        self.drag = None;
        self.navigator_drag = None;
    }

    /// `fitToWindow`.
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

    /// `fitActual`.
    pub fn actual_size(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.actual_size(cx);
            return;
        }
        self.glide = None;
        self.set_camera(Camera::actual(), cx);
    }

    /// The slider and the zoom menu: a scale, keeping the offset.
    pub fn set_percent(&mut self, percent: f32, cx: &mut Context<Self>) {
        if let Some(document) = self.document() {
            document.set_percent(percent, cx);
            return;
        }
        self.glide = None;
        let camera = self.camera.zoomed(percent / 100., None);
        self.set_camera(camera, cx);
    }

    /// Zoom in or out by one step, gliding. Steps pressed while a glide is
    /// under way build on where it is heading.
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

    fn glide_to(&mut self, to: Camera, cx: &mut Context<Self>) {
        self.glide = Some(Glide {
            from: self.camera,
            to,
            started: Instant::now(),
        });
        cx.notify();
    }

    /// A wheel or trackpad scroll over the image zooms about the pointer.
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
            // Pan by the scroll; lines count as wheel notches of pixels.
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
        // The browser's `deltaY` is positive scrolling down; GPUI's is the
        // other way round.
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
            // Each notch zooms by one step, gliding; notches in quick
            // succession build on where the last one was heading.
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

    /// A trackpad pinch zooms about its centre. Chromium hands a pinch to
    /// the page as a wheel event with a `deltaY` of −100 × ln(scale).
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

    /// The navigator's square, in window coordinates.
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

    fn centre_on(&mut self, point: (f32, f32), cx: &mut Context<Self>) {
        if let Some(image) = self.image_size() {
            let camera = self.camera.centred_on(point.0, point.1, image);
            self.set_camera(camera, cx);
        }
    }

    pub fn open(
        &mut self,
        key: String,
        thumbnail: PathBuf,
        natural: Option<Size>,
        path: PathBuf,
        mime: &str,
        cx: &mut Context<Self>,
    ) {
        let media = super::video::kind_of(mime);
        let kind = document::kind::renderer_kind(mime);
        // The same item again keeps playing or showing, unless what showed
        // it was stopped in the meantime.
        if self.opened == key
            && (mime != "application/x-shockwave-flash" || self.flash.is_some())
            && (media.is_none() || self.player.is_some())
            && (!kind.is_document() || self.document.is_some())
        {
            return;
        }
        let mode = match kind {
            RendererKind::Flash => Mode::Flash,
            RendererKind::Image | RendererKind::JpegXl => Mode::Image,
            RendererKind::Video | RendererKind::Audio if media.is_some() => Mode::Media,
            kind if kind.is_document() => Mode::Document,
            _ => Mode::Other,
        };
        self.show(key.clone(), thumbnail.clone(), natural, None, cx);
        self.opened = key.clone();
        self.error = None;
        // What `show` kept of the previous item stays over this one.
        let held = (self.held.take(), self.held_since.take());
        self.stop();
        (self.held, self.held_since) = held;
        self.live = true;
        if mode != self.mode {
            self.mode = mode;
            cx.emit(Event::Zoom);
        }
        match mode {
            Mode::Flash => {
                self.flash = Some(super::flash::Flash::open(path));
                self.reveal_flash_controls(cx);
                self.flash_task = Some(cx.spawn(async move |entity, cx| {
                    loop {
                        cx.background_executor().timer(motion::FLASH_FRAME).await;
                        let keep = entity.update(cx, |this, cx| {
                            if this.key != key {
                                return false;
                            }
                            let Some(flash) = &this.flash else {
                                return false;
                            };
                            let frame = flash.frame.lock().unwrap().take();
                            if let Some(frame) = frame {
                                match frame {
                                    Ok(mut rgba) => {
                                        for pixel in rgba.as_chunks_mut::<4>().0 {
                                            pixel.swap(0, 2);
                                        }
                                        if let Some(old) = this.full.take() {
                                            cx.drop_image(old, None);
                                        }
                                        this.full =
                                            Some(Arc::new(RenderImage::new(smallvec::smallvec![
                                                image::Frame::new(rgba)
                                            ])));
                                        if this.full_key != this.key {
                                            this.full_key = this.key.clone();
                                            cx.emit(Event::Progressed);
                                        }
                                    }
                                    Err(error) => this.error = Some(error),
                                }
                                cx.notify();
                            }
                            true
                        });
                        if !matches!(keep, Ok(true)) {
                            break;
                        }
                    }
                }));
            }
            Mode::Image if kind == RendererKind::JpegXl => self.decode_jpeg_xl(key, path, cx),
            Mode::Image => self.show(key, PathBuf::new(), natural, Some(path), cx),
            Mode::Document => {
                let document = Document::open(
                    kind,
                    path,
                    mime,
                    self.display_name.clone(),
                    self.options.member,
                    cx,
                );
                self._document_zoom =
                    document.observe_zoom(cx, |this, cx| this.document_zoom_changed(cx));
                self._document_progress = document.observe_progress(cx, |_, cx| {
                    cx.emit(Event::Progressed);
                    cx.notify();
                });
                // A thumbnail already decoded for this item goes to the
                // document at once.
                if self.shown == self.key {
                    document.set_thumbnail(self.thumbnail.clone(), cx);
                }
                self.document = Some(document);
                self.document_zoom_changed(cx);
                cx.notify();
            }
            Mode::Media => {
                let kind = media.unwrap_or(super::video::model::Kind::Video);
                let settings = crate::settings::current(cx);
                let mut options = super::video::model::Options::for_item(
                    kind,
                    settings.video_auto_play,
                    settings.video_loop,
                );
                if self.options.member {
                    options.auto_play = false;
                    options.looping = false;
                    options.muted = false;
                }
                // `PICTO_GPUI_SILENT=1`: nothing plays aloud, for checks run
                // while nobody is at the machine.
                options.muted |= std::env::var_os("PICTO_GPUI_SILENT").is_some();
                let mime = mime.to_owned();
                let player = cx.new(|cx| {
                    super::video::VideoPlayer::new(kind, path, &mime, options, Some(thumbnail), cx)
                });
                self._player_progress = Some(cx.subscribe(
                    &player,
                    |_, _, _: &super::video::FirstPicture, cx| {
                        cx.emit(Event::Progressed);
                        cx.notify();
                    },
                ));
                self.player = Some(player);
            }
            Mode::Other => {
                self.error = Some(format!("Native preview is not implemented for {mime}"));
            }
        }
    }

    /// Shows an item by its thumbnail, and its original once `full` decodes.
    /// What is on screen stays until the new thumbnail has decoded.
    pub fn show(
        &mut self,
        key: String,
        thumbnail: PathBuf,
        natural: Option<Size>,
        full: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.key != key {
            let held = self.capture(cx);
            self.key = key.clone();
            self.natural = natural;
            self.full_task = None;
            self.stop();
            if held.is_some() {
                self.held = held;
                self.held_since = Some(Instant::now());
                // Wakes the surface when the wait runs out.
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(motion::MEDIA_HOLD).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
            self.error = None;
            let load = ImageAssetLoader::load(Resource::Path(thumbnail.into()), cx);
            let load = cx.background_executor().spawn(load);
            let expected = key.clone();
            self.thumbnail_task = Some(cx.spawn(async move |entity, cx| {
                let result = load.await;
                let _ = entity.update(cx, |this, cx| {
                    if this.key == expected {
                        // A thumbnail that fails to decode still counts as
                        // settled: the original follows on its own.
                        this.thumbnail = result.ok();
                        if this.full_key != expected {
                            this.full = None;
                        }
                        this.present_key();
                        if let Some(document) = &this.document {
                            document.set_thumbnail(this.thumbnail.clone(), cx);
                        }
                        cx.emit(Event::Progressed);
                        cx.notify();
                    }
                });
            }));
        }
        if let Some(full) = full {
            let load = ImageAssetLoader::load(Resource::Path(full.into()), cx);
            let load = cx.background_executor().spawn(load);
            self.full_task = Some(cx.spawn(async move |entity, cx| {
                let result = load.await;
                let _ = entity.update(cx, |this, cx| {
                    if this.key == key {
                        this.full = result.ok();
                        this.full_key = key.clone();
                        this.present_key();
                        cx.emit(Event::Progressed);
                        cx.notify();
                    }
                });
            }));
        }
    }

    /// `JpegXlViewer.tsx`: the original decoded off the drawing thread
    /// (jxl-oxide here, the jsquash decoder there) and shown on the image
    /// camera, over its thumbnail until then.
    fn decode_jpeg_xl(&mut self, key: String, path: PathBuf, cx: &mut Context<Self>) {
        let decode = cx.background_executor().spawn(async move {
            document::pages::decode_jpeg_xl(&path).map(document::render_image)
        });
        self.full_task = Some(cx.spawn(async move |entity, cx| {
            let result = decode.await;
            let _ = entity.update(cx, |this, cx| {
                if this.key != key {
                    return;
                }
                match result {
                    Ok(image) => {
                        this.full = Some(image);
                        this.full_key = key.clone();
                        this.present_key();
                    }
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
    }

    /// The requested item is now the one on screen; it opens fitted.
    fn present_key(&mut self) {
        if self.shown != self.key {
            self.shown = self.key.clone();
            self.shown_natural = self.natural;
            self.refit();
        }
    }

    /// Moves gliding and following cameras on by a frame; returns whether
    /// another frame is needed.
    fn animate(&mut self, cx: &mut Context<Self>) -> bool {
        let mut more = false;
        if let Some(glide) = self.glide {
            let (camera, done) = glide.at(glide.started.elapsed());
            if done {
                self.glide = None;
            }
            more |= !done;
            self.set_camera(camera, cx);
        }
        if let Some(mut drag) = self.navigator_drag {
            drag.current = camera::follow(drag.current, drag.target);
            self.navigator_drag = Some(drag);
            self.centre_on(drag.current, cx);
            more = true;
        }
        more
    }

    /// The image, its thumbnail beneath until the original is decoded, at
    /// the camera's place and scale.
    fn image_layers(&self, image: Size) -> Vec<AnyElement> {
        let (left, top, width, height) = self.camera.image_rect(image, self.pane());
        let key = self.shown.clone();
        let pixelated = self.options.pixelated;
        let place = |source: Arc<RenderImage>| {
            // An image with several frames (GIF, animated WebP) animates only
            // with an id, which keeps its frame between draws.
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

    /// `.navigator`: the thumbnail in a 120px square at the bottom right,
    /// with the visible part framed and the rest shaded.
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
        // The shade around the frame (`box-shadow: 0 0 0 999px`), as four
        // bands; each rounds the navigator's corners it touches.
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let entity = cx.entity().downgrade();
        let image_mode = self.mode == Mode::Image;
        let image = self.image_size().filter(|_| image_mode);
        let pane = self.pane();
        let camera_before = self.camera;
        // A newly shown image opens fitted, once the pane has been measured.
        if self.needs_fit
            && pane.0 > 0.
            && let Some(image) = image
        {
            self.needs_fit = false;
            self.glide = None;
            let opening = self.opening(image, pane);
            self.set_camera(opening, cx);
        }
        // A camera moved while drawing notifies no one (the detail window
        // observes the surface for its zoom percentage, read before this
        // ran): another frame tells them, also after a glide's last step.
        if self.animate(cx) || self.camera != camera_before {
            window.request_animation_frame();
        }
        // The navigator appears a moment after the image first overflows.
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
        let listeners = entity.clone();
        let mut surface = div()
            .id("media-surface")
            .track_focus(&focus)
            // Only a Flash movie keeps the keys for itself; on anything else
            // a click that focuses the surface leaves the host's keys working.
            .when(self.flash.is_some(), |surface| surface.key_context("Flash"))
            .relative()
            .size_full()
            .overflow_hidden()
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let _ = entity.update(cx, |this, cx| {
                            if this.bounds != bounds {
                                let from = this.pane();
                                this.bounds = bounds;
                                // What follows changes what is drawn, and a
                                // notification made while a frame is being
                                // drawn does not by itself bring another:
                                // without one the picture stays fitted to
                                // the size the pane had before.
                                window.request_animation_frame();
                                if this.options.follows_resize
                                    && !this.needs_fit
                                    && let Some(image) = this.image_size()
                                {
                                    let camera = this.camera.resized(image, from, this.pane());
                                    this.set_camera(camera, cx);
                                }
                                // The first measurement lets a waiting
                                // image be fitted.
                                if this.needs_fit {
                                    cx.notify();
                                }
                            }
                        });
                    },
                    move |_, _, window, _| {
                        if !dragging {
                            return;
                        }
                        // A pan or navigator drag follows the pointer
                        // anywhere in the window until the button is up.
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
                        // Grab the image to pan it.
                        this.glide = None;
                        this.drag = Some((event.position, this.camera));
                        cx.notify();
                        return;
                    }
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
            // `.zoomContainer`: grab, and grabbing while dragged.
            surface = surface.cursor(if self.drag.is_some() {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            });
        }
        surface = surface
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" || event.keystroke.modifiers.platform {
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
            // Nothing is drawn until the pane has been measured, so an image
            // never shows at a size it was not fitted for.
            if let Some(image) = image.filter(|_| pane.0 > 0. && !self.needs_fit) {
                surface = surface.children(self.image_layers(image));
            }
        } else if let Some(document) = self.document() {
            surface = surface.child(document.element(self.thumbnail.clone(), cx));
        } else if let Some(player) = self.player.clone().filter(|_| self.mode == Mode::Media) {
            // The player draws nothing until its first frame; the held
            // picture, or the pane's background, shows until then.
            surface = surface.child(div().absolute().inset_0().child(player));
        } else {
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
                Held::Document(document) => layer.child(document.element(None, cx)),
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
        // Hidden, the controls are left out, so they take no pointer.
        if let Some(flash) = &self.flash
            && (self.flash_controls.on() || self.flash_controls.animating())
        {
            if self.flash_controls.animating() {
                window.request_animation_frame();
            }
            surface = surface.child(super::flash_controls::controls(
                cx.entity().downgrade(),
                &flash.playback,
                // `disabled={!controller}`: nothing to control until the
                // first frame has arrived.
                self.full.is_some() && self.full_key == self.key,
                self.flash_controls.value_with(motion::EASE),
                self.flash_controls.on(),
            ));
        }
        if let Some(flash) = &self.flash
            && let Some(trace) = flash.traces.lock().unwrap().back()
        {
            surface = surface.child(
                div()
                    .absolute()
                    .bottom_0()
                    .px_2()
                    .text_size(px(11.))
                    .bg(crate::theme::theme(cx).glass())
                    .child(format!("Ruffle: {trace}")),
            );
        }
        surface.into_any_element()
    }
}

impl MediaSurface {
    fn flash_position(&self, position: Point<Pixels>) -> (f64, f64) {
        let width = f32::from(self.bounds.size.width);
        let height = f32::from(self.bounds.size.height);
        let scale = (width / 960.).min(height / 720.).max(0.001);
        let x =
            (f32::from(position.x - self.bounds.origin.x) - (width - 960. * scale) / 2.) / scale;
        let y =
            (f32::from(position.y - self.bounds.origin.y) - (height - 720. * scale) / 2.) / scale;
        (x as f64, y as f64)
    }
}

/// Paints `.transparencyGrid` over `bounds`, the image's place, with
/// squares 8 image pixels wide at `scale`. Squares too small to tell apart
/// are painted as the even grey they blend into.
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
