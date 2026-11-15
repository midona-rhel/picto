//! Opening an item on the surface: choosing its renderer, showing its
//! thumbnail at once, and decoding the original (whole or at the size
//! shown, JPEG XL on its own thread) off the thread that draws.
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
            _ => Mode::Document,
        };
        self.show(key.clone(), thumbnail.clone(), natural, cx);
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
                // `PICTO_GPUI_SILENT=1`: nothing plays aloud, for checks run
                // while nobody is at the machine.
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
        cx: &mut Context<Self>,
    ) {
        if self.key != key {
            let held = self.capture(cx);
            self.key = key.clone();
            self.natural = natural;
            self.full_task = None;
            self.whole = None;
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

    /// Decodes the original of `key`. A JPEG the pane shows at under half
    /// its size is decoded at that size first ([`scaled`]), and whole only
    /// once the view needs more of its pixels.
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
                    // Without the library's size the camera was fitted to
                    // the thumbnail's pixels; the original's differ.
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

    /// `JpegXlViewer.tsx`: the original decoded off the drawing thread
    /// (jxl-oxide here, the jsquash decoder there) and shown on the image
    /// camera, over its thumbnail until then.
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

    /// The requested item is now the one on screen; it opens fitted.
    pub(super) fn present_key(&mut self) {
        if self.shown != self.key {
            self.shown = self.key.clone();
            self.shown_natural = self.natural;
            self.refit();
        }
    }

    /// Starts the whole decode of an original shown from a reduced one, once
    /// the view shows it larger than the reduced one's pixels.
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
