use gpui::*;
use std::{path::PathBuf, sync::Arc, time::Duration};

/// Shared image presentation: retain the previous frame until the next thumbnail
/// decodes, then reveal full resolution over that thumbnail only after decoding.
#[derive(Default)]
pub struct MediaSurface {
    key: String,
    thumbnail: Option<Arc<RenderImage>>,
    full: Option<Arc<RenderImage>>,
    full_key: String,
    thumbnail_task: Option<Task<()>>,
    full_task: Option<Task<()>>,
    pages: Option<crate::document::Pages>,
    document_task: Option<Task<()>>,
    flash: Option<crate::flash::Flash>,
    flash_task: Option<Task<()>>,
    opened: String,
    error: Option<String>,
    bounds: Bounds<Pixels>,
    focus: Option<FocusHandle>,
}
impl MediaSurface {
    pub fn stop(&mut self) {
        self.flash = None;
        self.flash_task = None;
    }

    pub fn open(
        &mut self,
        key: String,
        thumbnail: PathBuf,
        path: PathBuf,
        mime: &str,
        cx: &mut Context<Self>,
    ) {
        if self.opened == key && (mime != "application/x-shockwave-flash" || self.flash.is_some()) {
            return;
        }
        self.show(key.clone(), thumbnail, None, cx);
        self.opened = key.clone();
        self.pages = None;
        self.document_task = None;
        self.error = None;
        self.stop();
        if mime == "application/vnd.openxmlformats-officedocument.wordprocessingml.document" {
            let load = cx
                .background_executor()
                .spawn(async move { crate::document::render(&path) });
            self.document_task = Some(cx.spawn(async move |entity, cx| {
                let result = load.await;
                let _ = entity.update(cx, |this, cx| {
                    if this.key != key {
                        return;
                    }
                    match result {
                        Ok(pages) => this.pages = Some(pages),
                        Err(error) => this.error = Some(format!("{error:#}")),
                    }
                    cx.notify();
                });
            }));
        } else if mime == "application/x-shockwave-flash" {
            self.flash = Some(crate::flash::Flash::open(path));
            self.flash_task = Some(cx.spawn(async move |entity, cx| {
                loop {
                    Timer::after(Duration::from_millis(16)).await;
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
                                    for pixel in rgba.chunks_exact_mut(4) {
                                        pixel.swap(0, 2);
                                    }
                                    if let Some(old) = this.full.take() {
                                        let _ = cx.drop_image(old, None);
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
        } else if mime.starts_with("image/") {
            self.show(key, PathBuf::new(), Some(path), cx);
        } else {
            self.error = Some(format!("Native preview is not implemented for {mime}"));
        }
    }
    pub fn show(
        &mut self,
        key: String,
        thumbnail: PathBuf,
        full: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.key != key {
            self.key = key.clone();
            self.full_task = None;
            self.document_task = None;
            self.pages = None;
            self.stop();
            self.error = None;
            let load = ImageAssetLoader::load(Resource::Path(thumbnail.into()), cx);
            let load = cx.background_executor().spawn(load);
            let expected = key.clone();
            self.thumbnail_task = Some(cx.spawn(async move |entity, cx| {
                let result = load.await;
                let _ = entity.update(cx, |this, cx| {
                    if this.key == expected {
                        this.thumbnail = result.ok();
                        if this.full_key != expected {
                            this.full = None;
                        }
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
                        cx.notify();
                    }
                });
            }));
        }
    }
}
impl Render for MediaSurface {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus.get_or_insert_with(|| cx.focus_handle()).clone();
        let width = (f32::from(window.viewport_size().width) - 604.).max(1.);
        let height = (f32::from(window.viewport_size().height) - 210.).max(1.);
        if let Some(pages) = &self.pages {
            let mut content = div()
                .id("document-pages")
                .size_full()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(16.));
            for path in &pages.paths {
                content = content.child(
                    img(path.clone())
                        .w(px(width))
                        .h(px(width * 1.414))
                        .object_fit(ObjectFit::Contain),
                );
            }
            return content.into_any_element();
        }
        let entity = cx.entity().downgrade();
        let mut surface = div()
            .id("media-surface")
            .track_focus(&focus)
            .key_context("Flash")
            .relative()
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = entity.update(cx, |this, _| this.bounds = bounds);
                    },
                    |_, _, _, _| {},
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
                    if let Some(flash) = &this.flash {
                        if let Some(focus) = &this.focus {
                            window.focus(focus);
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
            );
        surface = surface
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" || event.keystroke.modifiers.platform {
                    return;
                }
                if let Some(flash) = &this.flash {
                    cx.stop_propagation();
                    let key = crate::flash::key_descriptor(&event.keystroke.key);
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
                        key: crate::flash::key_descriptor(&event.keystroke.key),
                    });
                }
            }));
        if let Some(thumbnail) = &self.thumbnail {
            surface = surface.child(
                img(thumbnail.clone())
                    .w(px(width))
                    .h(px(height))
                    .object_fit(ObjectFit::Contain),
            );
        }
        if let Some(full) = &self.full {
            surface = surface.child(
                img(full.clone())
                    .absolute()
                    .inset_0()
                    .w(px(width))
                    .h(px(height))
                    .object_fit(ObjectFit::Contain),
            );
        }
        if let Some(error) = &self.error {
            surface = surface.child(
                div()
                    .absolute()
                    .bottom_0()
                    .w_full()
                    .p_3()
                    .bg(rgb(0x292a2d))
                    .text_size(px(12.))
                    .child(error.clone()),
            );
        }
        if let Some(flash) = &self.flash {
            if let Some(trace) = flash.traces.lock().unwrap().back() {
                surface = surface.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .px_2()
                        .text_size(px(11.))
                        .bg(rgb(0x292a2d))
                        .child(format!("Ruffle: {trace}")),
                );
            }
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
