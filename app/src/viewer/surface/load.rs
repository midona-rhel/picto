//! Opens an item on the media surface.
//!
//! Opening an item has three steps. First the surface decides how the item
//! is shown: as a picture, a video or audio player, a document or a Flash
//! movie. Then it shows the item's thumbnail immediately. Last, after a
//! short wait, it loads what the item needs beyond its thumbnail. A
//! picture's original is decoded on a background thread, never on the
//! thread that draws the window.
use super::*;

impl MediaSurface {
    pub fn open(
        &mut self,
        key: String,
        thumbnail: PathBuf,
        natural: Option<Size>,
        path: PathBuf,
        mime: &str,
        cx: &mut Context<Self>,
    ) {
        let media = crate::viewer::video::kind_of(mime);
        let kind = document::kind::renderer_kind(mime);
        // When the item is already open, leave it as it is, so that a video
        // keeps playing. Open it again only if its player, document or
        // Flash movie has been stopped since.
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
            _ => Mode::Document,
        };
        self.show(key.clone(), thumbnail.clone(), natural, cx);
        self.opened = key.clone();
        self.error = None;
        // `show` may have kept the previous item's picture to draw over
        // this item until this item is ready. `stop` would discard it, so
        // it is taken out before `stop` and put back after.
        let held = (self.held.take(), self.held_since.take());
        self.stop();
        (self.held, self.held_since) = held;
        self.live = true;
        if mode != self.mode {
            self.mode = mode;
            cx.emit(Event::Zoom);
        }
        // What the item needs beyond its thumbnail (the original picture,
        // a video or audio player, a document, a Flash movie) is started
        // only after the item has been on screen for `ORIGINAL_DELAY`. When
        // the user goes through items quickly, for example with an arrow
        // key held down, each item is replaced before the wait ends, so
        // nothing is read from disk for the items passed.
        let mime = mime.to_owned();
        self.full_wait = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(motion::ORIGINAL_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                if this.key == key {
                    this.start(mode, kind, key, thumbnail, path, &mime, cx);
                }
            });
        }));
        self.starting = true;
    }

    /// Starts showing the item in full, after the wait in [`Self::open`]:
    /// decodes the original picture, or opens the video or audio player,
    /// the document or the Flash movie.
    fn start(
        &mut self,
        mode: Mode,
        kind: RendererKind,
        key: String,
        thumbnail: PathBuf,
        path: PathBuf,
        mime: &str,
        cx: &mut Context<Self>,
    ) {
        let media = crate::viewer::video::kind_of(mime);
        self.starting = false;
        match mode {
            Mode::Flash => {
                self.flash = Some(crate::viewer::flash::Flash::open(path));
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
                                        let old = this.full.replace(Arc::new(RenderImage::new(
                                            smallvec::smallvec![image::Frame::new(rgba)],
                                        )));
                                        this.retire(old);
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
            Mode::Image => self.load_full(key, path, mime == "image/jpeg", cx),
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
                // If this item's thumbnail has already been decoded, give
                // it to the document now, so the document can show it
                // while its first page loads.
                if self.shown == self.key {
                    document.set_thumbnail(self.thumbnail.clone(), cx);
                }
                self.document = Some(document);
                self.document_zoom_changed(cx);
                cx.notify();
            }
            Mode::Media => {
                let kind = media.unwrap_or(crate::viewer::video::model::Kind::Video);
                let settings = crate::settings::current(cx);
                let mut options = crate::viewer::video::model::Options::for_item(
                    kind,
                    settings.video_auto_play,
                    settings.video_loop,
                );
                if self.options.member {
                    options.auto_play = false;
                    options.looping = false;
                    options.muted = false;
                }
                // When the environment variable `PICTO_GPUI_SILENT` is set
                // to 1, the player starts muted. Automated checks use this
                // so that they make no sound.
                options.muted |= crate::seams::silent();
                let mime = mime.to_owned();
                let player = cx.new(|cx| {
                    crate::viewer::video::VideoPlayer::new(
                        kind,
                        path,
                        &mime,
                        options,
                        Some(thumbnail),
                        cx,
                    )
                });
                self._player_progress = Some(cx.subscribe(
                    &player,
                    |_, _, _: &crate::viewer::video::FirstPicture, cx| {
                        cx.emit(Event::Progressed);
                        cx.notify();
                    },
                ));
                self.player = Some(player);
                // The player is drawn from now on. It asks the decoder for
                // its first picture each time it is drawn, so without this
                // the first picture would only appear at the next redraw
                // from some other cause, such as a pointer movement.
                cx.notify();
            }
        }
    }

    /// Shows an item's thumbnail. The item's original picture replaces the
    /// thumbnail later, when it has been decoded. Until the new thumbnail
    /// has been decoded, the picture that was on screen before stays there.
    pub fn show(
        &mut self,
        key: String,
        thumbnail: PathBuf,
        natural: Option<Size>,
        cx: &mut Context<Self>,
    ) {
        if self.key != key {
            let held = self.capture(cx);
            self.key = key.clone();
            self.natural = natural;
            self.full_task = None;
            self.full_wait = None;
            self.whole = None;
            self.stop();
            if held.is_some() {
                self.held = held;
                self.held_since = Some(Instant::now());
                // Draw the surface again when the wait is over.
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
                        // A thumbnail that could not be decoded is treated
                        // as finished, so the surface stops waiting for it.
                        // The original picture is loaded separately.
                        let old = std::mem::replace(&mut this.thumbnail, result.ok());
                        this.retire(old);
                        if this.full_key != expected {
                            let old = this.full.take();
                            this.retire(old);
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
    }

    /// Decodes the original picture of the item `key`.
    ///
    /// With `reduce`, a JPEG that is shown at less than half its own size is
    /// first decoded at the size it is shown ([`scaled`]), which is faster.
    /// It is decoded at its full size later, when the user zooms in far
    /// enough to need more pixels ([`Self::decode_whole_when_needed`]).
    pub(super) fn load_full(
        &mut self,
        key: String,
        path: PathBuf,
        reduce: bool,
        cx: &mut Context<Self>,
    ) {
        let reduced = self
            .natural
            .filter(|_| reduce)
            .and_then(|natural| scaled::fitted_pixels(natural, self.pane(), self.scale_factor));
        self.whole = reduced.map(|_| path.clone());
        let load = match reduced {
            Some(size) => cx.background_executor().spawn(async move {
                document::guarded(|| Ok(scaled::decode(&path, size, true)?.into_rgba8()))
                    .map(document::render_image)
                    .ok()
            }),
            None => {
                let load = ImageAssetLoader::load(Resource::Path(path.into()), cx);
                cx.background_executor()
                    .spawn(async move { load.await.ok() })
            }
        };
        self.full_task = Some(cx.spawn(async move |entity, cx| {
            let result = load.await;
            let _ = entity.update(cx, |this, cx| {
                if this.key == key {
                    let old = std::mem::replace(&mut this.full, result);
                    this.retire(old);
                    this.full_key = key.clone();
                    // When the library has no size on record for this
                    // picture, the view was fitted using the thumbnail's
                    // size. The original has a different size, so the view
                    // is fitted again.
                    if this.shown == key && this.shown_natural.is_none() {
                        this.refit();
                    }
                    this.present_key();
                    cx.emit(Event::Progressed);
                    cx.notify();
                }
            });
        }));
    }

    /// Decodes a JPEG XL original on a background thread with the
    /// `jxl-oxide` crate and shows it in place of its thumbnail. If the
    /// file cannot be decoded, the error is shown instead.
    pub(super) fn decode_jpeg_xl(&mut self, key: String, path: PathBuf, cx: &mut Context<Self>) {
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
                        let old = this.full.replace(image);
                        this.retire(old);
                        this.full_key = key.clone();
                        this.present_key();
                    }
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
    }

    /// Records that the item that was asked for is now the item on screen,
    /// and fits it to the view.
    pub(super) fn present_key(&mut self) {
        if self.shown != self.key {
            self.shown = self.key.clone();
            self.shown_natural = self.natural;
            self.refit();
        }
    }

    /// Decodes the original at its full size if it is currently shown from
    /// a reduced decode and the view now draws it larger than the reduced
    /// decode's size in pixels.
    pub(super) fn decode_whole_when_needed(&mut self, image: Option<Size>, cx: &mut Context<Self>) {
        let (Some(image), Some(decoded)) = (image, self.full.as_ref().map(|full| full.size(0)))
        else {
            return;
        };
        if self.whole.is_none() || self.full_key != self.key || self.shown != self.key {
            return;
        }
        let (_, _, width, height) = self.camera.image_rect(image, self.pane());
        let short = |shown: f32, pixels: i32| shown * self.scale_factor > pixels as f32 * 1.01;
        if (short(width, decoded.width.0) || short(height, decoded.height.0))
            && let Some(path) = self.whole.take()
        {
            self.load_full(self.key.clone(), path, false, cx);
        }
    }
}
