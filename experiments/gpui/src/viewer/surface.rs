//! The media area of the viewer (`MediaView.tsx`): full-bleed, the previous
//! item kept until the next one's thumbnail has decoded, the original laid
//! over the thumbnail once decoded (`ImageCrossfadeFrame`). Images zoom and
//! pan (`useImageZoom.ts`) and show the navigator while they overflow; Flash
//! plays through Ruffle and fills the pane.
use super::camera::{self, Camera, Glide, Size};
use crate::{motion, theme};
use gpui::*;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

/// `NAV_SIZE`: the navigator's side.
const NAVIGATOR: f32 = 120.;
/// The navigator's inset from the pane's bottom-right corner.
const NAVIGATOR_INSET: f32 = 12.;

/// What the surface tells the viewer toolbar.
pub enum Event {
    /// The zoom, or whether the shown item zooms at all, changed.
    Zoom,
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
}

impl Default for Options {
    fn default() -> Self {
        Self {
            navigator: true,
            opens_actual: false,
            scroll_pans: false,
            follows_resize: false,
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
    }

    /// Whether the shown item has zoom controls: images do, Flash does not.
    pub fn zoomable(&self) -> bool {
        self.mode == Mode::Image
    }

    /// Whether a Flash movie is playing in the pane.
    pub fn flash(&self) -> bool {
        self.flash.is_some()
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
        self.camera.percent()
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
        if let Some(image) = self.image_size() {
            self.glide = None;
            let fitted = Camera::fit(image, self.pane());
            self.set_camera(fitted, cx);
        }
    }

    /// `fitActual`.
    pub fn actual_size(&mut self, cx: &mut Context<Self>) {
        self.glide = None;
        self.set_camera(Camera::actual(), cx);
    }

    /// The slider and the zoom menu: a scale, keeping the offset.
    pub fn set_percent(&mut self, percent: f32, cx: &mut Context<Self>) {
        self.glide = None;
        let camera = self.camera.zoomed(percent / 100., None);
        self.set_camera(camera, cx);
    }

    /// Zoom in or out by one step, gliding. Steps pressed while a glide is
    /// under way build on where it is heading.
    pub fn step_zoom(&mut self, zoom_in: bool, cx: &mut Context<Self>) {
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
        if self.opened == key && (mime != "application/x-shockwave-flash" || self.flash.is_some()) {
            return;
        }
        self.show(key.clone(), thumbnail, natural, None, cx);
        self.opened = key.clone();
        self.error = None;
        self.stop();
        let mode = if mime == "application/x-shockwave-flash" {
            Mode::Flash
        } else if mime.starts_with("image/") {
            Mode::Image
        } else {
            Mode::Other
        };
        if mode != self.mode {
            self.mode = mode;
            cx.emit(Event::Zoom);
        }
        match mode {
            Mode::Flash => {
                self.flash = Some(super::flash::Flash::open(path));
                self.flash_task = Some(cx.spawn(async move |entity, cx| {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(16))
                            .await;
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
                                        this.full_key = this.key.clone();
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
            Mode::Image => self.show(key, PathBuf::new(), natural, Some(path), cx),
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
            self.key = key.clone();
            self.natural = natural;
            self.full_task = None;
            self.stop();
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
                        cx.notify();
                    }
                });
            }));
        }
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
        let place = |source: Arc<RenderImage>| {
            img(source)
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(width))
                .h(px(height))
                .object_fit(ObjectFit::Fill)
                .into_any_element()
        };
        let full = self.full.clone().filter(|_| self.full_key == self.shown);
        match full {
            Some(full) => vec![place(full)],
            None => self.thumbnail.clone().map(place).into_iter().collect(),
        }
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
        if self.animate(cx) {
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
            .key_context("Flash")
            .relative()
            .size_full()
            .overflow_hidden()
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = entity.update(cx, |this, cx| {
                            if this.bounds != bounds {
                                let from = this.pane();
                                this.bounds = bounds;
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
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, _| {
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
