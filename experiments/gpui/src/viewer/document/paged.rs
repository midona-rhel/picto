//! PDF, DjVu and comic archives, one page at a time in the document shell
//! (`PdfViewer.tsx`, `DjvuViewer.tsx`, the CBZ half of
//! `ArchiveDocumentViewer.tsx`). Pages are drawn on the document's own
//! thread at the size they take on screen; the page on screen stays until
//! the next one is drawn. PDF pages zoom from the viewer toolbar.
use super::geometry::{self, PageSize, ScaleMode};
use super::kind::RendererKind;
use super::pages::{PageWorker, RenderedPage};
use super::shell::{self, VIEWPORT_PADDING};
use crate::theme;
use gpui::*;
use std::path::PathBuf;
use std::sync::Arc;

/// The zoom changed, or whether the document zooms at all.
pub struct ZoomChanged;

/// A drawn page on screen.
struct Shown {
    index: usize,
    natural: PageSize,
    image: Arc<RenderImage>,
}

/// What was last asked of the document's thread, so a frame that changes
/// nothing asks nothing.
#[derive(Clone, Copy, PartialEq)]
struct Asked {
    page: usize,
    mode: ScaleMode,
    available: (i32, i32),
    scale_factor: f32,
}

pub struct PagedViewer {
    kind: RendererKind,
    worker: Option<PageWorker>,
    count: usize,
    /// 0-based.
    page: usize,
    mode: ScaleMode,
    /// The natural size of the current page, once known.
    natural: Option<PageSize>,
    shown: Option<Shown>,
    asked: Option<Asked>,
    render_task: Option<Task<()>>,
    _open_task: Option<Task<()>>,
    error: Option<SharedString>,
    /// The viewport's content box (inside its 24px padding).
    available: PageSize,
    scale_factor: f32,
    scroll: ScrollHandle,
}

impl EventEmitter<ZoomChanged> for PagedViewer {}

impl PagedViewer {
    pub fn new(kind: RendererKind, path: PathBuf, cx: &mut Context<Self>) -> Self {
        let opened = PageWorker::open(kind, path);
        let open_task = cx.spawn(async move |this, cx| {
            let result = opened.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok((worker, count))) => {
                        this.worker = Some(worker);
                        this.count = count;
                    }
                    Ok(Err(error)) => this.error = Some(error.to_string().into()),
                    Err(_) => this.error = Some(open_failure(kind).into()),
                }
                cx.notify();
            });
        });
        Self {
            kind,
            worker: None,
            count: 0,
            page: 0,
            mode: ScaleMode::PageFit,
            natural: None,
            shown: None,
            asked: None,
            render_task: None,
            _open_task: Some(open_task),
            error: None,
            available: (0., 0.),
            scale_factor: 1.,
            scroll: ScrollHandle::new(),
        }
    }

    /// Only PDF pages take the toolbar's zoom controls.
    pub fn zoomable(&self) -> bool {
        self.kind == RendererKind::Pdf
    }

    /// The PDF page's scale in percent, as the toolbar shows it.
    pub fn percent(&self) -> f32 {
        (self.scale() * 100.).round()
    }

    fn scale(&self) -> f32 {
        match self.natural {
            Some(natural) => self.mode.scale(self.available, natural),
            None => match self.mode {
                ScaleMode::Custom(scale) => scale,
                _ => 1.,
            },
        }
    }

    fn set_mode(&mut self, mode: ScaleMode, cx: &mut Context<Self>) {
        if !self.zoomable() {
            return;
        }
        self.mode = mode;
        cx.emit(ZoomChanged);
        cx.notify();
    }

    /// `fitToWindow`
    pub fn fit(&mut self, cx: &mut Context<Self>) {
        self.set_mode(ScaleMode::PageFit, cx);
    }

    /// `fitActual`
    pub fn actual_size(&mut self, cx: &mut Context<Self>) {
        self.set_mode(ScaleMode::Actual, cx);
    }

    /// `setZoomScale`, from the slider and the zoom menu.
    pub fn set_percent(&mut self, percent: f32, cx: &mut Context<Self>) {
        self.set_mode(ScaleMode::custom(percent / 100.), cx);
    }

    /// `zoomIn` / `zoomOut`: one 1.2× step from the scale on screen.
    pub fn step_zoom(&mut self, zoom_in: bool, cx: &mut Context<Self>) {
        let factor = if zoom_in {
            geometry::STEP
        } else {
            1. / geometry::STEP
        };
        self.set_mode(ScaleMode::custom(self.scale() * factor), cx);
    }

    pub fn previous_page(&mut self, cx: &mut Context<Self>) {
        if self.page > 0 {
            self.go_to(self.page - 1, cx);
        }
    }

    pub fn next_page(&mut self, cx: &mut Context<Self>) {
        if self.page + 1 < self.count {
            self.go_to(self.page + 1, cx);
        }
    }

    fn go_to(&mut self, page: usize, cx: &mut Context<Self>) {
        self.page = page;
        self.natural = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    /// The page's size on screen for the viewport and its natural size.
    fn layout(
        kind: RendererKind,
        mode: ScaleMode,
        available: PageSize,
        natural: PageSize,
    ) -> PageSize {
        match kind {
            RendererKind::Pdf => {
                let scale = mode.scale(available, natural);
                (natural.0 * scale, natural.1 * scale)
            }
            // `max-width: 100%; max-height: 100%; object-fit: contain`.
            _ => geometry::contain_page(available, natural).unwrap_or(natural),
        }
    }

    /// Asks the document's thread for the current page at its size on
    /// screen, unless that is what was last asked.
    fn request(&mut self, cx: &mut Context<Self>) {
        let Some(worker) = &self.worker else {
            return;
        };
        if self.available.0 <= 0. || self.available.1 <= 0. {
            return;
        }
        let asked = Asked {
            page: self.page,
            mode: self.mode,
            available: (
                self.available.0.round() as i32,
                self.available.1.round() as i32,
            ),
            scale_factor: self.scale_factor,
        };
        if self.asked == Some(asked) {
            return;
        }
        self.asked = Some(asked);
        let (kind, mode, available, scale_factor) =
            (self.kind, self.mode, self.available, self.scale_factor);
        let answer = worker.render(
            self.page,
            Box::new(move |natural| {
                // `documentCanvasGeometry`: the page's scale on screen
                // times the display's pixel ratio.
                let css = Self::layout(kind, mode, available, natural);
                geometry::canvas_geometry(natural, css.0 / natural.0, scale_factor)
                    .map(|canvas| canvas.pixels)
                    .filter(|(width, height)| *width >= 1 && *height >= 1)
            }),
        );
        self.render_task = Some(cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Ok(page)) => this.present(page, cx),
                Ok(Err(error)) => {
                    this.error = Some(error.to_string().into());
                    cx.notify();
                }
                // Superseded by a newer request.
                Err(_) => {}
            });
        }));
    }

    fn present(&mut self, page: RenderedPage, cx: &mut Context<Self>) {
        if page.index != self.page {
            return;
        }
        let first_measure = self.natural.is_none();
        self.natural = Some(page.natural);
        if let Some(old) = self.shown.take() {
            cx.drop_image(old.image, None);
        }
        self.shown = Some(Shown {
            index: page.index,
            natural: page.natural,
            image: super::render_image(page.image),
        });
        self.error = None;
        if first_measure {
            cx.emit(ZoomChanged);
        }
        cx.notify();
    }

    /// Whether a page has been drawn, for checks that wait on it.
    #[cfg(test)]
    pub fn drawn(&self) -> bool {
        self.shown.is_some()
    }
}

fn open_failure(kind: RendererKind) -> &'static str {
    match kind {
        RendererKind::Pdf => "Could not open this PDF.",
        RendererKind::Djvu => "Could not open this DjVu document.",
        _ => "Could not open this CBZ.",
    }
}

impl Render for PagedViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scale_factor = window.scale_factor();
        if scale_factor != self.scale_factor {
            self.scale_factor = scale_factor;
        }
        self.request(cx);

        let page = self.shown.as_ref().map(|shown| {
            // The page on screen at the current layout: a page drawn for an
            // older zoom stretches until its redraw arrives.
            let natural = if shown.index == self.page {
                self.natural.unwrap_or(shown.natural)
            } else {
                shown.natural
            };
            let size = Self::layout(self.kind, self.mode, self.available, natural);
            let shadow = if self.kind == RendererKind::Pdf {
                theme::page_shadow()
            } else {
                theme::sheet_shadow()
            };
            div()
                .flex_none()
                .w(px(size.0))
                .h(px(size.1))
                .bg(rgb(theme::DOCUMENT_PAPER))
                .shadow(shadow)
                .child(
                    img(shown.image.clone())
                        .size_full()
                        .object_fit(ObjectFit::Fill),
                )
        });
        let entity = cx.entity().downgrade();
        let (previous, next) = (entity.clone(), entity.clone());
        shell::shell(&self.scroll, self.page + 1, self.count)
            .error(self.error.clone())
            .content(page)
            .on_previous(move |_, cx| {
                let _ = previous.update(cx, |this, cx| this.previous_page(cx));
            })
            .on_next(move |_, cx| {
                let _ = next.update(cx, |this, cx| this.next_page(cx));
            })
            .on_measure(move |bounds, cx| {
                let available = (
                    (f32::from(bounds.size.width) - 2. * VIEWPORT_PADDING).max(0.),
                    (f32::from(bounds.size.height) - 2. * VIEWPORT_PADDING).max(0.),
                );
                let _ = entity.update(cx, |this, cx| {
                    if this.available != available {
                        let first = this.available.0 <= 0.;
                        this.available = available;
                        if first || this.mode == ScaleMode::PageFit {
                            cx.emit(ZoomChanged);
                        }
                        cx.notify();
                    }
                });
            })
    }
}
