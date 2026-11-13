//! The chrome every document viewer shares (`DocumentViewerShell.tsx`): a
//! scrolling viewport on the dark backdrop with the page centred in it, and
//! a footer as tall as the titlebar with Previous page, "Page n of m" and
//! Next page.
use crate::theme::Text as _;
use crate::{theme, ui};
use gpui::*;

type ClickHandler = Box<dyn Fn(&mut Window, &mut App) + 'static>;
type MeasureHandler = Box<dyn Fn(Bounds<Pixels>, &mut App) + 'static>;

/// `.viewport`'s padding around the page.
pub const VIEWPORT_PADDING: f32 = 24.;

#[derive(IntoElement)]
pub struct DocumentShell {
    /// 1-based, as the counter shows it.
    page: usize,
    count: usize,
    /// Shown in place of the page when it cannot be shown.
    error: Option<SharedString>,
    content: Option<AnyElement>,
    scroll: ScrollHandle,
    on_previous: Option<ClickHandler>,
    on_next: Option<ClickHandler>,
    on_measure: Option<MeasureHandler>,
}

pub fn shell(scroll: &ScrollHandle, page: usize, count: usize) -> DocumentShell {
    DocumentShell {
        page,
        count,
        error: None,
        content: None,
        scroll: scroll.clone(),
        on_previous: None,
        on_next: None,
        on_measure: None,
    }
}

impl DocumentShell {
    /// The page, or nothing while none has been drawn.
    pub fn content(mut self, content: Option<impl IntoElement>) -> Self {
        self.content = content.map(IntoElement::into_any_element);
        self
    }

    /// Shows a message in place of the page (`.message`, `role="alert"`).
    pub fn error(mut self, error: Option<SharedString>) -> Self {
        self.error = error;
        self
    }

    pub fn on_previous(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_previous = Some(Box::new(handler));
        self
    }

    pub fn on_next(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_next = Some(Box::new(handler));
        self
    }

    /// Reports the viewport's bounds, padding included, every frame.
    pub fn on_measure(mut self, handler: impl Fn(Bounds<Pixels>, &mut App) + 'static) -> Self {
        self.on_measure = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for DocumentShell {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = theme::theme(cx);
        let can_previous = self.page > 1 && self.on_previous.is_some();
        let can_next = self.count > 0 && self.page < self.count && self.on_next.is_some();

        let measure = self.on_measure.map(|measure| {
            canvas(move |bounds, _, cx| measure(bounds, cx), |_, _, _, _| {})
                .absolute()
                .size_full()
        });
        let body: AnyElement = match self.error {
            // `.message`: 12px on a 1.5 line, centred in the viewport.
            Some(error) => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(theme::TEXT_SM))
                .line_height(px(theme::TEXT_SM * 1.5))
                .text_color(theme.text_secondary())
                .child(error)
                .into_any_element(),
            None => div()
                .id("document-viewport")
                .track_scroll(&self.scroll)
                .overflow_scroll()
                .size_full()
                .flex()
                .child(
                    // `display: grid; place-items: center` with the padding
                    // inside the scroller. A page larger than the viewport
                    // starts at its top-left corner, so all of it scrolls
                    // into view.
                    div()
                        .flex_none()
                        .min_w_full()
                        .min_h_full()
                        .p(px(VIEWPORT_PADDING))
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(self.content),
                )
                .into_any_element(),
        };
        let viewport = div()
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .bg(rgb(theme::DOCUMENT_BACKDROP))
            .children(measure)
            .child(body)
            .child(ui::scrollbar(
                "document-scrollbar",
                &self.scroll,
                window,
                cx,
            ));

        let previous = self.on_previous;
        let next = self.on_next;
        let footer = div()
            .flex_none()
            .h(px(theme::TITLEBAR_HEIGHT))
            .min_w_0()
            .flex()
            .items_center()
            .justify_center()
            .border_t_1()
            .border_color(theme.border_secondary())
            .bg(theme.background())
            .font_family(theme::FONT_UI)
            .child(
                // `TitlebarControlGroup`
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        ui::icon_button("document-previous", "toolbar-chevron-left")
                            .disabled(!can_previous)
                            .tip(ui::tip("Previous page").shortcut("document.previousPage", cx))
                            .on_click(move |_, window, cx| {
                                if let Some(previous) = &previous {
                                    previous(window, cx);
                                }
                            }),
                    )
                    .child(
                        // `.pageStatus`
                        div()
                            .min_w(px(92.))
                            .flex()
                            .justify_center()
                            .whitespace_nowrap()
                            .text(theme::TEXT_SM)
                            .font_weight(FontWeight::MEDIUM)
                            .font_features(FontFeatures(std::sync::Arc::new(vec![(
                                "tnum".into(),
                                1,
                            )])))
                            .text_color(theme.text_secondary())
                            .child(page_status(self.page, self.count)),
                    )
                    .child(
                        ui::icon_button("document-next", "toolbar-chevron-right")
                            .disabled(!can_next)
                            .tip(ui::tip("Next page").shortcut("document.nextPage", cx))
                            .on_click(move |_, window, cx| {
                                if let Some(next) = &next {
                                    next(window, cx);
                                }
                            }),
                    ),
            );

        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.background())
            .text_color(theme.text())
            .child(viewport)
            .child(footer)
    }
}

/// "Page n of m", with a dash while the count is unknown.
pub fn page_status(page: usize, count: usize) -> String {
    if count == 0 {
        format!("Page {page} of —")
    } else {
        format!("Page {page} of {count}")
    }
}

#[cfg(test)]
mod tests {
    use super::page_status;

    #[test]
    fn the_counter_reads_as_production_does() {
        assert_eq!(page_status(1, 0), "Page 1 of —");
        assert_eq!(page_status(3, 12), "Page 3 of 12");
    }
}
